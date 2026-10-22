#!/bin/sh
# Builds target/release/SwitchControl.app. Run on a Mac from anywhere in the repo.
#
# The app is signed ad hoc, so macOS ties its privacy permissions to this exact
# build: after rebuilding, remove SwitchControl from Input Monitoring and
# Accessibility and add it again.
set -eu
cd "$(dirname "$0")/.."

cargo build --release -p switchcontrol

app=target/release/SwitchControl.app
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/switchcontrol "$app/Contents/MacOS/SwitchControl"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>SwitchControl</string>
    <key>CFBundleDisplayName</key>
    <string>SwitchControl</string>
    <key>CFBundleIdentifier</key>
    <string>com.switchcontrol.app</string>
    <key>CFBundleExecutable</key>
    <string>SwitchControl</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
codesign --force --sign - --identifier com.switchcontrol.app "$app"
echo "built $app"
