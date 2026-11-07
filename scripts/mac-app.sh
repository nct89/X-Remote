#!/bin/bash
# Package the release build as X-Remote.app on macOS (a GUI window needs a
# bundle for correct keyboard focus) and open it. Optional: --screenshot <file.jpg>
# so the app captures its own window and exits (no Screen Recording permission).
#
#   sh scripts/mac-app.sh                       # build + package + open
#   sh scripts/mac-app.sh --screenshot /tmp/x.jpg
set -eu
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release -p uvclient 2>&1 | tail -1

APP="target/X-Remote.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/uvclient "$APP/Contents/MacOS/x-remote"
cp packaging/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>X-Remote</string>
    <key>CFBundleDisplayName</key><string>X-Remote</string>
    <key>CFBundleIdentifier</key><string>net.xremote.app</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>x-remote</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

pkill -f "X-Remote.app/Contents/MacOS/x-remote" 2>/dev/null || true
if [ "${1:-}" = "--screenshot" ] && [ -n "${2:-}" ]; then
    rm -f "$2"
    open -n "$APP" --args --screenshot "$2"
    for _ in $(seq 1 20); do
        sleep 1
        [ -s "$2" ] && break
    done
    ls -l "$2" 2>/dev/null || { echo "no screenshot"; exit 1; }
else
    open -n "$APP"
fi
