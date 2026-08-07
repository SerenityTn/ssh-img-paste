#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ssh-img-paste-tests.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

export HOME="$TMP/home"
export XDG_CONFIG_HOME="$HOME/.config"
export PATH="$TMP/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export TEST_LOG="$TMP/invocations.log"
export PBCOPY_LOG="$TMP/pbcopy.log"
export OSASCRIPT_LOG="$TMP/osascript.log"
export OSASCRIPT_SOURCE_LOG="$TMP/osascript-source.log"
export OSASCRIPT_MARKER="$TMP/osascript-pwned"
mkdir -p "$HOME/.config" "$TMP/bin"
: > "$TEST_LOG"
: > "$PBCOPY_LOG"
: > "$OSASCRIPT_LOG"
: > "$OSASCRIPT_SOURCE_LOG"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
pass() { printf 'PASS: %s\n' "$*"; }
assert_contains() {
  case "$1" in *"$2"*) ;; *) fail "expected [$1] to contain [$2]" ;; esac
}
assert_not_contains() {
  case "$1" in *"$2"*) fail "expected [$1] not to contain [$2]" ;; *) ;; esac
}
assert_eq() {
  [ "$1" = "$2" ] || fail "expected [$1] to equal [$2]"
}
assert_file_eq() {
  [ -f "$1" ] || fail "missing file: $1"
  local got
  got="$(cat "$1")"
  [ "$got" = "$2" ] || fail "expected $1 content [$got] to equal [$2]"
}
reset_state() {
  rm -rf "$HOME/.config/ssh-img-paste" \
    "$TEST_LOG" "$PBCOPY_LOG" "$OSASCRIPT_LOG" "$OSASCRIPT_SOURCE_LOG" "$OSASCRIPT_MARKER"
  mkdir -p "$HOME/.config"
  : > "$TEST_LOG"
  : > "$PBCOPY_LOG"
  : > "$OSASCRIPT_LOG"
  : > "$OSASCRIPT_SOURCE_LOG"
}
write_profiles() {
  mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
  cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/dev.env" <<'EOF'
SSH_PROFILE_LABEL="Development"
SSH_HOST="dev-host"
SSH_REMOTE_HOME="/srv/dev"
SSH_REMOTE_DIR="dev-images"
SSH_CLIP_RESTORE_SECONDS=0
EOF
  cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/prod.env" <<'EOF'
SSH_PROFILE_LABEL="Production"
SSH_HOST="prod-host"
SSH_REMOTE_HOME="/srv/prod"
SSH_REMOTE_DIR="prod-images"
SSH_CLIP_RESTORE_SECONDS=0
EOF
}

cat > "$TMP/bin/ssh" <<'EOF'
#!/usr/bin/env bash
printf 'ssh' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
cmd="${*: -1}"
case "$cmd" in
  *"find"*"sort"*) printf '123\tuploaded.png\n' ;;
  *"-delete"*) printf '2\n' ;;
esac
EOF
chmod +x "$TMP/bin/ssh"

