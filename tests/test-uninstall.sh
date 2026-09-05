#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "${TMP_ROOT%/}/ssh-img-paste-uninstall-tests.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
assert_exists() { [ -e "$1" ] || [ -L "$1" ] || fail "missing path: $1"; }
assert_not_exists() { [ ! -e "$1" ] && [ ! -L "$1" ] || fail "expected path not to exist: $1"; }
assert_contains() { case "$1" in *"$2"*) ;; *) fail "expected [$1] to contain [$2]" ;; esac; }

export HOME="$TMP/home"
export XDG_CONFIG_HOME="$HOME/.config"
export PATH="$TMP/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export TEST_LOG="$TMP/invocations.log"
mkdir -p "$TMP/repo/bin" "$TMP/bin" "$HOME/Applications/SSHImagePaste.app/Contents/MacOS" \
  "$HOME/Library/LaunchAgents" "$HOME/bin" "$XDG_CONFIG_HOME/ssh-img-paste/profiles" \
  "$TMP/Applications"
cp "$ROOT/uninstall.sh" "$TMP/repo/uninstall.sh"
printf '#!/usr/bin/env bash\n' > "$TMP/repo/bin/ssh-img-paste"
chmod +x "$TMP/repo/uninstall.sh" "$TMP/repo/bin/ssh-img-paste"
printf app > "$HOME/Applications/SSHImagePaste.app/Contents/MacOS/SSHImagePaste"
printf plist > "$HOME/Library/LaunchAgents/com.khaireddine.sshimagepaste.plist"
ln -s "$TMP/repo/bin/ssh-img-paste" "$HOME/bin/ssh-img-paste"
ln -s "$HOME/Applications/SSHImagePaste.app" "$TMP/Applications/SSHImagePaste.app"
printf profile > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/default.env"
printf default > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
: > "$TEST_LOG"

cat > "$TMP/bin/launchctl" <<'MOCK'
#!/usr/bin/env bash
printf 'launchctl' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
exit 0
MOCK
cat > "$TMP/bin/pgrep" <<'MOCK'
#!/usr/bin/env bash
printf 'pgrep' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
exit 1
MOCK
chmod +x "$TMP/bin/launchctl" "$TMP/bin/pgrep"

# The script's /Applications cleanup is intentionally not exercised against the
# real system path. The source app, LaunchAgent, and owned CLI link are isolated.
(
  cd "$TMP/repo"
  ./uninstall.sh
) > "$TMP/uninstall.out"

assert_not_exists "$HOME/Applications/SSHImagePaste.app"
assert_not_exists "$HOME/Library/LaunchAgents/com.khaireddine.sshimagepaste.plist"
assert_not_exists "$HOME/bin/ssh-img-paste"
assert_exists "$XDG_CONFIG_HOME/ssh-img-paste/profiles/default.env"
assert_exists "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
assert_contains "$(cat "$TEST_LOG")" "$(printf 'pgrep\t-f\t^%s$' "$HOME/Applications/SSHImagePaste.app/Contents/MacOS/SSHImagePaste")"

# Never remove a CLI symlink owned by another installation.
printf '#!/usr/bin/env bash\n' > "$TMP/other-cli"
chmod +x "$TMP/other-cli"
ln -s "$TMP/other-cli" "$HOME/bin/ssh-img-paste"
(
  cd "$TMP/repo"
  ./uninstall.sh
) >/dev/null
assert_exists "$HOME/bin/ssh-img-paste"
[ "$(readlink "$HOME/bin/ssh-img-paste")" = "$TMP/other-cli" ] || fail "foreign CLI symlink changed"

printf 'PASS: source uninstaller removes owned files and preserves profiles and foreign links\n'
