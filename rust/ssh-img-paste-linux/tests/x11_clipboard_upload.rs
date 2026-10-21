//! End-to-end coverage of the clipboard upload on a real X11 clipboard.
//!
//! Needs `DISPLAY` (for example under `xvfb-run`) and `xclip`; it skips
//! otherwise. The upload half also needs the production CLI and its
//! `fake_openssh` example, passed as absolute paths in
//! `SSH_IMG_PASTE_E2E_CLI` and `SSH_IMG_PASTE_E2E_FAKE_OPENSSH`.

use ssh_img_paste_linux::cli::CliClient;
use ssh_img_paste_linux::clipboard::{Backend, ClipboardAccess, SystemClipboard};
use ssh_img_paste_linux::staging::StagingArea;
use ssh_img_paste_linux::upload::{UploadOutcome, upload_clipboard};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\rIDATx\x9cc\xf8\xcf\xc0\xf0\x1f\0\x05\0\x01\xff\x89\x99=\x1d\0\0\0\0IEND\xaeB`\x82";

/// X11 clipboard only; the session's Wayland tools are deliberately ignored.
fn x11_clipboard() -> Option<SystemClipboard> {
    let clipboard = SystemClipboard::detect(
        |name| match name {
            "WAYLAND_DISPLAY" => None,
            other => std::env::var_os(other),
        },
        None,
    );
    match clipboard {
        Ok(clipboard) if clipboard.backend() == Backend::X11 => Some(clipboard),
        _ => {
            eprintln!("skipping: needs DISPLAY and xclip");
            None
        }
    }
}

fn copy_image(path: &Path) {
    let status = Command::new("xclip")
        .args(["-selection", "clipboard", "-target", "image/png", "-in"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run xclip");
    assert!(status.success());
}

fn clipboard_text() -> Option<String> {
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-out"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// xclip takes the selection from a forked child, so ownership can lag.
fn eventually<T>(mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "clipboard did not settle");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn install_executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write script");
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("chmod");
}

fn e2e_binaries() -> Option<(PathBuf, PathBuf)> {
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_file())
    };
    let binaries =
        absolute("SSH_IMG_PASTE_E2E_CLI").zip(absolute("SSH_IMG_PASTE_E2E_FAKE_OPENSSH"));
    if binaries.is_none() {
        eprintln!("skipping upload: set SSH_IMG_PASTE_E2E_CLI and SSH_IMG_PASTE_E2E_FAKE_OPENSSH");
    }
    binaries
}

// One test so the two halves never race on the shared X selection.
#[test]
fn x11_clipboard_image_uploads_and_the_remote_path_is_copied() {
    let Some(clipboard) = x11_clipboard() else {
        return;
    };
    let root = tempfile::tempdir().expect("tempdir");
    let image = root.path().join("clip.png");
    std::fs::write(&image, PNG).expect("image");

    // Adapter round trip: PNG in, text out.
    copy_image(&image);
    let read = eventually(|| clipboard.read_png().expect("read clipboard"));
    assert_eq!(read, PNG);
    clipboard
        .set_text("/home/me/img-uploads/round-trip.png")
        .expect("set text");
    eventually(|| clipboard_text().filter(|text| text == "/home/me/img-uploads/round-trip.png"));
    assert_eq!(clipboard.read_png().expect("read clipboard"), None);

    let Some((cli, fake_openssh)) = e2e_binaries() else {
        return;
    };
    let openssh = root.path().join("openssh");
    std::fs::create_dir(&openssh).expect("openssh dir");
    for program in ["ssh", "scp"] {
        std::fs::copy(&fake_openssh, openssh.join(program)).expect("install fake OpenSSH");
    }
    let config = root.path().join("config");
    std::fs::create_dir_all(config.join("profiles")).expect("profiles");
    std::fs::write(
        config.join("profiles").join("work.env"),
        "SSH_PROFILE_LABEL='Work VPS'\nSSH_HOST='work-ssh'\nSSH_REMOTE_HOME='/home/me'\nSSH_REMOTE_DIR='img-uploads'\n",
    )
    .expect("profile");
    let transferred = root.path().join("transferred.png");
    let wrapper = root.path().join("ssh-img-paste");
    install_executable(
        &wrapper,
        &format!(
            "#!/bin/sh\nexport SSH_IMG_PASTE_OPENSSH_DIR='{}'\nexport FAKE_OPENSSH_UPLOAD_COPY='{}'\nexec '{}' \"$@\"\n",
            openssh.display(),
            transferred.display(),
            cli.display()
        ),
    );

    copy_image(&image);
    eventually(|| clipboard.read_png().expect("read clipboard"));
    let client = CliClient::new(wrapper, Some(config));
    let staging = StagingArea::at(root.path().join("staging"));
    let outcome = upload_clipboard(&client, Ok(&clipboard), &staging);

    let UploadOutcome::Uploaded {
        remote_path,
        destination,
        path_copied,
    } = outcome
    else {
        panic!("upload failed: {outcome:?}");
    };
    assert!(
        remote_path.starts_with("/home/me/img-uploads/ssh-img-"),
        "{remote_path}"
    );
    assert_eq!(destination, "Work VPS");
    assert!(path_copied);
    assert_eq!(std::fs::read(&transferred).expect("transferred bytes"), PNG);
    eventually(|| clipboard_text().filter(|text| *text == remote_path));
    let leftovers = std::fs::read_dir(staging.directory())
        .expect("staging")
        .count();
    assert_eq!(leftovers, 0, "staging file must be removed");
}
