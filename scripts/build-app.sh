#!/bin/sh
# Builds a release RemoteDesktop.app into ./build and signs it.
#   scripts/build-app.sh            build only
#   scripts/build-app.sh --install  build and copy to /Applications
# SIGN_IDENTITY overrides the signing identity (default: first "Apple Development" identity, else ad-hoc).
set -eu
cd "$(dirname "$0")/.."

# Static FreeRDP for the embedded RDP client (one-time, ~10 min).
[ -d Vendor/FreeRDP.xcframework ] || scripts/build-freerdp.sh

swift build -c release
BIN="$(swift build -c release --show-bin-path)/RemoteDesktop"

APP=build/RemoteDesktop.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/RemoteDesktop"
cp Resources/Info.plist "$APP/Contents/Info.plist"
[ -f Resources/AppIcon.icns ] && cp Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# Bundled licenses: FreeRDP and OpenSSL (Apache-2.0) must ship with the binary.
cp LICENSE THIRD_PARTY_NOTICES.md "$APP/Contents/Resources/"
# SwiftPM resource bundles of dependencies (SwiftTerm shaders, GRDB privacy manifest).
for bundle in "$(dirname "$BIN")"/*.bundle; do
    [ -d "$bundle" ] && cp -R "$bundle" "$APP/Contents/Resources/"
done

IDENTITY="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/ { print $2; exit }')}"
# A stable identity keeps the Accessibility permission across rebuilds; ad-hoc resets it every time.
codesign --force --options runtime --sign "${IDENTITY:--}" "$APP"
echo "Built $APP (signed: ${IDENTITY:-ad-hoc})"

if [ "${1:-}" = "--install" ]; then
    # Ask the running app to quit (it confirms open sessions) and wait up to 30 s; if it stays
    # running, stop: installing over a running copy would leave the old version in use.
    if pgrep -x RemoteDesktop >/dev/null; then
        osascript -e 'quit app "RemoteDesktop"' >/dev/null 2>&1 || true
        for _ in $(seq 1 30); do pgrep -x RemoteDesktop >/dev/null || break; sleep 1; done
        if pgrep -x RemoteDesktop >/dev/null; then
            echo "RemoteDesktop did not quit (open sessions not confirmed?); not installing" >&2
            exit 1
        fi
    fi
    rm -rf /Applications/RemoteDesktop.app
    cp -R "$APP" /Applications/
    echo "Installed /Applications/RemoteDesktop.app"
fi
