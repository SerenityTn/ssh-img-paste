//! Cross-platform CLI contracts for commands that run OpenSSH, using the
//! `fake_openssh` example as the `ssh`/`scp` executables.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-image-bytes";

struct Fixture {
    root: PathBuf,
    bin: PathBuf,
    log: PathBuf,
    envs: Vec<(String, String)>,
}

fn fake_openssh_executable() -> PathBuf {
    let deps = std::env::current_exe().expect("test exe");
    let profile_dir = deps.parent().and_then(Path::parent).expect("target profile dir");
    profile_dir
        .join("examples")
        .join(format!("fake_openssh{}", std::env::consts::EXE_SUFFIX))
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ssh-img-paste-cli-remote-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("catalog").join("profiles")).expect("catalog");
        let bin = root.join("openssh bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let fake = fake_openssh_executable();
        assert!(fake.is_file(), "build the fake_openssh example first: {fake:?}");
        for program in ["ssh", "scp"] {
            std::fs::copy(&fake, bin.join(format!("{program}{}", std::env::consts::EXE_SUFFIX)))
                .expect("install fake OpenSSH");
        }
        let log = root.join("openssh.log");
        std::fs::write(
            root.join("catalog").join("profiles").join("work.env"),
            "SSH_PROFILE_LABEL='Work'\nSSH_HOST='work-ssh'\nSSH_REMOTE_HOME='/home/me'\nSSH_REMOTE_DIR='img-uploads'\n",
        )
        .expect("profile");
        Self {
            root,
            bin,
            log,
            envs: Vec::new(),
        }
    }

    fn env(mut self, name: &str, value: impl Into<String>) -> Self {
        self.envs.push((name.to_owned(), value.into()));
        self
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ssh-img-paste"));
        command
            .arg("--config-root")
            .arg(self.root.join("catalog"))
            .args(args)
            .env("SSH_IMG_PASTE_OPENSSH_DIR", &self.bin)
            .env("FAKE_OPENSSH_LOG", &self.log)
            .env_remove("FAKE_OPENSSH_STDOUT")
            .env_remove("FAKE_OPENSSH_STDERR")
            .env_remove("FAKE_OPENSSH_EXIT")
            .env_remove("FAKE_OPENSSH_FETCH_SOURCE")
            .env_remove("FAKE_OPENSSH_UPLOAD_COPY");
        for (name, value) in &self.envs {
            command.env(name, value);
        }
        command.output().expect("run CLI")
    }

    fn invocations(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("log line"))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn json(output: &Output) -> Value {
    assert!(output.stderr.is_empty(), "stderr must stay machine-clean: {output:?}");
    serde_json::from_slice(&output.stdout).expect("single JSON document")
}

fn ok(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    let value = json(output);
    assert_eq!(value["ok"], true);
    value["result"].clone()
}

fn error(output: &Output, exit_code: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit_code), "{output:?}");
    let value = json(output);
    assert_eq!(value["ok"], false);
    value["error"].clone()
}

#[test]
fn connection_test_reports_the_detected_home_and_whether_it_matches() {
    let fixture = Fixture::new("test-match").env("FAKE_OPENSSH_STDOUT", "Welcome!\n/home/me\n");
    let result = ok(&fixture.run(&["profiles", "test", "work"]));
    assert_eq!(result["type"], "connection_test");
    assert_eq!(result["detected_remote_home"], "/home/me");
    assert_eq!(result["remote_home_matches"], true);
    let calls = fixture.invocations();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["program"], "ssh");
    assert_eq!(
        calls[0]["arguments"],
        serde_json::json!(["-o", "BatchMode=yes", "-o", "ConnectTimeout=6", "work-ssh", "printf '%s\\n' \"$HOME\""])
    );

    let fixture = Fixture::new("test-mismatch").env("FAKE_OPENSSH_STDOUT", "/srv/users/me\n");
    let result = ok(&fixture.run(&["profiles", "test", "work"]));
    assert_eq!(result["remote_home_matches"], false);
    assert_eq!(result["detected_remote_home"], "/srv/users/me");
}

