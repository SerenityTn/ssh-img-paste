#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d -t ssh-img-paste-install-tests)"
trap 'rm -rf "$TMP"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
pass() { printf 'PASS: %s\n' "$*"; }
assert_contains() { case "$1" in *"$2"*) ;; *) fail "expected [$1] to contain [$2]" ;; esac; }
assert_file_exists() { [ -f "$1" ] || fail "missing file: $1"; }
assert_not_exists() { [ ! -e "$1" ] || fail "expected path not to exist: $1"; }

export HOME="$TMP/home"
export XDG_CONFIG_HOME="$HOME/.config"
export PATH="$TMP/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export TEST_LOG="$TMP/invocations.log"
export SSH_IMG_PASTE_GLOBAL_APPLICATIONS_DIR="$TMP/Applications"
export SSH_IMG_PASTE_LSREGISTER="$TMP/bin/lsregister"
mkdir -p "$HOME" "$TMP/bin" "$TMP/repo/bin"
: > "$TEST_LOG"

cp "$ROOT/install.sh" "$TMP/repo/install.sh"
cp "$ROOT/ssh-img-paste.env.example" "$TMP/repo/ssh-img-paste.env.example"
printf '#!/usr/bin/env bash\nexit 0\n' > "$TMP/repo/bin/ssh-img-paste"
chmod +x "$TMP/repo/install.sh" "$TMP/repo/bin/ssh-img-paste"

# Mock build.sh so the installer test does not compile or write to the real app tree.
printf '#!/usr/bin/env bash\nprintf "build\\n" >> "$TEST_LOG"\nmkdir -p "$HOME/Applications/SSHImagePaste.app/Contents/MacOS"\nprintf app > "$HOME/Applications/SSHImagePaste.app/Contents/MacOS/SSHImagePaste"\n' > "$TMP/repo/build.sh"
chmod +x "$TMP/repo/build.sh"

cat > "$TMP/bin/pngpaste" <<'MOCK'
#!/usr/bin/env bash
printf 'pngpaste\n' >> "$TEST_LOG"
exit 0
MOCK
chmod +x "$TMP/bin/pngpaste"

cat > "$TMP/bin/launchctl" <<'MOCK'
#!/usr/bin/env bash
printf 'launchctl' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
exit 0
MOCK
chmod +x "$TMP/bin/launchctl"

cat > "$TMP/bin/lsregister" <<'MOCK'
#!/usr/bin/env bash
printf 'lsregister' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
exit 0
MOCK
chmod +x "$TMP/bin/lsregister"

# Never inspect or terminate real desktop processes from the isolated test.
cat > "$TMP/bin/pgrep" <<'MOCK'
#!/usr/bin/env bash
printf 'pgrep' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
exit 1
MOCK
chmod +x "$TMP/bin/pgrep"

cat > "$TMP/bin/brew" <<'MOCK'
#!/usr/bin/env bash
printf 'brew' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
exit 99
MOCK
chmod +x "$TMP/bin/brew"

assert_not_exists "$HOME/Library"
(
  cd "$TMP/repo"
  ./install.sh
) > "$TMP/install.out" 2>&1

plist="$HOME/Library/LaunchAgents/com.khaireddine.sshimagepaste.plist"
assert_file_exists "$plist"
assert_file_exists "$HOME/bin/ssh-img-paste"
assert_file_exists "$XDG_CONFIG_HOME/ssh-img-paste/profiles/default.env"
assert_contains "$(cat "$XDG_CONFIG_HOME/ssh-img-paste/active-profile")" "default"
assert_contains "$(cat "$plist")" "$HOME/Applications/SSHImagePaste.app/Contents/MacOS/SSHImagePaste"
assert_contains "$(cat "$plist")" "$HOME/bin/ssh-img-paste"
assert_contains "$(cat "$TEST_LOG")" "build"
assert_contains "$(cat "$TEST_LOG")" $'launchctl\tload\t-w\t'
assert_contains "$(cat "$TEST_LOG")" "$(printf 'pgrep\t-f\t^%s$' "$HOME/Applications/SSHImagePaste.app/Contents/MacOS/SSHImagePaste")"
assert_contains "$(readlink "$SSH_IMG_PASTE_GLOBAL_APPLICATIONS_DIR/SSHImagePaste.app")" "$HOME/Applications/SSHImagePaste.app"
assert_contains "$(cat "$TEST_LOG")" $'lsregister\t-f\t'
if grep -q '^brew' "$TEST_LOG"; then
  fail "installer should use mocked pngpaste and not call brew"
fi

pass "fresh install creates missing LaunchAgents directory and plist under isolated HOME"

# A stale Homebrew launcher and global symlink are removed/repointed.
mkdir -p "$HOME/Library/LaunchAgents"
printf old > "$HOME/Library/LaunchAgents/homebrew.mxcl.ssh-img-paste.plist"
ln -sfn /opt/homebrew/opt/ssh-img-paste/SSHImagePaste.app "$SSH_IMG_PASTE_GLOBAL_APPLICATIONS_DIR/SSHImagePaste.app"
: > "$TEST_LOG"
(
  cd "$TMP/repo"
  ./install.sh
) > "$TMP/reinstall.out" 2>&1
assert_not_exists "$HOME/Library/LaunchAgents/homebrew.mxcl.ssh-img-paste.plist"
assert_contains "$(readlink "$SSH_IMG_PASTE_GLOBAL_APPLICATIONS_DIR/SSHImagePaste.app")" "$HOME/Applications/SSHImagePaste.app"
assert_contains "$(cat "$TEST_LOG")" $'launchctl\tunload\t'
pass "reinstall removes obsolete Homebrew launch path and repoints global app"
printf 'All installer tests passed.\n'
