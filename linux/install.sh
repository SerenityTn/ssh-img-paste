#!/usr/bin/env bash
# Installs SSH Image Paste for Linux from an extracted release archive.
#
#   ./install.sh                     # per-user, into ~/.local
#   sudo ./install.sh --prefix /usr/local
#   ./install.sh --uninstall [--prefix DIR]
#
# The app and its CLI go side by side in PREFIX/lib/ssh-img-paste, so the app
# finds the CLI next to itself; PREFIX/bin gets links to both. A menu entry and
# the icon go under PREFIX/share. Profiles in ~/.config/ssh-img-paste are
# never touched, not even by --uninstall.
set -euo pipefail

APP_ID="io.github.serenitytn.SSHImagePaste"
HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${HOME}/.local"
UNINSTALL=0

usage() {
  sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix)
      [ $# -ge 2 ] || { usage >&2; exit 2; }
      PREFIX="$2"
      shift 2
      ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h | --help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done
case "$PREFIX" in
  /*) ;;
  *) printf 'The prefix must be an absolute path: %s\n' "$PREFIX" >&2; exit 2 ;;
esac

LIB="$PREFIX/lib/ssh-img-paste"
BIN="$PREFIX/bin"
APPS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor/scalable/apps"

# True when the path is free or is our link into LIB; other files are left alone.
ours_or_free() {
  [ ! -e "$1" ] && [ ! -L "$1" ] && return 0
  [ -L "$1" ] && case "$(readlink "$1")" in "$LIB"/*) return 0 ;; esac
  return 1
}

link() {
  if ours_or_free "$BIN/$1"; then
    ln -sfn "$LIB/$1" "$BIN/$1"
  else
    printf 'Skipped %s: a different file is already there. Run %s directly.\n' "$BIN/$1" "$LIB/$1" >&2
  fi
}

refresh_caches() {
  command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS" 2>/dev/null || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
}

if [ "$UNINSTALL" -eq 1 ]; then
  "$BIN/ssh-img-paste-linux" --quit >/dev/null 2>&1 || true
  for name in ssh-img-paste-linux ssh-img-paste; do
    if [ -L "$BIN/$name" ] && ours_or_free "$BIN/$name"; then rm -f "$BIN/$name"; fi
  done
  rm -f "$APPS/$APP_ID.desktop" "$ICONS/$APP_ID.svg"
  rm -rf "$LIB"
  rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$APP_ID.desktop"
  refresh_caches
  printf 'Removed SSH Image Paste from %s. Your destinations in ~/.config/ssh-img-paste were kept.\n' "$PREFIX"
  exit 0
fi

for file in ssh-img-paste-linux ssh-img-paste "$APP_ID.desktop.in" "$APP_ID.svg"; do
  [ -f "$HERE/$file" ] || { printf 'Missing %s next to install.sh.\n' "$file" >&2; exit 1; }
done

install -d -m 755 "$LIB" "$BIN" "$APPS" "$ICONS"
install -m 755 "$HERE/ssh-img-paste-linux" "$HERE/ssh-img-paste" "$LIB/"
link ssh-img-paste-linux
link ssh-img-paste
sed "s|@BINDIR@|$BIN|g" "$HERE/$APP_ID.desktop.in" > "$APPS/$APP_ID.desktop"
chmod 644 "$APPS/$APP_ID.desktop"
install -m 644 "$HERE/$APP_ID.svg" "$ICONS/$APP_ID.svg"
refresh_caches

printf 'Installed SSH Image Paste to %s.\n' "$LIB"
printf 'Start it from your applications menu, or run: %s/ssh-img-paste-linux\n' "$BIN"
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) printf 'Note: %s is not on your PATH.\n' "$BIN" ;;
esac

missing=()
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
  command -v wl-copy >/dev/null || missing+=(wl-clipboard)
else
  command -v xclip >/dev/null || missing+=(xclip)
fi
command -v ssh >/dev/null || missing+=(openssh-client)
if [ ${#missing[@]} -gt 0 ]; then
  printf 'Also install: %s (for example: sudo apt install %s)\n' "${missing[*]}" "${missing[*]}"
fi