#[test]
fn connection_failures_carry_a_bounded_diagnostic() {
    let fixture = Fixture::new("test-fail")
        .env("FAKE_OPENSSH_EXIT", "255")
        .env("FAKE_OPENSSH_STDERR", "me@work-ssh: Permission denied (publickey).\r\n");
    let error = error(&fixture.run(&["profiles", "test", "work"]), 3);
    assert_eq!(error["code"], "connection_failed");
    assert_eq!(error["reason"], "exit");
    assert_eq!(error["exit_code"], 255);
    assert!(
        error["diagnostic"]
            .as_str()
            .is_some_and(|text| text.contains("Permission denied (publickey).") && !text.contains('\n')),
        "{error}"
    );
}

#[test]
fn host_probe_validates_before_spawning_openssh() {
    let fixture = Fixture::new("probe");
    let error = error(&fixture.run(&["hosts", "probe", "--host", "-oProxyCommand=calc"]), 3);
    assert_eq!(error["code"], "invalid_field");
    assert_eq!(error["field"], "host");
    assert!(fixture.invocations().is_empty(), "invalid hosts must never reach OpenSSH");

    let fixture = Fixture::new("probe-ok").env("FAKE_OPENSSH_STDOUT", "/home/new\n");
    let result = ok(&fixture.run(&["hosts", "probe", "--host", "new@host.example"]));
    assert_eq!(result["type"], "host_probe");
    assert_eq!(result["detected_remote_home"], "/home/new");
}

#[test]
fn listing_returns_validated_rows_with_remote_paths() {
    let fixture = Fixture::new("list").env(
        "FAKE_OPENSSH_STDOUT",
        "1700000000.25\t10\told.png\n1700000500.0\t2048\tnew.png\nbad\trow\n",
    );
    let result = ok(&fixture.run(&["uploads", "list", "--profile", "work"]));
    assert_eq!(result["type"], "upload_listing");
    assert_eq!(result["remote_directory"], "/home/me/img-uploads");
    assert_eq!(result["uploads"][0]["name"], "new.png");
    assert_eq!(result["uploads"][0]["size_bytes"], 2048);
    assert_eq!(result["uploads"][0]["remote_path"], "/home/me/img-uploads/new.png");
    assert_eq!(result["uploads"][1]["modified_epoch_seconds"], 1_700_000_000_i64);
    assert_eq!(result["skipped_rows"], 1);
}

#[test]
fn fetch_downloads_into_a_private_directory_and_rejects_non_png_content() {
    let fixture = Fixture::new("fetch");
    let source = fixture.root.join("remote.png");
    std::fs::write(&source, PNG).expect("source");
    let fixture = fixture.env("FAKE_OPENSSH_FETCH_SOURCE", source.to_string_lossy());
    let result = ok(&fixture.run(&["uploads", "fetch", "--profile", "work", "--name", "shot.png"]));
    let local = PathBuf::from(result["local_path"].as_str().expect("local path"));
    assert_eq!(std::fs::read(&local).expect("downloaded"), PNG);
    assert_eq!(local.file_name().and_then(|name| name.to_str()), Some("shot.png"));
    let calls = fixture.invocations();
    assert_eq!(calls[0]["program"], "scp");
    assert_eq!(calls[0]["arguments"][7], "work-ssh:/home/me/img-uploads/shot.png");
    let _ = std::fs::remove_dir_all(local.parent().expect("download dir"));

    let fixture = Fixture::new("fetch-bad");
    let source = fixture.root.join("remote.png");
    std::fs::write(&source, b"<html>not an image</html>").expect("source");
    let fixture = fixture.env("FAKE_OPENSSH_FETCH_SOURCE", source.to_string_lossy());
    let error = error(&fixture.run(&["uploads", "fetch", "--profile", "work", "--name", "shot.png"]), 3);
    assert_eq!(error["code"], "fetched_not_png");
    let destination = PathBuf::from(
        fixture.invocations()[0]["arguments"][8]
            .as_str()
            .expect("destination"),
    );
    assert!(!destination.exists(), "rejected downloads must be removed");
}

