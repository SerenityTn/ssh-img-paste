//! Bounded child processes for the CLI and the clipboard tools.
//!
//! Every program runs from an argument array (never a shell), in its own
//! process group, with a wall-clock limit and bounded output capture. On a
//! timeout the whole group receives `SIGTERM`, then `SIGKILL` after a short
//! grace, so the CLI can tear down its own `ssh`/`scp` children first.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const TERMINATE_GRACE: Duration = Duration::from_secs(3);
const READER_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Capture stdout and stderr up to `max_output_bytes` each.
    Capture,
    /// Discard output. Required for `wl-copy` and `xclip -i`, which fork a
    /// background owner of the selection that would otherwise hold the pipes
    /// open after the foreground process exits.
    Discard,
}

#[derive(Debug, Clone)]
pub struct ProcessRequest {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub output: OutputMode,
}

impl ProcessRequest {
    pub fn new(program: impl Into<PathBuf>, arguments: &[&str], timeout: Duration) -> Self {
        Self {
            program: program.into(),
            arguments: arguments.iter().map(OsString::from).collect(),
            stdin: None,
            timeout,
            max_output_bytes: 64 * 1024,
            output: OutputMode::Capture,
        }
    }
}

#[derive(Debug)]
pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// True when stdout exceeded the capture bound and was cut short.
    pub stdout_truncated: bool,
}

#[derive(Debug)]
pub enum ProcessError {
    Spawn(std::io::Error),
    Timeout,
    Wait(std::io::Error),
}

pub fn run(request: &ProcessRequest) -> Result<ProcessOutput, ProcessError> {
    let mut command = Command::new(&request.program);
    command
        .args(&request.arguments)
        .stdin(if request.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .process_group(0);
    match request.output {
        OutputMode::Capture => command.stdout(Stdio::piped()).stderr(Stdio::piped()),
        OutputMode::Discard => command.stdout(Stdio::null()).stderr(Stdio::null()),
    };

    let mut child = command.spawn().map_err(ProcessError::Spawn)?;
    let writer = request.stdin.clone().and_then(|bytes| {
        let mut stdin = child.stdin.take()?;
        Some(thread::spawn(move || {
            // A child that exits without reading reports EPIPE; its exit
            // status carries the real outcome.
            let _ = stdin.write_all(&bytes);
        }))
    });
    let stdout = child
        .stdout
        .take()
        .map(|pipe| spawn_reader(pipe, request.max_output_bytes));
    let stderr = child
        .stderr
        .take()
        .map(|pipe| spawn_reader(pipe, request.max_output_bytes));

    let status = wait_with_deadline(&mut child, request.timeout)?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let (stdout, stdout_truncated) = collect(stdout);
    let (stderr, _) = collect(stderr);
    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
    })
}

fn wait_with_deadline(child: &mut Child, timeout: Duration) -> Result<ExitStatus, ProcessError> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                terminate_group(child);
                return Err(ProcessError::Timeout);
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => {
                terminate_group(child);
                return Err(ProcessError::Wait(error));
            }
        }
    }
}

fn terminate_group(child: &mut Child) {
    let Ok(group) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    // SAFETY: `kill` has no memory-safety preconditions; a negative pid names
    // the process group this module created for the child.
    unsafe {
        libc::kill(-group, libc::SIGTERM);
    }
    let grace_ends = Instant::now() + TERMINATE_GRACE;
    while Instant::now() < grace_ends {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        thread::sleep(POLL_INTERVAL);
    }
    // SAFETY: as above. Members that already exited are unaffected.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
    let _ = child.wait();
}

type ReaderResult = mpsc::Receiver<(Vec<u8>, bool)>;

fn spawn_reader(mut pipe: impl Read + Send + 'static, limit: usize) -> ReaderResult {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let room = limit.saturating_sub(kept.len());
                    if read > room {
                        truncated = true;
                    }
                    kept.extend_from_slice(&buffer[..read.min(room)]);
                }
            }
        }
        let _ = sender.send((kept, truncated));
    });
    receiver
}

/// A descendant that escaped the group can keep a pipe open after the child
/// exits; give up on its output instead of blocking the caller.
fn collect(reader: Option<ReaderResult>) -> (Vec<u8>, bool) {
    reader
        .and_then(|receiver| receiver.recv_timeout(READER_GRACE).ok())
        .unwrap_or_default()
}

/// Finds an executable by name on `PATH`.
pub fn find_on_path(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    std::env::split_paths(path?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(name))
        .find(|candidate| {
            candidate.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str, timeout: Duration) -> ProcessRequest {
        ProcessRequest::new("/bin/sh", &["-c", script], timeout)
    }

    #[test]
    fn captures_output_and_exit_status() {
        let output = run(&shell(
            "printf out; printf err >&2; exit 3",
            Duration::from_secs(5),
        ))
        .expect("process runs");
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
        assert_eq!(output.status.code(), Some(3));
        assert!(!output.stdout_truncated);
    }

    #[test]
    fn feeds_stdin() {
        let mut request = ProcessRequest::new("/bin/cat", &[], Duration::from_secs(5));
        request.stdin = Some(b"remote/path.png".to_vec());
        assert_eq!(run(&request).expect("cat runs").stdout, b"remote/path.png");
    }

    #[test]
    fn bounds_captured_output() {
        let mut request = shell("head -c 100000 /dev/zero", Duration::from_secs(5));
        request.max_output_bytes = 1000;
        let output = run(&request).expect("process runs");
        assert_eq!(output.stdout.len(), 1000);
        assert!(output.stdout_truncated);
    }

    #[test]
    fn timeout_stops_the_whole_process_group() {
        let started = Instant::now();
        let result = run(&shell(
            "sleep 30 & sleep 30; wait",
            Duration::from_millis(200),
        ));
        assert!(matches!(result, Err(ProcessError::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let result = run(&ProcessRequest::new(
            "/nonexistent/ssh-img-paste",
            &[],
            Duration::from_secs(1),
        ));
        assert!(matches!(result, Err(ProcessError::Spawn(_))));
    }

    #[test]
    fn path_lookup_ignores_relative_entries() {
        let path = std::ffi::OsString::from(".:/bin:/usr/bin");
        assert!(find_on_path("sh", Some(&path)).is_some_and(|found| found.is_absolute()));
        assert_eq!(find_on_path("sh", Some(std::ffi::OsStr::new("."))), None);
        assert_eq!(find_on_path("sh", None), None);
    }
}
