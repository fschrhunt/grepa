#!/bin/sh
# Exercise the installer offline with mocked network and platform commands.
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/grepa-install-test.XXXXXX")
trap 'rm -rf -- "${TMP:?}"' EXIT HUP INT TERM
MOCK_BIN="$TMP/mock-bin"
ASSETS="$TMP/assets"
LATEST_URL="https://api.github.com/repos/fschrhunt/grepa/releases/latest"
mkdir -p "$MOCK_BIN" "$ASSETS"

cat > "$MOCK_BIN/uname" <<'EOF'
#!/bin/sh
case "$1" in
    -s) printf '%s\n' "${MOCK_OS:-Darwin}" ;;
    -m) printf '%s\n' "${MOCK_ARCH:-x86_64}" ;;
    *) exit 1 ;;
esac
EOF
cat > "$MOCK_BIN/curl" <<'EOF'
#!/bin/sh
set -eu
out=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --proto) shift 2 ;;
        --tlsv1.2|-fsSL) shift ;;
        -o) out=$2; shift 2 ;;
        *) url=$1; shift ;;
    esac
done
[ -n "$out" ] || exit 1
case "$url" in
    "$EXPECT_LATEST_URL") printf '{"tag_name":"v%s"}\n' "$MOCK_LATEST_VERSION" > "$out" ;;
    "$EXPECT_CHECKSUM_URL") cp "$INSTALL_TEST_ASSETS/SHA256SUMS" "$out" ;;
    "$EXPECT_ARCHIVE_URL") cp "$INSTALL_TEST_ASSETS/$EXPECTED_ARCHIVE" "$out" ;;
    *)
        printf 'unexpected curl url: %s\n' "$url" >&2
        exit 1
        ;;
esac
EOF
chmod 755 "$MOCK_BIN/uname" "$MOCK_BIN/curl"

archive_checksum() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

reset_install_state() {
    rm -rf -- "${TMP:?}/bin" "${TMP:?}/home"
}

make_assets() {
    version=$1
    target=$2
    archive="grepa-$version-$target.tar.gz"
    prefix="grepa-$version-$target"

    rm -rf -- "${ASSETS:?}" "${TMP:?}/package"
    mkdir -p "$ASSETS" "$TMP/package/$prefix"
    printf '#!/bin/sh\necho grepa\n' > "$TMP/package/$prefix/grepa"
    chmod 755 "$TMP/package/$prefix/grepa"
    tar -czf "$ASSETS/$archive" -C "$TMP/package" "$prefix"
    sum=$(archive_checksum "$ASSETS/$archive")
    printf '%s  %s\n' "$sum" "$archive" > "$ASSETS/SHA256SUMS"
}

run_install() {
    os=$1
    arch=$2
    version=$3
    target=$4
    archive="grepa-$version-$target.tar.gz"

    HOME="$TMP/home" \
    GREPA_VERSION="$version" \
    GREPA_BIN_DIR="$TMP/bin" \
    INSTALL_TEST_ASSETS="$ASSETS" \
    EXPECT_LATEST_URL="$LATEST_URL" \
    EXPECT_CHECKSUM_URL="https://github.com/fschrhunt/grepa/releases/download/v$version/SHA256SUMS" \
    EXPECT_ARCHIVE_URL="https://github.com/fschrhunt/grepa/releases/download/v$version/$archive" \
    EXPECTED_ARCHIVE="$archive" \
    MOCK_LATEST_VERSION="$version" \
    MOCK_OS="$os" \
    MOCK_ARCH="$arch" \
    PATH="$MOCK_BIN:$PATH" \
    sh "$ROOT/scripts/install.sh"
}

run_latest_install() {
    os=$1
    arch=$2
    version=$3
    target=$4
    archive="grepa-$version-$target.tar.gz"

    HOME="$TMP/home" \
    GREPA_BIN_DIR="$TMP/bin" \
    INSTALL_TEST_ASSETS="$ASSETS" \
    EXPECT_LATEST_URL="$LATEST_URL" \
    EXPECT_CHECKSUM_URL="https://github.com/fschrhunt/grepa/releases/download/v$version/SHA256SUMS" \
    EXPECT_ARCHIVE_URL="https://github.com/fschrhunt/grepa/releases/download/v$version/$archive" \
    EXPECTED_ARCHIVE="$archive" \
    MOCK_LATEST_VERSION="$version" \
    MOCK_OS="$os" \
    MOCK_ARCH="$arch" \
    PATH="$MOCK_BIN:$PATH" \
    sh "$ROOT/scripts/install.sh"
}

assert_installed() {
    [ -x "$TMP/bin/grepa" ] || {
        echo "installer did not install binary for $1" >&2
        exit 1
    }
}

exercise_mapping() {
    os=$1
    arch=$2
    target=$3

    reset_install_state
    make_assets 0.2.0 "$target"
    run_install "$os" "$arch" 0.2.0 "$target"
    assert_installed "$target"
}

exercise_mapping Darwin x86_64 x86_64-apple-darwin
exercise_mapping Darwin arm64 aarch64-apple-darwin
exercise_mapping Linux x86_64 x86_64-unknown-linux-gnu
exercise_mapping Linux aarch64 aarch64-unknown-linux-gnu

reset_install_state
make_assets 0.2.0 x86_64-apple-darwin
run_latest_install Darwin x86_64 0.2.0 x86_64-apple-darwin
assert_installed latest

reset_install_state
make_assets 0.2.0 x86_64-apple-darwin
printf '%064d  grepa-0.2.0-x86_64-apple-darwin.tar.gz\n' 0 > "$ASSETS/SHA256SUMS"
if run_install Darwin x86_64 0.2.0 x86_64-apple-darwin; then
    echo "installer accepted an invalid checksum" >&2
    exit 1
fi

reset_install_state
if run_install FreeBSD x86_64 0.2.0 x86_64-apple-darwin; then
    echo "installer accepted an unsupported platform" >&2
    exit 1
fi

reset_install_state
make_assets 0.2.0 x86_64-apple-darwin
mkdir -p "$TMP/bin"
ln -s "$TMP/not-a-binary" "$TMP/bin/grepa"
if run_install Darwin x86_64 0.2.0 x86_64-apple-darwin; then
    echo "installer replaced a symlink" >&2
    exit 1
fi
[ -L "$TMP/bin/grepa" ] || { echo "installer changed symlink" >&2; exit 1; }

printf '%s\n' 'installer tests passed'
