#!/bin/sh
# Install the X-Remote egui client into the user directory. No root needed.
#
#   sh native/scripts/install-linux.sh [path to uvclient]
#
# Default is the release build ($CARGO_TARGET_DIR or native/target).
# Result:
#   ~/.local/share/x-remote/x-remote             binary
#   ~/.local/bin/x-remote                        launcher (args passed through)
#   ~/.local/share/applications/x-remote.desktop "X-Remote" menu entry
#   ~/.local/share/icons/hicolor/{32,48,64,128,256}x…/apps/x-remote.png
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${1:-${CARGO_TARGET_DIR:-$ROOT/target}/release/uvclient}"
DEST="$HOME/.local/share/x-remote"
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
HICOLOR="$HOME/.local/share/icons/hicolor"

[ -x "$SRC" ] || { echo "Binary not found: $SRC (run 'cargo build --release -p uvclient' first)" >&2; exit 1; }

mkdir -p "$DEST" "$BIN" "$APPS"
install -m 755 "$SRC" "$DEST/x-remote.new"
mv -f "$DEST/x-remote.new" "$DEST/x-remote"

install_icon() {
    size="$1"
    src="$2"
    if [ -f "$src" ]; then
        dir="$HICOLOR/${size}x${size}/apps"
        mkdir -p "$dir"
        install -m 644 "$src" "$dir/x-remote.png"
    fi
}
install_icon 32 "$ROOT/packaging/icon-32.png"
install_icon 48 "$ROOT/packaging/icon-48.png"
install_icon 64 "$ROOT/packaging/icon-64.png"
install_icon 128 "$ROOT/packaging/x-remote.png"
install_icon 256 "$ROOT/packaging/icon.png"

cat > "$BIN/x-remote" <<SH
#!/bin/sh
# X-Remote — no args: open the remote ID/password screen.
# Connect now: x-remote --uv --id <ID> --pw <password> --connect
exec "$DEST/x-remote" "\$@"
SH
chmod 755 "$BIN/x-remote"

cat > "$APPS/x-remote.desktop" <<DESK
[Desktop Entry]
Type=Application
Version=1.0
Name=X-Remote
Comment=Control a Windows machine through UltraViewer
Exec=$BIN/x-remote
Icon=x-remote
Terminal=false
Categories=Network;RemoteAccess;
Keywords=remote;ultraviewer;desktop;
StartupNotify=true
StartupWMClass=x-remote
DESK

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true

echo "Installed X-Remote:"
echo "  binary:   $DEST/x-remote ($(sha256sum "$DEST/x-remote" | cut -c1-16)...)"
echo "  command:  x-remote"
echo "  menu:     X-Remote (Internet/Network)"
