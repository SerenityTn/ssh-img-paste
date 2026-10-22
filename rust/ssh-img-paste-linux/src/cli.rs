//! Client for the authoritative Rust CLI (protocol v1).
//!
//! The desktop shell never writes profile files or builds SSH commands
//! itself: it runs `ssh-img-paste` with an argument array and parses the single
//! JSON document the CLI prints.

use crate::process::{self, ProcessError, ProcessRequest};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PROTOCOL_VERSION: u64 = 1;
pub const CLI_FILE_NAME: &str = "ssh-img-paste";
/// Absolute path that replaces the bundled CLI lookup, for development.
pub const CLI_PATH_ENV: &str = "SSH_IMG_PASTE_CLI";
/// Absolute directory that replaces the default settings root, as on Windows.
pub const CONFIG_ROOT_ENV: &str = "SSH_IMG_PASTE_CONFIG_ROOT";

const LOCAL_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_TIMEOUT: Duration = Duration::from_secs(40);
const LIST_TIMEOUT: Duration = Duration::from_secs(50);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const FETCH_TIMEOUT: Duration = Duration::from_secs(150);
const CLEAN_TIMEOUT: Duration = Duration::from_secs(80);
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_RESTORE_SECONDS: u32 = 60;

/// A structured CLI failure (the protocol's `error` object).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliError {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub step: Option<String>,
    pub reason: Option<String>,
    pub exit_code: Option<i64>,
    pub diagnostic: Option<String>,
}

impl CliError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            ..Self::default()
        }
    }
}

pub trait CliRunner {
    fn run(&self, arguments: &[&str], timeout: Duration) -> Result<Value, CliError>;
}

#[derive(Debug, Clone)]
pub struct CliClient {
    executable: PathBuf,
    config_root: Option<PathBuf>,
}

impl CliClient {
    pub fn new(executable: PathBuf, config_root: Option<PathBuf>) -> Self {
        Self {
            executable,
            config_root,
        }
    }

    /// Resolves the CLI: an absolute `SSH_IMG_PASTE_CLI`, then the binary
    /// installed next to this executable, then `PATH`.
    pub fn locate(
        env: impl Fn(&str) -> Option<OsString>,
        current_exe: Option<PathBuf>,
    ) -> Result<Self, CliError> {
        let config_root = absolute_env(&env, CONFIG_ROOT_ENV);
        if let Some(explicit) = absolute_env(&env, CLI_PATH_ENV) {
            return Ok(Self::new(explicit, config_root));
        }
        let sibling = current_exe
            .as_deref()
            .and_then(Path::parent)
            .map(|directory| directory.join(CLI_FILE_NAME))
            .filter(|candidate| candidate.is_file());
        sibling
            .or_else(|| process::find_on_path(CLI_FILE_NAME, env("PATH").as_deref()))
            .map(|executable| Self::new(executable, config_root))
            .ok_or_else(|| {
                CliError::new(
                    "cli_missing",
                    "The ssh-img-paste command-line component was not found. Install it next to \
                     ssh-img-paste-linux or on PATH.",
                )
            })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

fn absolute_env(env: &impl Fn(&str) -> Option<OsString>, name: &str) -> Option<PathBuf> {
    env(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

impl CliRunner for CliClient {
    fn run(&self, arguments: &[&str], timeout: Duration) -> Result<Value, CliError> {
        let mut full = Vec::with_capacity(arguments.len() + 2);
        if let Some(root) = &self.config_root {
            full.push(OsString::from("--config-root"));
            full.push(root.clone().into_os_string());
        }
        full.extend(arguments.iter().map(OsString::from));
        let mut request = ProcessRequest::new(&self.executable, &[], timeout);
        request.arguments = full;
        request.max_output_bytes = MAX_OUTPUT_BYTES;

        let output = process::run(&request).map_err(|error| match error {
            ProcessError::Spawn(error) if error.kind() == std::io::ErrorKind::NotFound => {
                CliError::new(
                    "cli_missing",
                    format!(
                        "The ssh-img-paste command-line component is missing: {}",
                        self.executable.display()
                    ),
                )
            }
            ProcessError::Spawn(_) | ProcessError::Wait(_) => CliError::new(
                "cli_start_failed",
                "The ssh-img-paste command-line component could not start.",
            ),
            ProcessError::Timeout => CliError {
                reason: Some("timeout".into()),
                ..CliError::new(
                    "cli_timeout",
                    "The operation took too long and was stopped.",
                )
            },
        })?;
        if output.stdout_truncated {
            return Err(CliError::new(
                "cli_protocol",
                "The command-line component returned an oversized response.",
            ));
        }
        parse_document(
            &String::from_utf8_lossy(&output.stdout),
            output.status.code(),
        )
    }
}

/// Validates one protocol document and returns its `result`.
pub fn parse_document(stdout: &str, exit_code: Option<i32>) -> Result<Value, CliError> {
    let root: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        CliError::new(
            "cli_protocol",
            "The command-line component returned an unexpected response.",
        )
    })?;
    if root.get("protocol_version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(CliError::new(
            "cli_protocol",
            "The command-line component speaks an unsupported protocol version.",
        ));
    }
    match (root.get("ok").and_then(Value::as_bool), root.get("result")) {
        (Some(true), Some(result)) if exit_code == Some(0) => Ok(result.clone()),
        _ => match root.get("error") {
            Some(error @ Value::Object(_)) => Err(parse_error(error)),
            _ => Err(CliError::new(
                "cli_protocol",
                "The command-line component reported an inconsistent result.",
            )),
        },
    }
}

