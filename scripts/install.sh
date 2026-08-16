#!/bin/sh
# Install a checksum-verified grepa release without a package manager.
set -eu

REPOSITORY="fschrhunt/grepa"
VERSION="${GREPA_VERSION:-}"
BIN_DIR="${GREPA_BIN_DIR:-$HOME/.local/bin}"
API="https://api.github.com/repos/$REPOSITORY/releases/latest"

fail() { printf '%s\n' "error: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "$1 is required"; }

need curl
need tar
need awk
need grep
need mktemp
need sed

TMP=$(mktemp -d "${TMPDIR:-/tmp}/grepa-install.XXXXXX") || fail "could not create temporary directory"
STAGED=""
cleanup() {
    [ -z "$STAGED" ] || rm -f -- "$STAGED"
    rm -rf -- "${TMP:?}"
}
trap cleanup EXIT HUP INT TERM

if [ -z "$VERSION" ]; then
    curl --proto '=https' --tlsv1.2 -fsSL "$API" -o "$TMP/latest.json" || fail "could not determine the latest grepa release"
    VERSION=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([^"]*\)".*/\1/p' "$TMP/latest.json" | head -n 1)
fi
printf '%s\n' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' || fail "version must be semantic versioning without a v prefix"

OS=$(uname -s)
ARCH=$(uname -m)
case "$OS/$ARCH" in
    Darwin/x86_64|Darwin/amd64) TARGET="x86_64-apple-darwin" ;;
    Darwin/arm64|Darwin/aarch64) TARGET="aarch64-apple-darwin" ;;
    Linux/x86_64|Linux/amd64) TARGET="x86_64-unknown-linux-gnu" ;;
    Linux/arm64|Linux/aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
    *) fail "unsupported platform: $OS/$ARCH (supported: macOS/Linux x86_64 or arm64)" ;;
esac

BASE="https://github.com/$REPOSITORY/releases/download/v$VERSION"
ARCHIVE="grepa-$VERSION-$TARGET.tar.gz"
PREFIX="grepa-$VERSION-$TARGET"
ENTRY="$PREFIX/grepa"

printf 'Downloading grepa %s for %s...\n' "$VERSION" "$TARGET"
curl --proto '=https' --tlsv1.2 -fsSL "$BASE/$ARCHIVE" -o "$TMP/$ARCHIVE" || fail "could not download $ARCHIVE"
curl --proto '=https' --tlsv1.2 -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS" || fail "could not download release checksums"
EXPECTED=$(awk -v file="$ARCHIVE" '$2 == file { print $1; exit }' "$TMP/SHA256SUMS")
printf '%s\n' "$EXPECTED" | grep -Eq '^[0-9A-Fa-f]{64}$' || fail "release checksums do not list a valid checksum for $ARCHIVE"
if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL=$(sha256sum "$TMP/$ARCHIVE" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
    ACTUAL=$(shasum -a 256 "$TMP/$ARCHIVE" | awk '{print $1}')
else
    fail "sha256sum or shasum is required"
fi
[ "$ACTUAL" = "$EXPECTED" ] || fail "download checksum did not match"

tar -tzf "$TMP/$ARCHIVE" > "$TMP/archive-entries" || fail "could not inspect release archive"
[ "$(awk -v entry="$ENTRY" '$0 == entry { count++ } END { print count + 0 }' "$TMP/archive-entries")" -eq 1 ] || fail "release archive has an unexpected layout"
tar -xzf "$TMP/$ARCHIVE" -C "$TMP" "$ENTRY" || fail "could not extract release binary"
SOURCE="$TMP/$ENTRY"
[ -f "$SOURCE" ] && [ ! -L "$SOURCE" ] || fail "release archive binary is not a regular file"

mkdir -p "$BIN_DIR" || fail "could not create $BIN_DIR"
TARGET_PATH="$BIN_DIR/grepa"
if [ -e "$TARGET_PATH" ] || [ -L "$TARGET_PATH" ]; then
    [ -f "$TARGET_PATH" ] && [ ! -L "$TARGET_PATH" ] || fail "refusing to replace symlink or non-regular target: $TARGET_PATH"
fi
STAGED=$(mktemp "$BIN_DIR/.grepa.XXXXXX") || fail "could not stage binary in $BIN_DIR"
cp "$SOURCE" "$STAGED" || fail "could not stage binary"
chmod 755 "$STAGED" || fail "could not make binary executable"
mv -f "$STAGED" "$TARGET_PATH" || fail "could not install binary"
STAGED=""

printf 'Installed grepa %s to %s\n' "$VERSION" "$TARGET_PATH"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) printf 'Add %s to your PATH.\n' "$BIN_DIR" ;;
esac
