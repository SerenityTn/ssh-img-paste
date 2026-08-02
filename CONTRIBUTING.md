# Contributing to SSH Image Paste

Thanks for helping improve SSH Image Paste. Small, focused changes are easiest to review and safest for a utility that handles screenshots, clipboard contents, local configuration, and remote deletion.

## Before you start

- Search the existing issues before opening a new one.
- Use a feature request for behavior changes and a bug report for regressions.
- For a security issue, follow [SECURITY.md](SECURITY.md). Do not open a public issue.
- Keep pull requests focused. Unrelated cleanup belongs in a separate change.

## Development setup

You need macOS 13 or newer and the Xcode Command Line Tools:

```sh
xcode-select --install
brew install shellcheck pngpaste
git clone https://github.com/SerenityTn/ssh-img-paste.git
cd ssh-img-paste
make check
```

`make check` runs the Swift model/process tests, isolated shell integration tests, installer tests, app build/signature checks, Bash syntax checks, and ShellCheck. The integration tests use temporary homes and mocked SSH, SCP, clipboard, screenshot, and LaunchAgent commands. They do not contact a real server.

Build the app without installing it:

```sh
make build
```

Install your working copy for local testing:

```sh
./install.sh
```

This replaces the source-installed app in `~/Applications` and reloads its LaunchAgent. It preserves existing profile configuration.

## Architecture and security boundaries

- `bin/ssh-img-paste` owns profile parsing, validation, persistence, active-profile resolution, and remote operations. `bin/vps-img-paste` is the compatibility entry point.
- The AppKit code calls the CLI with argument arrays. It must not build shell command strings from user input.
- Profile files are parsed as literal data. Do not reintroduce `source`, `eval`, or executable config semantics.
- SSH credentials and transport options stay in OpenSSH configuration, the keychain, and the SSH agent. The app stores only a destination host or alias and remote paths.
- Profile writes must remain atomic, lock-protected, symlink-safe, and mode `0600`; the profile directory remains mode `0700`.
- Any destructive action must use the same snapshotted profile in its confirmation and execution paths.
- Keep Bash code compatible with the macOS system Bash 3.2.

Changes that touch profile parsing, SSH/SCP arguments, AppleScript, file permissions, locking, clipboard restoration, screenshot permissions, or deletion need adversarial regression coverage.

## Pull requests

1. Create a branch from `main`.
2. Add or update tests for behavior changes.
3. Run `make check`.
4. Update `CHANGELOG.md` for user-visible changes.
5. Open a pull request and complete the template.

Use clear commit subjects such as `fix: prevent duplicate menu-bar instances` or `docs: clarify SSH profile setup`.

## Updating visual assets

Regenerate the app icon after editing `assets/AppIcon.svg`:

```sh
make icon
```

Regenerate the README profile-manager screenshot with safe fixture data:

```sh
make screenshot
```

Never publish screenshots containing real hosts, usernames, home directories, profile paths, or uploaded filenames.
