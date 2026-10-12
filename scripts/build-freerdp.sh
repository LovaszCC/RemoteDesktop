#!/bin/sh
# Builds a static, self-contained FreeRDP (+ OpenSSL) for the embedded RDP client and packages it as
# Vendor/FreeRDP.xcframework (one merged static library + headers, incl. the native Mac client).
#
#   scripts/build-freerdp.sh          build if Vendor/FreeRDP.xcframework is missing
#   scripts/build-freerdp.sh --force  rebuild from scratch
#
# Requires cmake and ninja (brew install cmake ninja). Sources are pinned and checksum-verified.
set -eu
cd "$(dirname "$0")/.."

FREERDP_VERSION=3.32.1
FREERDP_SHA256=3021cf8848efbd0064664e187cb61e6101e4e74a4d8ffb495d9f7715e22d67de
OPENSSL_VERSION=3.5.9
OPENSSL_SHA256=603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a
MACOS_MIN=15.0
ARCH=arm64

ROOT="$PWD"
WORK="$ROOT/.build-freerdp"
PREFIX="$WORK/prefix"
OUT="$ROOT/Vendor/FreeRDP.xcframework"
PATCHES="$ROOT/patches/freerdp"

if [ -d "$OUT" ] && [ "${1:-}" != "--force" ]; then
    echo "$OUT already exists (use --force to rebuild)"
    exit 0
fi
command -v cmake >/dev/null && command -v ninja >/dev/null || { echo "cmake and ninja are required: brew install cmake ninja" >&2; exit 1; }

rm -rf "$WORK" "$OUT"
mkdir -p "$WORK/src" "$PREFIX"
JOBS="$(sysctl -n hw.ncpu)"
export MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN"

fetch() { # url sha256 file
    curl -fsSL -o "$WORK/src/$3" "$1"
    echo "$2  $WORK/src/$3" | shasum -a 256 -c - >/dev/null || { echo "checksum mismatch: $3" >&2; exit 1; }
    tar -xzf "$WORK/src/$3" -C "$WORK/src"
}

echo "==> OpenSSL $OPENSSL_VERSION"
fetch "https://github.com/openssl/openssl/releases/download/openssl-$OPENSSL_VERSION/openssl-$OPENSSL_VERSION.tar.gz" \
    "$OPENSSL_SHA256" openssl.tar.gz
(
    cd "$WORK/src/openssl-$OPENSSL_VERSION"
    ./Configure darwin64-arm64-cc no-shared no-tests no-docs no-apps no-module \
        --prefix="$PREFIX" --libdir=lib "-mmacosx-version-min=$MACOS_MIN" >/dev/null
    make -j"$JOBS" build_libs >/dev/null
    make install_dev >/dev/null
)

echo "==> FreeRDP $FREERDP_VERSION"
fetch "https://github.com/FreeRDP/FreeRDP/releases/download/$FREERDP_VERSION/freerdp-$FREERDP_VERSION.tar.gz" \
    "$FREERDP_SHA256" freerdp.tar.gz
