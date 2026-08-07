#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "${TMP_ROOT%/}/ssh-img-paste-profile-tests.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

export HOME="$TMP/home"
export XDG_CONFIG_HOME="$HOME/.config"
export SSH_IMG_PASTE_CONFIG_DIR="$XDG_CONFIG_HOME/ssh-img-paste"
export PATH="$TMP/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export TEST_LOG="$TMP/invocations.log"
mkdir -p "$HOME/.config" "$TMP/bin"
: > "$TEST_LOG"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
pass() { printf 'PASS: %s\n' "$*"; }
assert_contains() { case "$1" in *"$2"*) ;; *) fail "expected [$1] to contain [$2]" ;; esac; }
assert_not_contains() { case "$1" in *"$2"*) fail "expected [$1] not to contain [$2]" ;; *) ;; esac; }
assert_eq() { [ "$1" = "$2" ] || fail "expected [$1] to equal [$2]"; }
assert_file_mode() {
  local path="$1" expected="$2" got
  case "$(uname -s)" in
    Darwin) got="$(stat -f '%OLp' "$path")" ;;
    *) got="$(stat -c '%a' "$path")" ;;
  esac
  [ "$got" = "$expected" ] || fail "expected mode $expected for $path, got $got"
}
assert_profile_file_exists() { [ -f "$XDG_CONFIG_HOME/ssh-img-paste/profiles/$1.env" ] || fail "expected profile file for $1"; }
assert_profile_file_missing() { [ ! -e "$XDG_CONFIG_HOME/ssh-img-paste/profiles/$1.env" ] || fail "expected no profile file for $1"; }
assert_key() {
  local output="$1" key="$2" expected="$3" got
  got="$(printf '%s\n' "$output" | while IFS=$'\t' read -r k v; do [ "$k" = "$key" ] && { printf '%s' "$v"; exit 0; }; done)"
  [ "$got" = "$expected" ] || fail "expected inspect $key=[$expected], got [$got] in [$output]"
}
reset_state() {
  rm -rf "$HOME/.config/ssh-img-paste" "$TEST_LOG" "$TMP/marker"
  mkdir -p "$HOME/.config"
  : > "$TEST_LOG"
}
run_ok() { "$ROOT/bin/ssh-img-paste" "$@"; }
run_status() {
  local status
  set +e
  "$ROOT/bin/ssh-img-paste" "$@" > "$TMP/out" 2>&1
  status=$?
  RUN_OUT="$(cat "$TMP/out")"
  return "$status"
}
expect_status() {
  local expected="$1" status
  shift
  set +e
  run_status "$@"
  status=$?
  set -e
  [ "$status" -eq "$expected" ] || fail "expected status $expected for $*, got $status output: $RUN_OUT"
}
write_manual() {
  mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
  cat > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/manual.env" <<'EOF'
# unsupported, but still inspectable and usable without executing this line
SSH_PROFILE_LABEL="Manual"
SSH_HOST="manual-host"
SSH_REMOTE_HOME="/srv/manual"
SSH_REMOTE_DIR="manual-images"
touch "$TMP/marker"
EOF
}

cat > "$TMP/bin/ssh" <<'EOF'
#!/usr/bin/env bash
printf 'ssh' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
if [ "${SSH_FAIL:-0}" = 1 ]; then exit 9; fi
case "${*: -1}" in *find*) printf '1\tfile.png\n' ;; esac
exit 0
EOF
chmod +x "$TMP/bin/ssh"
cat > "$TMP/bin/scp" <<'EOF'
#!/usr/bin/env bash
printf 'scp' >> "$TEST_LOG"
for arg in "$@"; do printf '\t%s' "$arg" >> "$TEST_LOG"; done
printf '\n' >> "$TEST_LOG"
exit 0
EOF
chmod +x "$TMP/bin/scp"
cat > "$TMP/bin/mv" <<'EOF'
#!/usr/bin/env bash
last=""
for arg in "$@"; do last="$arg"; done
if [ "${FAIL_ACTIVE_WRITE:-0}" = 1 ]; then
  case "$last" in */active-profile) exit 73 ;; esac
