use super::*;
use std::{ffi::OsString, io::Write, time::Duration};

fn helper_command(name: &str) -> CommandPlan {
    CommandPlan {
        program: std::env::current_exe()
            .expect("test executable path")
            .to_string_lossy()
            .into_owned(),
        arguments: vec![
            OsString::from("--ignored"),
            OsString::from("--exact"),
            OsString::from(name),
            OsString::from("--nocapture"),
        ],
    }
}

#[test]
fn real_executor_runs_an_argument_array_to_success() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 4096);
    assert_eq!(
        executor.execute(
            &helper_command("process_executor_tests::helper_success"),
            Duration::from_secs(5)
        ),
        CommandOutcome::Success
    );
}

#[test]
fn real_executor_reports_a_bounded_spawn_failure() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 64);
    let missing = CommandPlan {
        program: "ssh-img-paste-definitely-missing-executable".into(),
        arguments: Vec::new(),
    };
    match executor.execute(&missing, Duration::from_secs(1)) {
        CommandOutcome::Failure(CommandFailure::Spawn { message }) => {
            assert!(!message.is_empty());
            assert!(message.len() <= 64);
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn real_executor_bounds_stderr_and_preserves_exit_status() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
    match executor.execute(
        &helper_command("process_executor_tests::helper_failure_with_output"),
        Duration::from_secs(5),
    ) {
        CommandOutcome::Failure(CommandFailure::Exit { code, stderr }) => {
            assert_eq!(code, Some(17));
            assert_eq!(stderr.len(), 128);
            assert!(stderr.chars().all(|character| character == 'x'));
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn diagnostics_remove_terminal_and_bidi_controls_within_the_byte_limit() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 64);
    match executor.execute(
        &helper_command("process_executor_tests::helper_control_diagnostic"),
        Duration::from_secs(5),
    ) {
        CommandOutcome::Failure(CommandFailure::Exit { stderr, .. }) => {
            assert!(stderr.len() <= 64);
            assert!(!stderr.contains('\u{1b}'));
            assert!(!stderr.contains('\n'));
            assert!(!stderr.contains('\u{202e}'));
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn group_drain_requires_empty_membership_and_zero_active_processes() {
    assert!(!group_membership_is_drained(&[41], 0));
    assert!(!group_membership_is_drained(&[], 1));
    assert!(group_membership_is_drained(&[], 0));
}

#[test]
fn completion_race_uses_event_timestamps_with_cancellation_first_on_ties() {
    let base = std::time::Instant::now();
    assert_eq!(
        requested_stop_before_completion(
            Some(base + Duration::from_millis(10)),
            Some(base + Duration::from_millis(20)),
            base + Duration::from_millis(30),
        ),
        Some(RequestedStop::Cancelled)
    );
    assert_eq!(
        requested_stop_before_completion(
            Some(base + Duration::from_millis(25)),
            Some(base + Duration::from_millis(20)),
            base + Duration::from_millis(30),
        ),
        Some(RequestedStop::Timeout)
    );
    assert_eq!(
        requested_stop_before_completion(
            Some(base + Duration::from_millis(20)),
            Some(base + Duration::from_millis(20)),
            base + Duration::from_millis(30),
        ),
        Some(RequestedStop::Cancelled)
    );
    assert_eq!(
        requested_stop_before_completion(
            Some(base + Duration::from_millis(20)),
            Some(base + Duration::from_millis(30)),
            base + Duration::from_millis(10),
        ),
        None
    );
}

#[test]
fn cleanup_expiry_rearbitrates_stop_timestamps() {
    let base = std::time::Instant::now();
    assert_eq!(
        cleanup_trigger_at(
            Some(base + Duration::from_millis(20)),
            Some(base + Duration::from_millis(10)),
            base + Duration::from_millis(30),
            RequestedStop::Cancelled,
        ),
        CleanupTrigger::Timeout
    );
    assert_eq!(
        cleanup_trigger_at(
            Some(base + Duration::from_millis(10)),
            Some(base + Duration::from_millis(20)),
            base + Duration::from_millis(30),
            RequestedStop::Timeout,
        ),
        CleanupTrigger::Cancelled
    );
    assert_eq!(
        cleanup_trigger_at(
            Some(base + Duration::from_millis(10)),
            Some(base + Duration::from_millis(10)),
            base + Duration::from_millis(30),
            RequestedStop::Timeout,
        ),
        CleanupTrigger::Cancelled
    );
}

#[test]
fn teardown_cleanup_rearbitrates_outer_stop_timestamps() {
    let base = std::time::Instant::now();
    let cleanup = |trigger| CommandFailure::Cleanup {
        trigger,
        message: "teardown failed".into(),
    };

    assert_eq!(
        reattribute_cleanup_failure(
            cleanup(CleanupTrigger::Cancelled),
            None,
            Some(base + Duration::from_millis(10)),
            base + Duration::from_millis(30),
        ),
        cleanup(CleanupTrigger::Timeout)
    );
    assert_eq!(
        reattribute_cleanup_failure(
            cleanup(CleanupTrigger::Cancelled),
            Some(base + Duration::from_millis(20)),
            Some(base + Duration::from_millis(10)),
            base + Duration::from_millis(30),
        ),
        cleanup(CleanupTrigger::Timeout)
    );
    assert_eq!(
        reattribute_cleanup_failure(
            cleanup(CleanupTrigger::Timeout),
            Some(base + Duration::from_millis(10)),
            Some(base + Duration::from_millis(20)),
            base + Duration::from_millis(30),
        ),
        cleanup(CleanupTrigger::Cancelled)
    );
    assert_eq!(
        reattribute_cleanup_failure(
            cleanup(CleanupTrigger::Timeout),
            Some(base + Duration::from_millis(10)),
            Some(base + Duration::from_millis(10)),
            base + Duration::from_millis(30),
        ),
        cleanup(CleanupTrigger::Cancelled)
    );
}

#[test]
fn real_executor_enforces_the_wall_clock_timeout() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
    let started = std::time::Instant::now();
    assert_eq!(
        executor.execute(
            &helper_command("process_executor_tests::helper_sleeps"),
            Duration::from_millis(100),
        ),
        CommandOutcome::Failure(CommandFailure::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn excessive_timeout_does_not_panic() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        executor.execute(
            &helper_command("process_executor_tests::helper_success"),
            Duration::MAX,
        )
    }));
    assert!(outcome.is_ok(), "public timeout must not overflow Instant");
}

#[test]
fn synchronous_executor_is_safe_inside_an_existing_tokio_runtime() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("outer Tokio runtime");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
            executor.execute(
                &helper_command("process_executor_tests::helper_success"),
                Duration::from_secs(5),
            )
        })
    }));
    assert_eq!(
        outcome.expect("nested runtime must not panic"),
        CommandOutcome::Success
    );
}

