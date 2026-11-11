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
# Keep build-machine paths (and user names) out of the binaries.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$ROOT=/build/x-remote"

VERSION="$(awk -F\" '/^version/{print $2; exit}' Cargo.toml)"
[ -n "$VERSION" ] || VERSION="0.1.0"
ARCH_DEB="amd64"
ARCH_RPM="x86_64"
DIST="$ROOT/dist"
PKG="$ROOT/packaging"
BIN_SRC="$ROOT/target/release/uvclient"

# Build against an old glibc so one binary runs on every distro. A binary built on
# the host links the host glibc (Fedora 44 -> GLIBC_2.43: acosf/atan2f), and then
# fails at start on Ubuntu 22.04/24.04, Debian 12, Mint, RHEL 9, openSUSE Leap.
# Debian bullseye = glibc 2.31. LOCAL_BUILD=1 builds on the host instead.
BUILD_IMAGE="${BUILD_IMAGE:-docker.io/library/rust:1.88-bullseye}"
ENGINE=""
if [ -z "${LOCAL_BUILD:-}" ]; then
    for e in podman docker; do
        command -v "$e" >/dev/null 2>&1 && { ENGINE="$e"; break; }
    done
fi
if [ -n "$ENGINE" ]; then
    echo "==> cargo build --release -p uvclient (in $BUILD_IMAGE via $ENGINE)"
    BIN_SRC="$ROOT/target/compat/release/uvclient"
    mkdir -p "$ROOT/target/compat-cargo"
    USER_ARGS=""
    [ "$ENGINE" = docker ] && USER_ARGS="-u $(id -u):$(id -g)"
    # shellcheck disable=SC2086
    "$ENGINE" run --rm $USER_ARGS \
        -v "$ROOT":/build/x-remote:z \
        -v "$ROOT/target/compat-cargo":/cargo-home:z \
        -w /build/x-remote \
        -e CARGO_HOME=/cargo-home \
        -e CARGO_TARGET_DIR=/build/x-remote/target/compat \
        -e RUSTFLAGS="--remap-path-prefix=/cargo-home=/cargo" \
        "$BUILD_IMAGE" cargo build --release --locked -p uvclient
else
    echo "==> cargo build --release -p uvclient (host toolchain)"
    echo "    WARNING: binary needs this machine's glibc or newer; use podman/docker for a portable build" >&2
    cargo build --release -p uvclient
fi
[ -x "$BIN_SRC" ] || { echo "missing $BIN_SRC" >&2; exit 1; }

# Lowest glibc the binary accepts (goes into the .deb/.rpm dependencies).
GLIBC_MIN="2.31"
if command -v objdump >/dev/null 2>&1; then
    g="$(objdump -T "$BIN_SRC" | grep -v ' w ' | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/GLIBC_//' | sort -Vu | tail -1)"
    [ -n "$g" ] && GLIBC_MIN="$g"
fi
echo "    needs glibc >= $GLIBC_MIN"

# The screen-frame dictionary is third-party (see .gitignore / README): ship it only on request.
DICT=""
if [ -n "${BUNDLE_DICT:-}" ] && [ -f "$PKG/8bit.dat" ]; then
    DICT="$PKG/8bit.dat"
fi

# Clear only this script's outputs (keep e.g. dist/X-Remote.flatpak from build-flatpak.sh).
mkdir -p "$DIST"
rm -rf "$DIST/x-remote" "$DIST"/x-remote[-_]* "$DIST/X-Remote.AppDir" "$DIST/X-Remote-x86_64.AppImage" "$DIST/deb" "$DIST/rpmbuild"
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
[ -n "$DICT" ] && install -m 644 "$DICT" "$TREE/8bit.dat"
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
[ -f "$HERE/8bit.dat" ] && install -m 644 "$HERE/8bit.dat" "$DEST/8bit.dat"
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
         "$APPDIR/usr/share/x-remote" \
         "$APPDIR/usr/share/icons/hicolor/32x32/apps" \
         "$APPDIR/usr/share/icons/hicolor/48x48/apps" \
         "$APPDIR/usr/share/icons/hicolor/64x64/apps" \
         "$APPDIR/usr/share/icons/hicolor/128x128/apps" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"
