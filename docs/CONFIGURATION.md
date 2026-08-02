# Profile configuration

The native profile manager is the recommended way to add and edit destinations. Secondary-click the menu-bar icon, open **Destination**, then choose **Manage Profiles**.

## Storage

Named profiles use:

```text
~/.config/vps-img-paste/profiles/<id>.env
~/.config/vps-img-paste/active-profile
```

`XDG_CONFIG_HOME` replaces `~/.config` when set. Profile files are private (`0600`), and the profile directory is private (`0700`).

A profile ID must match `[A-Za-z0-9][A-Za-z0-9_-]*`. The ID is the filename and the value used by CLI commands. Rename profiles through the app or CLI rather than moving files while the app is running.

## Supported fields

```sh
VPS_PROFILE_LABEL="Work VPS"
VPS_HOST="work-vps"
VPS_REMOTE_HOME="/home/me"
VPS_REMOTE_DIR="img-uploads"
VPS_SHOT_MODE="region"
VPS_CLIP_RESTORE_SECONDS="60"
```

| Field | Required | Meaning |
| --- | --- | --- |
| `VPS_PROFILE_LABEL` | No | Name shown in the app. Defaults to the profile ID. |
| `VPS_HOST` | Yes | OpenSSH host alias or `user@host`. |
| `VPS_REMOTE_HOME` | Yes | Absolute remote base used in copied paths. |
| `VPS_REMOTE_DIR` | No | Relative upload folder. Defaults to `img-uploads`. |
| `VPS_SHOT_MODE` | No | `region` or `full`; defaults to `region`. |
| `VPS_CLIP_RESTORE_SECONDS` | No | `0` through `86400`; defaults to `60`. |

Upload, list, fetch, and cleanup all use the same validated absolute root: `VPS_REMOTE_HOME/VPS_REMOTE_DIR`.

## SSH setup

Keep SSH authentication and transport settings outside profile files:

```sshconfig
Host work-vps
  HostName 203.0.113.10
  User me
  Port 2222
  IdentityFile ~/.ssh/work_ed25519
  ProxyJump bastion
```

Then use `VPS_HOST="work-vps"`. OpenSSH, the macOS keychain, and your SSH agent remain responsible for keys and passphrases. VPS Image Paste does not store passwords or private keys.

Create the upload directory once:

```sh
ssh work-vps 'mkdir -p ~/img-uploads'
```

If `VPS_REMOTE_HOME` is not the login account's real home, create and authorize the configured absolute directory instead.

## Literal parser contract

Profile files are parsed as data, not executed as shell scripts. Supported fields must be literal assignments, quoted or unquoted.

Accepted:

```sh
VPS_HOST="work-vps"
VPS_REMOTE_HOME=/home/me
```

Rejected for supported fields:

```sh
VPS_HOST="${USER}@work-vps"
VPS_REMOTE_HOME="$(ssh work-vps pwd)"
```

Backticks, command substitution, variable expansion, functions, `source`, and `eval` are not evaluated. A legacy file with literal supported values plus extra unsupported statements remains usable but appears as **Manual** and read-only in the GUI. Replace dynamic supported values with resolved literals before editing that profile in the app.

Verify a migrated profile without uploading:

```sh
vps-img-paste profile inspect work
vps-img-paste profile test work
```

`profile test` uses noninteractive SSH batch mode.

## Legacy default profile

The original single-file config remains supported as profile `default`:

```text
~/.config/vps-img-paste.env
```

When it exists, it takes precedence over `profiles/default.env`. The manager marks a shadowed named default profile accordingly. Add named profiles with other IDs before removing the legacy file. The installer never moves or deletes existing configuration.

Deleting a local profile never deletes remote uploads. Remote files are removed only through the explicit **Clean All Uploads** action or `vps-img-paste clean`.
