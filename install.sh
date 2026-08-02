#!/usr/bin/env bash
# Install SSH Image Paste: link the canonical and legacy CLIs into ~/bin, build
# the menu-bar app, and register it to launch at login. Idempotent.
set -euo pipefail
cd "$(dirname "$0")"
REPO="$(pwd)"

# 1. Dependency check
if ! command -v /opt/homebrew/bin/pngpaste >/dev/null 2>&1 && ! command -v pngpaste >/dev/null 2>&1; then
  echo "→ Installing pngpaste (needed to read clipboard images)…"
  brew install pngpaste
fi

# 2. Symlink the canonical CLI and the 1.x compatibility entry point into ~/bin.
mkdir -p "$HOME/bin"
ln -sf "$REPO/bin/ssh-img-paste" "$HOME/bin/ssh-img-paste"
ln -sf "$REPO/bin/vps-img-paste" "$HOME/bin/vps-img-paste"
chmod +x "$REPO/bin/ssh-img-paste"
chmod +x "$REPO/bin/vps-img-paste"
echo "✓ ~/bin/ssh-img-paste -> $REPO/bin/ssh-img-paste"
echo "✓ ~/bin/vps-img-paste -> $REPO/bin/vps-img-paste"

# 3. Seed local config for fresh installs only.
# VPS Image Paste 1.x configuration remains supported and is never moved or
# deleted. Fresh installs use the SSH Image Paste paths.
CONFIG_ROOT="${XDG_CONFIG_HOME:-$HOME/.config}"
OLD_LEGACY_CONF="$CONFIG_ROOT/vps-img-paste.env"
NEW_LEGACY_CONF="$CONFIG_ROOT/ssh-img-paste.env"
OLD_CONFIG_DIR="$CONFIG_ROOT/vps-img-paste"
NEW_CONFIG_DIR="$CONFIG_ROOT/ssh-img-paste"
if [ -e "$NEW_CONFIG_DIR" ] || [ ! -e "$OLD_CONFIG_DIR" ]; then
  CONFIG_DIR="$NEW_CONFIG_DIR"
else
  CONFIG_DIR="$OLD_CONFIG_DIR"
fi
if [ -e "$NEW_LEGACY_CONF" ] || [ ! -e "$OLD_LEGACY_CONF" ]; then
  LEGACY_CONF="$NEW_LEGACY_CONF"
else
  LEGACY_CONF="$OLD_LEGACY_CONF"
