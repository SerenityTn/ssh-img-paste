//! Contract-first core for Windows and Linux SSH Image Paste editions.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidProfileId;

impl ProfileId {
    pub fn parse(value: &str) -> Result<Self, InvalidProfileId> {
        let mut chars = value.chars();
        let first = chars.next().ok_or(InvalidProfileId)?;
        if !first.is_ascii_alphanumeric()
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(InvalidProfileId);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileDocument {
    pub label: Option<String>,
    pub host: Option<String>,
    pub remote_home: Option<String>,
    pub remote_dir: Option<String>,
    pub shot_mode: Option<String>,
    pub restore_seconds: Option<String>,
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    DynamicSupportedAssignment(String),
}

pub fn parse_profile(input: &str) -> Result<ProfileDocument, ParseError> {
    let mut profile = ProfileDocument {
        editable: true,
        ..ProfileDocument::default()
    };

    for original in input.lines() {
        if original.is_empty() || original.starts_with('#') {
            continue;
        }

        let mut line = original;
        if let Some(rest) = line.strip_prefix("export ") {
            profile.editable = false;
            line = rest;
        }

        let Some((key, raw)) = line.split_once('=') else {
            profile.editable = false;
            continue;
        };
        if !valid_assignment_key(key) {
            profile.editable = false;
            continue;
        }

        let value = match parse_literal(raw) {
            Some(value) => value,
            None if supported_profile_key(key) => {
                return Err(ParseError::DynamicSupportedAssignment(key.to_owned()));
            }
            None => {
                profile.editable = false;
                continue;
            }
        };

        match key {
            "SSH_PROFILE_LABEL" => profile.label = Some(value),
            "SSH_HOST" => profile.host = Some(value),
            "SSH_REMOTE_HOME" => profile.remote_home = Some(value),
            "SSH_REMOTE_DIR" => profile.remote_dir = Some(value),
            "SSH_SHOT_MODE" => profile.shot_mode = Some(value),
            "SSH_CLIP_RESTORE_SECONDS" => profile.restore_seconds = Some(value),
            _ => {}
        }
    }

    Ok(profile)
}

fn valid_assignment_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn supported_profile_key(key: &str) -> bool {
    matches!(
        key,
        "SSH_PROFILE_LABEL"
            | "SSH_HOST"
            | "SSH_REMOTE_HOME"
            | "SSH_REMOTE_DIR"
            | "SSH_SHOT_MODE"
            | "SSH_CLIP_RESTORE_SECONDS"
    )
}

fn parse_literal(raw: &str) -> Option<String> {
    if raw.starts_with('"') && raw.ends_with('"') {
        return Some(unescape_double_quoted(&raw[1..raw.len() - 1]));
    }
    if raw.starts_with('\'') && raw.ends_with('\'') {
        let inner = &raw[1..raw.len() - 1];
        return (!inner.contains('\'')).then(|| inner.to_owned());
    }
    if raw.chars().any(|c| {
        matches!(
            c,
            '$' | '`'
                | ';'
                | '&'
                | '|'
                | '<'
                | '>'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '*'
                | '?'
                | '\\'
                | '\''
                | '"'
        )
    }) {
        return None;
    }
    Some(raw.to_owned())
}

fn unescape_double_quoted(inner: &str) -> String {
    let mut output = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                if matches!(next, '"' | '$' | '`' | '\\') {
                    output.push(next);
                } else {
                    output.push('\\');
                    output.push(next);
                }
            } else {
                output.push('\\');
            }
        } else {
            output.push(c);
        }
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedProfile {
    pub label: String,
    pub host: String,
    pub remote_home: String,
    pub remote_dir: String,
    pub shot_mode: Option<String>,
    pub restore_seconds: Option<String>,
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidField(&'static str),
}

pub fn validate_profile(
    id: &ProfileId,
    document: ProfileDocument,
) -> Result<ValidatedProfile, ValidationError> {
    let label = document
        .label
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| id.as_str().to_owned());
    let host = document.host.unwrap_or_default();
    let remote_home = document
        .remote_home
        .unwrap_or_else(|| "/home/user".to_owned());
    let remote_dir = document
        .remote_dir
        .unwrap_or_else(|| "img-uploads".to_owned());

    if has_control(&label) {
        return Err(ValidationError::InvalidField("label"));
    }
    if !valid_host(&host) {
        return Err(ValidationError::InvalidField("host"));
    }
    if !valid_absolute_path(&remote_home) {
        return Err(ValidationError::InvalidField("remote_home"));
    }
    if !valid_remote_dir(&remote_dir) {
        return Err(ValidationError::InvalidField("remote_dir"));
    }
    if document
        .shot_mode
        .as_deref()
        .is_some_and(|value| !matches!(value, "region" | "full") || has_control(value))
    {
        return Err(ValidationError::InvalidField("shot_mode"));
    }
    if document
        .restore_seconds
        .as_deref()
        .is_some_and(|value| !valid_restore_seconds(value))
    {
        return Err(ValidationError::InvalidField("restore_seconds"));
    }

    Ok(ValidatedProfile {
        label,
        host,
        remote_home,
        remote_dir,
        shot_mode: document.shot_mode,
        restore_seconds: document.restore_seconds,
        editable: document.editable,
    })
}

fn has_control(value: &str) -> bool {
    value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}

fn has_shell_meta(value: &str) -> bool {
    value.chars().any(|c| {
        matches!(
            c,
            '\'' | '"'
                | '`'
                | '$'
                | ';'
                | '&'
                | '|'
                | '<'
                | '>'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '*'
                | '?'
                | '\\'
        )
    })
}

fn valid_host(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | ':' | '-'))
}

