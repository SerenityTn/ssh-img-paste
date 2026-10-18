# Literal profile contract fixtures

These files freeze the macOS v2.0.2 parser boundary for the Windows/Linux Rust core.

| Fixture | Expected result |
|---|---|
| `app-literal.env` | Parse successfully; editable/app-owned; ignore the literal unknown key while retaining supported values. |
| `manual-command.env` | Parse supported literals without executing the command; classify as manual/read-only. |
| `dynamic-supported.env` | Fail with the supported key `SSH_HOST`; no side effects. |
| `manual-dynamic-extra.env` | Parse supported literals; classify as manual/read-only because unsupported dynamic syntax exists. |

The parser is data-only. It must never source a fixture or invoke a shell.

Escaped double-quoted values are intentionally not frozen by these fixtures yet: the Linux Bash audit exposed version-sensitive behavior in the existing helper. Freeze that case only after a macOS v2.0.2 differential run.
