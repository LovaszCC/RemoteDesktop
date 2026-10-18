#!/usr/bin/env bash
# Launch smoke test (macOS, Linux): starts the app, checks that it still runs after N seconds, logs the
# resident memory of its process tree, then stops it.
#
#   scripts/smoke-test.sh <executable> [seconds]
#
# On Linux run it under a display, e.g. `xvfb-run -a scripts/smoke-test.sh …`. An AppImage is run with
# APPIMAGE_EXTRACT_AND_RUN=1, so FUSE isn't needed. Windows uses scripts/smoke-test.ps1.
set -uo pipefail

exe=${1:?usage: smoke-test.sh <executable> [seconds]}
seconds=${2:-15}
log=$(mktemp)

export APPIMAGE_EXTRACT_AND_RUN=1

# The app is single-instance: with a copy already running (a runner on a developer's machine), the new one
# hands over to it and exits 0, which says nothing about this build.
name=$(basename "$exe")
if pgrep -x "$name" >/dev/null 2>&1; then
  echo "::notice title=Smoke test skipped::$name is already running on this machine (single instance); quit it to smoke test here."
  [ -n "${GITHUB_STEP_SUMMARY:-}" ] && echo "- ${RUNNER_OS:-local} ${SMOKE_LABEL:-}: skipped, $name already running" >>"$GITHUB_STEP_SUMMARY"
  exit 0
fi

"$exe" >"$log" 2>&1 &
pid=$!
sleep "$seconds"

# Sum of RSS (KiB) of the process and all its descendants.
tree_rss() {
  ps -A -o pid=,ppid=,rss= | awk -v root="$1" '
    { parent[$1] = $2; rss[$1] = $3 }
    END {
      for (p in rss) {
        q = p
        while (q != "" && q != 0 && q != root) q = parent[q]
        if (q == root) total += rss[p]
      }
      print total + 0
    }'
}

if ! kill -0 "$pid" 2>/dev/null; then
  wait "$pid"
  code=$?
  echo "::error::$(basename "$exe") exited within ${seconds}s (exit code $code)"
  echo "--- app output ---"
  cat "$log"
  exit 1
fi

rss=$(tree_rss "$pid")
message="$(basename "$exe") is running after ${seconds}s; RSS of its process tree: $((rss / 1024)) MiB (${rss} KiB)"
echo "$message"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  echo "- ${RUNNER_OS:-local} ${SMOKE_LABEL:-}: $message" >>"$GITHUB_STEP_SUMMARY"
fi

pkill -TERM -P "$pid" 2>/dev/null
kill -TERM "$pid" 2>/dev/null
for _ in 1 2 3 4 5; do
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
kill -KILL "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
echo "--- app output ---"
cat "$log"
exit 0
