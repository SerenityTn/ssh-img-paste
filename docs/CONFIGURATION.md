# Profile configuration

The native profile manager is the recommended way to add and edit destinations. Secondary-click the menu-bar icon, open **Destination**, then choose **Manage Profiles**.

## Storage

Named profiles use:

```text
~/.config/ssh-img-paste/profiles/<id>.env
~/.config/ssh-img-paste/active-profile
```

`XDG_CONFIG_HOME` replaces `~/.config` when set. Profile files are private (`0600`), and the profile directory is private (`0700`).

A profile ID must match `[A-Za-z0-9][A-Za-z0-9_-]*`. The ID is the filename and the value used by CLI commands. Rename profiles through the app or CLI rather than moving files while the app is running.

## Supported fields

```sh
SSH_PROFILE_LABEL="Work SSH Host"
SSH_HOST="work-ssh"
SSH_REMOTE_HOME="/home/me"
SSH_REMOTE_DIR="img-uploads"
SSH_SHOT_MODE="region"
SSH_CLIP_RESTORE_SECONDS="60"
```

| Field | Required | Meaning |
| --- | --- | --- |
| `SSH_PROFILE_LABEL` | No | Name shown in the app. Defaults to the profile ID. |
| `SSH_HOST` | Yes | OpenSSH host alias or `user@host`. |
| `SSH_REMOTE_HOME` | Yes | Absolute remote base used in copied paths. |
| `SSH_REMOTE_DIR` | No | Relative upload folder. Defaults to `img-uploads`. |
| `SSH_SHOT_MODE` | No | `region` or `full`; defaults to `region`. |
| `SSH_CLIP_RESTORE_SECONDS` | No | `0` through `86400`; defaults to `60`. |

Upload, list, fetch, and cleanup all use the same validated absolute root: `SSH_REMOTE_HOME/SSH_REMOTE_DIR`.

## SSH setup

Keep SSH authentication and transport settings outside profile files:

```sshconfig
Host work-ssh
  HostName 203.0.113.10
  User me
  Port 2222
  IdentityFile ~/.ssh/work_ed25519
  ProxyJump bastion
```

Then use `SSH_HOST="work-ssh"`. OpenSSH, the macOS keychain, and your SSH agent remain responsible for keys and passphrases. SSH Image Paste does not store passwords or private keys.

Create the upload directory once:

```sh
ssh work-ssh 'mkdir -p ~/img-uploads'
```

If `SSH_REMOTE_HOME` is not the login account's real home, create and authorize the configured absolute directory instead.

## Literal parser contract

Profile files are parsed as data, not executed as shell scripts. Supported fields must be literal assignments, quoted or unquoted.

Accepted:

```sh
SSH_HOST="work-ssh"
SSH_REMOTE_HOME=/home/me
```

Rejected for supported fields:

```sh
SSH_HOST="${USER}@work-ssh"
SSH_REMOTE_HOME="$(ssh work-ssh pwd)"
```

Backticks, command substitution, variable expansion, functions, `source`, and `eval` are not evaluated. A profile with literal supported values plus extra unsupported statements remains usable but appears as **Manual** and read-only in the GUI. Replace dynamic supported values with resolved literals before editing that profile in the app.

Verify a profile without uploading:

```sh
ssh-img-paste profile inspect work
ssh-img-paste profile test work
```

`profile test` uses noninteractive SSH batch mode.

Deleting a local profile never deletes remote uploads. Remote files are removed only through the explicit **Clean All Uploads** action or `ssh-img-paste clean`.
