#!/usr/bin/env bash
# Copies the release bundles of one target out of target/<triple>/release/bundle into <out dir> under
# space-free release asset names, with their updater signatures (.sig) when they were made:
#
#   RemoteDesktop_<version>_macos_<arch>.dmg(.sha256, for the Swift updater) / .app.tar.gz(.sig)
#   RemoteDesktop_<version>_windows_x64-setup.exe(.sig)
#   RemoteDesktop_<version>_linux_amd64.AppImage(.sig) / .deb(.sig) / _linux_x86_64.rpm(.sig)
#
#   UPDATER=true|false [NO_RPM=true] scripts/collect-bundles.sh <target triple> <version> <out dir>
#
# NO_RPM=true (beta builds): no .rpm is expected; rpm versions can't hold semver's "-beta.N".
#
# With UPDATER=true (the build made signed updater artifacts) every updatable bundle must have its .sig and
# macOS must have its .app.tar.gz; with false those are skipped. Fails when an expected bundle is missing.
# scripts/make-latest-json.mjs reads these names.
set -euo pipefail

target=${1:?usage: collect-bundles.sh <target triple> <version> <out dir>}
version=${2:?missing version}
out=${3:?missing out dir}
updater=${UPDATER:-false}
bundle="target/$target/release/bundle"
name="RemoteDesktop_${version}"
mkdir -p "$out"

# copy <dir> <pattern> <new name> [updatable]: exactly one match; an updatable bundle's .sig is required
# with UPDATER=true.
copy() {
  local dir=$1 pattern=$2 dest=$3 updatable=${4:-} matches=()
  shopt -s nullglob
  # shellcheck disable=SC2206 # the pattern is meant to glob
  matches=("$bundle/$dir/"$pattern)
  shopt -u nullglob
  if [ "${#matches[@]}" -ne 1 ]; then
    echo "::error::expected one $bundle/$dir/$pattern, found ${#matches[@]}" >&2
    ls -la "$bundle/$dir" >&2 || true
    exit 1
  fi
  cp "${matches[0]}" "$out/$dest"
  echo "$out/$dest"
  if [ -n "$updatable" ] && [ "$updater" = true ]; then
    if [ ! -f "${matches[0]}.sig" ]; then
      echo "::error::${matches[0]} has no updater signature (.sig)" >&2
      exit 1
    fi
    cp "${matches[0]}.sig" "$out/$dest.sig"
    echo "$out/$dest.sig"
  fi
}

case "$target" in
  aarch64-apple-darwin | x86_64-apple-darwin)
    arch=${target%%-*}
    copy dmg "*.dmg" "${name}_macos_${arch}.dmg"
    # The Swift app's updater (1.6.4 and older) takes the release's first `.dmg` plus its `.dmg.sha256`, a
    # `shasum -a 256` line (`<hex>  <name>`); without it, Swift installs can't download the handover release.
    (cd "$out" && shasum -a 256 "${name}_macos_${arch}.dmg" > "${name}_macos_${arch}.dmg.sha256")
    echo "$out/${name}_macos_${arch}.dmg.sha256"
    if [ "$updater" = true ]; then
      copy macos "*.app.tar.gz" "${name}_macos_${arch}.app.tar.gz" updatable
    fi
    ;;
  x86_64-pc-windows-msvc)
    copy nsis "*-setup.exe" "${name}_windows_x64-setup.exe" updatable
    ;;
  x86_64-unknown-linux-gnu)
    copy appimage "*.AppImage" "${name}_linux_amd64.AppImage" updatable
    copy deb "*.deb" "${name}_linux_amd64.deb" updatable
    if [ "${NO_RPM:-false}" != true ]; then
      copy rpm "*.rpm" "${name}_linux_x86_64.rpm" updatable
    fi
    ;;
  *)
    echo "::error::unsupported target $target" >&2
    exit 1
    ;;
esac
