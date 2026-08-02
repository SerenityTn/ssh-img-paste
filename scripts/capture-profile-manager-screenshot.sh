#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${1:-$ROOT/docs/images/profile-manager.png}"
TMP="$(mktemp -d -t ssh-img-paste-screenshot)"
trap 'rm -rf "$TMP"' EXIT
MOCK="$ROOT/tests/fixtures/ssh-img-paste-docs-mock"

swiftc -target "$(uname -m)-apple-macos13.0" \
  "$ROOT/src/ProfileModels.swift" \
  "$ROOT/src/ScriptClient.swift" \
  "$ROOT/src/ProfileManagerWindowController.swift" \
  "$ROOT/scripts/ProfileManagerScreenshot.swift" \
  -framework AppKit \
  -o "$TMP/ProfileManagerScreenshot"

"$TMP/ProfileManagerScreenshot" "$MOCK" "$OUTPUT"
printf 'Captured %s using fixture profiles only.\n' "$OUTPUT"
