#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="$ROOT/assets/AppIcon.svg"
OUTPUT="$ROOT/assets/AppIcon.icns"
TMP="$(mktemp -d -t ssh-img-paste-icon)"
trap 'rm -rf "$TMP"' EXIT
ICONSET="$TMP/AppIcon.iconset"
MASTER="$TMP/AppIcon.png"
mkdir -p "$ICONSET"

sips -s format png "$SOURCE" --out "$MASTER" >/dev/null
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$MASTER" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$MASTER" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done

iconutil -c icns "$ICONSET" -o "$OUTPUT"
printf 'Generated %s\n' "$OUTPUT"
