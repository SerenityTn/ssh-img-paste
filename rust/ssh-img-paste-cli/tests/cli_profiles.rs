use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temporary_catalog() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ssh-img-paste-cli-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("profiles")).expect("create profiles");
    root
}

fn write_profile(root: &Path, id: &str, label: &str, host: &str) {
    std::fs::write(
        root.join("profiles").join(format!("{id}.env")),
        format!(
            "SSH_PROFILE_LABEL='{label}'\nSSH_HOST='{host}'\nSSH_REMOTE_HOME='/home/user'\nSSH_REMOTE_DIR='img-uploads'\n"
        ),
    )
    .expect("write profile");
}

fn run(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"));
    command.arg("--config-root").arg(root).args(args);
    command.output().expect("run CLI")
}

fn assert_structured_error(output: &Output, exit_code: i32, error_code: &str) {
    assert_eq!(output.status.code(), Some(exit_code), "{output:?}");
    assert!(output.stderr.is_empty(), "stderr must stay machine-clean");
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON document");
    assert_eq!(value["protocol_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], error_code);
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn interrupt_cancels_upload_and_removes_private_staging() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    use std::time::{Duration, Instant};

    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");
    let source = root.join("source.png");
    std::fs::write(&source, b"\x89PNG\r\n\x1a\nbytes").expect("source image");
    let bin = root.join("cancel bin");
    std::fs::create_dir(&bin).expect("fake OpenSSH bin");
    let staging_path_log = root.join("staging-path.log");
    let ssh = bin.join("ssh");
    std::fs::write(&ssh, "#!/bin/sh\nexit 0\n").expect("fake ssh");
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).expect("ssh mode");
    let scp = bin.join("scp");
    std::fs::write(
        &scp,
        "#!/bin/sh\nprintf '%s' \"$8\" > \"$STAGING_PATH_LOG\"\n/bin/sleep 10\n",
    )
    .expect("fake scp");
    std::fs::set_permissions(&scp, std::fs::Permissions::from_mode(0o700)).expect("scp mode");

    let child = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"))
        .arg("--config-root")
        .arg(&root)
        .args(["upload-file", "--source"])
        .arg(&source)
        .args(["--profile", "alpha", "--confirm-profile", "alpha"])
        .env("PATH", &bin)
        .env("STAGING_PATH_LOG", &staging_path_log)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn upload CLI");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !staging_path_log.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(staging_path_log.exists(), "upload did not reach scp");
    let staging_path =
        PathBuf::from(std::fs::read_to_string(&staging_path_log).expect("recorded staging path"));
    let signal_result = unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    assert_eq!(signal_result, 0, "send interrupt");
    let output = child.wait_with_output().expect("cancelled CLI output");

    assert!(
        output.status.code().is_some(),
        "CLI must handle SIGINT instead of dying by signal: {:?}",
        output.status.signal()
    );
    assert_structured_error(&output, 3, "upload_failed");
    let value: Value = serde_json::from_slice(&output.stdout).expect("error JSON");
    assert_eq!(value["error"]["reason"], "cancelled");
    assert!(
        !staging_path.exists(),
        "staging file remained after cancellation"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn upload_stages_validated_bytes_before_the_original_path_can_change() {
    use std::os::unix::fs::PermissionsExt;

    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");
    let original_bytes = b"\x89PNG\r\n\x1a\nvalidated bytes";
    let source = root.join("replaceable.png");
    std::fs::write(&source, original_bytes).expect("source image");
    let bin = root.join("race bin");
    std::fs::create_dir(&bin).expect("fake OpenSSH bin");
    let marker = root.join("replaced.marker");
    let captured = root.join("captured-source.bin");
    let staging_path_log = root.join("staging-path.log");
    let ssh_script = "#!/bin/sh\nif [ ! -e \"$RACE_MARKER\" ]; then\n  printf 'replacement bytes' > \"$ORIGINAL_SOURCE\"\n  printf done > \"$RACE_MARKER\"\nfi\nexit 0\n";
    let scp_script = "#!/bin/sh\n/bin/cp \"$8\" \"$CAPTURED_SOURCE\"\nprintf '%s' \"$8\" > \"$STAGING_PATH_LOG\"\nexit 0\n";
    for (program, script) in [("ssh", ssh_script), ("scp", scp_script)] {
        let path = bin.join(program);
        std::fs::write(&path, script).expect("fake OpenSSH program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("fake executable permissions");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"))
        .arg("--config-root")
        .arg(&root)
        .args(["upload-file", "--source"])
        .arg(&source)
        .args(["--profile", "alpha", "--confirm-profile", "alpha"])
        .env("PATH", &bin)
        .env("RACE_MARKER", &marker)
        .env("ORIGINAL_SOURCE", &source)
        .env("CAPTURED_SOURCE", &captured)
        .env("STAGING_PATH_LOG", &staging_path_log)
        .output()
        .expect("run upload CLI");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read(&captured).expect("captured bytes"),
        original_bytes
    );
    assert_eq!(
        std::fs::read(&source).expect("replaced original"),
        b"replacement bytes"
    );
    let staging_path =
        PathBuf::from(std::fs::read_to_string(staging_path_log).expect("recorded staging path"));
    assert_ne!(staging_path, source);
    assert!(
        !staging_path.exists(),
        "staging file must be removed on return"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn upload_file_executes_the_sealed_plan_and_returns_only_the_remote_path() {
    use std::os::unix::fs::PermissionsExt;

    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");
    let source = root.join("source image.png");
    std::fs::write(&source, b"\x89PNG\r\n\x1a\nimage bytes").expect("source image");
    let bin = root.join("bin");
    std::fs::create_dir(&bin).expect("fake OpenSSH bin");
    let log = root.join("argv.log");
    let script = "#!/bin/sh\nprintf '%s\\n' \"${0##*/}\" >> \"$SSH_IMG_PASTE_TEST_LOG\"\nexit 0\n";
    for program in ["ssh", "scp"] {
        let path = bin.join(program);
        std::fs::write(&path, script).expect("fake OpenSSH program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("fake executable permissions");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"))
        .arg("--config-root")
        .arg(&root)
        .args(["upload-file", "--source"])
        .arg(&source)
        .args(["--profile", "alpha", "--confirm-profile", "alpha"])
        .env("PATH", &bin)
        .env("SSH_IMG_PASTE_TEST_LOG", &log)
        .output()
        .expect("run upload CLI");

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON document");
    assert_eq!(value["protocol_version"], 1);
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["type"], "upload");
    assert_eq!(value["result"]["profile_id"], "alpha");
    let remote_path = value["result"]["remote_path"]
        .as_str()
        .expect("remote path");
    assert!(remote_path.starts_with("/home/user/img-uploads/ssh-img-"));
    assert!(remote_path.ends_with(".png"));
    assert_eq!(
        std::fs::read_to_string(log).expect("argv log"),
        "ssh\nscp\nssh\n"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn upload_rejects_a_missing_source_before_spawning_openssh() {
    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");

    let output = run(
        &root,
        &[
            "upload-file",
            "--source",
            if cfg!(target_os = "windows") {
                r"C:\definitely-missing-ssh-img-paste.png"
            } else {
                "/definitely-missing-ssh-img-paste.png"
            },
            "--profile",
            "alpha",
            "--confirm-profile",
            "alpha",
        ],
    );

    assert_structured_error(&output, 3, "source_not_found");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn upload_requires_matching_explicit_target_confirmation_before_execution() {
    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");

    let output = run(
        &root,
        &[
            "upload-file",
            "--source",
            if cfg!(target_os = "windows") {
                r"C:\missing.png"
            } else {
                "/missing.png"
            },
            "--profile",
            "alpha",
            "--confirm-profile",
            "other",
        ],
    );

    assert_structured_error(&output, 2, "profile_confirmation_mismatch");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn misplaced_global_config_option_is_rejected_as_structured_json() {
    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");

    let output = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"))
        .args(["profiles", "list", "--config-root"])
        .arg(&root)
        .output()
        .expect("run CLI");

    assert_structured_error(&output, 2, "invalid_arguments");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn malformed_and_duplicate_options_fail_closed() {
    let root = temporary_catalog();
    let executable = env!("CARGO_BIN_EXE_ssh-img-paste");
    let cases: Vec<(Vec<OsString>, &str)> = vec![
        (vec!["--config-root".into()], "missing_config_root"),
        (
            vec![
                "--config-root".into(),
                root.clone().into_os_string(),
                "--config-root".into(),
                root.clone().into_os_string(),
                "profiles".into(),
                "list".into(),
            ],
            "invalid_arguments",
        ),
        (
            vec![
                "--config-root".into(),
                root.clone().into_os_string(),
                "profiles".into(),
                "resolve".into(),
                "--profile".into(),
                "../escape".into(),
            ],
            "invalid_profile_id",
        ),
        (
            vec!["profiles".into(), "unknown".into()],
            "invalid_arguments",
        ),
    ];

    for (arguments, expected_code) in cases {
        let output = Command::new(executable)
            .args(arguments)
            .output()
            .expect("run malformed CLI invocation");
        assert_structured_error(&output, 2, expected_code);
    }
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn non_unicode_profile_id_fails_closed() {
    use std::os::unix::ffi::OsStringExt;

    let root = temporary_catalog();
    let output = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"))
        .arg("--config-root")
        .arg(&root)
        .args(["profiles", "resolve", "--profile"])
        .arg(OsString::from_vec(vec![0xff]))
        .output()
        .expect("run CLI with non-Unicode ID");

    assert_structured_error(&output, 2, "invalid_profile_id");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn relative_config_root_and_unavailable_default_are_structured_failures() {
    let relative = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"))
        .args(["--config-root", "relative", "profiles", "list"])
        .output()
        .expect("run relative config root");
    assert_structured_error(&relative, 3, "config_root_not_absolute");

    let unavailable = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"))
        .args(["profiles", "list"])
        .env_clear()
        .output()
        .expect("run without config environment");
    assert_structured_error(&unavailable, 3, "config_root_unavailable");
}

#[test]
fn profile_resolve_without_override_uses_the_active_profile() {
    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");
    write_profile(&root, "zeta", "Zeta", "zeta@example.test");
    std::fs::write(root.join("active-profile"), "zeta\n").expect("active profile");

    let output = run(&root, &["profiles", "resolve"]);

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON document");
    assert_eq!(value["result"]["profile"]["id"], "zeta");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn platform_default_config_root_is_exercised_by_the_binary() {
    let base = temporary_catalog();
    let root = if cfg!(target_os = "windows") {
        base.join("SSH Image Paste")
    } else {
        base.join("ssh-img-paste")
    };
    std::fs::create_dir_all(root.join("profiles")).expect("default profile root");
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");

    let mut command = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"));
    command.args(["profiles", "list"]).env_clear();
    if cfg!(target_os = "windows") {
        command.env("LOCALAPPDATA", &base);
    } else {
        command.env("XDG_CONFIG_HOME", &base);
    }
    let output = command.output().expect("run with platform config root");

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON document");
    assert_eq!(value["result"]["profiles"][0]["id"], "alpha");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn profile_resolve_emits_the_explicit_validated_profile_without_mutating_active_state() {
    let root = temporary_catalog();
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");
    std::fs::write(root.join("active-profile"), "../invalid\n").expect("invalid active state");
    let before = std::fs::read(root.join("active-profile")).expect("snapshot active state");

    let output = run(&root, &["profiles", "resolve", "--profile", "alpha"]);

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON document");
    assert_eq!(value["protocol_version"], 1);
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["type"], "profile");
    assert_eq!(value["result"]["profile"]["id"], "alpha");
    assert_eq!(value["result"]["profile"]["label"], "Alpha");
    assert_eq!(value["result"]["profile"]["host"], "alpha@example.test");
    assert_eq!(value["result"]["profile"]["remote_home"], "/home/user");
    assert_eq!(value["result"]["profile"]["remote_dir"], "img-uploads");
    assert_eq!(value["result"]["profile"]["editable"], true);
    assert_eq!(
        std::fs::read(root.join("active-profile")).expect("active state after resolve"),
        before
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn profile_list_emits_one_strict_versioned_json_document() {
    let root = temporary_catalog();
    write_profile(&root, "zeta", "Zeta", "zeta@example.test");
    write_profile(&root, "alpha", "Alpha", "alpha@example.test");
    std::fs::write(root.join("active-profile"), "zeta\n").expect("active profile");

    let output = run(&root, &["profiles", "list"]);

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "stderr must be empty on success");
    let value: Value = serde_json::from_slice(&output.stdout).expect("single JSON document");
    assert_eq!(value["protocol_version"], 1);
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["type"], "profile_catalog");
    assert_eq!(value["result"]["active_profile_id"], "zeta");
    assert_eq!(value["result"]["profiles"][0]["id"], "alpha");
    assert_eq!(value["result"]["profiles"][0]["active"], false);
    assert_eq!(value["result"]["profiles"][1]["id"], "zeta");
    assert_eq!(value["result"]["profiles"][1]["active"], true);
    assert_eq!(value["result"]["profiles"][1]["editable"], true);
    assert_eq!(value["result"]["profiles"][1]["label"], "Zeta");
    let keys: Vec<_> = value["result"]["profiles"][1]
        .as_object()
        .expect("profile summary object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["active", "editable", "id", "label"]);
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );

    let _ = std::fs::remove_dir_all(root);
}