fi
exec /bin/mv "$@"
EOF
chmod +x "$TMP/bin/mv"
cat > "$TMP/bin/cp" <<'EOF'
#!/usr/bin/env bash
if [ "${KILL_DURING_CP:-0}" = 1 ]; then
  kill -TERM "$PPID" 2>/dev/null || true
  sleep 1
fi
exec /bin/cp "$@"
EOF
chmod +x "$TMP/bin/cp"
cat > "$TMP/bin/rm" <<'EOF'
#!/usr/bin/env bash
if [ -n "${FAIL_PROFILE_RM_ID:-}" ]; then
  for arg in "$@"; do
    case "$arg" in */profiles/"$FAIL_PROFILE_RM_ID".env) exit 73 ;; esac
  done
fi
exec /bin/rm "$@"
EOF
chmod +x "$TMP/bin/rm"
cat > "$TMP/bin/osascript" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "$TMP/bin/osascript"

# CRUD, inspect contract, first-create activation, exact profiles columns.
reset_state
output="$(run_ok profile create dev --label Development --host dev-host --remote-home /srv/dev --remote-dir dev-images --shot-mode full --restore-seconds 7)"
assert_contains "$output" "Created profile: dev"
assert_eq "$(run_ok profile current)" "dev"
inspect="$(run_ok profile inspect dev)"
assert_key "$inspect" id dev
assert_key "$inspect" label Development
assert_key "$inspect" host dev-host
assert_key "$inspect" remote_home /srv/dev
assert_key "$inspect" remote_dir dev-images
assert_key "$inspect" shot_mode full
assert_key "$inspect" restore_seconds 7
assert_key "$inspect" kind app
assert_key "$inspect" editable true
assert_key "$inspect" active true
profiles="$(run_ok profiles)"
assert_eq "$profiles" $'*\tdev\tDevelopment\tdev-host'
assert_file_mode "$XDG_CONFIG_HOME/ssh-img-paste/profiles/dev.env" 600
assert_file_mode "$XDG_CONFIG_HOME/ssh-img-paste/active-profile" 600
pass "create inspect activation permissions and profiles contract"

# Partial update preserves comments and unrelated supported settings.
printf '# keep me\nSSH_PROFILE_LABEL="Development"\nSSH_HOST="dev-host"\nSSH_REMOTE_HOME="/srv/dev"\nSSH_REMOTE_DIR="dev-images"\nSSH_EXTRA="preserve"\nSSH_CLIP_RESTORE_SECONDS=7\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/dev.env"
run_ok profile update dev --label Dev2 --restore-seconds 0 >/dev/null
inspect="$(run_ok profile inspect dev)"
assert_key "$inspect" label Dev2
assert_key "$inspect" host dev-host
assert_key "$inspect" restore_seconds 0
assert_contains "$(cat "$XDG_CONFIG_HOME/ssh-img-paste/profiles/dev.env")" "# keep me"
assert_contains "$(cat "$XDG_CONFIG_HOME/ssh-img-paste/profiles/dev.env")" 'SSH_EXTRA="preserve"'
pass "partial update preserves supported file content"

# Rename updates active under same operation; delete active requires valid switch; last usable delete is blocked.
run_ok profile create prod --label Production --host prod-host --remote-home /srv/prod --remote-dir prod-images >/dev/null
run_ok profile rename dev stage >/dev/null
assert_eq "$(run_ok profile current)" "stage"
expect_status 64 profile delete stage
assert_contains "$RUN_OUT" "--switch-to"
run_ok profile delete stage --switch-to prod >/dev/null
assert_eq "$(run_ok profile current)" "prod"
[ ! -e "$XDG_CONFIG_HOME/ssh-img-paste/profiles/stage.env" ] || fail "stage file still exists"
expect_status 64 profile delete prod
assert_contains "$RUN_OUT" "last usable"
pass "rename active replacement and last delete"

