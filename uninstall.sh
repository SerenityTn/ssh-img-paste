#!/usr/bin/env bash
# Remove the source-installed menu-bar app, LaunchAgent, and CLI symlink.
# Profile configuration is always preserved.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"

PL="$HOME/Library/LaunchAgents/com.khaireddine.vpsimgpaste.plist"
APP="$HOME/Applications/VpsImgPaste.app"
BIN="$APP/Contents/MacOS/VpsImgPaste"
CLI="$HOME/bin/vps-img-paste"
launchctl unload "$PL" 2>/dev/null || true
rm -f "$PL"

pids="$(pgrep -f "^$BIN$" 2>/dev/null || true)"
if [ -n "$pids" ]; then
  for pid in $pids; do
    kill "$pid" 2>/dev/null || true
  done
fi

if [ -L /Applications/VpsImgPaste.app ] && [ "$(readlink /Applications/VpsImgPaste.app)" = "$APP" ]; then
  rm -f /Applications/VpsImgPaste.app 2>/dev/null || true
fi
rm -rf "$APP"
if [ -L "$CLI" ] && [ "$(readlink "$CLI")" = "$ROOT/bin/vps-img-paste" ]; then
  rm -f "$CLI"
fi

echo "✓ Source installation removed."
echo "  Profile configuration was preserved under ${XDG_CONFIG_HOME:-$HOME/.config}."
