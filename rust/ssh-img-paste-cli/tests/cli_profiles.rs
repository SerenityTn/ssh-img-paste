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
