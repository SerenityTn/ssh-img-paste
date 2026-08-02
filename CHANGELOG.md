# Changelog

This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Dates use `YYYY-MM-DD`.

## [Unreleased]

## [2.0.0] - 2026-08-02

### Changed

- Completed the breaking SSH-only identity across the app, CLI, profile schema, bundle identifier, LaunchAgent, tests, documentation, and Homebrew formula.
- Configuration now uses named profiles exclusively under `~/.config/ssh-img-paste/profiles/`.
- Changed the bundle and LaunchAgent identifier to `com.khaireddine.sshimagepaste`; macOS will request Screen Recording permission again.

### Removed

- Removed all former command aliases, configuration paths, environment variables, package migrations, and runtime fallbacks.
- Removed single-file and environment-only profile discovery.

## [1.3.0] - 2026-08-02

### Changed

- Renamed the public product, app, CLI, repository, and Homebrew formula to SSH Image Paste / `ssh-img-paste`.
- Added SSH-branded installation and configuration paths.

## [1.2.0] - 2026-08-01

### Added

- Native profile manager for creating, editing, duplicating, renaming, activating, testing, and deleting SSH destinations.
- Secure CLI profile CRUD commands and literal, non-executing profile parsing.
- Named destinations with persistent active-profile selection and one-command profile overrides.
- App icon, About panel, GitHub menu link, accessibility labels, and release version metadata.
- Automated macOS CI, public contribution guidance, security policy, and issue/PR templates.

### Changed

- Upload, list, fetch, and cleanup now share one validated absolute remote root.
- Profile management and uploaded-image refresh run outside the AppKit main thread.
- The profile manager uses aligned native controls and separates initial loading from unsaved user edits.
- Source installation removes obsolete Homebrew launch paths that could reopen an older app.
- Ad-hoc builds use a stable bundle requirement so Screen Recording permission survives rebuilds.

### Security

- Profile writes are atomic, lock-protected, mode-restricted, and symlink-safe.
- SSH/SCP inputs, fetched filenames, machine-readable output, notification text, and restore-delay bounds receive stricter validation.
- Process output capture and timeout termination avoid pipe deadlocks and lingering child processes.

## [1.1.0] - 2026-07-05

- Added region selection as the default screenshot fallback.
- Added explicit region and full-screen capture menu actions.

## [1.0.0] - 2026-07-05

- First public release of the menu-bar app and CLI.
- Added clipboard image upload, screenshot fallback, remote image browsing and cleanup, clipboard restoration, Homebrew packaging, and MIT licensing.

[Unreleased]: https://github.com/SerenityTn/ssh-img-paste/compare/v2.0.0...HEAD
[2.0.0]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.3.0...v2.0.0
[1.3.0]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/SerenityTn/ssh-img-paste/releases/tag/v1.0.0
