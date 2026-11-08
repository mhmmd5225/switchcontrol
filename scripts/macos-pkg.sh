#!/bin/sh
# Builds target/release/SwitchControl-macOS.pkg: the installer that puts the app
# in /Applications and installs the login-screen service (it runs as root, so
# macOS asks for the administrator password once). Run on a Mac from anywhere
# in the repo. Uses target/release/SwitchControl.app if scripts/macos-app.sh
# already made it.
#
# The installer is not signed, so Gatekeeper makes the first opening a
# right-click > Open.
set -eu
cd "$(dirname "$0")/.."

[ -d target/release/SwitchControl.app ] || sh scripts/macos-app.sh
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)

root=target/release/pkgroot
scripts=target/release/pkgscripts
rm -rf "$root" "$scripts"
mkdir -p "$root" "$scripts"
cp -R target/release/SwitchControl.app "$root/"

cat > "$scripts/postinstall" <<'SCRIPT'
#!/bin/sh
# Runs as root once the app is in place. A failure here must not fail the install:
# the same switch is in the app's settings.
/Applications/SwitchControl.app/Contents/MacOS/SwitchControl --install-login-agent || true
exit 0
SCRIPT
chmod +x "$scripts/postinstall"

pkgbuild --root "$root" \
    --identifier com.switchcontrol.pkg \
    --version "$version" \
    --install-location /Applications \
    --scripts "$scripts" \
    target/release/SwitchControl-macOS.pkg
echo "target/release/SwitchControl-macOS.pkg"
