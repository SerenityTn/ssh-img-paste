# VPS Image Paste

A tiny macOS menu-bar app that sends the image on your clipboard to a remote
host over SSH in one click, then puts the uploaded file's remote path on your
clipboard so you can paste it straight into a terminal / SSH session.

It exists because clipboard **image** paste can't cross an SSH session — the
remote app reads the remote (headless) clipboard, not your Mac's. Pasting
**text** works fine, though, and many CLI/agent tools auto-attach any on-disk
file path they see. So this turns "clipboard image" into "clipboard path".

## Flow

1. Copy an image, or screenshot to clipboard with **⌘⌃⇧4** — *optional*
2. **Left-click the menu-bar icon** (📷) to upload to the active profile
3. In your SSH session, **⌘V** the path and send → the tool attaches the image

If there's **no image on the clipboard**, clicking the icon instead lets you
**drag to select a screen region** to upload (set `VPS_SHOT_MODE=full` for a
whole-display grab). The right-click menu also has explicit **Capture Region →
Profile…** and **Capture Full Screen → Profile…** items. Screenshots are named
`shot-*.png` vs `clip-*.png`.

The app only ever acts on an icon click — it has no global hotkey and does not
watch the clipboard, so your normal keyboard-shortcut screenshots/copies are
never touched. After an upload it puts the VPS path on the clipboard for the
⌘V-into-SSH step, then **restores your previous clipboard** (image or text)
after a grace window (`VPS_CLIP_RESTORE_SECONDS`, default 60s) so the link never
lingers to be pasted into a Mac app by mistake.

The icon shows ↑ while uploading, ✓ on success, ⚠️ on failure.

**Right-click** (or ⌥-click) the icon for a menu that also lets you choose and
manage destinations:

- **Destination: Profile** — submenu listing configured profiles. Choosing a
  profile runs `vps-img-paste profile use NAME`, so the selection is persistent
  and becomes the active profile for future left-click uploads, menu actions,
  and CLI commands that do not pass `--profile`. The submenu also includes
  **Manage Profiles…**; if no usable destination exists it offers
  **Add VPS Profile…** as empty-state recovery.
- **Upload Clipboard Image / Screenshot → Profile** — uploads to the currently
  active profile shown in the menu.
- **Capture Region → Profile…** and **Capture Full Screen → Profile** — capture
  and upload to the currently active profile shown in the menu.
- **Uploaded Images (N, size)** — submenu listing images on the active profile;
  click one to download and open it in Preview.
- **Clean All Uploads (N)…** — deletes every uploaded image on the active
  profile (with a confirmation; shown only when there are uploads).
- **Quit**.

### Manage Profiles window

**Manage Profiles…** opens the native profile manager. It supports:

- **Add** a VPS profile, **edit** profiles that can be safely rewritten,
  **duplicate**, **rename** with a dedicated rename action, **make active**,
  **test connection**, and **delete**.
- GUI-managed fields: immutable profile **ID** during normal editing, display
  label, SSH host/alias, absolute remote home, and relative upload folder.
  Advanced fields cover region/full screenshot mode and clipboard restore delay.
- Manual/custom profile files are parsed without executing shell code. Files
  with literal supported assignments plus unsupported extra statements remain
  selectable, testable, and deletable, but are read-only in the GUI and offer
  **Open in Text Editor** instead of pretending every shell expression is
  editable. Dynamic values such as `VPS_HOST="${USER}@host"` are rejected; write
  the literal target instead (for example `VPS_HOST="me@host"`).
- Deleting the active profile requires choosing another usable profile first.
  Deleting the last usable profile is blocked. Profile deletion removes only the
  local profile configuration; it never deletes remote uploaded images.

Security boundary: SSH passwords, private keys, custom ports, and ProxyJump/jump
host settings are **never** stored or managed by VPS Image Paste. Keep them in
`~/.ssh/config`. **Test Connection** is read-only and runs SSH in `BatchMode` so
it is noninteractive and will not prompt for passwords.

