//! Test double for `ssh` and `scp`, used by the CLI integration tests.
//!
//! Copy it into a directory as `ssh`/`scp` (plus the platform executable
//! suffix) and point `SSH_IMG_PASTE_OPENSSH_DIR` at that directory.
//!
//! - `FAKE_OPENSSH_LOG`: append one JSON line per invocation with the program
//!   name and arguments.
//! - `FAKE_OPENSSH_STDOUT` / `FAKE_OPENSSH_STDERR`: text to print.
//! - `FAKE_OPENSSH_EXIT`: exit code (default 0).
//! - `FAKE_OPENSSH_FETCH_SOURCE`: for an `scp HOST:PATH LOCAL` download,
//!   the local file copied to `LOCAL`.
//! - `FAKE_OPENSSH_UPLOAD_COPY`: for an `scp LOCAL HOST:PATH` upload, where
//!   to copy `LOCAL` so a test can inspect the transferred bytes.

use std::io::Write;

fn main() {
    let program = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let arguments: Vec<String> = std::env::args().skip(1).collect();

    if let Some(log) = std::env::var_os("FAKE_OPENSSH_LOG") {
        let line = serde_json::json!({ "program": program, "arguments": arguments });
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .expect("open fake OpenSSH log");
        writeln!(file, "{line}").expect("write fake OpenSSH log");
    }

    if program == "scp" && arguments.len() >= 2 {
        let source = &arguments[arguments.len() - 2];
        let destination = &arguments[arguments.len() - 1];
        let remote = |value: &str| value.split_once(':').is_some_and(|(host, _)| host.len() > 1);
        if remote(source) {
            if let Some(fetch) = std::env::var_os("FAKE_OPENSSH_FETCH_SOURCE") {
                std::fs::copy(fetch, destination).expect("fake download");
            }
        } else if let Some(copy) = std::env::var_os("FAKE_OPENSSH_UPLOAD_COPY") {
            std::fs::copy(source, copy).expect("fake upload copy");
        }
    }

    if let Some(stdout) = std::env::var_os("FAKE_OPENSSH_STDOUT") {
        print!("{}", stdout.to_string_lossy());
    }
    if let Some(stderr) = std::env::var_os("FAKE_OPENSSH_STDERR") {
        eprint!("{}", stderr.to_string_lossy());
    }
    let code = std::env::var("FAKE_OPENSSH_EXIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
