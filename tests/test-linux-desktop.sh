#!/usr/bin/env bash
# Linux desktop end-to-end against the running app, through the real CLI
# (fake OpenSSH) and the real X11 clipboard:
#   - a fake StatusNotifierWatcher brings the tray icon online; the tray menu
#     is read over D-Bus and a tray click uploads the clipboard image;
#   - the earlier clipboard image is restored after the profile's delay;
#   - a forwarded --capture-full takes a screenshot (scrot fallback, no portal)
#     and copies its remote path;
#   - --quit stops the running instance.
#
# Needs a built CLI, its fake_openssh example, the Linux app, xclip, scrot,
# gdbus, and python3-gi. Run under a display and session bus:
#   dbus-run-session -- xvfb-run -a tests/test-linux-desktop.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_BIN="${APP_BIN:-$ROOT/rust/ssh-img-paste-linux/target/debug/ssh-img-paste-linux}"
CLI_BIN="${CLI_BIN:-$ROOT/rust/ssh-img-paste-cli/target/debug/ssh-img-paste}"
FAKE_OPENSSH="${FAKE_OPENSSH:-$ROOT/rust/ssh-img-paste-cli/target/debug/examples/fake_openssh}"
# The system interpreter has the GObject bindings; CI's setup-python does not.
SYSTEM_PYTHON="${SYSTEM_PYTHON:-/usr/bin/python3}"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "${TMP_ROOT%/}/ssh-img-paste-desktop.XXXXXX")"
APP_PID=""
WATCHER_PID=""
cleanup() {
  for pid in "$APP_PID" "$WATCHER_PID"; do
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then kill "$pid"; fi
  done
  rm -rf "$TMP"
}
trap cleanup EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  [ -f "$TMP/app.log" ] && sed 's/^/app: /' "$TMP/app.log" >&2
  exit 1
}

# Polls `$@` every 0.1s for up to $WAIT_SECONDS (default 15).
eventually() {
  local tries=$(( ${WAIT_SECONDS:-15} * 10 ))
  for _ in $(seq 1 "$tries"); do
    if "$@"; then return 0; fi
    sleep 0.1
  done
  return 1
}

clipboard_text() { xclip -selection clipboard -out 2>/dev/null | tr -d '\000' || true; }
clipboard_has_path() { case "$(clipboard_text)" in /home/me/img-uploads/ssh-img-*.png) return 0 ;; esac; return 1; }
clipboard_offers_png() { xclip -selection clipboard -target TARGETS -out 2>/dev/null | grep -qx 'image/png'; }
transferred_is_png() { [ -s "$TMP/transferred.png" ] && head -c 8 "$TMP/transferred.png" | od -An -c | grep -q 'P   N   G'; }
tray_item() { [ -s "$TMP/watcher.log" ]; }

for binary in "$APP_BIN" "$CLI_BIN" "$FAKE_OPENSSH"; do
  [ -x "$binary" ] || fail "missing executable $binary"
done
[ -n "${DISPLAY:-}" ] || fail "DISPLAY is not set; run under xvfb-run"

mkdir -p "$TMP/config/profiles" "$TMP/openssh"
cp "$FAKE_OPENSSH" "$TMP/openssh/ssh"
cp "$FAKE_OPENSSH" "$TMP/openssh/scp"
printf "%s\n" "SSH_PROFILE_LABEL='Work VPS'" "SSH_HOST='work-ssh'" \
  "SSH_REMOTE_HOME='/home/me'" "SSH_REMOTE_DIR='img-uploads'" \
  "SSH_CLIP_RESTORE_SECONDS='2'" > "$TMP/config/profiles/work.env"

export SSH_IMG_PASTE_CLI="$CLI_BIN"
export SSH_IMG_PASTE_CONFIG_ROOT="$TMP/config"
export SSH_IMG_PASTE_OPENSSH_DIR="$TMP/openssh"
export FAKE_OPENSSH_UPLOAD_COPY="$TMP/transferred.png"
export XDG_RUNTIME_DIR="$TMP/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
unset WAYLAND_DISPLAY

"$SYSTEM_PYTHON" "$ROOT/tests/fake_sni_watcher.py" > "$TMP/watcher.log" 2>&1 &
WATCHER_PID=$!
sleep 0.5
"$APP_BIN" --background > "$TMP/app.log" 2>&1 &
APP_PID=$!
eventually tray_item || fail "the tray icon never registered"
ITEM="$(head -1 "$TMP/watcher.log")"
kill -0 "$APP_PID" 2>/dev/null || fail "the app exited at startup"

tooltip_has() {
  gdbus call --session --dest "$ITEM" --object-path /StatusNotifierItem \
    --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierItem ToolTip |
    grep -q "$1"
}
# The tooltip names the destination once the first catalog load finishes.
eventually tooltip_has 'SSH Image Paste — Work VPS' || fail "idle tooltip never named the destination"

# Tray menu, as a desktop panel sees it.
menu="$(gdbus call --session --dest "$ITEM" --object-path /MenuBar \
  --method com.canonical.dbusmenu.GetLayout -- 0 -1 '["label"]')"
for label in "Open SSH Image Paste" "Upload Clipboard Image" "Capture Region" \
  "Capture Full Screen" "Uploaded Images…" "Destination" "Work VPS  (work-ssh)" \
  "Manage Profiles…" "Quit"; do
  case "$menu" in *"'$label'"*) ;; *) fail "tray menu lacks \"$label\": $menu" ;; esac
done

# A tray click uploads the clipboard image and copies the remote path.
scrot -o "$TMP/clip.png"
xclip -selection clipboard -target image/png -in "$TMP/clip.png"
eventually clipboard_offers_png || fail "the test image never reached the clipboard"
gdbus call --session --dest "$ITEM" --object-path /StatusNotifierItem \
  --method org.kde.StatusNotifierItem.Activate -- 0 0 > /dev/null
eventually clipboard_has_path || fail "the tray upload never copied the remote path"
cmp -s "$TMP/clip.png" "$TMP/transferred.png" || fail "uploaded bytes differ from the clipboard image"
eventually tooltip_has 'Image path copied — Work VPS' || fail "the tray never reported success"
[ -z "$(ls -A "$XDG_RUNTIME_DIR/ssh-img-paste/staging" | grep -v '^capture$' || true)" ] \
  || fail "a staging file was left behind"

# The earlier clipboard image returns after the profile's 2s delay.
WAIT_SECONDS=10 eventually clipboard_offers_png || fail "the earlier clipboard image was not restored"
xclip -selection clipboard -target image/png -out > "$TMP/restored.png"
cmp -s "$TMP/clip.png" "$TMP/restored.png" || fail "the restored image differs from the original"

# A forwarded full-screen capture (scrot fallback) uploads a fresh screenshot.
rm -f "$TMP/transferred.png"
timeout 10 "$APP_BIN" --capture-full || fail "forwarding --capture-full failed"
eventually transferred_is_png || fail "the full-screen capture was never uploaded"
eventually clipboard_has_path || fail "the capture's remote path never reached the clipboard"

timeout 10 "$APP_BIN" --quit || fail "forwarding --quit failed"
WAIT_SECONDS=5 eventually sh -c "! kill -0 $APP_PID 2>/dev/null" || fail "the app ignored --quit"
wait "$APP_PID" || fail "the app exited with an error"
APP_PID=""

if "$APP_BIN" --bogus 2>/dev/null; then fail "an unknown option was accepted"; fi

printf 'ok: tray, clipboard upload, clipboard restore, full-screen capture, and quit\n'