## CLI

```sh
vps-img-paste                       # upload to active profile
vps-img-paste --profile work upload # upload once to profile "work"
vps-img-paste list                  # SIZE<TAB>NAME on active profile, newest first
vps-img-paste fetch NAME            # download NAME from active profile, print path
vps-img-paste clean                 # delete active profile's uploaded images

vps-img-paste profiles              # list configured profiles
vps-img-paste profile current       # print active profile
vps-img-paste profile create work \
  --label "Work VPS" \
  --host work-vps \
  --remote-home /home/me \
  --remote-dir img-uploads
vps-img-paste profile create fallback \
  --label "Fallback VPS" \
  --host fallback-vps \
  --remote-home /home/me \
  --remote-dir img-uploads
vps-img-paste profile create old-client \
  --label "Old Client VPS" \
  --host old-client-vps \
  --remote-home /home/me \
  --remote-dir img-uploads
vps-img-paste profile use work      # make "work" the active profile
vps-img-paste profile inspect work  # show non-secret profile configuration
vps-img-paste profile update work --label "Work Laptop" --shot-mode full
vps-img-paste profile rename work client-a
vps-img-paste profile test client-a # read-only, BatchMode SSH connectivity check
vps-img-paste profile delete old-client
vps-img-paste profile delete client-a --switch-to fallback
```

`--profile NAME` is an explicit one-command override; it does not change the
active profile. Left-click uploads, `list`, `fetch`, and `clean` use the active
profile unless you pass an explicit profile override. Profile create/update
commands manage only app-owned destination fields; do not put credentials,
private-key paths, ports, or jump-host details in profile files. CLI deletion
uses the same safeguards as the GUI: deleting an inactive profile, such as
`old-client` above, can omit `--switch-to`; deleting the active profile requires
a usable replacement (`profile delete client-a --switch-to fallback`), and
deleting the last usable profile is blocked.

> **Screen Recording permission:** the screenshot fallback needs it. The first
> time it fires, grant it under **System Settings → Privacy & Security → Screen
> Recording** for *VPS Image Paste*. Until then, a screenshot is blank/windowless.

## Install (Homebrew)

```sh
brew install SerenityTn/tap/vps-img-paste
```

Then configure your default profile and start the menu-bar app:

```sh
mkdir -p ~/.config/vps-img-paste/profiles
cp "$(brew --prefix)/share/vps-img-paste/vps-img-paste.env.example" \
  ~/.config/vps-img-paste/profiles/default.env
printf 'default\n' > ~/.config/vps-img-paste/active-profile
$EDITOR ~/.config/vps-img-paste/profiles/default.env  # set VPS_HOST / VPS_REMOTE_HOME

ssh your-vps-alias 'mkdir -p ~/img-uploads'     # create the upload dir
brew services start vps-img-paste              # run now + at login
```

Upgrade later with `brew upgrade vps-img-paste`.

### Install from source (no Homebrew)

```sh
git clone https://github.com/SerenityTn/vps-img-paste
cd vps-img-paste
./install.sh          # builds app to ~/Applications, symlinks CLI to ~/bin, login agent
$EDITOR ~/.config/vps-img-paste/profiles/default.env
```

On a fresh source install, `install.sh` seeds
`~/.config/vps-img-paste/profiles/default.env` and
`~/.config/vps-img-paste/active-profile`. If it finds either the legacy config
or existing named profiles, it leaves them untouched.

## Configuration

Named profiles live in:

```text
~/.config/vps-img-paste/profiles/<id>.env
~/.config/vps-img-paste/active-profile   # contains the active <id>
```

Each GUI-managed profile is a small shell env file:

```sh
VPS_PROFILE_LABEL="Work VPS"      # optional; shown in menus/listings
VPS_HOST="work-vps"               # user@host or ~/.ssh/config alias
VPS_REMOTE_HOME="/home/me"        # absolute remote home for pasted paths
VPS_REMOTE_DIR="img-uploads"      # optional; relative upload folder
VPS_SHOT_MODE="region"            # optional; region or full
VPS_CLIP_RESTORE_SECONDS="60"     # optional; 0 keeps the pasted path
```

