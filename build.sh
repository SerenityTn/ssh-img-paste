#!/usr/bin/env bash
# Compile the menu-bar app from src/ into ~/Applications/SSHImagePaste.app.
set -euo pipefail
cd "$(dirname "$0")"

APP="${APP_DIR:-$HOME/Applications}/SSHImagePaste.app"
SRC_FILES=(src/*.swift)
ARCH="$(uname -m)"
VERSION="$(tr -d '[:space:]' < VERSION)"

case "$VERSION" in
  ''|*[!0-9.]*)
    echo "Invalid VERSION: $VERSION" >&2
    exit 1
    ;;
esac

echo "Building $APP ..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>SSHImagePaste</string>
  <key>CFBundleDisplayName</key><string>SSH Image Paste</string>
  <key>CFBundleIdentifier</key><string>com.khaireddine.vpsimgpaste</string>
  <key>CFBundleExecutable</key><string>SSHImagePaste</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSUIElement</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

swiftc -O -target "$ARCH-apple-macos13.0" -o "$APP/Contents/MacOS/SSHImagePaste" "${SRC_FILES[@]}" -framework AppKit

# A default ad-hoc signature uses the binary's changing cdhash as its designated
# requirement. macOS TCC then treats every rebuilt app as a new Screen Recording
# client. Keep the ad-hoc build, but give it a stable bundle-ID requirement so a
# user's permission survives source rebuilds.
codesign --force --sign - \
  --requirements '=designated => identifier "com.khaireddine.vpsimgpaste"' \
  "$APP"
echo "✓ Built $APP"
