#!/usr/bin/env bash
# Linux desktop end-to-end: a forwarded --upload-clipboard uploads the X11
# clipboard image through the real CLI (fake OpenSSH) and the remote path
# replaces it on the clipboard; --quit then stops the running instance.
#
# Needs a built CLI, its fake_openssh example, the Linux app, xclip and scrot.
# Run under a display and session bus:
#   dbus-run-session -- xvfb-run -a tests/test-linux-desktop.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_BIN="${APP_BIN:-$ROOT/rust/ssh-img-paste-linux/target/debug/ssh-img-paste-linux}"
CLI_BIN="${CLI_BIN:-$ROOT/rust/ssh-img-paste-cli/target/debug/ssh-img-paste}"
FAKE_OPENSSH="${FAKE_OPENSSH:-$ROOT/rust/ssh-img-paste-cli/target/debug/examples/fake_openssh}"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "${TMP_ROOT%/}/ssh-img-paste-desktop.XXXXXX")"
APP_PID=""
cleanup() {
  if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then kill "$APP_PID"; fi
  rm -rf "$TMP"
}
trap cleanup EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  [ -f "$TMP/app.log" ] && sed 's/^/app: /' "$TMP/app.log" >&2
  exit 1
}

for binary in "$APP_BIN" "$CLI_BIN" "$FAKE_OPENSSH"; do
  [ -x "$binary" ] || fail "missing executable $binary"
done
[ -n "${DISPLAY:-}" ] || fail "DISPLAY is not set; run under xvfb-run"

mkdir -p "$TMP/config/profiles" "$TMP/openssh"
cp "$FAKE_OPENSSH" "$TMP/openssh/ssh"
cp "$FAKE_OPENSSH" "$TMP/openssh/scp"
printf "%s\n" "SSH_PROFILE_LABEL='Work VPS'" "SSH_HOST='work-ssh'" \
  "SSH_REMOTE_HOME='/home/me'" "SSH_REMOTE_DIR='img-uploads'" > "$TMP/config/profiles/work.env"

export SSH_IMG_PASTE_CLI="$CLI_BIN"
export SSH_IMG_PASTE_CONFIG_ROOT="$TMP/config"
export SSH_IMG_PASTE_OPENSSH_DIR="$TMP/openssh"
export FAKE_OPENSSH_UPLOAD_COPY="$TMP/transferred.png"
export XDG_RUNTIME_DIR="$TMP/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
unset WAYLAND_DISPLAY

"$APP_BIN" > "$TMP/app.log" 2>&1 &
APP_PID=$!
sleep 2
kill -0 "$APP_PID" 2>/dev/null || fail "the app exited at startup"

scrot -o "$TMP/clip.png"
xclip -selection clipboard -target image/png -in "$TMP/clip.png"

timeout 10 "$APP_BIN" --upload-clipboard || fail "forwarding --upload-clipboard failed"

remote=""
for _ in $(seq 1 150); do
  remote="$(xclip -selection clipboard -out 2>/dev/null | tr -d '\000' || true)"
  case "$remote" in /home/me/img-uploads/ssh-img-*.png) break ;; esac
  remote=""
  sleep 0.1
done
[ -n "$remote" ] || fail "the remote path never reached the clipboard"
cmp -s "$TMP/clip.png" "$TMP/transferred.png" || fail "uploaded bytes differ from the clipboard image"
[ -z "$(ls -A "$XDG_RUNTIME_DIR/ssh-img-paste/staging")" ] || fail "a staging file was left behind"

timeout 10 "$APP_BIN" --quit || fail "forwarding --quit failed"
for _ in $(seq 1 50); do kill -0 "$APP_PID" 2>/dev/null || break; sleep 0.1; done
if kill -0 "$APP_PID" 2>/dev/null; then fail "the app ignored --quit"; fi
wait "$APP_PID" || fail "the app exited with an error"
APP_PID=""

if "$APP_BIN" --bogus 2>/dev/null; then fail "an unknown option was accepted"; fi

printf 'ok: clipboard image uploaded and %s copied\n' "$remote"
