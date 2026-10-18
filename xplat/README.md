# SSH Image Paste — Windows/Linux contract tracer

This directory contains an **experimental contract spike**, not the production cross-platform runtime. It does not replace the stable macOS v2.0.2 CLI or AppKit app. The accepted production architecture is documented in [`docs/CROSS_PLATFORM_ARCHITECTURE.md`](../docs/CROSS_PLATFORM_ARCHITECTURE.md): a Rust core/CLI for Windows and Linux with thin native desktop shells.

## What works now

- Literal, non-executable JSON profiles.
- Windows `%LOCALAPPDATA%` and Linux XDG config-location rules in the shared core.
- Explicit PNG-file upload planning through `scp`.
- SSH/SCP invocation as an argument array, never a shell command string.
- Timestamped, collision-resistant remote PNG names.
- Validation for profile IDs, SSH targets, remote homes, remote directories, and remote filenames.
- A runnable Python module and installable console entry point.

Clipboard-image capture, screenshots, path clipboard writing, tray UI, installers, and Windows-device verification are still under development.

## Development run

```bash
python3 -m xplat.ssh_image_paste \
  --config-dir /tmp/ssh-img-paste-xplat \
  profile create work \
  --label "Work host" \
  --host work-ssh \
  --remote-home /home/me \
  --remote-dir img-uploads

python3 -m xplat.ssh_image_paste \
  --config-dir /tmp/ssh-img-paste-xplat \
  --profile work \
  upload-file ./image.png
```

The host can be an SSH config alias. The remote upload directory must already exist.

## Tests

```bash
python3 -m unittest tests.test_xplat -v
python3 -m unittest tests.test_linux_e2e -v  # Ubuntu/Linux only
```

The ordinary tests cover Windows/Linux config paths, literal profile persistence, adversarial path and host rejection, argument-array SCP construction, file upload execution, and the module entry point.

The Ubuntu end-to-end test starts a real Xvfb display, captures a real PNG with `scrot`, creates a profile through the CLI, starts an ephemeral unprivileged OpenSSH server, uploads through the real `scp` client, and verifies the destination bytes. It requires `openssh-server`, `scrot`, and `xvfb`. This is headless X11 and transport evidence—not GNOME/Wayland portal, clipboard, notification, tray, or installer acceptance.

## Current product boundary

This tracer proves the shared security-sensitive core before OS-specific UI work. Do not publish it as a stable Windows or Linux release yet.
