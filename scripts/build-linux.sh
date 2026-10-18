#!/usr/bin/env bash
# Linux release build: AppImage, .deb and .rpm (x86_64) in dist/release/<version>/.
# Run it on an x86_64 Linux machine; the machine's glibc is the bundles' minimum, so Ubuntu 22.04
# (glibc 2.35) is the reference. Needs rustup, Node 26, pnpm 12 and the packages below.
#
#   scripts/build-linux.sh                 AppImage + deb + rpm
#   scripts/build-linux.sh --no-updater    without updater signatures even if the key is found
#   scripts/build-linux.sh --allow-dirty   build with uncommitted changes (test builds only)
#
#   → RemoteDesktop_<version>_linux_amd64.AppImage, …_linux_amd64.deb, …_linux_x86_64.rpm
#     (+ .sig for the updater when the key is found)
set -euo pipefail
[ "$(uname -s)" = Linux ] || { echo "error: run this on Linux" >&2; exit 1; }
[ "$(uname -m)" = x86_64 ] || { echo "error: the Linux release is x86_64; this machine is $(uname -m)" >&2; exit 1; }

# Debian/Ubuntu package names; on other distros install the equivalents (webkit2gtk4.1-devel, …).
packages=(libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  build-essential file cmake ninja-build perl curl)
if command -v dpkg >/dev/null; then
  missing=()
  for p in "${packages[@]}"; do dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p"); done
  if [ "${#missing[@]}" -gt 0 ]; then
    echo "error: missing packages; install them with:" >&2
    echo "  sudo apt-get install -y ${missing[*]}" >&2
    exit 1
  fi
fi

# shellcheck source=build-common.sh
. "$(dirname "$0")/build-common.sh"

# linuxdeploy's bundled strip can't read newer ELF sections; the release profile strips already.
export NO_STRIP=true
# linuxdeploy is itself an AppImage: extracting it instead of mounting means no FUSE is needed to build.
export APPIMAGE_EXTRACT_AND_RUN=1
build_target x86_64-unknown-linux-gnu --bundles appimage,deb,rpm

step "Done"
ls -lh "$out"/*_linux_*