fn parse_error(error: &Value) -> CliError {
    let text = |name: &str| error.get(name).and_then(Value::as_str).map(str::to_owned);
    CliError {
        code: text("code").unwrap_or_else(|| "unknown".into()),
        message: text("message").unwrap_or_else(|| "The operation failed.".into()),
        field: text("field"),
        step: text("step"),
        reason: text("reason"),
        exit_code: error.get("exit_code").and_then(Value::as_i64),
        diagnostic: text("diagnostic"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotMode {
    Region,
    Full,
}

impl ShotMode {
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Region => "region",
            Self::Full => "full",
        }
    }
}

/// Why a profile file cannot be used (`problem` in the catalog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProblem {
    pub code: String,
    pub field: Option<String>,
    pub key: Option<String>,
}

impl ProfileProblem {
    pub fn describe(&self) -> String {
        match self.code.as_str() {
            "dynamic_syntax" => format!(
                "{} uses shell syntax that is never evaluated",
                self.key.as_deref().unwrap_or("A field")
            ),
            "invalid_field" => format!("{} is invalid", field_label(self.field.as_deref())),
            "unreadable" => "file is a link or cannot be read".into(),
            _ => "cannot be used".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub usable: bool,
    pub active: bool,
    pub label: String,
    pub host: String,
    pub remote_home: String,
    pub remote_dir: String,
    pub shot_mode: ShotMode,
    pub restore_seconds: u32,
    pub editable: bool,
    pub problem: Option<ProfileProblem>,
}

impl Profile {
    pub fn display_name(&self) -> &str {
        if self.usable { &self.label } else { &self.id }
    }

    pub fn remote_root(&self) -> String {
        format!(
            "{}/{}",
            self.remote_home.trim_end_matches('/'),
            self.remote_dir.trim_end_matches('/')
        )
    }

    /// `Label  (host)`, as in the tray's Destination menu.
    pub fn menu_label(&self) -> String {
        format!("{}  ({})", self.label, self.host)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalog {
    pub configured: bool,
    pub config_root: String,
    pub active_profile_id: Option<String>,
    pub profiles: Vec<Profile>,
}

impl Catalog {
    pub fn active(&self) -> Option<&Profile> {
        let id = self.active_profile_id.as_deref()?;
        self.profiles
            .iter()
            .find(|profile| profile.id == id && profile.usable)
    }

    pub fn usable(&self) -> impl Iterator<Item = &Profile> {
        self.profiles.iter().filter(|profile| profile.usable)
    }

    pub fn find(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    fn from_json(result: &Value) -> Self {
        let profiles = result
            .get("profiles")
            .and_then(Value::as_array)
            .map(|profiles| profiles.iter().map(Profile::from_json).collect())
            .unwrap_or_default();
        Self {
            configured: result.get("configured").and_then(Value::as_bool) == Some(true),
            config_root: string(result, "config_root").unwrap_or_default(),
            active_profile_id: string(result, "active_profile_id"),
            profiles,
        }
    }
}

impl Profile {
    fn from_json(element: &Value) -> Self {
        let id = string(element, "id").unwrap_or_default();
        // The CLI reports restore_seconds as the profile's literal string.
        let restore_seconds = match element.get("restore_seconds") {
            Some(Value::String(text)) => text.parse().ok(),
            Some(Value::Number(number)) => number.as_u64().and_then(|n| u32::try_from(n).ok()),
            _ => None,
        };
        let problem = element
            .get("problem")
            .filter(|problem| problem.is_object())
            .map(|problem| ProfileProblem {
                code: string(problem, "code").unwrap_or_else(|| "unknown".into()),
                field: string(problem, "field"),
                key: string(problem, "key"),
            });
        Self {
            usable: element.get("usable").and_then(Value::as_bool) == Some(true),
            active: element.get("active").and_then(Value::as_bool) == Some(true),
            label: string(element, "label").unwrap_or_else(|| id.clone()),
            host: string(element, "host").unwrap_or_default(),
            remote_home: string(element, "remote_home").unwrap_or_default(),
            remote_dir: string(element, "remote_dir").unwrap_or_default(),
            shot_mode: if string(element, "shot_mode").as_deref() == Some("full") {
                ShotMode::Full
            } else {
                ShotMode::Region
            },
            restore_seconds: restore_seconds.unwrap_or(DEFAULT_RESTORE_SECONDS),
            editable: element.get("editable").and_then(Value::as_bool) == Some(true),
            problem,
            id,
        }
    }
}

/// Mirrors `messages::field_label` so the model can describe problems.
fn field_label(field: Option<&str>) -> &'static str {
    crate::messages::field_label(field)
}

fn string(element: &Value, name: &str) -> Option<String> {
    element.get(name).and_then(Value::as_str).map(str::to_owned)
}

fn number(element: &Value, name: &str) -> i64 {
    element.get(name).and_then(Value::as_i64).unwrap_or(0)
}

pub fn catalog(runner: &impl CliRunner) -> Result<Catalog, CliError> {
    runner
        .run(&["profiles", "list", "--detailed"], LOCAL_TIMEOUT)
        .map(|result| Catalog::from_json(&result))
}

/// Editable profile fields; `None` leaves a field out of the command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileFields {
    pub label: Option<String>,
    pub host: Option<String>,
    pub remote_home: Option<String>,
    pub remote_dir: Option<String>,
    pub shot_mode: Option<ShotMode>,
    pub restore_seconds: Option<u32>,
}

impl ProfileFields {
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = Vec::new();
        let mut push = |flag: &str, value: Option<String>| {
            if let Some(value) = value {
                arguments.push(flag.to_owned());
                arguments.push(value);
            }
        };
        push("--label", self.label.clone());
        push("--host", self.host.clone());
        push("--remote-home", self.remote_home.clone());
        push("--remote-dir", self.remote_dir.clone());
        push(
            "--shot-mode",
            self.shot_mode.map(|mode| mode.as_arg().to_owned()),
        );
        push(
            "--restore-seconds",
            self.restore_seconds.map(|seconds| seconds.to_string()),
        );
        arguments
    }
}

fn run_owned(
    runner: &impl CliRunner,
    arguments: Vec<String>,
    timeout: Duration,
) -> Result<Value, CliError> {
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    runner.run(&borrowed, timeout)
}

/// Creates a profile; returns whether it became the active destination.
pub fn create_profile(
    runner: &impl CliRunner,
    id: &str,
    fields: &ProfileFields,
) -> Result<bool, CliError> {
    let mut arguments = vec!["profiles".into(), "create".into(), id.to_owned()];
    arguments.extend(fields.arguments());
    let result = run_owned(runner, arguments, LOCAL_TIMEOUT)?;
    Ok(result.get("activated").and_then(Value::as_bool) == Some(true))
}

pub fn update_profile(
    runner: &impl CliRunner,
    id: &str,
    fields: &ProfileFields,
) -> Result<(), CliError> {
    let mut arguments = vec!["profiles".into(), "update".into(), id.to_owned()];
    arguments.extend(fields.arguments());
    run_owned(runner, arguments, LOCAL_TIMEOUT).map(drop)
}

pub fn rename_profile(runner: &impl CliRunner, old: &str, new: &str) -> Result<(), CliError> {
    runner
        .run(&["profiles", "rename", old, new], LOCAL_TIMEOUT)
        .map(drop)
}

pub fn use_profile(runner: &impl CliRunner, id: &str) -> Result<(), CliError> {
    runner
        .run(&["profiles", "use", id], LOCAL_TIMEOUT)
        .map(drop)
}

pub fn delete_profile(
    runner: &impl CliRunner,
    id: &str,
    switch_to: Option<&str>,
) -> Result<(), CliError> {
    match switch_to {
        Some(other) => runner.run(
            &["profiles", "delete", id, "--switch-to", other],
            LOCAL_TIMEOUT,
        ),
        None => runner.run(&["profiles", "delete", id], LOCAL_TIMEOUT),
    }
    .map(drop)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionTest {
    pub host: String,
    pub configured_home: String,
    pub detected_home: String,
    pub home_matches: bool,
}

pub fn test_profile(runner: &impl CliRunner, id: &str) -> Result<ConnectionTest, CliError> {
    let result = runner.run(&["profiles", "test", id], PROBE_TIMEOUT)?;
    Ok(ConnectionTest {
        host: string(&result, "host").unwrap_or_default(),
        configured_home: string(&result, "remote_home").unwrap_or_default(),
        detected_home: string(&result, "detected_remote_home").unwrap_or_default(),
        home_matches: result.get("remote_home_matches").and_then(Value::as_bool) == Some(true),
    })
}

/// Connects to `host` and returns the home directory it reports.
pub fn probe_host(runner: &impl CliRunner, host: &str) -> Result<String, CliError> {
    let result = runner.run(&["hosts", "probe", "--host", host], PROBE_TIMEOUT)?;
    Ok(string(&result, "detected_remote_home").unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUpload {
    pub name: String,
    pub size_bytes: i64,
    pub modified_epoch_seconds: i64,
    pub remote_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UploadListing {
    pub remote_directory: String,
    pub uploads: Vec<RemoteUpload>,
    pub skipped_rows: i64,
    pub truncated: bool,
}

pub fn list_uploads(runner: &impl CliRunner, profile_id: &str) -> Result<UploadListing, CliError> {
    let result = runner.run(&["uploads", "list", "--profile", profile_id], LIST_TIMEOUT)?;
    let uploads = result
        .get("uploads")
        .and_then(Value::as_array)
        .map(|uploads| {
            uploads
                .iter()
                .map(|upload| RemoteUpload {
                    name: string(upload, "name").unwrap_or_default(),
                    size_bytes: number(upload, "size_bytes"),
                    modified_epoch_seconds: number(upload, "modified_epoch_seconds"),
                    remote_path: string(upload, "remote_path").unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(UploadListing {
        remote_directory: string(&result, "remote_directory").unwrap_or_default(),
        uploads,
        skipped_rows: number(&result, "skipped_rows"),
        truncated: result.get("truncated").and_then(Value::as_bool) == Some(true),
    })
}

/// Downloads one upload and returns the local file the CLI wrote.
pub fn fetch_upload(
    runner: &impl CliRunner,
    profile_id: &str,
    name: &str,
) -> Result<String, CliError> {
    let result = runner.run(
        &["uploads", "fetch", "--profile", profile_id, "--name", name],
        FETCH_TIMEOUT,
    )?;
    Ok(string(&result, "local_path").unwrap_or_default())
}

/// Deletes every uploaded PNG on the destination; returns how many.
pub fn clean_uploads(runner: &impl CliRunner, profile_id: &str) -> Result<i64, CliError> {
    let result = runner.run(
        &[
            "uploads",
            "clean",
            "--profile",
            profile_id,
            "--confirm-profile",
            profile_id,
        ],
        CLEAN_TIMEOUT,
    )?;
    Ok(number(&result, "deleted_count"))
}

/// Uploads one private PNG and returns the remote path. The profile is named
/// twice so the CLI refuses to fall back to a different active destination.
pub fn upload_file(
    runner: &impl CliRunner,
    source: &Path,
    profile_id: &str,
) -> Result<String, CliError> {
    let source = source
        .to_str()
        .ok_or_else(|| CliError::new("source_not_png", "The staging path is not valid UTF-8."))?;
    let result = runner.run(
        &[
            "upload-file",
            "--source",
            source,
            "--profile",
            profile_id,
            "--confirm-profile",
            profile_id,
        ],
        UPLOAD_TIMEOUT,
    )?;
    string(&result, "remote_path")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            CliError::new(
                "cli_protocol",
                "The command-line component did not report the remote path.",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn success_document_returns_result() {
        let result = parse_document(
            r#"{"protocol_version":1,"ok":true,"result":{"type":"upload","remote_path":"/home/me/img/a.png"}}"#,
            Some(0),
        )
        .expect("success");
        assert_eq!(result["remote_path"], "/home/me/img/a.png");
    }

    #[test]
    fn error_document_keeps_structured_fields() {
        let error = parse_document(
            r#"{"protocol_version":1,"ok":false,"error":{"code":"upload_failed","message":"Upload failed.","step":"upload","reason":"exit","exit_code":1,"diagnostic":"Permission denied (publickey)."}}"#,
            Some(1),
        )
        .expect_err("failure");
        assert_eq!(error.code, "upload_failed");
        assert_eq!(error.step.as_deref(), Some("upload"));
        assert_eq!(error.exit_code, Some(1));
        assert_eq!(
            error.diagnostic.as_deref(),
            Some("Permission denied (publickey).")
        );
    }

    #[test]
    fn rejects_other_protocols_and_garbage() {
        for (stdout, exit) in [
            ("not json", Some(0)),
            (r#"{"protocol_version":2,"ok":true,"result":{}}"#, Some(0)),
            (r#"{"ok":true,"result":{}}"#, Some(0)),
            // ok:true with a failing exit status is inconsistent.
            (r#"{"protocol_version":1,"ok":true,"result":{}}"#, Some(1)),
        ] {
            let error = parse_document(stdout, exit).expect_err(stdout);
            assert_eq!(error.code, "cli_protocol", "{stdout}");
        }
    }

    #[test]
    fn catalog_picks_only_a_usable_active_profile() {
        let catalog = Catalog::from_json(&json!({
            "configured": true,
            "active_profile_id": "work",
            "profiles": [
                {"id": "broken", "usable": false, "active": false, "problem": {"code": "unreadable"}},
                {"id": "work", "usable": true, "active": true, "label": "Work VPS", "host": "work",
                 "shot_mode": "full", "restore_seconds": "15"}
            ]
        }));
        let active = catalog.active().expect("active profile");
        assert_eq!(active.label, "Work VPS");
        assert_eq!(active.shot_mode, ShotMode::Full);
        assert_eq!(active.restore_seconds, 15);
        assert_eq!(catalog.profiles[0].label, "broken");

        let unusable = Catalog::from_json(&json!({
            "configured": true,
            "active_profile_id": "broken",
            "profiles": [{"id": "broken", "usable": false}]
        }));
        assert!(unusable.active().is_none());
    }

    #[test]
    fn restore_seconds_defaults_when_absent_or_invalid() {
        let profile =
            Profile::from_json(&json!({"id": "a", "usable": true, "restore_seconds": "soon"}));
        assert_eq!(profile.restore_seconds, DEFAULT_RESTORE_SECONDS);
        let profile = Profile::from_json(&json!({"id": "a", "usable": true, "restore_seconds": 0}));
        assert_eq!(profile.restore_seconds, 0);
    }

    struct Recording(std::cell::RefCell<Vec<Vec<String>>>, Value);

    impl CliRunner for Recording {
        fn run(&self, arguments: &[&str], _timeout: Duration) -> Result<Value, CliError> {
            self.0.borrow_mut().push(
                arguments
                    .iter()
                    .map(|argument| argument.to_string())
                    .collect(),
            );
            Ok(self.1.clone())
        }
    }

    #[test]
    fn upload_confirms_the_profile_it_targets() {
        let runner = Recording(
            Default::default(),
            json!({"remote_path": "/home/me/img/x.png"}),
        );
        let path = upload_file(&runner, Path::new("/run/user/1000/s/upload-1.png"), "work")
            .expect("upload");
        assert_eq!(path, "/home/me/img/x.png");
        assert_eq!(
            runner.0.borrow()[0],
            [
                "upload-file",
                "--source",
                "/run/user/1000/s/upload-1.png",
                "--profile",
                "work",
                "--confirm-profile",
                "work"
            ]
        );
    }

    #[test]
    fn upload_without_remote_path_is_a_protocol_error() {
        let runner = Recording(Default::default(), json!({"type": "upload"}));
        let error = upload_file(&runner, Path::new("/tmp/a.png"), "work").expect_err("no path");
        assert_eq!(error.code, "cli_protocol");
    }

    #[test]
    fn locate_prefers_an_absolute_override_and_ignores_relative_ones() {
        let env = |name: &str| match name {
            CLI_PATH_ENV => Some(OsString::from("/opt/sip/ssh-img-paste")),
            CONFIG_ROOT_ENV => Some(OsString::from("relative/root")),
            _ => None,
        };
        let client = CliClient::locate(env, None).expect("override");
        assert_eq!(client.executable(), Path::new("/opt/sip/ssh-img-paste"));
        assert_eq!(client.config_root, None);

        let missing = CliClient::locate(|_| None, None).expect_err("nothing to find");
        assert_eq!(missing.code, "cli_missing");
    }

    #[test]
    fn client_passes_config_root_before_the_command() {
        let directory = tempfile::tempdir().expect("tempdir");
        let script = directory.path().join("ssh-img-paste");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '{\"protocol_version\":1,\"ok\":true,\"result\":{\"args\":\"%s\"}}' \"$*\"\n",
        )
        .expect("write fake cli");
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .expect("chmod");
        let client = CliClient::new(script, Some(PathBuf::from("/srv/root")));
        let result = client
            .run(&["version"], Duration::from_secs(5))
            .expect("fake cli");
        assert_eq!(result["args"], "--config-root /srv/root version");
    }

    #[test]
    fn catalog_reads_the_detail_fields_and_problems() {
        let catalog = Catalog::from_json(&json!({
            "configured": true,
            "config_root": "/home/me/.config/ssh-img-paste",
            "active_profile_id": "work",
            "profiles": [
                {"id": "work", "usable": true, "active": true, "label": "Work", "host": "w",
                 "remote_home": "/home/me/", "remote_dir": "img-uploads/", "editable": true},
                {"id": "manual", "usable": false, "active": false,
                 "problem": {"code": "dynamic_syntax", "key": "SSH_HOST"}}
            ]
        }));
        assert_eq!(catalog.config_root, "/home/me/.config/ssh-img-paste");
        let work = catalog.find("work").expect("work");
        assert_eq!(work.remote_root(), "/home/me/img-uploads");
        assert_eq!(work.menu_label(), "Work  (w)");
        assert!(work.editable);
        let manual = catalog.find("manual").expect("manual");
        assert_eq!(manual.display_name(), "manual");
        assert_eq!(
            manual
                .problem
                .as_ref()
                .map(ProfileProblem::describe)
                .as_deref(),
            Some("SSH_HOST uses shell syntax that is never evaluated")
        );
        assert_eq!(catalog.usable().count(), 1);
    }

    #[test]
    fn profile_fields_emit_only_present_flags() {
        let fields = ProfileFields {
            label: Some("Work VPS".into()),
            shot_mode: Some(ShotMode::Full),
            restore_seconds: Some(0),
            ..Default::default()
        };
        assert_eq!(
            fields.arguments(),
            [
                "--label",
                "Work VPS",
                "--shot-mode",
                "full",
                "--restore-seconds",
                "0"
            ]
        );
        assert!(ProfileFields::default().arguments().is_empty());
    }

    #[test]
    fn mutations_send_the_documented_arguments() {
        let runner = Recording(
            Default::default(),
            json!({"activated": true, "deleted_count": 3,
            "detected_remote_home": "/home/me", "local_path": "/tmp/a.png"}),
        );
        let fields = ProfileFields {
            host: Some("work".into()),
            ..Default::default()
        };
        assert!(create_profile(&runner, "work", &fields).expect("create"));
        update_profile(&runner, "work", &fields).expect("update");
        rename_profile(&runner, "work", "job").expect("rename");
        use_profile(&runner, "job").expect("use");
        delete_profile(&runner, "job", Some("home")).expect("delete");
        assert_eq!(probe_host(&runner, "work").expect("probe"), "/home/me");
        assert_eq!(
            fetch_upload(&runner, "job", "a.png").expect("fetch"),
            "/tmp/a.png"
        );
        assert_eq!(clean_uploads(&runner, "job").expect("clean"), 3);
        let calls: Vec<String> = runner
            .0
            .borrow()
            .iter()
            .map(|call| call.join(" "))
            .collect();
        assert_eq!(
            calls,
            [
                "profiles create work --host work",
                "profiles update work --host work",
                "profiles rename work job",
                "profiles use job",
                "profiles delete job --switch-to home",
                "hosts probe --host work",
                "uploads fetch --profile job --name a.png",
                "uploads clean --profile job --confirm-profile job",
            ]
        );
    }

    #[test]
    fn upload_listing_keeps_cli_order_and_notices() {
        let runner = Recording(
            Default::default(),
            json!({
                "remote_directory": "/home/me/img-uploads",
                "uploads": [
                    {"name": "b.png", "size_bytes": 2048, "modified_epoch_seconds": 20, "remote_path": "/home/me/img-uploads/b.png"},
                    {"name": "a.png", "size_bytes": 10, "modified_epoch_seconds": 10, "remote_path": "/home/me/img-uploads/a.png"}
                ],
                "skipped_rows": 1,
                "truncated": true
            }),
        );
        let listing = list_uploads(&runner, "work").expect("list");
        assert_eq!(listing.uploads[0].name, "b.png");
        assert_eq!(listing.uploads[1].size_bytes, 10);
        assert_eq!(listing.skipped_rows, 1);
        assert!(listing.truncated);
    }
}
