# Security policy

## Reporting a vulnerability

Please report security problems privately through [GitHub Security Advisories](https://github.com/SerenityTn/vps-img-paste/security/advisories/new). Do not include exploit details, credentials, private hostnames, or private keys in a public issue.

Include the affected version, installation method, macOS version, reproduction steps, impact, and any suggested mitigation. Maintainers will acknowledge reports as time permits and coordinate disclosure after a fix is available.

## Supported versions

Security fixes target the latest tagged release and `main`. Older Homebrew installations should upgrade before reporting an issue:

```sh
brew update
brew upgrade vps-img-paste
```

## Security model

VPS Image Paste is a local macOS utility that sends PNG files to a host you configure over OpenSSH.

- Profile files store destination labels, an SSH host or alias, and remote paths. They do not store passwords or private keys.
- SSH authentication and transport settings remain with OpenSSH, the macOS keychain, and the SSH agent.
- Supported profile fields are parsed as literal assignments. Profile files are never sourced or evaluated as shell code.
- AppKit passes CLI arguments as an array rather than interpolating them into a shell command.
- GUI-managed profile writes are atomic, lock-protected, symlink-safe, and private to the current user.
- Connection tests use SSH batch mode and do not prompt for credentials.
- Remote cleanup requires confirmation and deletes only PNG uploads in the selected profile's configured upload directory.
- Screen Recording access is requested only for explicit screenshot capture. Clipboard content is read when the user invokes an upload.

You are responsible for trusting and securing the configured SSH host. A compromised local account, compromised SSH client configuration, or malicious remote host is outside the app's protection boundary.

## Build provenance

Source and Homebrew builds are compiled locally and ad-hoc signed. The build uses a stable designated requirement so macOS can preserve Screen Recording permission across rebuilds, but ad-hoc signing is not Apple notarization and does not prove publisher identity. Review the source and release tag before installing.
