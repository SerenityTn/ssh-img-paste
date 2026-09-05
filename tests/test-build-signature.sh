#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "${TMP_ROOT%/}/ssh-img-paste-signature-tests.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

APP_DIR="$TMP/Applications" "$ROOT/build.sh" >/dev/null
APP="$TMP/Applications/SSHImagePaste.app"

codesign --verify --strict "$APP"
requirement="$(codesign -d --requirements - "$APP" 2>&1)"
case "$requirement" in
  *'designated => identifier "com.khaireddine.sshimagepaste"'*) ;;
  *)
    printf 'FAIL: unstable or missing designated requirement: %s\n' "$requirement" >&2
    exit 1
    ;;
esac

expected_version="$(tr -d '[:space:]' < "$ROOT/VERSION")"
actual_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")"
[ "$actual_version" = "$expected_version" ] || {
  printf 'FAIL: expected app version %s, got %s\n' "$expected_version" "$actual_version" >&2
  exit 1
}
[ -s "$APP/Contents/Resources/AppIcon.icns" ] || {
  printf 'FAIL: app icon is missing from the bundle\n' >&2
  exit 1
}

display_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleDisplayName' "$APP/Contents/Info.plist")"
executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
[ "$display_name" = "SSH Image Paste" ] || {
  printf 'FAIL: expected SSH Image Paste display name, got %s\n' "$display_name" >&2
  exit 1
}
[ "$executable" = "SSHImagePaste" ] && [ -x "$APP/Contents/MacOS/SSHImagePaste" ] || {
  printf 'FAIL: renamed SSHImagePaste executable is missing\n' >&2
  exit 1
}

printf 'PASS: app has a stable identity, release version, and icon\n'