#[test]
fn containment_fallback_is_exposed_to_callers() {
    assert!(matches!(
        process_containment_mechanism(),
        "job_object" | "cgroup_v2" | "process_group" | "process_reaper"
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn zero_timeout_does_not_spawn_the_command() {
    let marker = std::env::temp_dir().join(format!(
        "ssh-img-paste-zero-timeout-{}.marker",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
    assert_eq!(
        executor.execute(
            &helper_command("process_executor_tests::helper_marks_zero_timeout_spawn"),
            Duration::ZERO,
        ),
        CommandOutcome::Failure(CommandFailure::Timeout)
    );
    assert!(!marker.exists(), "zero-timeout command was spawned");
}

#[test]
fn timeout_drains_large_stdout_and_stderr_without_deadlock() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 256);
    let started = std::time::Instant::now();
    assert_eq!(
        executor.execute(
            &helper_command("process_executor_tests::helper_floods_both_pipes"),
            Duration::from_millis(150),
        ),
        CommandOutcome::Failure(CommandFailure::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn real_executor_observes_cancellation_during_execution() {
    let token = CancellationToken::new();
    let cancel = token.clone();
    let mut executor = ProcessExecutor::new(token, 128);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let outcome = executor.execute(
        &helper_command("process_executor_tests::helper_sleeps"),
        Duration::from_secs(5),
    );
    canceller.join().expect("canceller thread");
    assert_eq!(outcome, CommandOutcome::Failure(CommandFailure::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(target_os = "linux")]
#[test]
fn cancellation_terminates_descendant_processes() {
    let marker = std::env::temp_dir().join(format!(
        "ssh-img-paste-process-tree-{}.pid",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let token = CancellationToken::new();
    let cancel = token.clone();
    let worker = std::thread::spawn(move || {
        let mut executor = ProcessExecutor::new(token, 128);
        executor.execute(
            &helper_command("process_executor_tests::helper_spawns_descendant"),
            Duration::from_secs(5),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !marker.is_file() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let descendant_pid = std::fs::read_to_string(&marker)
        .expect("descendant PID marker")
        .trim()
        .parse::<u32>()
        .expect("numeric descendant PID");
    cancel.cancel();
    assert_eq!(
        worker.join().expect("executor thread"),
        CommandOutcome::Failure(CommandFailure::Cancelled)
    );
    let descendant = std::path::PathBuf::from(format!("/proc/{descendant_pid}"));
    assert!(
        !descendant.exists(),
        "descendant {descendant_pid} survived cancellation return"
    );
    let _ = std::fs::remove_file(marker);
}

#[cfg(target_os = "linux")]
#[test]
fn leader_exit_kills_a_descendant_that_closed_inherited_pipes() {
    let marker = std::env::temp_dir().join(format!(
        "ssh-img-paste-exit-process-tree-{}.pid",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
    let started = std::time::Instant::now();
    assert_eq!(
        executor.execute(
            &helper_command("process_executor_tests::helper_exits_with_pipe_descendant"),
            Duration::from_secs(5),
        ),
        CommandOutcome::Success
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let descendant_pid = std::fs::read_to_string(&marker)
        .expect("exit descendant PID marker")
        .trim()
        .parse::<u32>()
        .expect("numeric descendant PID");
    let descendant = std::path::PathBuf::from(format!("/proc/{descendant_pid}"));
    let status = std::fs::read_to_string(descendant.join("status")).unwrap_or_default();
    assert!(
        !descendant.exists() || status.lines().any(|line| line.starts_with("State:\tZ")),
        "descendant {descendant_pid} remained live after normal leader exit return: {status}"
    );
    let _ = std::fs::remove_file(marker);
}

#[cfg(target_os = "linux")]
#[test]
fn timeout_bounds_post_leader_pipe_drain_and_never_reports_success() {
    let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
    let started = std::time::Instant::now();
    let outcome = executor.execute(
        &helper_command("process_executor_tests::helper_exits_with_pipe_holder"),
        Duration::from_millis(100),
    );
    assert!(
        matches!(
            outcome,
            CommandOutcome::Failure(CommandFailure::Timeout)
                | CommandOutcome::Failure(CommandFailure::Cleanup {
                    trigger: CleanupTrigger::Timeout,
                    ..
                })
        ),
        "deadline drain returned {outcome:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[cfg(target_os = "windows")]
fn windows_descendant_pid_marker() -> std::path::PathBuf {
    std::env::temp_dir().join("ssh-img-paste-windows-job-descendant.pid")
}

#[cfg(target_os = "windows")]
#[test]
fn deadline_kills_a_windows_job_descendant_before_return() {
    let pid_marker = windows_descendant_pid_marker();
    let _ = std::fs::remove_file(&pid_marker);
    let executor = std::thread::spawn(move || {
        let mut executor = ProcessExecutor::new(CancellationToken::new(), 128);
        executor.execute(
            &helper_command("process_executor_tests::helper_exits_with_windows_job_descendant"),
            Duration::from_secs(1),
        )
    });
    let marker_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !pid_marker.is_file() && std::time::Instant::now() < marker_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let descendant_pid = std::fs::read_to_string(&pid_marker)
        .expect("Windows descendant PID marker")
        .trim()
        .parse::<u32>()
        .expect("numeric Windows descendant PID");
    let identity = processkit::process_info(descendant_pid)
        .expect("inspect Windows descendant")
        .expect("Windows descendant should be live before parent exits");
    let outcome = executor.join().expect("Windows executor thread");
    let descendant_alive = processkit::process_is_alive(descendant_pid, identity.start_time())
        .expect("check Windows descendant after executor return");
    assert_eq!(
        outcome,
        CommandOutcome::Failure(CommandFailure::Timeout),
        "deadline must become Timeout only after explicit group-drain confirmation"
    );
    assert!(
        !descendant_alive,
        "Windows Job Object descendant survived executor return"
    );
    let _ = std::fs::remove_file(pid_marker);
}

#[cfg(target_os = "windows")]
fn windows_cancellation_descendant_pid_marker() -> std::path::PathBuf {
    std::env::temp_dir().join("ssh-img-paste-windows-job-cancellation-descendant.pid")
}

#[cfg(target_os = "windows")]
#[test]
fn cancellation_kills_a_windows_job_descendant_before_return() {
    let pid_marker = windows_cancellation_descendant_pid_marker();
    let _ = std::fs::remove_file(&pid_marker);
    let token = CancellationToken::new();
    let cancel = token.clone();
    let executor = std::thread::spawn(move || {
        let mut executor = ProcessExecutor::new(token, 128);
        executor.execute(
            &helper_command(
                "process_executor_tests::helper_exits_with_windows_cancellation_descendant",
            ),
            Duration::from_secs(30),
        )
    });
    let marker_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !pid_marker.is_file() && std::time::Instant::now() < marker_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let descendant_pid = std::fs::read_to_string(&pid_marker)
        .expect("Windows cancellation descendant PID marker")
        .trim()
        .parse::<u32>()
        .expect("numeric Windows cancellation descendant PID");
    let identity = processkit::process_info(descendant_pid)
        .expect("inspect Windows cancellation descendant")
        .expect("Windows cancellation descendant should be live before cancellation");
    cancel.cancel();
    let outcome = executor
        .join()
        .expect("Windows cancellation executor thread");
    let descendant_alive = processkit::process_is_alive(descendant_pid, identity.start_time())
        .expect("check Windows cancellation descendant after executor return");
    assert_eq!(outcome, CommandOutcome::Failure(CommandFailure::Cancelled));
    assert!(
        !descendant_alive,
        "Windows Job Object descendant survived cancellation return"
    );
    let _ = std::fs::remove_file(pid_marker);
}

#[cfg(target_os = "windows")]
#[test]
#[ignore]
fn helper_exits_with_windows_cancellation_descendant() {
    let descendant = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "process_executor_tests::helper_descendant_sleeps",
            "--nocapture",
        ])
        .spawn()
        .expect("spawn Windows cancellation descendant");
    std::fs::write(
        windows_cancellation_descendant_pid_marker(),
        descendant.id().to_string(),
    )
    .expect("write Windows cancellation descendant PID");
    std::mem::forget(descendant);
    std::thread::sleep(Duration::from_millis(500));
}

#[cfg(target_os = "windows")]
#[test]
#[ignore]
fn helper_exits_with_windows_job_descendant() {
    let descendant = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "process_executor_tests::helper_descendant_sleeps",
            "--nocapture",
        ])
        .spawn()
        .expect("spawn Windows Job Object descendant");
    std::fs::write(windows_descendant_pid_marker(), descendant.id().to_string())
        .expect("write Windows descendant PID");
    std::mem::forget(descendant);
    std::thread::sleep(Duration::from_millis(500));
}

#[cfg(target_os = "linux")]
#[test]
#[ignore]
fn helper_spawns_descendant() {
    let marker = std::env::temp_dir().join(format!("ssh-img-paste-process-tree-{}.pid", unsafe {
        libc::getppid()
    }));
    let mut descendant =
        std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--ignored",
                "--exact",
                "process_executor_tests::helper_descendant_sleeps",
                "--nocapture",
            ])
            .spawn()
            .expect("spawn descendant");
    std::fs::write(marker, descendant.id().to_string()).expect("write descendant PID");
    let _ = descendant.wait();
}

#[cfg(target_os = "linux")]
#[test]
#[ignore]
fn helper_exits_with_pipe_descendant() {
    let marker = std::env::temp_dir()
        .join(format!("ssh-img-paste-exit-process-tree-{}.pid", unsafe {
            libc::getppid()
        }));
    let descendant = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "process_executor_tests::helper_descendant_sleeps",
            "--nocapture",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn detached-pipe descendant");
    std::fs::write(marker, descendant.id().to_string()).expect("write exit descendant PID");
    std::mem::forget(descendant);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore]
fn helper_exits_with_pipe_holder() {
    let descendant = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "process_executor_tests::helper_descendant_sleeps",
            "--nocapture",
        ])
        .spawn()
        .expect("spawn pipe-holding descendant");
    std::mem::forget(descendant);
}

#[test]
#[ignore]
fn helper_descendant_sleeps() {
    std::thread::sleep(Duration::from_secs(10));
}

#[cfg(target_os = "linux")]
#[test]
#[ignore]
fn helper_marks_zero_timeout_spawn() {
    let marker = std::env::temp_dir()
        .join(format!("ssh-img-paste-zero-timeout-{}.marker", unsafe {
            libc::getppid()
        }));
    std::fs::write(marker, b"spawned").expect("write spawn marker");
}

#[test]
#[ignore]
fn helper_control_diagnostic() {
    std::io::stderr()
        .write_all("\u{1b}[31mspoof\n\u{202e}txt".as_bytes())
        .expect("write control diagnostic");
    std::process::exit(19);
}

#[test]
#[ignore]
fn helper_floods_both_pipes() {
    let block = vec![b'x'; 1024 * 1024];
    let stdout = std::thread::spawn({
        let block = block.clone();
        move || std::io::stdout().write_all(&block).expect("write stdout")
    });
    let stderr =
        std::thread::spawn(move || std::io::stderr().write_all(&block).expect("write stderr"));
    stdout.join().expect("stdout writer");
    stderr.join().expect("stderr writer");
    std::thread::sleep(Duration::from_secs(2));
}

#[test]
#[ignore]
fn helper_sleeps() {
    std::thread::sleep(Duration::from_secs(2));
}

#[test]
#[ignore]
fn helper_failure_with_output() {
    std::io::stderr()
        .write_all(&vec![b'x'; 10_000])
        .expect("write stderr");
    std::process::exit(17);
}

#[test]
#[ignore]
fn helper_success() {}