#[test]
fn clean_requires_confirmation_and_reports_the_count() {
    let fixture = Fixture::new("clean").env("FAKE_OPENSSH_STDOUT", "      3\n");
    let error = error(
        &fixture.run(&["uploads", "clean", "--profile", "work", "--confirm-profile", "other"]),
        2,
    );
    assert_eq!(error["code"], "profile_confirmation_mismatch");
    assert!(fixture.invocations().is_empty());

    let result = ok(&fixture.run(&["uploads", "clean", "--profile", "work", "--confirm-profile", "work"]));
    assert_eq!(result["type"], "uploads_cleaned");
    assert_eq!(result["deleted_count"], 3);
}

#[test]
fn upload_file_runs_mkdir_scp_and_finalize_in_order() {
    let fixture = Fixture::new("upload");
    let source = fixture.root.join("clip image.png");
    std::fs::write(&source, PNG).expect("source");
    let copy = fixture.root.join("transferred.png");
    let fixture = fixture.env("FAKE_OPENSSH_UPLOAD_COPY", copy.to_string_lossy());
    let source_text = source.to_string_lossy().into_owned();
    let result = ok(&fixture.run(&[
        "upload-file",
        "--source",
        &source_text,
        "--profile",
        "work",
        "--confirm-profile",
        "work",
    ]));
    let remote_path = result["remote_path"].as_str().expect("remote path");
    assert!(remote_path.starts_with("/home/me/img-uploads/ssh-img-"), "{remote_path}");
    assert_eq!(std::fs::read(&copy).expect("transferred bytes"), PNG);

    let calls = fixture.invocations();
    let programs: Vec<_> = calls.iter().map(|call| call["program"].as_str().unwrap_or("")).collect();
    assert_eq!(programs, ["ssh", "scp", "ssh"]);
    assert_eq!(calls[0]["arguments"][5], "mkdir -p -- /home/me/img-uploads");
    let staged = calls[1]["arguments"][7].as_str().expect("staged source");
    assert_ne!(staged, source_text, "scp must read the private staged copy");
    assert!(!Path::new(staged).exists(), "staged copy must be removed after upload");
    assert!(
        calls[2]["arguments"][5]
            .as_str()
            .is_some_and(|command| command.starts_with("mv -- /home/me/img-uploads/.ssh-img-")),
        "{}",
        calls[2]
    );
}

#[test]
fn profile_details_report_broken_files_without_failing() {
    let fixture = Fixture::new("details");
    std::fs::write(
        fixture.root.join("catalog").join("profiles").join("manual.env"),
        "SSH_HOST=$(hostname)\n",
    )
    .expect("dynamic profile");
    let result = ok(&fixture.run(&["profiles", "list", "--detailed"]));
    assert_eq!(result["type"], "profile_details");
    assert_eq!(result["active_profile_id"], "work");
    assert_eq!(result["profiles"][0]["id"], "manual");
    assert_eq!(result["profiles"][0]["usable"], false);
    assert_eq!(result["profiles"][0]["problem"]["code"], "dynamic_syntax");
    assert_eq!(result["profiles"][0]["problem"]["key"], "SSH_HOST");
    assert_eq!(result["profiles"][1]["id"], "work");
    assert_eq!(result["profiles"][1]["usable"], true);
    assert_eq!(result["profiles"][1]["host"], "work-ssh");
}

#[test]
fn version_reports_the_protocol_and_openssh_programs() {
    let fixture = Fixture::new("version");
    let result = ok(&fixture.run(&["version"]));
    assert_eq!(result["protocol_version"], 1);
    let ssh = result["ssh_program"].as_str().expect("ssh program");
    assert!(Path::new(ssh).starts_with(&fixture.bin), "{ssh}");
}