# Adversarial active-state write failures must not leave active pointing at a missing profile.
reset_state
export FAIL_ACTIVE_WRITE=1
expect_status 73 profile create first --label First --host first-host --remote-home /srv/first --remote-dir imgs
unset FAIL_ACTIVE_WRITE
assert_profile_file_missing first
[ ! -e "$XDG_CONFIG_HOME/ssh-img-paste/active-profile" ] || fail "active-profile should not exist after failed first create"
pass "first create active write failure rolls back profile file"

reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste"
printf 'missing\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
export FAIL_ACTIVE_WRITE=1
expect_status 73 profile create stalefail --label StaleFail --host stale-host --remote-home /srv/stale --remote-dir imgs
unset FAIL_ACTIVE_WRITE
assert_profile_file_missing stalefail
assert_eq "$(cat "$XDG_CONFIG_HOME/ssh-img-paste/active-profile")" "missing"
pass "stale active write failure rolls back profile file"

reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste"
printf 'missing\n' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
run_ok profile create recovered --label Recovered --host recovered-host --remote-home /srv/recovered --remote-dir imgs >/dev/null
assert_profile_file_exists recovered
assert_eq "$(run_ok profile current)" "recovered"
pass "create repairs stale active profile pointer"

reset_state
run_ok profile create old --label Old --host old-host --remote-home /srv/old --remote-dir imgs >/dev/null
run_ok profile create spare --label Spare --host spare-host --remote-home /srv/spare --remote-dir imgs >/dev/null
run_ok profile use old >/dev/null
export FAIL_ACTIVE_WRITE=1
expect_status 73 profile rename old new
unset FAIL_ACTIVE_WRITE
assert_eq "$(run_ok profile current)" "old"
assert_profile_file_exists old
assert_profile_file_missing new
assert_profile_file_exists spare
pass "active rename write failure leaves old active state valid"

reset_state
run_ok profile create old --label Old --host old-host --remote-home /srv/old --remote-dir imgs >/dev/null
run_ok profile create spare --label Spare --host spare-host --remote-home /srv/spare --remote-dir imgs >/dev/null
run_ok profile use old >/dev/null
export FAIL_ACTIVE_WRITE=1
expect_status 73 profile delete old --switch-to spare
unset FAIL_ACTIVE_WRITE
assert_eq "$(run_ok profile current)" "old"
assert_profile_file_exists old
assert_profile_file_exists spare
pass "active delete write failure leaves old active state valid"

reset_state
run_ok profile create old --label Old --host old-host --remote-home /srv/old --remote-dir imgs >/dev/null
run_ok profile create spare --label Spare --host spare-host --remote-home /srv/spare --remote-dir imgs >/dev/null
run_ok profile use old >/dev/null
export FAIL_PROFILE_RM_ID=old
expect_status 73 profile delete old --switch-to spare
unset FAIL_PROFILE_RM_ID
assert_contains "$RUN_OUT" "Active profile switched to spare"
assert_eq "$(run_ok profile current)" "spare"
assert_profile_file_exists old
assert_profile_file_exists spare
pass "active delete removal failure leaves switched active state valid"

# Manual unsupported logic is never executed, remains usable/testable/deletable, and update returns 77.
reset_state
write_manual
inspect="$(run_ok profile inspect manual)"
assert_key "$inspect" editable false
assert_key "$inspect" label Manual
[ ! -e "$TMP/marker" ] || fail "manual config executed"
run_ok profile use manual >/dev/null
run_ok profile test manual >/dev/null
assert_contains "$(cat "$TEST_LOG")" $'ssh\t-o\tBatchMode=yes'
assert_contains "$(cat "$TEST_LOG")" $'\tmanual-host\t/usr/bin/true'
expect_status 77 profile update manual --label Nope
run_ok profile create keep --label Keep --host keep-host --remote-home /srv/keep --remote-dir imgs >/dev/null
run_ok profile delete manual --switch-to keep >/dev/null
[ ! -e "$XDG_CONFIG_HOME/ssh-img-paste/profiles/manual.env" ] || fail "manual file still exists"
pass "manual profile is read-only and non-executing"

reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
printf 'SSH_PROFILE_LABEL="Manual"\nSSH_HOST="literal-host"\nSSH_REMOTE_HOME="/srv/manual"\nSSH_REMOTE_DIR="manual-images"\nSSH_EXTRA=${USER}\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/manual.env"
inspect="$(run_ok profile inspect manual)"
assert_key "$inspect" label Manual
assert_key "$inspect" host literal-host
assert_key "$inspect" editable false
printf 'SSH_PROFILE_LABEL="Dynamic"\nSSH_HOST=${USER}@host\nSSH_REMOTE_HOME="/srv/manual"\nSSH_REMOTE_DIR="manual-images"\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/dynamic.env"
expect_status 64 profile inspect dynamic
assert_contains "$RUN_OUT" "Only literal assignments are supported"
assert_contains "$RUN_OUT" "SSH_HOST"
assert_eq "$(cat "$TEST_LOG")" ""
pass "dynamic supported assignments fail clearly while unsupported extras remain read-only usable"

# Input validation: injection, traversal, controls, ssh target leading dash/whitespace.
reset_state
huge_delay=999999999999999999999999999999999999999999999999999999999999
huge_delay="$huge_delay$huge_delay$huge_delay$huge_delay"
expect_status 64 profile create '../bad' --label Bad --host host --remote-home /srv --remote-dir imgs
expect_status 64 profile create bad --label Bad --host '-host' --remote-home /srv --remote-dir imgs
expect_status 64 profile create bad --label Bad --host 'host name' --remote-home /srv --remote-dir imgs
expect_status 64 profile create bad --label Bad --host '$(touch pwn)' --remote-home /srv --remote-dir imgs
expect_status 64 profile create bad --label Bad --host host --remote-home srv --remote-dir imgs
expect_status 64 profile create bad --label Bad --host host --remote-home /srv --remote-dir '../imgs'
expect_status 64 profile create bad --label Bad --host host --remote-home /srv --remote-dir 'img;touch-pwn'
expect_status 64 profile create bad --label $'Bad\tTab' --host host --remote-home /srv --remote-dir imgs
expect_status 64 profile create too_slow --label Slow --host slow-host --remote-home /srv/slow --remote-dir imgs --restore-seconds 86401
assert_profile_file_missing too_slow
expect_status 64 profile create way_too_slow --label Slow --host slow-host --remote-home /srv/slow --remote-dir imgs --restore-seconds 999999
assert_profile_file_missing way_too_slow
expect_status 64 profile create overflow_slow --label Slow --host slow-host --remote-home /srv/slow --remote-dir imgs --restore-seconds "$huge_delay"
assert_contains "$RUN_OUT" "Invalid restore_seconds"
assert_profile_file_missing overflow_slow
assert_eq "$(cat "$TEST_LOG")" ""
run_ok profile create min_delay --label Min --host min-host --remote-home /srv/min --remote-dir imgs --restore-seconds 0 >/dev/null
assert_key "$(run_ok profile inspect min_delay)" restore_seconds 0
run_ok profile create zero_padded_delay --label Zero --host zero-host --remote-home /srv/zero --remote-dir imgs --restore-seconds 00000 >/dev/null
assert_key "$(run_ok profile inspect zero_padded_delay)" restore_seconds 00000
run_ok profile create max_delay --label Max --host max-host --remote-home /srv/max --remote-dir imgs --restore-seconds 86400 >/dev/null
assert_key "$(run_ok profile inspect max_delay)" restore_seconds 86400
run_ok profile create max_padded_delay --label MaxPad --host maxpad-host --remote-home /srv/maxpad --remote-dir imgs --restore-seconds 086400 >/dev/null
assert_key "$(run_ok profile inspect max_padded_delay)" restore_seconds 086400
profile_before="$(cat "$XDG_CONFIG_HOME/ssh-img-paste/profiles/max_delay.env")"
expect_status 64 profile update max_delay --restore-seconds 86401
assert_eq "$(cat "$XDG_CONFIG_HOME/ssh-img-paste/profiles/max_delay.env")" "$profile_before"
expect_status 64 profile update max_delay --restore-seconds 999999
assert_eq "$(cat "$XDG_CONFIG_HOME/ssh-img-paste/profiles/max_delay.env")" "$profile_before"
expect_status 64 profile update max_delay --restore-seconds "$huge_delay"
assert_contains "$RUN_OUT" "Invalid restore_seconds"
assert_eq "$(cat "$XDG_CONFIG_HOME/ssh-img-paste/profiles/max_delay.env")" "$profile_before"
pass "validation rejects injection traversal controls and out-of-range restore delay before side effects"

