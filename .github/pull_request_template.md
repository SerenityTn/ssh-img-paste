## Summary

<!-- What changed, and what user problem does it solve? -->

## Verification

- [ ] `make check` passes on macOS
- [ ] New behavior has regression coverage
- [ ] User-visible changes are recorded in `CHANGELOG.md`
- [ ] Documentation and examples contain no real hosts, usernames, IP addresses, local paths, keys, tokens, or uploaded filenames

## Security review

- [ ] User-controlled values are passed as arguments, not interpolated into shell or AppleScript source
- [ ] Profile files remain literal data and are never sourced or evaluated
- [ ] SSH credentials remain outside app-managed profile files
- [ ] Destructive actions confirm and execute against the same snapshotted profile
- [ ] File writes preserve atomicity, private modes, locking, and symlink protections

## Screenshots

<!-- For UI changes, use fixture data only. Omit this section for non-visual changes. -->
