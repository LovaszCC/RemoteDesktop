#!/usr/bin/env bash
# Builds a static FreeRDP (+ OpenSSL) for the embedded RDP engine (crates/rd-freerdp-sys), macOS and Linux.
# Windows uses scripts/build-freerdp.ps1. crates/rd-freerdp-sys/build.rs runs this when the build is
# missing, so a fresh clone needs no manual step; run it by hand to see the output or to rebuild.
#
#   scripts/build-freerdp.sh <target triple> [--force]
#
#   → third_party/freerdp/build/<freerdp version>/<target triple>/{include,lib}
#
# Needs cmake, ninja, perl and a C compiler (macOS: Xcode Command Line Tools + `brew install cmake ninja`;
# Debian/Ubuntu: `sudo apt-get install -y cmake ninja-build build-essential perl`). Sources are pinned
# and checksum-verified.
set -euo pipefail
cd "$(dirname "$0")/.."

FREERDP_VERSION=3.32.2
FREERDP_SHA256=e95a505c0057e4d06a3b120477c421028769deba4db9c639ef3465d050f0955c
OPENSSL_VERSION=3.5.9
OPENSSL_SHA256=603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a
# The app's minimum macOS (Rust's default for aarch64-apple-darwin; tauri.macos.conf.json matches).
MACOS_MIN=11.0

target=${1:?usage: build-freerdp.sh <target triple> [--force]}
force=${2:-}
root=$PWD
out="$root/third_party/freerdp/build/$FREERDP_VERSION/$target"
work="$root/third_party/freerdp/build/work-$target"

if [ -f "$out/.complete" ] && [ "$force" != --force ]; then
  echo "$out already built (--force rebuilds)"
  exit 0
fi

case "$target" in
  aarch64-apple-darwin) openssl_target=darwin64-arm64-cc cmake_arch=(-DCMAKE_OSX_ARCHITECTURES=arm64) ;;
  x86_64-apple-darwin) openssl_target=darwin64-x86_64-cc cmake_arch=(-DCMAKE_OSX_ARCHITECTURES=x86_64) ;;
  x86_64-unknown-linux-gnu) openssl_target=linux-x86_64 cmake_arch=() ;;
  aarch64-unknown-linux-gnu) openssl_target=linux-aarch64 cmake_arch=() ;;
  *) echo "error: unsupported target $target (Windows: scripts/build-freerdp.ps1)" >&2; exit 1 ;;
esac
for tool in cmake ninja perl curl; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found (see the header of $0)" >&2; exit 1; }
done

if command -v sysctl >/dev/null && sysctl -n hw.ncpu >/dev/null 2>&1; then jobs=$(sysctl -n hw.ncpu); else jobs=$(nproc); fi
if [[ $target == *-apple-darwin ]]; then
  export MACOSX_DEPLOYMENT_TARGET=$MACOS_MIN
  macos_flags=("-mmacosx-version-min=$MACOS_MIN")
else
  macos_flags=()
fi

rm -rf "$work" "$out"
mkdir -p "$work/src" "$out"
prefix="$work/prefix"

fetch() { # url sha256 file
  curl -fsSL --retry 3 -o "$work/src/$3" "$1"
  local sum
  sum=$( (command -v sha256sum >/dev/null && sha256sum "$work/src/$3") || shasum -a 256 "$work/src/$3")
  [ "${sum%% *}" = "$2" ] || { echo "error: checksum mismatch for $3" >&2; exit 1; }
  tar -xzf "$work/src/$3" -C "$work/src"
}

echo "==> OpenSSL $OPENSSL_VERSION ($target)"
fetch "https://github.com/openssl/openssl/releases/download/openssl-$OPENSSL_VERSION/openssl-$OPENSSL_VERSION.tar.gz" \
  "$OPENSSL_SHA256" openssl.tar.gz
(
  cd "$work/src/openssl-$OPENSSL_VERSION"
  # no-module: providers are built into libcrypto (nothing to load at run time); as in the Swift app.
  ./Configure "$openssl_target" no-shared no-tests no-docs no-apps no-module \
    --prefix="$prefix" --libdir=lib ${macos_flags[@]+"${macos_flags[@]}"} >/dev/null
  make -j"$jobs" build_libs >/dev/null
  make install_dev >/dev/null
)

echo "==> FreeRDP $FREERDP_VERSION ($target)"
fetch "https://github.com/FreeRDP/FreeRDP/releases/download/$FREERDP_VERSION/freerdp-$FREERDP_VERSION.tar.gz" \
  "$FREERDP_SHA256" freerdp.tar.gz
src="$work/src/freerdp-$FREERDP_VERSION"

# USE_VERSION_FROM_GIT_TAG=OFF: the sources sit inside this checkout; `git describe` would take our tag.
options=()
while read -r line; do
  case "$line" in -D*) options+=("$line") ;; esac
done <scripts/freerdp-cmake-options.txt
cmake -S "$src" -B "$work/build" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
  -DUSE_VERSION_FROM_GIT_TAG=OFF \
  -DCMAKE_INSTALL_PREFIX="$prefix" \
  -DCMAKE_PREFIX_PATH="$prefix" \
  ${cmake_arch[@]+"${cmake_arch[@]}"} \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN" \
  -DOPENSSL_ROOT_DIR="$prefix" \
  -DOPENSSL_USE_STATIC_LIBS=ON \
  "${options[@]}"
cmake --build "$work/build" -j "$jobs"
cmake --install "$work/build" >/dev/null

mkdir -p "$out/lib" "$out/include"
cp "$prefix"/lib/*.a "$out/lib/"
# Build-tree-only static libraries (client channel helpers), each once.
find "$work/build" -name '*.a' -not -path '*/CMakeFiles/*' | while read -r lib; do
  [ -f "$out/lib/$(basename "$lib")" ] || cp "$lib" "$out/lib/"
done
cp -R "$prefix/include/freerdp3" "$prefix/include/winpr3" "$prefix/include/openssl" "$out/include/"
echo "$FREERDP_VERSION" >"$out/VERSION"
cp scripts/freerdp-cmake-options.txt "$out/OPTIONS"
touch "$out/.complete"
rm -rf "$work"
echo "Built $out"
ls "$out/lib"