# Loaded remote fields are validated before SSH/SCP.
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
printf 'SSH_PROFILE_LABEL="Bad"\nSSH_HOST="ok-host"\nSSH_REMOTE_HOME="/srv"\nSSH_REMOTE_DIR="bad;dir"\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/bad.env"
expect_status 64 --profile bad list
assert_eq "$(cat "$TEST_LOG")" ""
printf 'SSH_PROFILE_LABEL="Slow"\nSSH_HOST="ok-host"\nSSH_REMOTE_HOME="/srv"\nSSH_REMOTE_DIR="imgs"\nSSH_CLIP_RESTORE_SECONDS=86401\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/slow.env"
expect_status 64 --profile slow list
assert_contains "$RUN_OUT" "Invalid restore_seconds"
assert_eq "$(cat "$TEST_LOG")" ""
printf 'SSH_PROFILE_LABEL="Overflow"\nSSH_HOST="ok-host"\nSSH_REMOTE_HOME="/srv"\nSSH_REMOTE_DIR="imgs"\nSSH_CLIP_RESTORE_SECONDS=%s\n' "$huge_delay" > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/slow.env"
expect_status 64 --profile slow list
assert_contains "$RUN_OUT" "Invalid restore_seconds"
assert_eq "$(cat "$TEST_LOG")" ""
printf 'SSH_PROFILE_LABEL="Max"\nSSH_HOST="ok-host"\nSSH_REMOTE_HOME="/srv"\nSSH_REMOTE_DIR="imgs"\nSSH_CLIP_RESTORE_SECONDS=86400\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/slow.env"
assert_key "$(run_ok profile inspect slow)" restore_seconds 86400
pass "loaded remote fields and restore delay validate before network"

# Loaded labels reject controls before machine output or network use; empty labels still fall back to ID.
reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
printf 'SSH_PROFILE_LABEL=""\nSSH_HOST="empty-host"\nSSH_REMOTE_HOME="/srv/empty"\nSSH_REMOTE_DIR="imgs"\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/empty.env"
assert_key "$(run_ok profile inspect empty)" label empty
printf 'bad' > "$XDG_CONFIG_HOME/ssh-img-paste/active-profile"
printf 'SSH_PROFILE_LABEL="Bad\tLabel"\nSSH_HOST="ok-host"\nSSH_REMOTE_HOME="/srv"\nSSH_REMOTE_DIR="imgs"\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/bad.env"
: > "$TEST_LOG"
expect_status 64 profiles
assert_contains "$RUN_OUT" "Invalid label"
assert_not_contains "$RUN_OUT" $'\t'
expect_status 64 profile inspect bad
assert_contains "$RUN_OUT" "Invalid label"
assert_not_contains "$RUN_OUT" $'\t'
assert_eq "$(cat "$TEST_LOG")" ""
printf 'SSH_PROFILE_LABEL="Bad\001Label"\nSSH_HOST="ok-host"\nSSH_REMOTE_HOME="/srv"\nSSH_REMOTE_DIR="imgs"\n' > "$XDG_CONFIG_HOME/ssh-img-paste/profiles/bad.env"
expect_status 64 profiles
assert_contains "$RUN_OUT" "Invalid label"
assert_not_contains "$RUN_OUT" $'\t'
expect_status 64 profile inspect bad
assert_contains "$RUN_OUT" "Invalid label"
assert_not_contains "$RUN_OUT" $'\t'
assert_eq "$(cat "$TEST_LOG")" ""
pass "loaded label controls fail safely and empty label falls back to id"

