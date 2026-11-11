#!/bin/sh
# Build X-Remote as a Flatpak bundle:
#   dist/X-Remote.flatpak        install with: flatpak install --user dist/X-Remote.flatpak
# Needs flatpak and flatpak-builder (or the org.flatpak.Builder flatpak). Runtimes come
# from Flathub (--install-deps-from). BUNDLE_DICT=1 ships packaging/8bit.dat too.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FP="$ROOT/packaging/flatpak"
STAGE="$FP/build"
DIST="$ROOT/dist"
APP_ID="net.xremote.app"
cd "$ROOT"

if command -v flatpak-builder >/dev/null 2>&1; then
    BUILDER="flatpak-builder"
elif flatpak info org.flatpak.Builder >/dev/null 2>&1; then
    BUILDER="flatpak run --filesystem=$ROOT org.flatpak.Builder"
else
    echo "Need flatpak-builder: 'sudo dnf install flatpak-builder' / 'sudo apt install flatpak-builder'" >&2
    echo "or: flatpak install --user flathub org.flatpak.Builder" >&2
    exit 1
fi

echo "==> vendoring crates and packing sources"
rm -rf "$STAGE"
mkdir -p "$STAGE/src"
tar -C "$ROOT" --exclude=./target --exclude=./dist --exclude=./.git \
    --exclude=./packaging/flatpak/build --exclude=./packaging/flatpak/.flatpak-builder \
    --exclude=./packaging/8bit.dat -cf - . | tar -C "$STAGE/src" -xf -
if [ -n "${BUNDLE_DICT:-}" ] && [ -f "$ROOT/packaging/8bit.dat" ]; then
    cp "$ROOT/packaging/8bit.dat" "$STAGE/src/packaging/8bit.dat.bundle"
fi
mkdir -p "$STAGE/src/.cargo"
( cd "$STAGE/src" && cargo vendor --locked --versioned-dirs vendor > .cargo/config.toml )
tar -C "$STAGE/src" -czf "$STAGE/x-remote-src.tar.gz" .
rm -rf "$STAGE/src"

echo "==> flatpak-builder"
REPO="$STAGE/repo"
cd "$FP"
# shellcheck disable=SC2086
$BUILDER --user --install-deps-from=flathub --force-clean --repo="$REPO" \
    --state-dir="$FP/.flatpak-builder" "$STAGE/app" "$FP/$APP_ID.yml"

mkdir -p "$DIST"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    "$REPO" "$DIST/X-Remote.flatpak" "$APP_ID"
echo
echo "Bundle: $DIST/X-Remote.flatpak"
echo "Install: flatpak install --user $DIST/X-Remote.flatpak && flatpak run $APP_ID"