fn safe_path_chars(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

fn valid_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains("//")
        && !value.split('/').any(|part| matches!(part, "." | ".."))
        && !has_control(value)
        && !has_shell_meta(value)
        && safe_path_chars(value)
}

fn valid_remote_dir(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('-')
        && value != "."
        && !value.contains("//")
        && !value.split('/').any(|part| matches!(part, "." | ".."))
        && !has_control(value)
        && !has_shell_meta(value)
        && safe_path_chars(value)
}

fn valid_restore_seconds(value: &str) -> bool {
    if value.is_empty() || has_control(value) || !value.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let normalized = value.trim_start_matches('0');
    let normalized = if normalized.is_empty() {
        "0"
    } else {
        normalized
    };
    normalized.len() < 5 || (normalized.len() == 5 && normalized <= "86400")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    pub program: String,
    pub arguments: Vec<std::ffi::OsString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadPlan {
    pub mkdir: CommandPlan,
    pub upload: CommandPlan,
    pub finalize: CommandPlan,
    pub remote_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    InvalidProfileField(&'static str),
    InvalidRemoteName,
    InvalidSource,
}

pub fn build_upload_plan(
    profile: &ValidatedProfile,
    source: &std::path::Path,
    remote_name: &str,
) -> Result<UploadPlan, PlanError> {
    revalidate_for_plan(profile)?;
    if !valid_local_source(source) {
        return Err(PlanError::InvalidSource);
    }
    if !valid_remote_name(remote_name) {
        return Err(PlanError::InvalidRemoteName);
    }

    let remote_home = profile.remote_home.trim_end_matches('/');
    let remote_dir = profile.remote_dir.trim_end_matches('/');
    let remote_root = if remote_home.is_empty() {
        format!("/{remote_dir}")
    } else {
        format!("{remote_home}/{remote_dir}")
    };
    let remote_path = format!("{remote_root}/{remote_name}");
    let partial_path = format!("{remote_root}/.{remote_name}.partial");

    let mkdir = ssh_plan(&profile.host, format!("mkdir -p -- {remote_root}"));
    let upload = CommandPlan {
        program: "scp".to_owned(),
        arguments: vec![
            "-q".into(),
            "-B".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "ConnectTimeout=6".into(),
            "--".into(),
            source.as_os_str().to_owned(),
            format!("{}:{partial_path}", profile.host).into(),
        ],
    };
    let finalize = ssh_plan(&profile.host, format!("mv -- {partial_path} {remote_path}"));

    Ok(UploadPlan {
        mkdir,
        upload,
        finalize,
        remote_path,
    })
}

fn ssh_plan(host: &str, remote_command: String) -> CommandPlan {
    CommandPlan {
        program: "ssh".to_owned(),
        arguments: vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "ConnectTimeout=6".into(),
            host.into(),
            remote_command.into(),
        ],
    }
}

fn revalidate_for_plan(profile: &ValidatedProfile) -> Result<(), PlanError> {
    if has_control(&profile.label) {
        return Err(PlanError::InvalidProfileField("label"));
    }
    if !valid_host(&profile.host) || profile.host.contains(':') {
        return Err(PlanError::InvalidProfileField("host"));
    }
    if !valid_absolute_path(&profile.remote_home) {
        return Err(PlanError::InvalidProfileField("remote_home"));
    }
    if !valid_remote_dir(&profile.remote_dir) {
        return Err(PlanError::InvalidProfileField("remote_dir"));
    }
    if profile
        .shot_mode
        .as_deref()
        .is_some_and(|value| !matches!(value, "region" | "full") || has_control(value))
    {
        return Err(PlanError::InvalidProfileField("shot_mode"));
    }
    if profile
        .restore_seconds
        .as_deref()
        .is_some_and(|value| !valid_restore_seconds(value))
    {
        return Err(PlanError::InvalidProfileField("restore_seconds"));
    }
    Ok(())
}

fn valid_local_source(source: &std::path::Path) -> bool {
    source.is_absolute() && !source.as_os_str().to_string_lossy().starts_with('-')
}

fn valid_remote_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value.ends_with(".png")
        && !value.contains("..")
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardTransaction {
    generation: u128,
    expected_text: String,
    ownership_marker: Option<u64>,
}

#[derive(Debug, Default)]
pub struct ClipboardCoordinator {
    next_generation: u128,
    active_generation: Option<u128>,
}

impl ClipboardCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(
        &mut self,
        expected_text: impl Into<String>,
        ownership_marker: Option<u64>,
    ) -> ClipboardTransaction {
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("clipboard transaction generation exhausted");
        self.active_generation = Some(self.next_generation);
        ClipboardTransaction {
            generation: self.next_generation,
            expected_text: expected_text.into(),
            ownership_marker,
        }
    }

    pub fn should_restore(
        &self,
        transaction: &ClipboardTransaction,
        current_text: Option<&str>,
        current_ownership_marker: Option<u64>,
    ) -> bool {
        let marker_matches = transaction
            .ownership_marker
            .is_some_and(|expected| current_ownership_marker == Some(expected));
        self.active_generation == Some(transaction.generation)
            && marker_matches
            && current_text == Some(transaction.expected_text.as_str())
    }

    pub fn complete(&mut self, transaction: &ClipboardTransaction) {
        if self.active_generation == Some(transaction.generation) {
            self.active_generation = None;
        }
    }

    pub fn cancel(&mut self, transaction: &ClipboardTransaction) {
        self.complete(transaction);
    }
}