SRC="$WORK/src/freerdp-$FREERDP_VERSION"
if [ -d "$PATCHES" ]; then
    for patch in "$PATCHES"/*.patch; do
        [ -f "$patch" ] || continue
        echo "    applying $(basename "$patch")"
        patch -s -p1 -d "$SRC" < "$patch"
    done
fi

cmake -S "$SRC" -B "$WORK/build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_PREFIX_PATH="$PREFIX" \
    -DCMAKE_OSX_ARCHITECTURES="$ARCH" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN" \
    -DOPENSSL_ROOT_DIR="$PREFIX" \
    -DOPENSSL_USE_STATIC_LIBS=ON \
    -DBUILD_SHARED_LIBS=OFF \
    -DBUILD_TESTING=OFF \
    -DCMAKE_INTERPROCEDURAL_OPTIMIZATION=OFF \
    -DWITH_CLIENT_MAC=ON \
    -DWITH_CLIENT=ON \
    -DWITH_CLIENT_COMMON=ON \
    -DWITH_CLIENT_CHANNELS=ON \
    -DWITH_CLIENT_SDL=OFF \
    -DWITH_CLIENT_SDL2=OFF \
    -DWITH_CLIENT_SDL3=OFF \
    -DWITH_X11=OFF \
    -DWITH_WAYLAND=OFF \
    -DWITH_SERVER=OFF \
    -DWITH_SERVER_CHANNELS=OFF \
    -DWITH_SAMPLE=OFF \
    -DWITH_PROXY=OFF \
    -DWITH_SHADOW=OFF \
    -DWITH_PLATFORM_SERVER=OFF \
    -DWITH_WINPR_TOOLS=OFF \
    -DWITH_MANPAGES=OFF \
    -DWITH_FFMPEG=OFF \
    -DWITH_DSP_FFMPEG=OFF \
    -DWITH_SWSCALE=OFF \
    -DWITH_CAIRO=OFF \
    -DWITH_JPEG=OFF \
    -DWITH_KRB5=OFF \
    -DWITH_AAD=OFF \
    -DWITH_CUPS=OFF \
    -DWITH_PCSC=OFF \
    -DWITH_FUSE=OFF \
    -DWITH_PULSE=OFF \
    -DWITH_ALSA=OFF \
    -DWITH_OSS=OFF \
    -DWITH_OPUS=OFF \
    -DWITH_FAAD2=OFF \
    -DWITH_FAAC=OFF \
    -DWITH_SOXR=OFF \
    -DWITH_LAME=OFF \
    -DWITH_URIPARSER=OFF \
    -DWITH_WEBVIEW=OFF \
    -DWITH_SSO_MIB=OFF \
    -DWITH_LIBSYSTEMD=OFF \
    -DWITH_JSON_DISABLED=ON \
    -DCHANNEL_URBDRC=OFF \
    -DUSE_UNWIND=OFF

cmake --build "$WORK/build" -j "$JOBS"
cmake --install "$WORK/build" >/dev/null

echo "==> Packaging $OUT"
STAGE="$WORK/stage"
mkdir -p "$STAGE/Headers/MacFreeRDP"
# Installed static libraries plus build-tree-only ones (Mac client, channel helpers), each once;
# CMake's own probe projects are skipped.
LIBS="$(ls "$PREFIX"/lib/*.a)"
for lib in $(find "$WORK/build" -name '*.a' -not -path '*/CMakeFiles/*'); do
    [ -f "$PREFIX/lib/$(basename "$lib")" ] || LIBS="$LIBS $lib"
done
# shellcheck disable=SC2086
libtool -static -no_warning_for_no_symbols -o "$STAGE/libFreeRDP.a" $LIBS

cp -R "$PREFIX/include/freerdp3/freerdp" "$STAGE/Headers/"
cp -R "$PREFIX/include/winpr3/winpr" "$STAGE/Headers/"
cp -R "$PREFIX/include/openssl" "$STAGE/Headers/" 2>/dev/null || true
for header in "$SRC"/client/Mac/*.h; do cp "$header" "$STAGE/Headers/MacFreeRDP/"; done
# Generated headers (e.g. version/build config) that live in the build tree.
find "$WORK/build" -path '*/include/freerdp/*.h' -newer "$SRC/CMakeLists.txt" -exec sh -c '
    for f; do rel="${f#*/include/}"; mkdir -p "$0/$(dirname "$rel")"; cp -n "$f" "$0/$rel" 2>/dev/null || true; done
' "$STAGE/Headers" {} +

# No module map: winpr's Windows types clash with CoreFoundation inside a Clang module, so
# RDPBridge includes the headers textually instead.
xcodebuild -create-xcframework -library "$STAGE/libFreeRDP.a" -headers "$STAGE/Headers" -output "$OUT" >/dev/null
echo "Built $OUT (FreeRDP $FREERDP_VERSION, OpenSSL $OPENSSL_VERSION)"
