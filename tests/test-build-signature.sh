#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d -t vps-img-paste-signature-tests)"
trap 'rm -rf "$TMP"' EXIT

APP_DIR="$TMP/Applications" "$ROOT/build.sh" >/dev/null
APP="$TMP/Applications/VpsImgPaste.app"

codesign --verify --strict "$APP"
requirement="$(codesign -d --requirements - "$APP" 2>&1)"
case "$requirement" in
  *'designated => identifier "com.khaireddine.vpsimgpaste"'*) ;;
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

printf 'PASS: app has a stable identity, release version, and icon\n'