install -m 755 "$BIN_SRC" "$APPDIR/usr/bin/x-remote"
install -m 644 "$PKG/x-remote.desktop" "$APPDIR/x-remote.desktop"
install -m 644 "$PKG/x-remote.desktop" "$APPDIR/usr/share/applications/x-remote.desktop"
install -m 644 "$PKG/net.xremote.app.metainfo.xml" "$APPDIR/usr/share/metainfo/net.xremote.app.metainfo.xml"
[ -n "$DICT" ] && install -m 644 "$DICT" "$APPDIR/usr/share/x-remote/8bit.dat"
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
# Pinned build tool: update this hash only after reviewing a new appimagetool.
TOOL_SHA256="e8c1169c4af66c3184d7ee110713d4be04befe671fa358e2c1591ee840553b03"
if [ ! -x "$TOOL" ]; then
    echo "    downloading appimagetool…"
    curl -fsSL -o "$TOOL" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$TOOL"
fi
if ! printf '%s  %s\n' "$TOOL_SHA256" "$TOOL" | sha256sum -c --status 2>/dev/null; then
    echo "appimagetool checksum mismatch — refusing to run it" >&2
    rm -f "$TOOL"
    exit 1
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
             "$DEB/usr/share/x-remote" \
             "$DEB/usr/share/icons/hicolor/32x32/apps" \
             "$DEB/usr/share/icons/hicolor/48x48/apps" \
             "$DEB/usr/share/icons/hicolor/64x64/apps" \
             "$DEB/usr/share/icons/hicolor/128x128/apps" \
             "$DEB/usr/share/icons/hicolor/256x256/apps"
    install -m 755 "$BIN_SRC" "$DEB/usr/bin/x-remote"
    install -m 644 "$PKG/x-remote.desktop" "$DEB/usr/share/applications/x-remote.desktop"
    install -m 644 "$PKG/net.xremote.app.metainfo.xml" "$DEB/usr/share/metainfo/net.xremote.app.metainfo.xml"
    [ -n "$DICT" ] && install -m 644 "$DICT" "$DEB/usr/share/x-remote/8bit.dat"
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
Homepage: https://github.com/nct89/X-Remote
Depends: libc6 (>= $GLIBC_MIN), libgcc-s1, libx11-6, libx11-xcb1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libxcursor1, libxi6, libxrandr2, libxext6, libegl1 | libgl1, ca-certificates
Recommends: gir1.2-webkit2-4.1, python3-gi
Description: Native remote-desktop client
 Control a Windows UltraViewer host from Linux. Connect by ID and password,
 stream the remote screen, and forward mouse and keyboard.
EOF
    dpkg-deb -Zxz --build --root-owner-group "$DEB" \
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

    RPM_DICT_INSTALL=""
    RPM_DICT_FILE=""
    if [ -n "$DICT" ]; then
        RPM_DICT_INSTALL="install -m 644 $DICT %{buildroot}/usr/share/x-remote/8bit.dat"
        RPM_DICT_FILE="/usr/share/x-remote/8bit.dat"
    fi
    cat > "$RPMDIR/SPECS/x-remote.spec" <<EOF
Name:           x-remote
Version:        $VERSION
Release:        1
Summary:        Native remote-desktop client
License:        MIT
URL:            https://github.com/nct89/X-Remote
AutoReqProv:    no
BuildArch:      $ARCH_RPM
Requires:       libc.so.6(GLIBC_$GLIBC_MIN)(64bit), libgcc_s.so.1()(64bit)
Requires:       libX11.so.6()(64bit), libX11-xcb.so.1()(64bit), libxcb.so.1()(64bit)
Requires:       libxkbcommon.so.0()(64bit), libxkbcommon-x11.so.0()(64bit)
Requires:       libXcursor.so.1()(64bit), libXi.so.6()(64bit), libXrandr.so.2()(64bit), libXext.so.6()(64bit)
Requires:       libEGL.so.1()(64bit)
Recommends:     (webkit2gtk4.1 or libwebkit2gtk-4_1-0), (python3-gobject or python3-gobject-Gdk)

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
         %{buildroot}/usr/share/x-remote \\
         %{buildroot}/usr/share/icons/hicolor/32x32/apps \\
         %{buildroot}/usr/share/icons/hicolor/48x48/apps \\
         %{buildroot}/usr/share/icons/hicolor/64x64/apps \\
         %{buildroot}/usr/share/icons/hicolor/128x128/apps \\
         %{buildroot}/usr/share/icons/hicolor/256x256/apps
install -m 755 $BIN_SRC %{buildroot}/usr/bin/x-remote
install -m 644 $PKG/x-remote.desktop %{buildroot}/usr/share/applications/x-remote.desktop
install -m 644 $PKG/net.xremote.app.metainfo.xml %{buildroot}/usr/share/metainfo/net.xremote.app.metainfo.xml
$RPM_DICT_INSTALL
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
$RPM_DICT_FILE
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
