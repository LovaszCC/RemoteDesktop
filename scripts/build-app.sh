#!/bin/sh
# Builds a release RemoteDesktop.app into ./build and signs it.
#   scripts/build-app.sh            build only
#   scripts/build-app.sh --install  build and copy to /Applications
# SIGN_IDENTITY overrides the signing identity (default: first "Apple Development" identity, else ad-hoc).
set -eu
cd "$(dirname "$0")/.."

swift build -c release
BIN="$(swift build -c release --show-bin-path)/RemoteDesktop"

APP=build/RemoteDesktop.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/RemoteDesktop"
cp Resources/Info.plist "$APP/Contents/Info.plist"
[ -f Resources/AppIcon.icns ] && cp Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

IDENTITY="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/ { print $2; exit }')}"
# A stable identity keeps the Accessibility permission across rebuilds; ad-hoc resets it every time.
codesign --force --options runtime --sign "${IDENTITY:--}" "$APP"
echo "Built $APP (signed: ${IDENTITY:-ad-hoc})"

if [ "${1:-}" = "--install" ]; then
    pkill -x RemoteDesktop 2>/dev/null || true
    rm -rf /Applications/RemoteDesktop.app
    cp -R "$APP" /Applications/
    echo "Installed /Applications/RemoteDesktop.app"
fi
