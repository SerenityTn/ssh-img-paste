# Cross-platform architecture decision

**Status:** accepted for implementation on `feat/cross-platform-core`

**Date:** 2026-08-07

**Scope:** Windows and Linux editions; macOS v2.0.2 remains unchanged

## Decision

Build one contract-first Rust core and CLI for Windows and Linux.

- Keep stable macOS v2.0.2 on the existing Bash CLI and AppKit menu-bar app.
- Do not port the Darwin-specific Bash runtime directly to Linux.
- Share behavior through a frozen CLI/profile contract and adversarial fixtures before considering any macOS backend migration.
- Use installed OpenSSH executables and user SSH configuration; do not embed an SSH library or store credentials.
- Keep OS integration behind explicit adapters.
- Keep every GUI thin and invoke the authoritative CLI with argument arrays.

## Platform shells

### Windows

- WPF on the current .NET LTS.
- `System.Windows.Forms.NotifyIcon` for tray behavior.
- `ProcessStartInfo.ArgumentList` with `UseShellExecute=false` for CLI invocation.
- WIC for clipboard-image normalization and PNG encoding.
- Owned per-monitor selection overlay plus GDI capture for the first region-capture implementation.
- Self-contained x64/arm64 MSIX artifacts; signing is release-gated.

### Linux

- Rust GTK4/libadwaita desktop companion.
- `ashpd` screenshot and notification portals first.
- Wayland clipboard through `wl-paste`/`wl-copy`; X11 through `xclip` in the first CLI adapter, with all commands invoked as arrays.
- Optional StatusNotifierItem integration; core workflows must remain usable without tray support.
- XDG config/cache/state/runtime paths; reject relative XDG overrides.

## Rust core responsibilities

- Literal, non-executing profile parser using the existing `.env` schema.
- Profile validation and manual/read-only classification.
- Profile CRUD state transitions, locking, atomic persistence, and rollback behavior.
- Stable command grammar, TSV outputs, error semantics, and exit codes.
- Remote path, upload name, SSH argv, and SCP argv planning.
- Fail-before-side-effect guarantees.
- Bounded error sanitization.

## Adapter responsibilities

- Configuration directory discovery and filesystem security.
- Windows DACL/reparse-point protection.
- Unix permissions and descriptor-safe path operations.
- OpenSSH process execution, timeout, and process-tree cleanup.
- Clipboard acquisition, snapshot, path writing, guarded restoration, and transaction supersession.
- Screenshot acquisition and PNG normalization.
- Notifications and capability diagnostics.

## Contract migration sequence

1. Freeze macOS v2.0.2 grammar, outputs, exit codes, literal parser behavior, validation, and fail-before-side-effect rules.
2. Extract platform-neutral golden and adversarial fixtures.
3. Implement Rust parse, validate, inspect, and list behavior.
4. Add secure persistence and profile mutation differential tests.
5. Add remote planning and OpenSSH process adapters.
6. Add Windows image/clipboard/capture adapters.
7. Add Linux Wayland/X11/portal adapters.
8. Add thin native desktop shells only after platform CLIs pass the contract suite.
9. Require hosted CI and real target-device evidence before any Windows or Linux release.

## Python tracer disposition

The `xplat/` Python implementation is retained temporarily as an executable spike and contract-design aid. It proved:

- Literal profile persistence
- Cross-platform config-location expectations
- Argument-array SCP planning
- Adversarial path/host validation
- Packaging and console entry-point viability

It is not the production runtime and must not accumulate clipboard, screenshot, tray, or installer implementations. Remove it after equivalent Rust contract coverage exists.

## Security invariants

- Never evaluate profile files as shell, PowerShell, or code.
- Never construct a local shell command string from profile or path data.
- Treat remote commands as a separate shell boundary; interpolate only strictly allowlisted remote paths into constant commands.
- Reject traversal, controls, whitespace where prohibited, shell metacharacters, leading-option values, symlinks/reparse points, malformed IDs, and unsafe basenames before side effects.
- Preserve user SSH aliases, keys, agents, ports, and jump hosts in OpenSSH configuration.
- Restore clipboard content only when the transaction still owns the clipboard value; newer uploads supersede older timers.
- Use collision-resistant upload names and private temporary files.

## Release evidence labels

- **Agent-local Linux evidence:** Rust/Python tests, mocked adapters, package builds, fake OpenSSH process tests.
- **Hosted CI evidence:** Windows and Linux runner results after branch publication.
- **User-device evidence:** real Windows and graphical Linux clipboard, screenshot, tray, installer, and SSH tests.
- **Release evidence:** signed/published artifacts only after explicit approval.
