# Changelog

This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Dates use `YYYY-MM-DD`.

## [Unreleased]

## [1.3.0] - 2026-08-02

### Changed

- Renamed the public product, app, CLI, repository, and Homebrew formula to SSH Image Paste / `ssh-img-paste`.
- Fresh installations use `~/.config/ssh-img-paste`, while existing VPS Image Paste 1.x configuration is detected and used in place.

### Compatibility

- Retained `vps-img-paste` as a CLI compatibility entry point.
- Retained the existing profile schema, bundle identifier, and LaunchAgent label so upgrades preserve profiles and macOS Screen Recording permission.

## [1.2.0] - 2026-08-01

### Added

- Native profile manager for creating, editing, duplicating, renaming, activating, testing, and deleting VPS destinations.
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

[Unreleased]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/SerenityTn/ssh-img-paste/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/SerenityTn/ssh-img-paste/releases/tag/v1.0.0
