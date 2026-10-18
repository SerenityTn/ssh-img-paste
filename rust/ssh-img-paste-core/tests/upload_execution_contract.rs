use ssh_img_paste_core::{
    CommandExecutor, CommandFailure, CommandOutcome, CommandPlan, ExecutionPolicy,
    UploadExecutionError, UploadPlan, UploadStep, ValidatedProfile, build_upload_plan,
    execute_upload_plan,
};
use std::time::Duration;

#[derive(Default)]
struct RecordingExecutor {
    calls: Vec<CommandPlan>,
}

impl CommandExecutor for RecordingExecutor {
    fn execute(&mut self, command: &CommandPlan, timeout: Duration) -> CommandOutcome {
        assert_eq!(timeout, Duration::from_secs(9));
        self.calls.push(command.clone());
        CommandOutcome::Success
    }
}

struct FailureExecutor {
    calls: Vec<CommandPlan>,
    failed_call: usize,
    failure: CommandFailure,
}

impl CommandExecutor for FailureExecutor {
    fn execute(&mut self, command: &CommandPlan, _timeout: Duration) -> CommandOutcome {
        self.calls.push(command.clone());
        if self.calls.len() == self.failed_call {
            CommandOutcome::Failure(self.failure.clone())
        } else {
            CommandOutcome::Success
        }
    }
}

fn plan() -> UploadPlan {
    let profile = ValidatedProfile {
        label: "Test".into(),
        host: "test-host".into(),
        remote_home: "/home/test".into(),
        remote_dir: "img-uploads".into(),
        shot_mode: Some("region".into()),
        restore_seconds: Some("60".into()),
        editable: true,
    };
    let source = if cfg!(windows) {
        std::path::Path::new(r"C:\Temp\source.png")
    } else {
        std::path::Path::new("/tmp/source.png")
    };
    build_upload_plan(&profile, source, "result.png").expect("validated test plan")
}

#[test]
fn executes_each_upload_step_in_order_and_returns_the_remote_path() {
    let upload = plan();
    let expected_calls = vec![
        upload.mkdir().clone(),
        upload.upload().clone(),
        upload.finalize().clone(),
    ];
    let mut executor = RecordingExecutor::default();

    let result = execute_upload_plan(
        &upload,
        &mut executor,
        ExecutionPolicy {
            command_timeout: Duration::from_secs(9),
        },
    )
    .expect("upload execution should succeed");

    assert_eq!(result.remote_path, upload.remote_path());
    assert_eq!(executor.calls, expected_calls);
}

#[test]
fn reports_the_failed_step_and_does_not_run_later_steps() {
    let upload = plan();
    let mut executor = FailureExecutor {
        calls: Vec::new(),
        failed_call: 2,
        failure: CommandFailure::Exit {
            code: Some(23),
            stderr: "transfer refused".into(),
        },
    };

    let error = execute_upload_plan(
        &upload,
        &mut executor,
        ExecutionPolicy {
            command_timeout: Duration::from_secs(9),
        },
    )
    .expect_err("upload step should fail");

    assert_eq!(
        error,
        UploadExecutionError {
            step: UploadStep::Upload,
            failure: CommandFailure::Exit {
                code: Some(23),
                stderr: "transfer refused".into(),
            },
        }
    );
    assert_eq!(
        executor.calls,
        vec![upload.mkdir().clone(), upload.upload().clone()]
    );
}

#[test]
fn reports_a_timeout_at_the_step_where_it_occurs() {
    let upload = plan();
    let mut executor = FailureExecutor {
        calls: Vec::new(),
        failed_call: 1,
        failure: CommandFailure::Timeout,
    };

    let error = execute_upload_plan(
        &upload,
        &mut executor,
        ExecutionPolicy {
            command_timeout: Duration::from_secs(9),
        },
    )
    .expect_err("mkdir should time out");

    assert_eq!(error.step, UploadStep::CreateRemoteDirectory);
    assert_eq!(error.failure, CommandFailure::Timeout);
    assert_eq!(executor.calls, vec![upload.mkdir().clone()]);
}

#[test]
fn preserves_a_structured_spawn_failure() {
    let upload = plan();
    let mut executor = FailureExecutor {
        calls: Vec::new(),
        failed_call: 1,
        failure: CommandFailure::Spawn {
            message: "ssh executable was not found".into(),
        },
    };

    let error = execute_upload_plan(
        &upload,
        &mut executor,
        ExecutionPolicy {
            command_timeout: Duration::from_secs(9),
        },
    )
    .expect_err("mkdir spawn should fail");

    assert_eq!(
        error.failure,
        CommandFailure::Spawn {
            message: "ssh executable was not found".into(),
        }
    );
}

#[test]
fn cancellation_stops_the_upload_before_finalize() {
    let upload = plan();
    let mut executor = FailureExecutor {
        calls: Vec::new(),
        failed_call: 2,
        failure: CommandFailure::Cancelled,
    };

    let error = execute_upload_plan(
        &upload,
        &mut executor,
        ExecutionPolicy {
            command_timeout: Duration::from_secs(9),
        },
    )
    .expect_err("upload should be cancelled");

    assert_eq!(error.step, UploadStep::Upload);
    assert_eq!(error.failure, CommandFailure::Cancelled);
    assert_eq!(
        executor.calls,
        vec![upload.mkdir().clone(), upload.upload().clone()]
    );
}

#[test]
fn attributes_a_finalize_failure_without_returning_success() {
    let upload = plan();
    let mut executor = FailureExecutor {
        calls: Vec::new(),
        failed_call: 3,
        failure: CommandFailure::Exit {
            code: Some(1),
            stderr: "rename failed".into(),
        },
    };

    let error = execute_upload_plan(
        &upload,
        &mut executor,
        ExecutionPolicy {
            command_timeout: Duration::from_secs(9),
        },
    )
    .expect_err("finalize should fail");

    assert_eq!(error.step, UploadStep::Finalize);
    assert_eq!(executor.calls.len(), 3);
}

struct PanicExecutor;

impl CommandExecutor for PanicExecutor {
    fn execute(&mut self, _command: &CommandPlan, _timeout: Duration) -> CommandOutcome {
        panic!("zero timeout must fail before invoking the executor")
    }
}

#[test]
fn zero_timeout_fails_before_spawning_the_first_step() {
    let upload = plan();
    let mut executor = PanicExecutor;

    let error = execute_upload_plan(
        &upload,
        &mut executor,
        ExecutionPolicy {
            command_timeout: Duration::ZERO,
        },
    )
    .expect_err("zero timeout should fail immediately");

    assert_eq!(error.step, UploadStep::CreateRemoteDirectory);
    assert_eq!(error.failure, CommandFailure::Timeout);
}
