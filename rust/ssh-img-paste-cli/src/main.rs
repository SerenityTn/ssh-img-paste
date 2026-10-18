use serde_json::{Value, json};
use ssh_img_paste_cli::{StageError, prepare_download_directory, stage_upload_source};
use ssh_img_paste_core::{
    CancellationToken, CatalogEntryStatus, CommandFailure, CommandPlan, ConfigRootError,
    ExecutionPolicy, MutationError, PlanError, ProfileFields, ProfileId, ProfileSelection,
    ProfileStoreError, SourceFileError, UnusableReason, UploadExecutionError, UploadStep,
    build_clean_plan, build_fetch_plan, build_list_plan, build_probe_plan, build_upload_plan,
    create_profile, current_environment, default_config_root, delete_profile, discover_profiles,
    execute_upload_plan_with_system, generate_remote_name, inspect_catalog, open_upload_source,
    openssh_program_path, parse_clean_output, parse_listing, parse_probe_output,
    process_containment_mechanism, remote_upload_root, rename_profile, resolve_profile,
    run_command_plan_with_system, set_active_profile, update_profile, valid_upload_name,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const PROTOCOL_VERSION: u64 = 1;
const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
const CLEAN_TIMEOUT: Duration = Duration::from_secs(60);
const UPLOAD_STEP_TIMEOUT: Duration = Duration::from_secs(30);

const USAGE: &str = "Usage: ssh-img-paste [--config-root ABSOLUTE_PATH] COMMAND
  profiles list [--detailed]
  profiles resolve [--profile ID]
  profiles create ID --label L --host H --remote-home P --remote-dir D [--shot-mode region|full] [--restore-seconds N]
  profiles update ID [--label L] [--host H] [--remote-home P] [--remote-dir D] [--shot-mode M] [--restore-seconds N]
  profiles rename OLD NEW
  profiles use ID
  profiles delete ID [--switch-to OTHER]
  profiles test ID
  hosts probe --host HOST
  upload-file --source ABSOLUTE_PATH --profile ID --confirm-profile ID
  uploads list --profile ID
  uploads fetch --profile ID --name NAME.png
  uploads clean --profile ID --confirm-profile ID
  version";

struct Invocation {
    config_root: Option<PathBuf>,
    command: Command,
}

enum Command {
    ProfilesList { detailed: bool },
    ProfilesResolve(Option<ProfileId>),
    ProfilesCreate { id: ProfileId, fields: ProfileFields },
    ProfilesUpdate { id: ProfileId, fields: ProfileFields },
    ProfilesRename { old: ProfileId, new: ProfileId },
    ProfilesUse(ProfileId),
    ProfilesDelete { id: ProfileId, switch_to: Option<ProfileId> },
    ProfilesTest(ProfileId),
    HostsProbe(String),
    UploadFile { source: PathBuf, profile: ProfileId },
    UploadsList(ProfileId),
    UploadsFetch { profile: ProfileId, name: String },
    UploadsClean(ProfileId),
    Version,
}

type ParseResult<T> = Result<T, &'static str>;

fn text(value: &OsString) -> ParseResult<&str> {
    value.to_str().ok_or("invalid_arguments")
}

fn profile_id(value: &OsString) -> ParseResult<ProfileId> {
    let value = value.to_str().ok_or("invalid_profile_id")?;
    ProfileId::parse(value).map_err(|_| "invalid_profile_id")
}

/// Parses `--flag VALUE` pairs. Unknown, repeated, and value-less flags fail.
fn parse_flags<'a>(
    arguments: &'a [OsString],
    allowed: &[&'static str],
) -> ParseResult<BTreeMap<&'static str, &'a OsString>> {
    let mut flags = BTreeMap::new();
    let mut remaining = arguments.iter();
    while let Some(flag) = remaining.next() {
        let name = allowed
            .iter()
            .copied()
            .find(|candidate| flag == *candidate)
            .ok_or("invalid_arguments")?;
        let value = remaining.next().ok_or("invalid_arguments")?;
        if flags.insert(name, value).is_some() {
            return Err("invalid_arguments");
        }
    }
    Ok(flags)
}

const FIELD_FLAGS: [&str; 6] = [
    "--label",
    "--host",
    "--remote-home",
    "--remote-dir",
    "--shot-mode",
    "--restore-seconds",
];

fn parse_fields(arguments: &[OsString]) -> ParseResult<ProfileFields> {
    let flags = parse_flags(arguments, &FIELD_FLAGS)?;
    let field = |name: &str| -> ParseResult<Option<String>> {
        flags
            .get(name)
            .map(|value| text(value).map(str::to_owned))
            .transpose()
    };
    Ok(ProfileFields {
        label: field("--label")?,
        host: field("--host")?,
        remote_home: field("--remote-home")?,
        remote_dir: field("--remote-dir")?,
        shot_mode: field("--shot-mode")?,
        restore_seconds: field("--restore-seconds")?,
    })
}

fn confirmed_profile(flags: &BTreeMap<&'static str, &OsString>) -> ParseResult<ProfileId> {
    let (Some(profile), Some(confirmation)) = (flags.get("--profile"), flags.get("--confirm-profile"))
    else {
        return Err("invalid_arguments");
    };
    let profile = profile_id(profile)?;
    if profile != profile_id(confirmation)? {
        return Err("profile_confirmation_mismatch");
    }
    Ok(profile)
}

fn parse_command(arguments: &[OsString]) -> ParseResult<Command> {
    let words: Vec<Option<&str>> = arguments.iter().map(|value| value.to_str()).collect();
    match words.as_slice() {
        [Some("profiles"), Some("list")] => Ok(Command::ProfilesList { detailed: false }),
        [Some("profiles"), Some("list"), Some("--detailed")] => {
            Ok(Command::ProfilesList { detailed: true })
        }
        [Some("profiles"), Some("resolve")] => Ok(Command::ProfilesResolve(None)),
        [Some("profiles"), Some("resolve"), Some("--profile"), _] => {
            Ok(Command::ProfilesResolve(Some(profile_id(&arguments[3])?)))
        }
        [Some("profiles"), Some("create"), _, ..] => Ok(Command::ProfilesCreate {
            id: profile_id(&arguments[2])?,
            fields: parse_fields(&arguments[3..])?,
        }),
        [Some("profiles"), Some("update"), _, ..] => Ok(Command::ProfilesUpdate {
            id: profile_id(&arguments[2])?,
            fields: parse_fields(&arguments[3..])?,
        }),
        [Some("profiles"), Some("rename"), _, _] => Ok(Command::ProfilesRename {
            old: profile_id(&arguments[2])?,
            new: profile_id(&arguments[3])?,
        }),
        [Some("profiles"), Some("use"), _] => Ok(Command::ProfilesUse(profile_id(&arguments[2])?)),
        [Some("profiles"), Some("delete"), _, ..] => {
            let id = profile_id(&arguments[2])?;
            let flags = parse_flags(&arguments[3..], &["--switch-to"])?;
            let switch_to = flags.get("--switch-to").map(|value| profile_id(value)).transpose()?;
            Ok(Command::ProfilesDelete { id, switch_to })
        }
        [Some("profiles"), Some("test"), _] => Ok(Command::ProfilesTest(profile_id(&arguments[2])?)),
        [Some("hosts"), Some("probe"), ..] => {
            let flags = parse_flags(&arguments[2..], &["--host"])?;
            let host = flags.get("--host").ok_or("invalid_arguments")?;
            Ok(Command::HostsProbe(text(host)?.to_owned()))
        }
        [Some("upload-file"), ..] => {
            let flags = parse_flags(
                &arguments[1..],
                &["--source", "--profile", "--confirm-profile"],
            )?;
            let source = flags.get("--source").ok_or("invalid_arguments")?;
            let profile = confirmed_profile(&flags)?;
            Ok(Command::UploadFile {
                source: PathBuf::from(source),
                profile,
            })
        }
        [Some("uploads"), Some("list"), ..] => {
            let flags = parse_flags(&arguments[2..], &["--profile"])?;
            let profile = flags.get("--profile").ok_or("invalid_arguments")?;
            Ok(Command::UploadsList(profile_id(profile)?))
        }
        [Some("uploads"), Some("fetch"), ..] => {
            let flags = parse_flags(&arguments[2..], &["--profile", "--name"])?;
            let (Some(profile), Some(name)) = (flags.get("--profile"), flags.get("--name")) else {
                return Err("invalid_arguments");
            };
            let name = text(name).map_err(|_| "invalid_upload_name")?;
            if !valid_upload_name(name) {
                return Err("invalid_upload_name");
            }
            Ok(Command::UploadsFetch {
                profile: profile_id(profile)?,
                name: name.to_owned(),
            })
        }
        [Some("uploads"), Some("clean"), ..] => {
            let flags = parse_flags(&arguments[2..], &["--profile", "--confirm-profile"])?;
            Ok(Command::UploadsClean(confirmed_profile(&flags)?))
        }
        [Some("version")] => Ok(Command::Version),
        _ => Err("invalid_arguments"),
    }
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
    Ok(Invocation {
        config_root,
        command: parse_command(&remaining)?,
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
    failure_with(code, message, json!({}))
}

/// A failure carrying extra machine-readable error fields such as `field`.
fn failure_with(code: &'static str, message: &'static str, extra: Value) -> Value {
    let mut error = json!({
        "code": code,
        "message": message,
    });
    if let (Some(error), Value::Object(extra)) = (error.as_object_mut(), extra) {
        error.extend(extra);
    }
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "ok": false,
        "error": error,
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
        ProfileStoreError::Validation {
            source: ssh_img_paste_core::ValidationError::InvalidField(field),
            ..
        } => failure_with(
            "profile_validation_failed",
            "A profile contains an invalid field.",
            json!({ "field": field }),
        ),
    }
}

fn mutation_error(error: MutationError) -> Value {
    match error {
        MutationError::MissingField(field) => failure_with(
            "missing_field",
            "A required profile field was not provided.",
            json!({ "field": field }),
        ),
        MutationError::NoFieldsToUpdate => failure(
            "no_fields",
            "Profile update requires at least one field option.",
        ),
        MutationError::InvalidField(field) => failure_with(
            "invalid_field",
            "A profile field is invalid.",
            json!({ "field": field }),
        ),
        MutationError::ProfileExists(_) => {
            failure("profile_exists", "A profile with that ID already exists.")
        }
        MutationError::ProfileNotFound(_) => {
            failure("profile_not_found", "The requested profile does not exist.")
        }
        MutationError::ProfileNotEditable(_) => failure(
            "profile_not_editable",
            "The profile is managed manually and cannot be rewritten.",
        ),
        MutationError::ProfileUnusable(_) => failure(
            "profile_unusable",
            "The profile cannot be used because it is invalid or unreadable.",
        ),
        MutationError::LastUsableProfile => failure(
            "last_usable_profile",
            "The last usable profile cannot be deleted.",
        ),
        MutationError::SwitchToRequired => failure(
            "switch_to_required",
            "Deleting the active profile requires --switch-to with another profile.",
        ),
        MutationError::SwitchToSameProfile => failure(
            "switch_to_same_profile",
            "--switch-to must name a different profile.",
        ),
        MutationError::UnsafePath(_) => failure(
            "profile_store_unsafe",
            "The profile store contains a link or unexpected file and was not modified.",
        ),
        MutationError::Store(error) => profile_error(error),
        MutationError::Io { .. } => failure(
            "profile_store_io",
            "The profile store could not be updated safely.",
        ),
        MutationError::PartiallyApplied { .. } => failure(
            "profile_store_partial",
            "The active profile changed, but the profile file could not be removed.",
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

fn failure_detail(failure: CommandFailure) -> (&'static str, Option<i32>, Option<String>) {
    match failure {
        CommandFailure::Spawn { message } => ("spawn", None, Some(message)),
        CommandFailure::Exit { code, stderr } => ("exit", code, Some(stderr)),
        CommandFailure::Timeout => ("timeout", None, None),
        CommandFailure::Cancelled => ("cancelled", None, None),
        CommandFailure::Adapter { message } => ("adapter", None, Some(message)),
        CommandFailure::Cleanup { message, .. } => ("cleanup", None, Some(message)),
    }
}

fn command_failure(
    code: &'static str,
    message: &'static str,
    step: &'static str,
    failure: CommandFailure,
) -> Value {
    let (reason, exit_code, diagnostic) = failure_detail(failure);
    failure_with(
        code,
        message,
        json!({
            "step": step,
            "reason": reason,
            "exit_code": exit_code,
            "diagnostic": diagnostic,
        }),
    )
}

fn upload_error(error: UploadExecutionError) -> Value {
    let step = match error.step {
        UploadStep::CreateRemoteDirectory => "create_remote_directory",
        UploadStep::Upload => "upload",
        UploadStep::Finalize => "finalize",
    };
    command_failure(
        "upload_failed",
        "The remote upload did not complete.",
        step,
        error.failure,
    )
}

fn install_cancellation() -> Result<CancellationToken, Value> {
    let cancellation = CancellationToken::new();
    let handler_cancellation = cancellation.clone();
    ctrlc::set_handler(move || handler_cancellation.cancel()).map_err(|_| {
        failure(
            "cancellation_unavailable",
            "Cancellation handling could not be installed.",
        )
    })?;
    Ok(cancellation)
}

fn run_plan(
    plan: &CommandPlan,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, CommandFailure> {
    run_command_plan_with_system(plan, timeout, cancellation.clone(), MAX_DIAGNOSTIC_BYTES)
}

fn probe_home(
    host: &str,
    cancellation: &CancellationToken,
) -> Result<String, Value> {
    let plan = build_probe_plan(host).map_err(|_| {
        failure_with(
            "invalid_field",
            "The SSH host is invalid.",
            json!({ "field": "host" }),
        )
    })?;
    let stdout = run_plan(&plan, PROBE_TIMEOUT, cancellation).map_err(|error| {
        command_failure(
            "connection_failed",
            "The SSH connection test did not succeed.",
            "connect",
            error,
        )
    })?;
    parse_probe_output(&stdout).map_err(|_| {
        failure(
            "remote_home_unusable",
            "The host connected but did not report a usable home directory.",
        )
    })
}

fn profile_summary(id: &ProfileId, status: &CatalogEntryStatus, active: bool) -> Value {
    match status {
        CatalogEntryStatus::Usable(profile) => json!({
            "id": id.as_str(),
            "usable": true,
            "active": active,
            "label": profile.label,
            "host": profile.host,
            "remote_home": profile.remote_home,
            "remote_dir": profile.remote_dir,
            "shot_mode": profile.shot_mode,
            "restore_seconds": profile.restore_seconds,
            "editable": profile.editable,
        }),
        CatalogEntryStatus::Unusable(reason) => {
            let problem = match reason {
                UnusableReason::DynamicSyntax(key) => json!({ "code": "dynamic_syntax", "key": key }),
                UnusableReason::InvalidField(field) => {
                    json!({ "code": "invalid_field", "field": field })
                }
                UnusableReason::Unreadable => json!({ "code": "unreadable" }),
            };
            json!({
                "id": id.as_str(),
                "usable": false,
                "active": false,
                "problem": problem,
            })
        }
    }
}

fn execute(invocation: Invocation) -> Result<Value, Value> {
    if let Command::Version = invocation.command {
        return Ok(success(json!({
            "type": "version",
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": PROTOCOL_VERSION,
            "process_containment": process_containment_mechanism(),
            "ssh_program": openssh_program_path("ssh"),
            "scp_program": openssh_program_path("scp"),
        })));
    }
    let environment = current_environment();
    let root = default_config_root(invocation.config_root.as_deref(), &environment)
        .map_err(config_error)?;
    match invocation.command {
        Command::Version => unreachable!("handled before resolving the config root"),
        Command::ProfilesList { detailed: false } => {
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
        Command::ProfilesList { detailed: true } => {
            let inspection = inspect_catalog(&root).map_err(profile_error)?;
            let effective = inspection.effective_active_profile_id().cloned();
            let profiles: Vec<_> = inspection
                .entries
                .iter()
                .map(|entry| {
                    profile_summary(&entry.id, &entry.status, effective.as_ref() == Some(&entry.id))
                })
                .collect();
            Ok(success(json!({
                "type": "profile_details",
                "configured": inspection.configured,
                "config_root": root.to_string_lossy(),
                "active_profile_id": effective.as_ref().map(ProfileId::as_str),
                "active_state_invalid": inspection.active_state_invalid,
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
        Command::ProfilesCreate { id, fields } => {
            let outcome = create_profile(&root, &id, &fields).map_err(mutation_error)?;
            Ok(success(json!({
                "type": "profile_created",
                "profile_id": id.as_str(),
                "activated": outcome.activated,
            })))
        }
        Command::ProfilesUpdate { id, fields } => {
            update_profile(&root, &id, &fields).map_err(mutation_error)?;
            Ok(success(json!({
                "type": "profile_updated",
                "profile_id": id.as_str(),
            })))
        }
        Command::ProfilesRename { old, new } => {
            let outcome = rename_profile(&root, &old, &new).map_err(mutation_error)?;
            Ok(success(json!({
                "type": "profile_renamed",
                "old_profile_id": old.as_str(),
                "profile_id": new.as_str(),
                "active_updated": outcome.active_updated,
            })))
        }
        Command::ProfilesUse(id) => {
            set_active_profile(&root, &id).map_err(mutation_error)?;
            Ok(success(json!({
                "type": "active_profile",
                "profile_id": id.as_str(),
            })))
        }
        Command::ProfilesDelete { id, switch_to } => {
            let outcome =
                delete_profile(&root, &id, switch_to.as_ref()).map_err(mutation_error)?;
            Ok(success(json!({
                "type": "profile_deleted",
                "profile_id": id.as_str(),
                "active_profile_id": outcome.active_profile_id.as_ref().map(ProfileId::as_str),
            })))
        }
        Command::ProfilesTest(id) => {
            let cancellation = install_cancellation()?;
            let selected = resolve_profile(&root, ProfileSelection::Explicit(id))
                .map_err(profile_error)?;
            let detected = probe_home(&selected.profile.host, &cancellation)?;
            let configured = selected.profile.remote_home.trim_end_matches('/');
            let configured = if configured.is_empty() { "/" } else { configured };
            Ok(success(json!({
                "type": "connection_test",
                "profile_id": selected.id.as_str(),
                "host": selected.profile.host,
                "remote_home": selected.profile.remote_home,
                "detected_remote_home": detected,
                "remote_home_matches": configured == detected,
            })))
        }
        Command::HostsProbe(host) => {
            let cancellation = install_cancellation()?;
            let detected = probe_home(&host, &cancellation)?;
            Ok(success(json!({
                "type": "host_probe",
                "host": host,
                "detected_remote_home": detected,
            })))
        }
        Command::UploadFile { source, profile } => {
            let cancellation = install_cancellation()?;
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
                    command_timeout: UPLOAD_STEP_TIMEOUT,
                },
                cancellation,
                MAX_DIAGNOSTIC_BYTES,
            )
            .map_err(upload_error)?;
            Ok(success(json!({
                "type": "upload",
                "profile_id": selected.id.as_str(),
                "remote_path": execution.remote_path,
            })))
        }
        Command::UploadsList(profile) => {
            let cancellation = install_cancellation()?;
            let selected = resolve_profile(&root, ProfileSelection::Explicit(profile))
                .map_err(profile_error)?;
            let remote_root = remote_upload_root(&selected.profile).map_err(plan_error)?;
            let plan = build_list_plan(&selected.profile).map_err(plan_error)?;
            let stdout = run_plan(&plan, LIST_TIMEOUT, &cancellation).map_err(|error| {
                command_failure(
                    "remote_command_failed",
                    "The remote upload directory could not be listed.",
                    "list",
                    error,
                )
            })?;
            let listing = parse_listing(&stdout);
            let uploads: Vec<_> = listing
                .uploads
                .iter()
                .map(|upload| {
                    json!({
                        "name": upload.name,
                        "size_bytes": upload.size_bytes,
                        "modified_epoch_seconds": upload.modified_epoch_seconds,
                        "remote_path": format!("{remote_root}/{}", upload.name),
                    })
                })
                .collect();
            Ok(success(json!({
                "type": "upload_listing",
                "profile_id": selected.id.as_str(),
                "remote_directory": remote_root,
                "uploads": uploads,
                "skipped_rows": listing.skipped_rows,
                "truncated": listing.truncated,
            })))
        }
        Command::UploadsFetch { profile, name } => {
            let cancellation = install_cancellation()?;
            let selected = resolve_profile(&root, ProfileSelection::Explicit(profile))
                .map_err(profile_error)?;
            let remote_root = remote_upload_root(&selected.profile).map_err(plan_error)?;
            let directory = prepare_download_directory(&std::env::temp_dir()).map_err(|_| {
                failure(
                    "download_staging_failed",
                    "A private download directory could not be created.",
                )
            })?;
            let destination = directory.path().join(&name);
            let plan =
                build_fetch_plan(&selected.profile, &name, &destination).map_err(plan_error)?;
            run_plan(&plan, FETCH_TIMEOUT, &cancellation).map_err(|error| {
                command_failure(
                    "remote_command_failed",
                    "The upload could not be downloaded.",
                    "fetch",
                    error,
                )
            })?;
            open_upload_source(&destination).map_err(|_| {
                failure(
                    "fetched_not_png",
                    "The downloaded file is not a PNG image and was discarded.",
                )
            })?;
            let directory = directory.keep();
            Ok(success(json!({
                "type": "upload_fetched",
                "profile_id": selected.id.as_str(),
                "name": name,
                "remote_path": format!("{remote_root}/{name}"),
                "local_path": directory.join(&name).to_string_lossy(),
            })))
        }
        Command::UploadsClean(profile) => {
            let cancellation = install_cancellation()?;
            let selected = resolve_profile(&root, ProfileSelection::Explicit(profile))
                .map_err(profile_error)?;
            let plan = build_clean_plan(&selected.profile).map_err(plan_error)?;
            let stdout = run_plan(&plan, CLEAN_TIMEOUT, &cancellation).map_err(|error| {
                command_failure(
                    "remote_command_failed",
                    "The remote uploads could not be deleted.",
                    "clean",
                    error,
                )
            })?;
            let deleted = parse_clean_output(&stdout).ok_or_else(|| {
                failure(
                    "remote_output_invalid",
                    "The host did not report how many uploads were deleted.",
                )
            })?;
            Ok(success(json!({
                "type": "uploads_cleaned",
                "profile_id": selected.id.as_str(),
                "deleted_count": deleted,
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
            emit(&failure(code, USAGE));
            return ExitCode::from(2);
        }
    };
    let usage_error = matches!(
        &invocation.command,
        Command::ProfilesUpdate { fields, .. } if *fields == ProfileFields::default()
    );
    match execute(invocation) {
        Ok(value) => {
            emit(&value);
            ExitCode::SUCCESS
        }
        Err(value) => {
            emit(&value);
            ExitCode::from(if usage_error { 2 } else { 3 })
        }
    }
}
