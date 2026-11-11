#!/bin/sh
# Package the macOS app: a universal binary (.app for Apple Silicon and Intel).
#
# Needs: Xcode Command Line Tools (`lipo`), Rust with both darwin targets.
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#
# Output: dist/X-Remote.app and dist/X-Remote-VERSION-macos-universal.dmg

set -eu

cd "$(dirname "$0")/.."

APP_NAME="X-Remote"
BUNDLE="dist/${APP_NAME}.app"
VERSION="$(awk -F\" '/^version/{print $2; exit}' Cargo.toml)"
[ -n "$VERSION" ] || VERSION="0.1.0"

echo "==> Build aarch64-apple-darwin"
cargo build --release -p uvclient --target aarch64-apple-darwin

echo "==> Build x86_64-apple-darwin"
cargo build --release -p uvclient --target x86_64-apple-darwin

echo "==> Link universal binary"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"

lipo -create \
    "target/aarch64-apple-darwin/release/uvclient" \
    "target/x86_64-apple-darwin/release/uvclient" \
    -output "$BUNDLE/Contents/MacOS/uvclient"

chmod +x "$BUNDLE/Contents/MacOS/uvclient"
cp packaging/AppIcon.icns "$BUNDLE/Contents/Resources/AppIcon.icns"

sed "s/<string>0.1.0</<string>${VERSION}</g" packaging/Info.plist \
    > "$BUNDLE/Contents/Info.plist"

# Ad-hoc sign so Gatekeeper does not block local runs.
# Publishing to other people needs a Developer ID and notarization.
codesign --force --deep --sign - "$BUNDLE" 2>/dev/null || \
    echo "   (skip codesign — missing; the app still opens via right-click > Open)"

echo "==> Universal check"
lipo -info "$BUNDLE/Contents/MacOS/uvclient"

echo "==> Disk image"
DMG="dist/X-Remote-${VERSION}-macos-universal.dmg"
rm -f "$DMG"
hdiutil create -volname "X-Remote ${VERSION}" -srcfolder "$BUNDLE" -ov -format UDZO "$DMG"

echo
echo "Done: $BUNDLE"
echo "Done: $DMG"
echo "Run: open \"$BUNDLE\""
