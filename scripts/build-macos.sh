#!/usr/bin/env bash
# macOS release build: an ad-hoc signed DMG for Apple Silicon in dist/release/<version>/.
# Run it on a Mac (Xcode Command Line Tools, rustup, Node 26, pnpm 12, `brew install cmake ninja`). See

#
#   scripts/build-macos.sh                 the DMG (aarch64; Intel Macs aren't supported)
#   scripts/build-macos.sh --no-updater    without updater signatures even if the key is found
#   scripts/build-macos.sh --allow-dirty   build with uncommitted changes (test builds only)
#
#   → RemoteDesktop_<version>_macos_aarch64.dmg
#     (+ .app.tar.gz and .sig for the updater when the key is found)
set -euo pipefail
[ "$(uname -s)" = Darwin ] || { echo "error: run this on macOS" >&2; exit 1; }
# shellcheck source=build-common.sh
. "$(dirname "$0")/build-common.sh"

# bundle.macOS.signingIdentity "-" in tauri.conf.json makes `tauri build` sign ad hoc.
bundles=app,dmg
for target in aarch64-apple-darwin; do
  build_target "$target" --bundles "$bundles"
  arch=${target%%-*}
  app=$(ls -d "target/$target/release/bundle/macos/"*.app)
  step "Check signature ($arch)"
  codesign --verify --verbose=2 "$app"
  codesign -dv "$app" 2>&1 | grep -E '^Signature=' || true
done

step "Done"
ls -lh "$out"/*.dmg