# Symlink and lock refusal/recovery.
reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"
ln -s "$TMP/elsewhere" "$XDG_CONFIG_HOME/ssh-img-paste/profiles/link.env"
expect_status 73 profile create link --label Link --host link-host --remote-home /srv/link --remote-dir imgs
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/.lock"
expect_status 73 profile create locked --label Locked --host locked-host --remote-home /srv/locked --remote-dir imgs
rm -rf "$XDG_CONFIG_HOME/ssh-img-paste/.lock"
ln -s "$TMP/elsewhere-lock" "$XDG_CONFIG_HOME/ssh-img-paste/.lock"
expect_status 73 profile create symlinklock --label Locked --host locked-host --remote-home /srv/locked --remote-dir imgs
rm -f "$XDG_CONFIG_HOME/ssh-img-paste/.lock"
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/.lock"
printf '%s\n' "$$" > "$XDG_CONFIG_HOME/ssh-img-paste/.lock/owner"
expect_status 73 profile create livelock --label Live --host live-host --remote-home /srv/live --remote-dir imgs
rm -f "$XDG_CONFIG_HOME/ssh-img-paste/.lock/owner"
rmdir "$XDG_CONFIG_HOME/ssh-img-paste/.lock"
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/.lock"
printf '999999\n' > "$XDG_CONFIG_HOME/ssh-img-paste/.lock/owner"
run_ok profile create stalerecovered --label Stale --host stale-host --remote-home /srv/stale --remote-dir imgs >/dev/null
assert_profile_file_exists stalerecovered
[ ! -e "$XDG_CONFIG_HOME/ssh-img-paste/.lock" ] || fail "stale lock was not cleaned after successful mutation"
reset_state
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste"
set +e
KILL_DURING_CP=1 "$ROOT/bin/ssh-img-paste" profile create interrupted --label Interrupted --host int-host --remote-home /srv/int --remote-dir imgs > "$TMP/out" 2>&1
status=$?
set -e
[ "$status" -ne 0 ] || fail "expected interrupted create to fail"
[ ! -e "$XDG_CONFIG_HOME/ssh-img-paste/.lock" ] || fail "interrupted mutation left lock behind"
pass "symlink live stale and interrupted lock handling"

# profile test status and delete never calls ssh.
reset_state
run_ok profile create ok --label OK --host ok-host --remote-home /srv/ok --remote-dir imgs >/dev/null
: > "$TEST_LOG"
run_ok profile test ok >/dev/null
assert_eq "$(cat "$TEST_LOG")" $'ssh\t-o\tBatchMode=yes\t-o\tConnectTimeout=6\tok-host\t/usr/bin/true'
export SSH_FAIL=1
expect_status 75 profile test ok
unset SSH_FAIL
run_ok profile create spare --label Spare --host spare-host --remote-home /srv/spare --remote-dir imgs >/dev/null
: > "$TEST_LOG"
run_ok profile delete ok --switch-to spare >/dev/null
assert_eq "$(cat "$TEST_LOG")" ""
pass "profile test and delete no ssh"

printf 'All profile management tests passed.\n'
