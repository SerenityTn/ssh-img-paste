#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "${TMP_ROOT%/}/ssh-img-paste-check.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

"$ROOT/tests/test-ssh-img-paste.sh"
"$ROOT/tests/test-profile-management.sh"
"$ROOT/tests/test-install.sh"
"$ROOT/tests/test-uninstall.sh"
"$ROOT/tests/test-cross-platform-contract.sh"
"$ROOT/tests/test-build-signature.sh"

swiftc -target "$(uname -m)-apple-macos13.0" \
  "$ROOT/src/ProfileModels.swift" \
  "$ROOT/src/ScriptClient.swift" \
  "$ROOT/tests/ProfileModelsTests.swift" \
  -o "$TMP/ProfileModelsTests"
"$TMP/ProfileModelsTests"

printf 'PASS: complete test suite\n'