fi
PROFILE_DIR="$CONFIG_DIR/profiles"
DEFAULT_PROFILE="$PROFILE_DIR/default.env"
ACTIVE_PROFILE="$CONFIG_DIR/active-profile"
shopt -s nullglob
existing_profiles=("$PROFILE_DIR"/*.env)
shopt -u nullglob

if [ ! -f "$LEGACY_CONF" ] && [ ${#existing_profiles[@]} -eq 0 ]; then
  mkdir -p "$PROFILE_DIR"
  cp "$REPO/ssh-img-paste.env.example" "$DEFAULT_PROFILE"
  printf 'default\n' > "$ACTIVE_PROFILE"
  echo "! Created $DEFAULT_PROFILE — edit it and set VPS_HOST before first use."
  echo "✓ Active profile: default ($ACTIVE_PROFILE)"
elif [ -f "$LEGACY_CONF" ]; then
  echo "✓ Legacy config present: $LEGACY_CONF (left untouched)"
else
  echo "✓ Named profile config present in $PROFILE_DIR (left untouched)"
fi

# 4. Build the app
./build.sh

# 5. Remove obsolete Homebrew and VPS Image Paste 1.x launch/reopen paths.
# Older formula installs place
# a second app in Homebrew's Cellar and may leave both a LaunchAgent and an
# /Applications symlink behind. Without removing those entry points, Finder or
# Spotlight can reopen the old app even after this source build is installed.
OLD_PL="$HOME/Library/LaunchAgents/homebrew.mxcl.vps-img-paste.plist"
if [ -e "$OLD_PL" ]; then
  launchctl unload "$OLD_PL" 2>/dev/null || true
  rm -f "$OLD_PL"
fi
NEW_BREW_PL="$HOME/Library/LaunchAgents/homebrew.mxcl.ssh-img-paste.plist"
if [ -e "$NEW_BREW_PL" ]; then
  launchctl unload "$NEW_BREW_PL" 2>/dev/null || true
  rm -f "$NEW_BREW_PL"
fi

old_pids="$(pgrep -f '^/opt/homebrew/(Cellar/(vps-img-paste|ssh-img-paste)/[^/]+|opt/(vps-img-paste|ssh-img-paste))/(VpsImgPaste|SSHImagePaste).app/Contents/MacOS/(VpsImgPaste|SSHImagePaste)$' 2>/dev/null || true)"
if [ -n "$old_pids" ]; then
  kill $old_pids 2>/dev/null || true
fi

APP="$HOME/Applications/SSHImagePaste.app"
LEGACY_APP="$HOME/Applications/VpsImgPaste.app"
GLOBAL_APPS="${SSH_IMG_PASTE_GLOBAL_APPLICATIONS_DIR:-${VPS_IMG_PASTE_GLOBAL_APPLICATIONS_DIR:-/Applications}}"
GLOBAL_APP="$GLOBAL_APPS/SSHImagePaste.app"
LEGACY_GLOBAL_APP="$GLOBAL_APPS/VpsImgPaste.app"
mkdir -p "$GLOBAL_APPS" 2>/dev/null || true
if [ -L "$GLOBAL_APP" ]; then
  ln -sfn "$APP" "$GLOBAL_APP"
elif [ ! -e "$GLOBAL_APP" ]; then
  ln -s "$APP" "$GLOBAL_APP" 2>/dev/null || true
else
  echo "! $GLOBAL_APP is a real app/directory; leaving it untouched."
  echo "  Open the latest build from $APP."
fi
if [ -L "$LEGACY_GLOBAL_APP" ]; then
  rm -f "$LEGACY_GLOBAL_APP"
fi

LSREGISTER="${SSH_IMG_PASTE_LSREGISTER:-${VPS_IMG_PASTE_LSREGISTER:-/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister}}"
if [ -x "$LSREGISTER" ]; then
  "$LSREGISTER" -u /opt/homebrew/opt/vps-img-paste/VpsImgPaste.app >/dev/null 2>&1 || true
  "$LSREGISTER" -u "$LEGACY_APP" >/dev/null 2>&1 || true
  "$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true
fi

# 6. LaunchAgent (start now + at every login)
PL="$HOME/Library/LaunchAgents/com.khaireddine.vpsimgpaste.plist"
BIN="$APP/Contents/MacOS/SSHImagePaste"
LEGACY_BIN="$LEGACY_APP/Contents/MacOS/VpsImgPaste"
mkdir -p "$(dirname "$PL")"
cat > "$PL" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.khaireddine.vpsimgpaste</string>
  <key>ProgramArguments</key><array><string>$BIN</string></array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>SSH_IMG_PASTE_BIN</key><string>$HOME/bin/ssh-img-paste</string>
    <key>VPS_IMG_PASTE_BIN</key><string>$HOME/bin/vps-img-paste</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
PLIST
launchctl unload "$PL" 2>/dev/null || true

# A manually opened copy is not owned by the LaunchAgent and survives unload.
# Stop every process using this exact source-installed executable before loading
# the single login instance, otherwise two menu-bar icons can remain visible.
current_pids="$(pgrep -f "^$BIN$" 2>/dev/null || true)"
if [ -n "$current_pids" ]; then
  for pid in $current_pids; do
    kill "$pid" 2>/dev/null || true
  done
fi

legacy_pids="$(pgrep -f "^$LEGACY_BIN$" 2>/dev/null || true)"
if [ -n "$legacy_pids" ]; then
  for pid in $legacy_pids; do
    kill "$pid" 2>/dev/null || true
  done
fi
rm -rf "$LEGACY_APP"

launchctl load -w "$PL"
echo "✓ Menu-bar app installed and running (auto-starts at login)."
