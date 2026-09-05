//! Contract-first core for Windows and Linux SSH Image Paste editions.

#[cfg(test)]
mod process_executor_tests;

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
    program: String,
    arguments: Vec<std::ffi::OsString>,
}

impl CommandPlan {
    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn arguments(&self) -> &[std::ffi::OsString] {
        &self.arguments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadPlan {
    mkdir: CommandPlan,
    upload: CommandPlan,
    finalize: CommandPlan,
    remote_path: String,
}

impl UploadPlan {
    pub fn mkdir(&self) -> &CommandPlan {
        &self.mkdir
    }

    pub fn upload(&self) -> &CommandPlan {
        &self.upload
    }

    pub fn finalize(&self) -> &CommandPlan {
        &self.finalize
    }

    pub fn remote_path(&self) -> &str {
        &self.remote_path
    }
}

/// Execution budgets passed to a platform command executor.
///
/// `command_timeout` applies independently to each of the three upload steps,
/// so the full upload may consume roughly three times this duration. A zero
/// duration requests an immediate timeout without spawning the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionPolicy {
    pub command_timeout: std::time::Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupTrigger {
    Timeout,
    Cancelled,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandFailure {
    Spawn {
        message: String,
    },
    Exit {
        code: Option<i32>,
        stderr: String,
    },
    Timeout,
    Cancelled,
    Adapter {
        message: String,
    },
    Cleanup {
        trigger: CleanupTrigger,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    Success,
    Failure(CommandFailure),
}

/// Platform boundary for executing already-validated command plans.
///
/// Implementations own wall-clock enforcement, cancellation observation,
/// bounded pipe draining, and best-available OS containment for the fixed,
/// validated OpenSSH programs in an `UploadPlan`. `Timeout` and `Cancelled`
/// mean the platform accepted teardown and the worker completed within a bounded
/// cleanup grace; `Cleanup` means completion could not be confirmed in that
/// grace. A reported `process_group` mechanism cannot contain a descendant that
/// deliberately calls `setsid`, so callers must not generalize this private
/// executor into an arbitrary untrusted-command sandbox.
pub trait CommandExecutor {
    fn execute(&mut self, command: &CommandPlan, timeout: std::time::Duration) -> CommandOutcome;
}

#[derive(Debug, Default)]
struct CancellationState {
    requested_at: std::sync::Mutex<Option<std::time::Instant>>,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(std::sync::Arc<CancellationState>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        let mut requested_at = self.0.requested_at.lock().expect("cancellation lock");
        requested_at.get_or_insert_with(std::time::Instant::now);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled_at().is_some()
    }

    fn cancelled_at(&self) -> Option<std::time::Instant> {
        *self.0.requested_at.lock().expect("cancellation lock")
    }
}

#[derive(Debug)]
struct ProcessExecutor {
    cancellation: CancellationToken,
    max_output_bytes: usize,
}

impl ProcessExecutor {
    fn new(cancellation: CancellationToken, max_output_bytes: usize) -> Self {
        Self {
            cancellation,
            max_output_bytes,
        }
    }
}

enum ProcessWorkerOutcome {
    Process(Result<processkit::ProcessResult<String>, processkit::Error>),
    Runtime(String),
}

struct ProcessWorkerCompletion {
    outcome: ProcessWorkerOutcome,
    completed_at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestedStop {
    Timeout,
    Cancelled,
}

impl RequestedStop {
    fn trigger(self) -> CleanupTrigger {
        match self {
            Self::Timeout => CleanupTrigger::Timeout,
            Self::Cancelled => CleanupTrigger::Cancelled,
        }
    }

    fn failure(self) -> CommandFailure {
        match self {
            Self::Timeout => CommandFailure::Timeout,
            Self::Cancelled => CommandFailure::Cancelled,
        }
    }
}

fn requested_stop_before_completion(
    cancelled_at: Option<std::time::Instant>,
    deadline: Option<std::time::Instant>,
    completed_at: std::time::Instant,
) -> Option<RequestedStop> {
    let cancellation_reached = cancelled_at.filter(|instant| *instant <= completed_at);
    let deadline_reached = deadline.filter(|instant| *instant <= completed_at);
    match (cancellation_reached, deadline_reached) {
        (Some(cancelled), Some(deadline)) if cancelled <= deadline => {
            Some(RequestedStop::Cancelled)
        }
        (Some(_), Some(_)) => Some(RequestedStop::Timeout),
        (Some(_), None) => Some(RequestedStop::Cancelled),
        (None, Some(_)) => Some(RequestedStop::Timeout),
        (None, None) => None,
    }
}

fn cleanup_trigger_at(
    cancelled_at: Option<std::time::Instant>,
    deadline: Option<std::time::Instant>,
    cleanup_expired_at: std::time::Instant,
    observed_stop: RequestedStop,
) -> CleanupTrigger {
    requested_stop_before_completion(cancelled_at, deadline, cleanup_expired_at)
        .unwrap_or(observed_stop)
        .trigger()
}

fn reattribute_cleanup_failure(
    failure: CommandFailure,
    cancelled_at: Option<std::time::Instant>,
    deadline: Option<std::time::Instant>,
    completed_at: std::time::Instant,
) -> CommandFailure {
    match failure {
        CommandFailure::Cleanup { trigger, message } => CommandFailure::Cleanup {
            trigger: requested_stop_before_completion(cancelled_at, deadline, completed_at)
                .map(RequestedStop::trigger)
                .unwrap_or(trigger),
            message,
        },
        other => other,
    }
}

impl CommandExecutor for ProcessExecutor {
    fn execute(&mut self, command: &CommandPlan, timeout: std::time::Duration) -> CommandOutcome {
        const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);
        const CLEANUP_GRACE: std::time::Duration = std::time::Duration::from_secs(2);
        const CAPTURE_LIMIT: usize = 64 * 1024;

        if timeout.is_zero() {
            return CommandOutcome::Failure(CommandFailure::Timeout);
        }
        if self.cancellation.is_cancelled() {
            return CommandOutcome::Failure(CommandFailure::Cancelled);
        }

        let started_at = std::time::Instant::now();
        let deadline = started_at.checked_add(timeout);
        let program = command.program.clone();
        let arguments = command.arguments.clone();
        let process_cancellation = processkit::CancellationToken::new();
        let worker_cancellation = process_cancellation.clone();
        let worker = match std::thread::Builder::new()
            .name("ssh-img-paste-process".into())
            .spawn(move || {
                let outcome = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => {
                        let process = processkit::Command::new(program)
                            .args(arguments)
                            .no_timeout()
                            .cancel_on(worker_cancellation)
                            .output_buffer(
                                processkit::OutputBufferPolicy::unbounded()
                                    .with_max_bytes(CAPTURE_LIMIT),
                            );
                        let result = runtime.block_on(process.output_string());
                        drop(runtime);
                        ProcessWorkerOutcome::Process(result)
                    }
                    Err(error) => ProcessWorkerOutcome::Runtime(error.to_string()),
                };
                ProcessWorkerCompletion {
                    outcome,
                    completed_at: std::time::Instant::now(),
                }
            }) {
            Ok(worker) => worker,
            Err(error) => {
                return CommandOutcome::Failure(CommandFailure::Adapter {
                    message: bounded_text(error.to_string().as_bytes(), self.max_output_bytes),
                });
            }
        };

        loop {
            if worker.is_finished() {
                return finish_completed_worker(
                    worker,
                    self.cancellation.cancelled_at(),
                    deadline,
                    self.max_output_bytes,
                );
            }

            let requested_stop = if self.cancellation.is_cancelled() {
                Some(RequestedStop::Cancelled)
            } else if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
                Some(RequestedStop::Timeout)
            } else {
                None
            };
            if let Some(requested_stop) = requested_stop {
                process_cancellation.cancel();
                return wait_for_stopped_worker(
                    worker,
                    requested_stop,
                    self.cancellation.clone(),
                    deadline,
                    CLEANUP_GRACE,
                    self.max_output_bytes,
                );
            }

            let remaining = deadline
                .map(|deadline| deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap_or(POLL_INTERVAL);
            std::thread::sleep(remaining.min(POLL_INTERVAL));
        }
    }
}

fn finish_completed_worker(
    worker: std::thread::JoinHandle<ProcessWorkerCompletion>,
    cancelled_at: Option<std::time::Instant>,
    deadline: Option<std::time::Instant>,
    limit: usize,
) -> CommandOutcome {
    let completion = match worker.join() {
        Ok(completion) => completion,
        Err(_) => {
            return CommandOutcome::Failure(CommandFailure::Cleanup {
                trigger: CleanupTrigger::Exit,
                message: "process worker panicked before cleanup confirmation".into(),
            });
        }
    };
    if let ProcessWorkerOutcome::Process(Err(error)) = &completion.outcome
        && error.is_teardown()
    {
        return CommandOutcome::Failure(reattribute_cleanup_failure(
            map_processkit_error(error, limit),
            cancelled_at,
            deadline,
            completion.completed_at,
        ));
    }
    if let Some(requested_stop) =
        requested_stop_before_completion(cancelled_at, deadline, completion.completed_at)
    {
        return CommandOutcome::Failure(requested_stop.failure());
    }
    map_worker_outcome(completion.outcome, limit)
}

fn wait_for_stopped_worker(
    worker: std::thread::JoinHandle<ProcessWorkerCompletion>,
    requested_stop: RequestedStop,
    cancellation: CancellationToken,
    deadline: Option<std::time::Instant>,
    cleanup_grace: std::time::Duration,
    limit: usize,
) -> CommandOutcome {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);
    let cleanup_deadline = std::time::Instant::now().checked_add(cleanup_grace);
    loop {
        if worker.is_finished() {
            return finish_completed_worker(worker, cancellation.cancelled_at(), deadline, limit);
        }
        let now = std::time::Instant::now();
        if cleanup_deadline.is_some_and(|cleanup_deadline| now >= cleanup_deadline) {
            return CommandOutcome::Failure(CommandFailure::Cleanup {
                trigger: cleanup_trigger_at(
                    cancellation.cancelled_at(),
                    deadline,
                    now,
                    requested_stop,
                ),
                message: "process cleanup was not confirmed within the bounded grace".into(),
            });
        }
        let remaining = cleanup_deadline
            .map(|deadline| deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap_or(POLL_INTERVAL);
        std::thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

fn map_worker_outcome(outcome: ProcessWorkerOutcome, limit: usize) -> CommandOutcome {
    match outcome {
        ProcessWorkerOutcome::Runtime(message) => {
            CommandOutcome::Failure(CommandFailure::Adapter {
                message: bounded_text(message.as_bytes(), limit),
            })
        }
        ProcessWorkerOutcome::Process(Ok(result)) if result.timed_out() => {
            CommandOutcome::Failure(CommandFailure::Timeout)
        }
        ProcessWorkerOutcome::Process(Ok(result)) if result.is_success() => CommandOutcome::Success,
        ProcessWorkerOutcome::Process(Ok(result)) => {
            CommandOutcome::Failure(CommandFailure::Exit {
                code: result.code(),
                stderr: bounded_text(result.stderr().as_bytes(), limit),
            })
        }
        ProcessWorkerOutcome::Process(Err(error)) => {
            CommandOutcome::Failure(map_processkit_error(&error, limit))
        }
    }
}

fn map_processkit_error(error: &processkit::Error, limit: usize) -> CommandFailure {
    match error.reason() {
        processkit::ErrorReason::Spawn { .. } | processkit::ErrorReason::NotFound { .. } => {
            CommandFailure::Spawn {
                message: bounded_text(error.to_string().as_bytes(), limit),
            }
        }
        processkit::ErrorReason::Cancelled { .. } => CommandFailure::Cancelled,
        processkit::ErrorReason::Timeout { .. } => CommandFailure::Timeout,
        processkit::ErrorReason::Teardown { cause, .. } => CommandFailure::Cleanup {
            trigger: match cause {
                processkit::TeardownCause::Cancellation => CleanupTrigger::Cancelled,
                processkit::TeardownCause::Timeout
                | processkit::TeardownCause::InactivityTimeout => CleanupTrigger::Timeout,
                _ => CleanupTrigger::Exit,
            },
            message: bounded_text(error.to_string().as_bytes(), limit),
        },
        processkit::ErrorReason::Exit { .. } | processkit::ErrorReason::Signalled { .. } => {
            let diagnostic = error
                .stderr()
                .map(str::to_owned)
                .unwrap_or_else(|| error.to_string());
            CommandFailure::Exit {
                code: error.code(),
                stderr: bounded_text(diagnostic.as_bytes(), limit),
            }
        }
        _ => CommandFailure::Adapter {
            message: bounded_text(error.to_string().as_bytes(), limit),
        },
    }
}

/// Reports the containment mechanism ProcessKit predicts for this host.
///
/// `process_group` is the Linux fallback and the macOS mechanism. It is suitable
/// for SSH Image Paste's fixed OpenSSH children, but cannot contain a descendant
/// that deliberately calls `setsid`; callers can surface this distinction.
pub fn process_containment_mechanism() -> &'static str {
    processkit::host_containment().mechanism().name()
}

fn bounded_text(bytes: &[u8], limit: usize) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    let mut output = String::with_capacity(limit.min(decoded.len()));
    for character in decoded.chars() {
        let safe = if character.is_control() || is_bidi_control(character) {
            ' '
        } else {
            character
        };
        if output.len().saturating_add(safe.len_utf8()) > limit {
            break;
        }
        output.push(safe);
    }
    output
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadExecution {
    pub remote_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStep {
    CreateRemoteDirectory,
    Upload,
    Finalize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadExecutionError {
    pub step: UploadStep,
    pub failure: CommandFailure,
}

pub fn execute_upload_plan(
    plan: &UploadPlan,
    executor: &mut impl CommandExecutor,
    policy: ExecutionPolicy,
) -> Result<UploadExecution, UploadExecutionError> {
    if policy.command_timeout.is_zero() {
        return Err(UploadExecutionError {
            step: UploadStep::CreateRemoteDirectory,
            failure: CommandFailure::Timeout,
        });
    }
    let steps = [
        (UploadStep::CreateRemoteDirectory, &plan.mkdir),
        (UploadStep::Upload, &plan.upload),
        (UploadStep::Finalize, &plan.finalize),
    ];
    for (step, command) in steps {
        if let CommandOutcome::Failure(failure) = executor.execute(command, policy.command_timeout)
        {
            return Err(UploadExecutionError { step, failure });
        }
    }
    Ok(UploadExecution {
        remote_path: plan.remote_path.clone(),
    })
}

pub fn execute_upload_plan_with_system(
    plan: &UploadPlan,
    policy: ExecutionPolicy,
    cancellation: CancellationToken,
    max_output_bytes: usize,
) -> Result<UploadExecution, UploadExecutionError> {
    let mut executor = ProcessExecutor::new(cancellation, max_output_bytes);
    execute_upload_plan(plan, &mut executor, policy)
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
