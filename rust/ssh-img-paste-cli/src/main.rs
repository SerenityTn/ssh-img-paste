use serde_json::{Value, json};
use ssh_img_paste_cli::{StageError, stage_upload_source};
use ssh_img_paste_core::{
    CancellationToken, CommandFailure, ConfigRootError, ExecutionPolicy, PlanError, ProfileId,
    ProfileSelection, ProfileStoreError, SourceFileError, UploadExecutionError, UploadStep,
    build_upload_plan, current_environment, default_config_root, discover_profiles,
    execute_upload_plan_with_system, generate_remote_name, open_upload_source, resolve_profile,
};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const PROTOCOL_VERSION: u64 = 1;

struct Invocation {
    config_root: Option<PathBuf>,
    command: Command,
}

enum Command {
    ProfilesList,
    ProfilesResolve(Option<ProfileId>),
    UploadFile { source: PathBuf, profile: ProfileId },
}

fn parse_invocation(mut args: impl Iterator<Item = OsString>) -> Result<Invocation, &'static str> {
    let mut config_root = None;
    let mut remaining = Vec::new();
    if let Some(argument) = args.next() {
        if argument == "--config-root" {
            config_root = Some(PathBuf::from(args.next().ok_or("missing_config_root")?));
        } else {
            remaining.push(argument);
        }
    }
    remaining.extend(args);
    let command = match remaining.as_slice() {
        [group, action] if group == "profiles" && action == "list" => Command::ProfilesList,
        [group, action] if group == "profiles" && action == "resolve" => {
            Command::ProfilesResolve(None)
        }
        [group, action, flag, id]
            if group == "profiles" && action == "resolve" && flag == "--profile" =>
        {
            let id = id.to_str().ok_or("invalid_profile_id")?;
            Command::ProfilesResolve(Some(
                ProfileId::parse(id).map_err(|_| "invalid_profile_id")?,
            ))
        }
        [
            command,
            source_flag,
            source,
            profile_flag,
            profile,
            confirmation_flag,
            confirmation,
        ] if command == "upload-file"
            && source_flag == "--source"
            && profile_flag == "--profile"
            && confirmation_flag == "--confirm-profile" =>
        {
            let profile = profile.to_str().ok_or("invalid_profile_id")?;
            let confirmation = confirmation.to_str().ok_or("invalid_profile_id")?;
            let profile = ProfileId::parse(profile).map_err(|_| "invalid_profile_id")?;
            let confirmation = ProfileId::parse(confirmation).map_err(|_| "invalid_profile_id")?;
            if profile != confirmation {
                return Err("profile_confirmation_mismatch");
            }
            Command::UploadFile {
                source: PathBuf::from(source),
                profile,
            }
        }
        _ => return Err("invalid_arguments"),
    };
    Ok(Invocation {
        config_root,
        command,
    })
}

fn success(result: Value) -> Value {
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "ok": true,
        "result": result,
    })
}

fn failure(code: &'static str, message: &'static str) -> Value {
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "ok": false,
        "error": {
            "code": code,
            "message": message,
        },
    })
}

fn config_error(error: ConfigRootError) -> Value {
    match error {
        ConfigRootError::MissingEnvironment(_) => failure(
            "config_root_unavailable",
            "The per-user configuration directory could not be determined.",
        ),
        ConfigRootError::PathMustBeAbsolute(_) => failure(
            "config_root_not_absolute",
            "The configuration root must be an absolute path.",
        ),
    }
}

fn profile_error(error: ProfileStoreError) -> Value {
    match error {
        ProfileStoreError::NotConfigured => failure(
            "not_configured",
            "No valid SSH Image Paste profile store is configured.",
        ),
        ProfileStoreError::ProfileNotFound(_) => {
            failure("profile_not_found", "The requested profile does not exist.")
        }
        ProfileStoreError::InvalidActiveProfile => failure(
            "invalid_active_profile",
            "The active profile state is invalid.",
        ),
        ProfileStoreError::Io { .. } => failure(
            "profile_store_io",
            "The profile store could not be read safely.",
        ),
        ProfileStoreError::Parse { .. } => failure(
            "profile_parse_failed",
            "A profile contains unsupported dynamic syntax.",
        ),
        ProfileStoreError::Validation { .. } => failure(
            "profile_validation_failed",
            "A profile contains an invalid field.",
        ),
    }
}

fn source_error(error: SourceFileError) -> Value {
    match error {
        SourceFileError::PathMustBeAbsolute => failure(
            "source_path_not_absolute",
            "The upload source path must be absolute.",
        ),
        SourceFileError::Open(std::io::ErrorKind::NotFound) => {
            failure("source_not_found", "The upload source does not exist.")
        }
        SourceFileError::Open(std::io::ErrorKind::PermissionDenied) => failure(
            "source_permission_denied",
            "The upload source cannot be read with current permissions.",
        ),
        SourceFileError::Open(_) => failure(
            "source_open_failed",
            "The upload source could not be opened safely.",
        ),
        SourceFileError::NotRegularFile => failure(
            "source_not_regular",
            "The upload source must be a regular file.",
        ),
        SourceFileError::ReparsePoint => failure(
            "source_link_not_allowed",
            "Linked or reparse-point upload sources are not allowed.",
        ),
        SourceFileError::InvalidPng => failure(
            "source_not_png",
            "The upload source must contain a PNG image.",
        ),
    }
}

