#!/bin/sh
# Packages build/RemoteDesktop.app (from build-app.sh) into a drag-to-Applications disk image:
#   build/RemoteDesktop-<version>.dmg and its .sha256 checksum file.
# The version is read from the built app's Info.plist.
set -eu
cd "$(dirname "$0")/.."

APP=build/RemoteDesktop.app
[ -d "$APP" ] || { echo "$APP is missing; run scripts/build-app.sh first" >&2; exit 1; }
VERSION="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP/Contents/Info.plist")"
DMG="build/RemoteDesktop-$VERSION.dmg"

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
cp LICENSE THIRD_PARTY_NOTICES.md "$STAGING/"

rm -f "$DMG" "$DMG.sha256"
hdiutil create -quiet -volname RemoteDesktop -srcfolder "$STAGING" -fs HFS+ -format UDZO "$DMG"
codesign --force --sign - "$DMG"
(cd build && shasum -a 256 "$(basename "$DMG")" > "$(basename "$DMG").sha256")
echo "Built $DMG"