The filename is the profile ID (`work.env` → `work`). The app treats that ID as
immutable during normal editing; use the dedicated rename action/CLI command to
change it. `VPS_REMOTE_HOME` must be the absolute destination base (normally the
SSH account's home, such as `/home/sysadmin`). `VPS_REMOTE_DIR` is appended to
it. Uploads, listing, fetching, cleanup, and the copied path all use that same
absolute directory, so do not leave the example `/home/user` placeholder in a
new profile.

Profile files are parsed as data, not sourced as shell scripts. Supported fields
must be literal assignments (quoted or unquoted): `VPS_HOST="me@work-vps"`, not
`VPS_HOST="${USER}@work-vps"`, backticks, `$(...)`, or other shell expansion. If
a legacy/manual profile contains commands, `source`, `export`, functions, or
unsupported syntax in addition to valid literal values for the supported fields,
the CLI can still select, inspect, test, and use it, but the GUI marks it manual
and read-only. To migrate, replace dynamic supported values with their resolved
literal values, move SSH options such as user, port, keys, and jump hosts into
`~/.ssh/config`, then use `vps-img-paste profile inspect NAME` and
`vps-img-paste profile test NAME` to verify the cleaned profile.

Example with two destinations:

```sh
mkdir -p ~/.config/vps-img-paste/profiles
cp vps-img-paste.env.example ~/.config/vps-img-paste/profiles/work.env
cp vps-img-paste.env.example ~/.config/vps-img-paste/profiles/personal.env
$EDITOR ~/.config/vps-img-paste/profiles/work.env
$EDITOR ~/.config/vps-img-paste/profiles/personal.env
vps-img-paste profile use work
```

The old single-file config at `~/.config/vps-img-paste.env` is still supported
as backward-compatible profile `default`. It is never moved or deleted by the
installer. While that legacy file exists, it takes precedence over
`profiles/default.env`: the manager may show `profiles/default.env` as shadowed,
but runtime uploads for profile `default` still read the legacy file. If you
choose to delete the legacy `default` profile, the app warns that this removes
the old local file and can reveal the previously shadowed `profiles/default.env`
as the new default. To add named destinations without removing the legacy file,
create a different ID such as `profiles/work.env`, then run
`vps-img-paste profile use work`.

Put SSH details in `~/.ssh/config`, not in profile env files:

```sshconfig
Host work-vps
  HostName 203.0.113.10
  User me
  Port 2222
  IdentityFile ~/.ssh/work_ed25519
  ProxyJump bastion
```

Then set `VPS_HOST="work-vps"`. This keeps ports, keys, and jump hosts in the
standard SSH place and lets `ssh`, `scp`, and this app share the same alias.

## Components

| Path | What |
|------|------|
| `bin/vps-img-paste` | The upload script (clipboard image → scp → clipboard path). Works standalone in a terminal too. |
| `src/VpsImgPaste.swift` | The AppKit menu-bar app; on click it runs `~/bin/vps-img-paste`. |
| `build.sh` | Compiles the app into `~/Applications/VpsImgPaste.app`. |
| `install.sh` / `uninstall.sh` | Set up / tear down the symlink, app, and LaunchAgent. |
| `vps-img-paste.env.example` | Template for legacy config or named profile env files. |

## Requirements

- macOS 13+ (Apple Silicon or Intel)
- Swift toolchain (Xcode Command Line Tools): `xcode-select --install`
- [`pngpaste`](https://github.com/jcsalterego/pngpaste) (installed automatically by `install.sh`)
- SSH key access to the host

## Rebuild after editing

```sh
./build.sh && launchctl kickstart -k gui/$(id -u)/com.khaireddine.vpsimgpaste
```
