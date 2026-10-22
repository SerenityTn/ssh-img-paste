#!/usr/bin/env bash
# Builds the Linux release archive:
#   dist/ssh-img-paste-linux-<version>-<arch>.tar.gz and dist/SHA256SUMS.txt
#
# The version comes from rust/ssh-img-paste-linux/Cargo.toml. Set
# SKIP_BUILD=1 to package binaries that are already built in release mode.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_ID="io.github.serenitytn.SSHImagePaste"
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/rust/ssh-img-paste-linux/Cargo.toml" | head -1)"
ARCH="$(uname -m)"
NAME="ssh-img-paste-linux-$VERSION-$ARCH"
DIST="$ROOT/dist"
APP="$ROOT/rust/ssh-img-paste-linux/target/release/ssh-img-paste-linux"
CLI="$ROOT/rust/ssh-img-paste-cli/target/release/ssh-img-paste"

if [ "${SKIP_BUILD:-0}" != 1 ]; then
  cargo build --release --locked --manifest-path "$ROOT/rust/ssh-img-paste-cli/Cargo.toml" --bin ssh-img-paste
  cargo build --release --locked --manifest-path "$ROOT/rust/ssh-img-paste-linux/Cargo.toml"
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir "$STAGE/$NAME"
install -m 755 "$APP" "$CLI" "$ROOT/linux/install.sh" "$STAGE/$NAME/"
install -m 644 "$ROOT/linux/$APP_ID.desktop.in" "$STAGE/$NAME/"
install -m 644 "$ROOT/assets/AppIcon.svg" "$STAGE/$NAME/$APP_ID.svg"
install -m 644 "$ROOT/LICENSE" "$STAGE/$NAME/LICENSE"
install -m 644 "$ROOT/docs/LINUX.md" "$STAGE/$NAME/README.md"
strip "$STAGE/$NAME/ssh-img-paste-linux" "$STAGE/$NAME/ssh-img-paste" 2>/dev/null || true

mkdir -p "$DIST"
tar -C "$STAGE" --owner=0 --group=0 --sort=name -czf "$DIST/$NAME.tar.gz" "$NAME"
(cd "$DIST" && sha256sum "$NAME.tar.gz" > SHA256SUMS.txt)
printf 'Built %s\n' "$DIST/$NAME.tar.gz"
cat "$DIST/SHA256SUMS.txt"
