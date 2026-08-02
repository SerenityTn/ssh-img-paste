#!/usr/bin/env bash
# Remove source-installed SSH Image Paste app files, LaunchAgent, and CLI links.
# Profile configuration is always preserved.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"

PL="$HOME/Library/LaunchAgents/com.khaireddine.vpsimgpaste.plist"
APP="$HOME/Applications/SSHImagePaste.app"
BIN="$APP/Contents/MacOS/SSHImagePaste"
LEGACY_APP="$HOME/Applications/VpsImgPaste.app"
LEGACY_BIN="$LEGACY_APP/Contents/MacOS/VpsImgPaste"
CLI="$HOME/bin/ssh-img-paste"
LEGACY_CLI="$HOME/bin/vps-img-paste"
launchctl unload "$PL" 2>/dev/null || true
rm -f "$PL"

pids="$(pgrep -f "^$BIN$" 2>/dev/null || true)"
if [ -n "$pids" ]; then
  for pid in $pids; do
    kill "$pid" 2>/dev/null || true
  done
fi

legacy_pids="$(pgrep -f "^$LEGACY_BIN$" 2>/dev/null || true)"
if [ -n "$legacy_pids" ]; then
  for pid in $legacy_pids; do
    kill "$pid" 2>/dev/null || true
  done
fi

if [ -L /Applications/SSHImagePaste.app ] && [ "$(readlink /Applications/SSHImagePaste.app)" = "$APP" ]; then
  rm -f /Applications/SSHImagePaste.app 2>/dev/null || true
fi
if [ -L /Applications/VpsImgPaste.app ] && [ "$(readlink /Applications/VpsImgPaste.app)" = "$LEGACY_APP" ]; then
  rm -f /Applications/VpsImgPaste.app 2>/dev/null || true
fi
rm -rf "$APP" "$LEGACY_APP"
if [ -L "$CLI" ] && [ "$(readlink "$CLI")" = "$ROOT/bin/ssh-img-paste" ]; then
  rm -f "$CLI"
fi
if [ -L "$LEGACY_CLI" ] && [ "$(readlink "$LEGACY_CLI")" = "$ROOT/bin/vps-img-paste" ]; then
  rm -f "$LEGACY_CLI"
fi

echo "✓ Source installation removed."
echo "  Profile configuration was preserved under ${XDG_CONFIG_HOME:-$HOME/.config}."
