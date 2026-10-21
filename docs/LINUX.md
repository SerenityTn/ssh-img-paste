# Linux edition (development preview)

The Linux edition is a GTK 4 / libadwaita app with a StatusNotifierItem tray icon. Like the Windows edition, it is a thin shell over the Rust CLI (`ssh-img-paste`), which owns profiles and every SSH step.

## What works today

- **Upload Clipboard Image**: copy an image, then click the tray icon, choose **Upload Clipboard Image**, or run `ssh-img-paste-linux --upload-clipboard`. The app uploads it to the active destination and replaces the clipboard with the remote path.
- PNG is preferred. Other image types offered by the clipboard (JPEG, BMP, …) are converted to PNG first.
- A desktop notification and the app window report the result, with actionable messages for common SSH failures.

Not yet: region and full-screen capture, the profile editor, restoring the previous clipboard, and packages. Create destinations with the CLI meanwhile.

## Requirements

- A Wayland or X11 session. Clipboard access uses `wl-clipboard` on Wayland and `xclip` on X11:

  ```bash
  sudo apt install wl-clipboard xclip openssh-client
  ```

- Key-based SSH access to the destination (`ssh-agent` or `IdentityFile`); passphrase prompts are not supported.
- For the tray icon on GNOME, the AppIndicator extension. Without a tray host, the window stays open as the fallback.

## Build and run from source

```bash
cargo build --manifest-path rust/ssh-img-paste-cli/Cargo.toml --release
cargo build --manifest-path rust/ssh-img-paste-linux/Cargo.toml --release
export SSH_IMG_PASTE_CLI="$PWD/rust/ssh-img-paste-cli/target/release/ssh-img-paste"

# One-time: add a destination and make it active.
"$SSH_IMG_PASTE_CLI" profiles create work --label "Work VPS" --host work-ssh \
  --remote-home /home/me --remote-dir img-uploads
"$SSH_IMG_PASTE_CLI" profiles use work
"$SSH_IMG_PASTE_CLI" profiles test work

rust/ssh-img-paste-linux/target/release/ssh-img-paste-linux
```

The app finds the CLI through `SSH_IMG_PASTE_CLI` (an absolute path), then next to its own executable, then on `PATH`. `SSH_IMG_PASTE_CONFIG_ROOT` overrides the settings root, as on Windows.

## Keyboard shortcut

Wayland does not let apps grab global shortcuts, so bind one in your desktop settings instead. For example, in GNOME open **Settings → Keyboard → Custom Shortcuts**, then set:

- **Command:** `/path/to/ssh-img-paste-linux --upload-clipboard`

The command hands the request to the running instance over D-Bus, or starts the app if it is not running.

| Option | Effect |
|---|---|
| *(none)* | Open the app, or show the running instance |
| `--upload-clipboard` | Upload the clipboard image and copy its remote path |
| `--quit` | Quit the running instance |
| `--help` | Show usage |

## Privacy

The clipboard image goes to a private staging file (`0600`, in a `0700` directory under `$XDG_RUNTIME_DIR`, or `~/.cache` as a fallback). The file is removed as soon as the CLI has taken its own copy. Leftovers from a crash are removed at the next start.

## Testing

```bash
# Unit tests, plus the X11 clipboard end-to-end test when a display is present.
cargo build --manifest-path rust/ssh-img-paste-cli/Cargo.toml --bin ssh-img-paste --example fake_openssh
SSH_IMG_PASTE_E2E_CLI="$PWD/rust/ssh-img-paste-cli/target/debug/ssh-img-paste" \
SSH_IMG_PASTE_E2E_FAKE_OPENSSH="$PWD/rust/ssh-img-paste-cli/target/debug/examples/fake_openssh" \
  xvfb-run -a cargo test --manifest-path rust/ssh-img-paste-linux/Cargo.toml

# Desktop end-to-end: a forwarded --upload-clipboard against the running app.
cargo build --manifest-path rust/ssh-img-paste-linux/Cargo.toml
dbus-run-session -- xvfb-run -a tests/test-linux-desktop.sh
```

These runs use a fake OpenSSH. They have not yet been verified on a real Wayland compositor (GNOME or KDE) or with a live SSH host.
