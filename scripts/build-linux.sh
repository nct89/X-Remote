#!/bin/sh
# Package X-Remote for Linux distros:
#   dist/x-remote                                 runnable binary
#   dist/X-Remote-x86_64.AppImage                 all distros (no root)
#   dist/x-remote_VERSION_amd64.deb               Debian / Ubuntu / Mint
#   dist/x-remote-VERSION-1.x86_64.rpm            Fedora / RHEL / openSUSE
#   dist/x-remote-VERSION-linux-x86_64.tar.gz     portable tree + install.sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

VERSION="$(awk -F\" '/^version/{print $2; exit}' Cargo.toml)"
[ -n "$VERSION" ] || VERSION="0.1.0"
ARCH_DEB="amd64"
ARCH_RPM="x86_64"
DIST="$ROOT/dist"
PKG="$ROOT/packaging"
BIN_SRC="$ROOT/target/release/uvclient"

echo "==> cargo build --release -p uvclient"
cargo build --release -p uvclient
[ -x "$BIN_SRC" ] || { echo "missing $BIN_SRC" >&2; exit 1; }

rm -rf "$DIST"
mkdir -p "$DIST"
install -m 755 "$BIN_SRC" "$DIST/x-remote"

# ---------- portable tarball ----------
echo "==> tar.gz"
TREE="$DIST/x-remote-${VERSION}-linux-${ARCH_RPM}"
rm -rf "$TREE"
mkdir -p "$TREE/icons"
install -m 755 "$BIN_SRC" "$TREE/x-remote"
install -m 644 "$PKG/x-remote.desktop" "$TREE/x-remote.desktop"
install -m 644 "$ROOT/README.md" "$TREE/README.md"
install -m 644 "$ROOT/LICENSE" "$TREE/LICENSE"
mkdir -p "$TREE/icons/32x32/apps" "$TREE/icons/48x48/apps" "$TREE/icons/64x64/apps" \
         "$TREE/icons/128x128/apps" "$TREE/icons/256x256/apps"
[ -f "$PKG/icon-32.png" ] && install -m 644 "$PKG/icon-32.png" "$TREE/icons/32x32/apps/x-remote.png"
[ -f "$PKG/icon-48.png" ] && install -m 644 "$PKG/icon-48.png" "$TREE/icons/48x48/apps/x-remote.png"
[ -f "$PKG/icon-64.png" ] && install -m 644 "$PKG/icon-64.png" "$TREE/icons/64x64/apps/x-remote.png"
[ -f "$PKG/x-remote.png" ] && install -m 644 "$PKG/x-remote.png" "$TREE/icons/128x128/apps/x-remote.png"
[ -f "$PKG/icon.png" ] && install -m 644 "$PKG/icon.png" "$TREE/icons/256x256/apps/x-remote.png"

cat > "$TREE/install.sh" <<'SH'
#!/bin/sh
# User-local install (no root). PREFIX defaults to ~/.local
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
SHARE="$PREFIX/share"
DEST="$HOME/.local/share/x-remote"
mkdir -p "$BIN" "$SHARE/applications" "$SHARE/icons/hicolor" "$DEST"
install -m 755 "$HERE/x-remote" "$DEST/x-remote"
cat > "$BIN/x-remote" <<EOF
#!/bin/sh
exec "$DEST/x-remote" "\$@"
EOF
chmod 755 "$BIN/x-remote"
install -m 644 "$HERE/x-remote.desktop" "$SHARE/applications/x-remote.desktop"
# Point Icon= at the theme name; copy hicolor sizes.
if [ -d "$HERE/icons" ]; then
    cp -a "$HERE/icons/." "$SHARE/icons/hicolor/"
fi
# Rewrite Exec= to the launcher we just wrote.
if command -v sed >/dev/null 2>&1; then
    sed -i "s|^Exec=.*|Exec=$BIN/x-remote|" "$SHARE/applications/x-remote.desktop" 2>/dev/null || true
fi
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$SHARE/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$SHARE/icons/hicolor" >/dev/null 2>&1 || true
echo "Installed X-Remote to $BIN/x-remote"
echo "Add $BIN to PATH if 'x-remote' is not found."
SH
chmod 755 "$TREE/install.sh"

tar -C "$DIST" -czf "$DIST/x-remote-${VERSION}-linux-${ARCH_RPM}.tar.gz" "$(basename "$TREE")"
rm -rf "$TREE"

# ---------- AppImage ----------
echo "==> AppImage"
APPDIR="$DIST/X-Remote.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/metainfo" \
         "$APPDIR/usr/share/icons/hicolor/32x32/apps" \
         "$APPDIR/usr/share/icons/hicolor/48x48/apps" \
         "$APPDIR/usr/share/icons/hicolor/64x64/apps" \
         "$APPDIR/usr/share/icons/hicolor/128x128/apps" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"
