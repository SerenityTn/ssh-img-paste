#!/usr/bin/env bash
# Install SSH Image Paste: link the CLI into ~/bin, build the menu-bar app, and
# register it to launch at login. Idempotent.
set -euo pipefail
cd "$(dirname "$0")"
REPO="$(pwd)"

# 1. Dependency check
if ! command -v /opt/homebrew/bin/pngpaste >/dev/null 2>&1 && ! command -v pngpaste >/dev/null 2>&1; then
  echo "→ Installing pngpaste (needed to read clipboard images)…"
  brew install pngpaste
fi

# 2. Symlink the CLI into ~/bin.
mkdir -p "$HOME/bin"
ln -sf "$REPO/bin/ssh-img-paste" "$HOME/bin/ssh-img-paste"
chmod +x "$REPO/bin/ssh-img-paste"
echo "✓ ~/bin/ssh-img-paste -> $REPO/bin/ssh-img-paste"

# 3. Seed local config for fresh installs only.
CONFIG_ROOT="${XDG_CONFIG_HOME:-$HOME/.config}"
CONFIG_DIR="$CONFIG_ROOT/ssh-img-paste"
PROFILE_DIR="$CONFIG_DIR/profiles"
DEFAULT_PROFILE="$PROFILE_DIR/default.env"
ACTIVE_PROFILE="$CONFIG_DIR/active-profile"
shopt -s nullglob
existing_profiles=("$PROFILE_DIR"/*.env)
shopt -u nullglob

if [ ${#existing_profiles[@]} -eq 0 ]; then
  mkdir -p "$PROFILE_DIR"
  cp "$REPO/ssh-img-paste.env.example" "$DEFAULT_PROFILE"
  printf 'default\n' > "$ACTIVE_PROFILE"
  echo "! Created $DEFAULT_PROFILE — edit it and set SSH_HOST before first use."
  echo "✓ Active profile: default ($ACTIVE_PROFILE)"
else
  echo "✓ Named profile config present in $PROFILE_DIR (left untouched)"
fi

# 4. Build the app
./build.sh

# 5. Remove competing Homebrew launch/reopen paths.
BREW_PL="$HOME/Library/LaunchAgents/homebrew.mxcl.ssh-img-paste.plist"
if [ -e "$BREW_PL" ]; then
  launchctl unload "$BREW_PL" 2>/dev/null || true
  rm -f "$BREW_PL"
fi

old_pids="$(pgrep -f '^/opt/homebrew/(Cellar/ssh-img-paste/[^/]+|opt/ssh-img-paste)/SSHImagePaste.app/Contents/MacOS/SSHImagePaste$' 2>/dev/null || true)"
if [ -n "$old_pids" ]; then
  kill $old_pids 2>/dev/null || true
fi

APP="$HOME/Applications/SSHImagePaste.app"
GLOBAL_APPS="${SSH_IMG_PASTE_GLOBAL_APPLICATIONS_DIR:-/Applications}"
GLOBAL_APP="$GLOBAL_APPS/SSHImagePaste.app"
mkdir -p "$GLOBAL_APPS" 2>/dev/null || true
if [ -L "$GLOBAL_APP" ]; then
  ln -sfn "$APP" "$GLOBAL_APP"
elif [ ! -e "$GLOBAL_APP" ]; then
  ln -s "$APP" "$GLOBAL_APP" 2>/dev/null || true
else
  echo "! $GLOBAL_APP is a real app/directory; leaving it untouched."
  echo "  Open the latest build from $APP."
fi
LSREGISTER="${SSH_IMG_PASTE_LSREGISTER:-/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister}"
if [ -x "$LSREGISTER" ]; then
  "$LSREGISTER" -u /opt/homebrew/opt/ssh-img-paste/SSHImagePaste.app >/dev/null 2>&1 || true
  "$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true
fi

# 6. LaunchAgent (start now + at every login)
PL="$HOME/Library/LaunchAgents/com.khaireddine.sshimagepaste.plist"
BIN="$APP/Contents/MacOS/SSHImagePaste"
mkdir -p "$(dirname "$PL")"
cat > "$PL" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.khaireddine.sshimagepaste</string>
  <key>ProgramArguments</key><array><string>$BIN</string></array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>SSH_IMG_PASTE_BIN</key><string>$HOME/bin/ssh-img-paste</string>
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

launchctl load -w "$PL"
echo "✓ Menu-bar app installed and running (auto-starts at login)."