fn stage_error(error: StageError) -> Value {
    match error {
        StageError::Cancelled => json!({
            "protocol_version": PROTOCOL_VERSION,
            "ok": false,
            "error": {
                "code": "upload_cancelled",
                "message": "The upload was cancelled while preparing the PNG.",
                "step": "staging",
                "reason": "cancelled",
                "exit_code": Value::Null,
                "diagnostic": Value::Null,
            },
        }),
        StageError::Io(_) => failure(
            "source_staging_failed",
            "The validated PNG could not be copied into private staging.",
        ),
    }
}

fn plan_error(_error: PlanError) -> Value {
    failure(
        "upload_plan_invalid",
        "The validated profile and source could not produce a safe upload plan.",
    )
}

fn upload_error(error: UploadExecutionError) -> Value {
    let step = match error.step {
        UploadStep::CreateRemoteDirectory => "create_remote_directory",
        UploadStep::Upload => "upload",
        UploadStep::Finalize => "finalize",
    };
    let (reason, exit_code, diagnostic) = match error.failure {
        CommandFailure::Spawn { message } => ("spawn", None, Some(message)),
        CommandFailure::Exit { code, stderr } => ("exit", code, Some(stderr)),
        CommandFailure::Timeout => ("timeout", None, None),
        CommandFailure::Cancelled => ("cancelled", None, None),
        CommandFailure::Adapter { message } => ("adapter", None, Some(message)),
        CommandFailure::Cleanup { message, .. } => ("cleanup", None, Some(message)),
    };
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "ok": false,
        "error": {
            "code": "upload_failed",
            "message": "The remote upload did not complete.",
            "step": step,
            "reason": reason,
            "exit_code": exit_code,
            "diagnostic": diagnostic,
        },
    })
}

fn execute(invocation: Invocation) -> Result<Value, Value> {
    let environment = current_environment();
    let root = default_config_root(invocation.config_root.as_deref(), &environment)
        .map_err(config_error)?;
    match invocation.command {
        Command::ProfilesList => {
            let catalog = discover_profiles(&root).map_err(profile_error)?;
            let active_profile_id = catalog
                .active_profile_id
                .as_ref()
                .map(|id| id.as_str().to_owned());
            let profiles: Vec<_> = catalog
                .profiles
                .iter()
                .map(|entry| {
                    json!({
                        "id": entry.id.as_str(),
                        "label": entry.profile.label,
                        "active": catalog.active_profile_id.as_ref() == Some(&entry.id),
                        "editable": entry.profile.editable,
                    })
                })
                .collect();
            Ok(success(json!({
                "type": "profile_catalog",
                "active_profile_id": active_profile_id,
                "profiles": profiles,
            })))
        }
        Command::ProfilesResolve(explicit_id) => {
            let selection = explicit_id
                .map(ProfileSelection::Explicit)
                .unwrap_or(ProfileSelection::Active);
            let selected = resolve_profile(&root, selection).map_err(profile_error)?;
            Ok(success(json!({
                "type": "profile",
                "profile": {
                    "id": selected.id.as_str(),
                    "label": selected.profile.label,
                    "host": selected.profile.host,
                    "remote_home": selected.profile.remote_home,
                    "remote_dir": selected.profile.remote_dir,
                    "shot_mode": selected.profile.shot_mode,
                    "restore_seconds": selected.profile.restore_seconds,
                    "editable": selected.profile.editable,
                },
            })))
        }
        Command::UploadFile { source, profile } => {
            let cancellation = CancellationToken::new();
            let handler_cancellation = cancellation.clone();
            ctrlc::set_handler(move || handler_cancellation.cancel()).map_err(|_| {
                failure(
                    "cancellation_unavailable",
                    "Upload cancellation handling could not be installed.",
                )
            })?;
            let selected = resolve_profile(&root, ProfileSelection::Explicit(profile))
                .map_err(profile_error)?;
            let mut source_file = open_upload_source(&source).map_err(source_error)?;
            let staged =
                stage_upload_source(&mut source_file, &cancellation, &std::env::temp_dir())
                    .map_err(stage_error)?;
            let epoch_seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| failure("clock_unavailable", "The system clock is invalid."))?
                .as_secs();
            let mut entropy = [0_u8; 8];
            getrandom::fill(&mut entropy).map_err(|_| {
                failure(
                    "entropy_unavailable",
                    "Secure remote-name generation is unavailable.",
                )
            })?;
            let remote_name = generate_remote_name(epoch_seconds, entropy);
            let plan = build_upload_plan(&selected.profile, staged.path(), &remote_name)
                .map_err(plan_error)?;
            let execution = execute_upload_plan_with_system(
                &plan,
                ExecutionPolicy {
                    command_timeout: std::time::Duration::from_secs(30),
                },
                cancellation,
                4 * 1024,
            )
            .map_err(upload_error)?;
            Ok(success(json!({
                "type": "upload",
                "profile_id": selected.id.as_str(),
                "remote_path": execution.remote_path,
            })))
        }
    }
}

fn emit(value: &Value) {
    println!("{value}");
}

fn main() -> ExitCode {
    let invocation = match parse_invocation(std::env::args_os().skip(1)) {
        Ok(invocation) => invocation,
        Err(code) => {
            emit(&failure(
                code,
                "Usage: ssh-img-paste [--config-root ABSOLUTE_PATH] profiles <list|resolve [--profile ID]> | upload-file --source ABSOLUTE_PATH --profile ID --confirm-profile ID",
            ));
            return ExitCode::from(2);
        }
    };
    match execute(invocation) {
        Ok(value) => {
            emit(&value);
            ExitCode::SUCCESS
        }
        Err(value) => {
            emit(&value);
            ExitCode::from(3)
        }
    }
}