install -m 755 "$BIN_SRC" "$APPDIR/usr/bin/x-remote"
install -m 644 "$PKG/x-remote.desktop" "$APPDIR/x-remote.desktop"
install -m 644 "$PKG/x-remote.desktop" "$APPDIR/usr/share/applications/x-remote.desktop"
install -m 644 "$PKG/net.xremote.app.metainfo.xml" "$APPDIR/usr/share/metainfo/net.xremote.app.metainfo.xml"
[ -f "$PKG/icon-32.png" ] && install -m 644 "$PKG/icon-32.png" "$APPDIR/usr/share/icons/hicolor/32x32/apps/x-remote.png"
[ -f "$PKG/icon-48.png" ] && install -m 644 "$PKG/icon-48.png" "$APPDIR/usr/share/icons/hicolor/48x48/apps/x-remote.png"
[ -f "$PKG/icon-64.png" ] && install -m 644 "$PKG/icon-64.png" "$APPDIR/usr/share/icons/hicolor/64x64/apps/x-remote.png"
[ -f "$PKG/x-remote.png" ] && install -m 644 "$PKG/x-remote.png" "$APPDIR/usr/share/icons/hicolor/128x128/apps/x-remote.png"
[ -f "$PKG/icon.png" ] && install -m 644 "$PKG/icon.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/x-remote.png"
# AppImage top-level icon (256)
[ -f "$PKG/icon.png" ] && install -m 644 "$PKG/icon.png" "$APPDIR/x-remote.png"

cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="$HERE/usr/bin:${PATH:-}"
exec "$HERE/usr/bin/x-remote" "$@"
EOF
chmod 755 "$APPDIR/AppRun"

TOOLDIR="$DIST/.tools"
mkdir -p "$TOOLDIR"
TOOL="$TOOLDIR/appimagetool-x86_64.AppImage"
if [ ! -x "$TOOL" ]; then
    echo "    downloading appimagetool…"
    curl -fsSL -o "$TOOL" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$TOOL"
fi
if ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" \
        "$DIST/X-Remote-x86_64.AppImage"; then
    chmod +x "$DIST/X-Remote-x86_64.AppImage"
else
    echo "    appimagetool failed — AppDir kept at $APPDIR" >&2
fi

# ---------- .deb ----------
if command -v dpkg-deb >/dev/null 2>&1; then
    echo "==> .deb"
    DEB="$DIST/deb"
    rm -rf "$DEB"
    mkdir -p "$DEB/DEBIAN" "$DEB/usr/bin" "$DEB/usr/share/applications" \
             "$DEB/usr/share/doc/x-remote" \
             "$DEB/usr/share/metainfo" \
             "$DEB/usr/share/icons/hicolor/32x32/apps" \
             "$DEB/usr/share/icons/hicolor/48x48/apps" \
             "$DEB/usr/share/icons/hicolor/64x64/apps" \
             "$DEB/usr/share/icons/hicolor/128x128/apps" \
             "$DEB/usr/share/icons/hicolor/256x256/apps"
    install -m 755 "$BIN_SRC" "$DEB/usr/bin/x-remote"
    install -m 644 "$PKG/x-remote.desktop" "$DEB/usr/share/applications/x-remote.desktop"
    install -m 644 "$PKG/net.xremote.app.metainfo.xml" "$DEB/usr/share/metainfo/net.xremote.app.metainfo.xml"
    install -m 644 "$ROOT/LICENSE" "$DEB/usr/share/doc/x-remote/copyright"
    install -m 644 "$ROOT/README.md" "$DEB/usr/share/doc/x-remote/README.md"
    [ -f "$PKG/icon-32.png" ] && install -m 644 "$PKG/icon-32.png" "$DEB/usr/share/icons/hicolor/32x32/apps/x-remote.png"
    [ -f "$PKG/icon-48.png" ] && install -m 644 "$PKG/icon-48.png" "$DEB/usr/share/icons/hicolor/48x48/apps/x-remote.png"
    [ -f "$PKG/icon-64.png" ] && install -m 644 "$PKG/icon-64.png" "$DEB/usr/share/icons/hicolor/64x64/apps/x-remote.png"
    [ -f "$PKG/x-remote.png" ] && install -m 644 "$PKG/x-remote.png" "$DEB/usr/share/icons/hicolor/128x128/apps/x-remote.png"
    [ -f "$PKG/icon.png" ] && install -m 644 "$PKG/icon.png" "$DEB/usr/share/icons/hicolor/256x256/apps/x-remote.png"
    SIZE_KB="$(du -sk "$DEB" | awk '{print $1}')"
    cat > "$DEB/DEBIAN/control" <<EOF
Package: x-remote
Version: $VERSION
Section: net
Priority: optional
Architecture: $ARCH_DEB
Installed-Size: $SIZE_KB
Maintainer: X-Remote authors
Homepage: https://github.com
Depends: libc6, libx11-6, libxcursor1, libxi6, libxrandr2, libxext6, libgl1
Recommends: gir1.2-webkit2-4.1, python3-gi
Description: Native remote-desktop client
 Control a Windows UltraViewer host from Linux. Connect by ID and password,
 stream the remote screen, and forward mouse and keyboard.
