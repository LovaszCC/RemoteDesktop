#!/bin/sh
# Launches the built app and fails if it exits within SMOKE_SECONDS (default 15): catches launch crashes
# such as the 1.1.0 missing resource bundle and the 1.2.0 missing SwiftUI environment object.
#   scripts/smoke-test-app.sh [path/to/RemoteDesktop.app]   (default: build/RemoteDesktop.app)
# The launch doesn't touch the Keychain, skips the update check and ignores saved window state.
set -eu
cd "$(dirname "$0")/.."

APP="${1:-build/RemoteDesktop.app}"
SECONDS_TO_SURVIVE="${SMOKE_SECONDS:-15}"
[ -x "$APP/Contents/MacOS/RemoteDesktop" ] || { echo "$APP is missing; run scripts/build-app.sh first" >&2; exit 1; }
LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

EXECUTABLE="$(cd "$APP" && pwd)/Contents/MacOS/RemoteDesktop"
pgrep -f "$EXECUTABLE" >/dev/null && { echo "$APP is already running; quit it first" >&2; exit 1; }

# Through LaunchServices, not the bare executable: only then does AppKit open the initial windows.
# Defaults overrides are plist values: the app reads them as Bool.
open -n --stdout "$LOG" --stderr "$LOG" "$APP" --args \
    -rememberMasterKey '<false/>' \
    -checkForUpdatesAutomatically '<false/>' \
    -ApplePersistenceIgnoreState YES

fail() {
    echo "$1. Output:" >&2
    tail -n 40 "$LOG" >&2
    exit 1
}

# The app's own process, not `open`'s, decides the result.
PID=""
for _ in $(seq 1 20); do
    PID="$(pgrep -n -f "$EXECUTABLE" || true)"
    [ -n "$PID" ] && break
    sleep 0.5
done
[ -n "$PID" ] || fail "RemoteDesktop did not start"

for _ in $(seq 1 "$SECONDS_TO_SURVIVE"); do
    sleep 1
    kill -0 "$PID" 2>/dev/null || fail "RemoteDesktop exited during launch"
done

kill "$PID"
echo "RemoteDesktop survived ${SECONDS_TO_SURVIVE}s after launch"
