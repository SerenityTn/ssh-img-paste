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

printf 'PASS: app has stable Screen Recording permission identity\n'