EOF
    dpkg-deb --build --root-owner-group "$DEB" \
        "$DIST/x-remote_${VERSION}_${ARCH_DEB}.deb"
    rm -rf "$DEB"
else
    echo "    (no dpkg-deb — skip .deb)"
fi

# ---------- .rpm ----------
if command -v rpmbuild >/dev/null 2>&1; then
    echo "==> .rpm"
    RPMDIR="$DIST/rpmbuild"
    rm -rf "$RPMDIR"
    mkdir -p "$RPMDIR/BUILD" "$RPMDIR/RPMS" "$RPMDIR/SOURCES" "$RPMDIR/SPECS" "$RPMDIR/SRPMS"

    cat > "$RPMDIR/SPECS/x-remote.spec" <<EOF
Name:           x-remote
Version:        $VERSION
Release:        1%{?dist}
Summary:        Native remote-desktop client
License:        MIT
URL:            https://github.com
AutoReqProv:    no
BuildArch:      $ARCH_RPM
Requires:       glibc, libgcc, libX11, libXcursor, libXi, libXrandr, libXext, mesa-libGL
Recommends:     webkit2gtk4.1, python3-gobject

%description
Control a Windows UltraViewer host from Linux. Connect by ID and password,
stream the remote screen, and forward mouse and keyboard.

%prep

%build

%install
mkdir -p %{buildroot}/usr/bin \\
         %{buildroot}/usr/share/applications \\
         %{buildroot}/usr/share/doc/x-remote \\
         %{buildroot}/usr/share/metainfo \\
         %{buildroot}/usr/share/icons/hicolor/32x32/apps \\
         %{buildroot}/usr/share/icons/hicolor/48x48/apps \\
         %{buildroot}/usr/share/icons/hicolor/64x64/apps \\
         %{buildroot}/usr/share/icons/hicolor/128x128/apps \\
         %{buildroot}/usr/share/icons/hicolor/256x256/apps
install -m 755 $BIN_SRC %{buildroot}/usr/bin/x-remote
install -m 644 $PKG/x-remote.desktop %{buildroot}/usr/share/applications/x-remote.desktop
install -m 644 $PKG/net.xremote.app.metainfo.xml %{buildroot}/usr/share/metainfo/net.xremote.app.metainfo.xml
install -m 644 $ROOT/LICENSE %{buildroot}/usr/share/doc/x-remote/LICENSE
install -m 644 $ROOT/README.md %{buildroot}/usr/share/doc/x-remote/README.md
install -m 644 $PKG/icon-32.png %{buildroot}/usr/share/icons/hicolor/32x32/apps/x-remote.png
install -m 644 $PKG/icon-48.png %{buildroot}/usr/share/icons/hicolor/48x48/apps/x-remote.png
install -m 644 $PKG/icon-64.png %{buildroot}/usr/share/icons/hicolor/64x64/apps/x-remote.png
install -m 644 $PKG/x-remote.png %{buildroot}/usr/share/icons/hicolor/128x128/apps/x-remote.png
install -m 644 $PKG/icon.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/x-remote.png

%files
/usr/bin/x-remote
/usr/share/applications/x-remote.desktop
/usr/share/metainfo/net.xremote.app.metainfo.xml
/usr/share/doc/x-remote/LICENSE
/usr/share/doc/x-remote/README.md
/usr/share/icons/hicolor/32x32/apps/x-remote.png
/usr/share/icons/hicolor/48x48/apps/x-remote.png
/usr/share/icons/hicolor/64x64/apps/x-remote.png
/usr/share/icons/hicolor/128x128/apps/x-remote.png
/usr/share/icons/hicolor/256x256/apps/x-remote.png

%changelog
* Sat Nov 07 2026 X-Remote authors
- Initial Linux packages
EOF

    rpmbuild --define "_topdir $RPMDIR" -bb "$RPMDIR/SPECS/x-remote.spec"
    find "$RPMDIR/RPMS" -name '*.rpm' -exec cp {} "$DIST/" \;
    rm -rf "$RPMDIR"
else
    echo "    (no rpmbuild — skip .rpm)"
fi

echo
echo "Packages in $DIST:"
ls -lh "$DIST" | sed -n '1,20p'
echo
echo "Install:"
echo "  all distros   chmod +x dist/X-Remote-x86_64.AppImage && ./dist/X-Remote-x86_64.AppImage"
echo "  Debian/Ubuntu sudo dpkg -i dist/x-remote_${VERSION}_${ARCH_DEB}.deb"
echo "  Fedora/RHEL   sudo dnf install dist/x-remote-${VERSION}-1.*.rpm"
echo "  portable      tar xf dist/x-remote-${VERSION}-linux-${ARCH_RPM}.tar.gz && ./x-remote-*/install.sh"
