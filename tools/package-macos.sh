#!/bin/sh
# Builds openMapper.app and a zip of it in target/package (macOS).
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release --locked
OUT=target/package
APP="$OUT/openMapper.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/openmapper "$APP/Contents/MacOS/openmapper"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>openMapper</string>
  <key>CFBundleDisplayName</key><string>openMapper</string>
  <key>CFBundleIdentifier</key><string>org.openomsi.openmapper</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>openmapper</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$APP" 2>/dev/null || true
(cd "$OUT" && rm -f "openMapper-$VERSION-macos.zip" && ditto -c -k --keepParent openMapper.app "openMapper-$VERSION-macos.zip")
echo "$OUT/openMapper-$VERSION-macos.zip"