cat > "$TMP/bin/scp" <<'EOF'
#!/usr/bin/env bash
printf 'scp' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
last="${@: -1}"
case "$last" in
  /*)
    mkdir -p "$(dirname "$last")"
    printf 'downloaded' > "$last"
    ;;
esac
exit 0
EOF
chmod +x "$TMP/bin/scp"

cat > "$TMP/bin/pngpaste" <<'EOF'
#!/usr/bin/env bash
printf 'pngpaste' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
printf 'PNGDATA' > "$1"
EOF
chmod +x "$TMP/bin/pngpaste"

cat > "$TMP/bin/pbcopy" <<'EOF'
#!/usr/bin/env bash
input="$(cat)"
printf '%s' "$input" > "$PBCOPY_LOG"
printf 'pbcopy\t%s\n' "$input" >> "$TEST_LOG"
EOF
chmod +x "$TMP/bin/pbcopy"

cat > "$TMP/bin/pbpaste" <<'EOF'
#!/usr/bin/env bash
printf 'clipboard-text'
EOF
chmod +x "$TMP/bin/pbpaste"

cat > "$TMP/bin/osascript" <<'EOF'
#!/usr/bin/env bash
script=""
expect_source=0
printf 'osascript' >> "$OSASCRIPT_LOG"
for arg in "$@"; do
  printf '\t%s' "$arg" >> "$OSASCRIPT_LOG"
  if [ "$expect_source" -eq 1 ]; then
    script="$script
$arg"
    expect_source=0
  fi
  case "$arg" in
    -e) expect_source=1 ;;
    -) ;;
  esac
done
printf '\n' >> "$OSASCRIPT_LOG"
if [ "${1:-}" = "-" ]; then
  stdin_script="$(cat)"
  script="$script
$stdin_script"
  printf '%s\n' "$stdin_script" >> "$OSASCRIPT_SOURCE_LOG"
fi
case "$script" in
  *"do shell script"*) printf 'pwned' > "$OSASCRIPT_MARKER" ;;
esac
exit 1
EOF
chmod +x "$TMP/bin/osascript"

cat > "$TMP/bin/screencapture" <<'EOF'
#!/usr/bin/env bash
out="${@: -1}"
printf 'screencapture' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
printf 'PNGDATA' > "$out"
EOF
chmod +x "$TMP/bin/screencapture"

run_ok() {
  "$ROOT/bin/ssh-img-paste" "$@"
}
run_fail() {
  local out status
  set +e
  out="$("$ROOT/bin/ssh-img-paste" "$@" 2>&1)"
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail "expected command to fail: $*"
  printf '%s' "$out"
}

# SSH Image Paste configuration uses the canonical profile path.
reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/main.env" <<'EOF'
SSH_PROFILE_LABEL="Canonical"
SSH_HOST="canonical-host"
SSH_REMOTE_HOME="/srv/canonical"
SSH_REMOTE_DIR="images"
EOF
printf 'main\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
assert_eq "$(run_ok profile current)" "main"
output="$(run_ok profiles)"
assert_contains "$output" $'*\tmain\tCanonical\tcanonical-host'
pass "canonical SSH Image Paste config path"

# Active named profile controls SSH-backed operations.
reset_state
write_profiles
printf 'prod\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
output="$(run_ok list)"
assert_contains "$output" $'123\tuploaded.png'
assert_contains "$(cat "$TEST_LOG")" "prod-host"
assert_not_contains "$(cat "$TEST_LOG")" "dev-host"
assert_eq "$(run_ok profile current)" "prod"
pass "active named profile"

# Explicit --profile targets that profile and does not mutate active-profile.
: > "$TEST_LOG"
output="$(run_ok --profile dev list)"
assert_contains "$output" $'123\tuploaded.png'
assert_contains "$(cat "$TEST_LOG")" "dev-host"
assert_not_contains "$(cat "$TEST_LOG")" "prod-host"
assert_file_eq "$XDG_CONFIG_HOME/ssh-img-paste/active-profile" "prod"
pass "explicit profile override is non-mutating"

# Invalid/traversal profile IDs and missing profiles fail before SSH.
: > "$TEST_LOG"
output="$(run_fail --profile ../escape list)"
assert_contains "$output" "Invalid profile"
assert_eq "$(cat "$TEST_LOG")" ""
output="$(run_fail --profile missing list)"
assert_contains "$output" "SSH profile not found: missing"
assert_eq "$(cat "$TEST_LOG")" ""
printf '../escape\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
output="$(run_fail list)"
assert_contains "$output" "Invalid profile"
assert_eq "$(cat "$TEST_LOG")" ""
pass "invalid and missing profiles fail before SSH"

# Metadata commands validate corrupted active state before reporting selection.
reset_state
write_profiles
printf 'bad/name\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
: > "$TEST_LOG"
output="$(run_fail profile current)"
assert_contains "$output" "Invalid profile"
assert_eq "$(cat "$TEST_LOG")" ""
output="$(run_fail profiles)"
assert_contains "$output" "Invalid profile"
assert_eq "$(cat "$TEST_LOG")" ""
printf 'missing\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
output="$(run_fail profile current)"
assert_contains "$output" "SSH profile not found: missing"
assert_eq "$(cat "$TEST_LOG")" ""
output="$(run_fail profiles)"
assert_contains "$output" "SSH profile not found: missing"
assert_eq "$(cat "$TEST_LOG")" ""
pass "metadata validates malformed and missing active state"

# profiles may emit no rows when truly no profiles exist.
reset_state
assert_eq "$(run_ok profiles)" ""
pass "profiles no-config emits no rows"

# If no active file exists, the first sorted named profile wins.
reset_state
write_profiles
output="$(run_ok profile current)"
assert_eq "$output" "dev"
: > "$TEST_LOG"
run_ok list >/dev/null
assert_contains "$(cat "$TEST_LOG")" "dev-host"
pass "first sorted profile fallback"

# Profiles emit stable tab-separated rows sorted by profile ID, without a header.
reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/default.env" <<'EOF'
SSH_PROFILE_LABEL="Default"
SSH_HOST="default-host"
SSH_REMOTE_HOME="/srv/default"
SSH_REMOTE_DIR="default-images"
EOF
write_profiles
printf 'default\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
output="$(run_ok profiles)"
expected_profiles="$(printf '*\tdefault\tDefault\tdefault-host\n\tdev\tDevelopment\tdev-host\n\tprod\tProduction\tprod-host')"
assert_eq "$output" "$expected_profiles"
pass "profiles list stable rows"

# Listing profiles loads each file independently; labels must not leak between rows.
reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/aaa.env" <<'EOF'
SSH_PROFILE_LABEL="First Label"
SSH_HOST="first-host"
EOF
cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/bbb.env" <<'EOF'
SSH_HOST="second-host"
EOF
output="$(run_ok profiles)"
assert_contains "$output" $'*\taaa\tFirst Label\tfirst-host'
assert_contains "$output" $'\tbbb\tbbb\tsecond-host'
assert_not_contains "$output" $'\tbbb\tFirst Label\tsecond-host'
pass "profile enumeration does not leak sourced values"

# profile use validates, writes active state atomically, and current reports it.
reset_state
write_profiles
printf 'prod\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
output="$(run_ok profile use dev)"
assert_contains "$output" "Development"
assert_contains "$output" "dev-host"
assert_file_eq "$XDG_CONFIG_HOME/ssh-img-paste/active-profile" "dev"
assert_eq "$(run_ok profile current)" "dev"
: > "$TEST_LOG"
output="$(run_fail profile use bad/name)"
assert_contains "$output" "Invalid profile"
assert_file_eq "$XDG_CONFIG_HOME/ssh-img-paste/active-profile" "dev"
assert_eq "$(cat "$TEST_LOG")" ""
output="$(run_fail profile use missing)"
assert_contains "$output" "SSH profile not found: missing"
assert_file_eq "$XDG_CONFIG_HOME/ssh-img-paste/active-profile" "dev"
assert_eq "$(cat "$TEST_LOG")" ""
pass "profile current and use"

# Upload/list/fetch/clean all target the selected profile and upload feedback names it.
reset_state
write_profiles
printf 'prod\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
: > "$TEST_LOG"
output="$(run_ok --profile dev upload)"
assert_contains "$output" "Development"
assert_contains "$output" "/srv/dev/dev-images/clip-"
assert_contains "$(cat "$PBCOPY_LOG")" "/srv/dev/dev-images/clip-"
assert_contains "$(cat "$TEST_LOG")" "dev-host:/srv/dev/dev-images/clip-"
assert_not_contains "$(cat "$TEST_LOG")" "prod-host"
assert_contains "$(cat "$OSASCRIPT_LOG")" "Development"

: > "$TEST_LOG"
: > "$PBCOPY_LOG"
output="$(run_ok --profile dev region)"
assert_contains "$output" "Development"
assert_contains "$output" "/srv/dev/dev-images/shot-"
assert_contains "$(cat "$PBCOPY_LOG")" "/srv/dev/dev-images/shot-"
assert_contains "$(cat "$TEST_LOG")" $'screencapture\t-i\t-x\t-t\tpng\t'
assert_contains "$(cat "$TEST_LOG")" "dev-host:/srv/dev/dev-images/shot-"
assert_not_contains "$(cat "$TEST_LOG")" "prod-host"

: > "$TEST_LOG"
: > "$PBCOPY_LOG"
output="$(run_ok --profile dev full)"
assert_contains "$output" "Development"
assert_contains "$output" "/srv/dev/dev-images/shot-"
assert_contains "$(cat "$PBCOPY_LOG")" "/srv/dev/dev-images/shot-"
assert_contains "$(cat "$TEST_LOG")" $'screencapture\t-x\t-t\tpng\t'
assert_not_contains "$(cat "$TEST_LOG")" $'screencapture\t-i\t-x\t-t\tpng\t'
assert_contains "$(cat "$TEST_LOG")" "dev-host:/srv/dev/dev-images/shot-"
assert_not_contains "$(cat "$TEST_LOG")" "prod-host"

: > "$TEST_LOG"
run_ok --profile dev list >/dev/null
assert_contains "$(cat "$TEST_LOG")" "dev-host"
assert_contains "$(cat "$TEST_LOG")" "/srv/dev/dev-images"
: > "$TEST_LOG"
fetch_path="$(run_ok --profile dev fetch image.png)"
assert_contains "$fetch_path" "/image.png"
assert_contains "$(cat "$TEST_LOG")" "dev-host:/srv/dev/dev-images/image.png"
: > "$TEST_LOG"
for bad_fetch in ../image.png subdir/image.png image.jpg 'bad name.png' 'bad;name.png' '-bad.png'; do
  output="$(run_fail --profile dev fetch "$bad_fetch")"
  assert_contains "$output" "Invalid fetch name"
done
assert_eq "$(cat "$TEST_LOG")" ""
: > "$TEST_LOG"
output="$(run_fail --profile dev fetch image.png extra)"
assert_contains "$output" "usage:"
assert_eq "$(cat "$TEST_LOG")" ""
: > "$TEST_LOG"
output="$(run_ok --profile dev clean)"
assert_contains "$output" "Development"
assert_contains "$output" "dev-host"
assert_contains "$(cat "$TEST_LOG")" "dev-host"
assert_contains "$(cat "$TEST_LOG")" "/srv/dev/dev-images"

# Notification AppleScript is constant source; hostile labels are argv data only.
reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
malicious_label="Evil\" & do shell script \"touch $OSASCRIPT_MARKER\" & \""
cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/evil.env" <<EOF
SSH_PROFILE_LABEL="$malicious_label"
SSH_HOST="evil-host"
SSH_REMOTE_HOME="/srv/evil"
SSH_REMOTE_DIR="evil-images"
SSH_CLIP_RESTORE_SECONDS=0
EOF
: > "$OSASCRIPT_LOG"
: > "$OSASCRIPT_SOURCE_LOG"
output="$(run_ok --profile evil upload)"
assert_contains "$output" "$malicious_label"
[ ! -e "$OSASCRIPT_MARKER" ] || fail "malicious profile label was executed through AppleScript source"
assert_contains "$(cat "$OSASCRIPT_LOG")" $'osascript\t-\tImage path copied — Evil" & do shell script'
assert_contains "$(cat "$OSASCRIPT_LOG")" $'\tclipboard restores in 0s'
assert_not_contains "$(cat "$OSASCRIPT_SOURCE_LOG")" "do shell script"
assert_contains "$(cat "$OSASCRIPT_SOURCE_LOG")" "on run argv"
pass "notification treats malicious labels as AppleScript argv data"
pass "profile-aware upload list fetch clean"

printf 'All tests passed.\n'
