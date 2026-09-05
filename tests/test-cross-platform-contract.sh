#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "${TMP_ROOT%/}/ssh-img-paste-contract.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

export HOME="$TMP/home"
export XDG_CONFIG_HOME="$TMP/config"
mkdir -p "$XDG_CONFIG_HOME/ssh-img-paste/profiles"

assert_key() {
  local output="$1" key="$2" expected="$3" actual
  actual="$(printf '%s\n' "$output" | while IFS=$'\t' read -r k v; do
    if [ "$k" = "$key" ]; then printf '%s' "$v"; exit 0; fi
  done)"
  [ "$actual" = "$expected" ] || {
    printf 'FAIL: expected %s=%q, got %q\n' "$key" "$expected" "$actual" >&2
    exit 1
  }
}

inspect_fixture() {
  local fixture="$1" id="$2"
  cp "$ROOT/contract/profiles/$fixture" "$XDG_CONFIG_HOME/ssh-img-paste/profiles/$id.env"
  "$ROOT/bin/ssh-img-paste" profile inspect "$id"
}

app="$(inspect_fixture app-literal.env app)"
assert_key "$app" label 'Development Host'
assert_key "$app" host dev-host
assert_key "$app" remote_home /srv/dev
assert_key "$app" remote_dir dev-images
assert_key "$app" shot_mode full
assert_key "$app" restore_seconds 00007
assert_key "$app" kind app
assert_key "$app" editable true

manual="$(inspect_fixture manual-command.env manual)"
assert_key "$manual" host manual-host
assert_key "$manual" kind manual
assert_key "$manual" editable false
[ ! -e /tmp/ssh-img-paste-must-not-exist ] || {
  printf 'FAIL: manual fixture command appears to have executed\n' >&2
  exit 1
}

extra="$(inspect_fixture manual-dynamic-extra.env extra)"
assert_key "$extra" label 'Manual Extra'
assert_key "$extra" host literal-host
assert_key "$extra" kind manual
assert_key "$extra" editable false

cp "$ROOT/contract/profiles/dynamic-supported.env" \
  "$XDG_CONFIG_HOME/ssh-img-paste/profiles/dynamic.env"
set +e
dynamic_output="$("$ROOT/bin/ssh-img-paste" profile inspect dynamic 2>&1)"
dynamic_status=$?
set -e
[ "$dynamic_status" -eq 64 ] || {
  printf 'FAIL: expected dynamic supported assignment status 64, got %s\n' "$dynamic_status" >&2
  exit 1
}
case "$dynamic_output" in
  *SSH_HOST*"Only literal assignments are supported"*) ;;
  *) printf 'FAIL: unexpected dynamic assignment diagnostic: %s\n' "$dynamic_output" >&2; exit 1 ;;
esac

printf 'PASS: macOS Bash and Rust profile contract fixtures are frozen\n'
