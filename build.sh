#!/usr/bin/env bash
# Compile the menu-bar app from src/ into ~/Applications/VpsImgPaste.app.
set -euo pipefail
cd "$(dirname "$0")"

APP="${APP_DIR:-$HOME/Applications}/VpsImgPaste.app"
SRC_FILES=(src/*.swift)
ARCH="$(uname -m)"

echo "Building $APP ..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>VpsImgPaste</string>
  <key>CFBundleDisplayName</key><string>VPS Image Paste</string>
  <key>CFBundleIdentifier</key><string>com.khaireddine.vpsimgpaste</string>
  <key>CFBundleExecutable</key><string>VpsImgPaste</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSUIElement</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

swiftc -O -target "$ARCH-apple-macos13.0" -o "$APP/Contents/MacOS/VpsImgPaste" "${SRC_FILES[@]}" -framework AppKit

# A default ad-hoc signature uses the binary's changing cdhash as its designated
# requirement. macOS TCC then treats every rebuilt app as a new Screen Recording
# client. Keep the ad-hoc build, but give it a stable bundle-ID requirement so a
# user's permission survives source rebuilds.
codesign --force --sign - \
  --requirements '=designated => identifier "com.khaireddine.vpsimgpaste"' \
  "$APP"
echo "✓ Built $APP"
