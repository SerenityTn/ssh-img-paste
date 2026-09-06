use serde_json::{Value, json};
use ssh_img_paste_core::{
    ConfigRootError, ProfileId, ProfileSelection, ProfileStoreError, current_environment,
    default_config_root, discover_profiles, resolve_profile,
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
                "Usage: ssh-img-paste [--config-root ABSOLUTE_PATH] profiles <list|resolve [--profile ID]>",
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
