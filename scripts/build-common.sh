# Shared part of scripts/build-macos.sh and scripts/build-linux.sh (sourced, not run).
#
# Sets: version, commit, out (dist/release/<version>), sign (true/false), tauri_args (extra `tauri build`
# arguments). Options of the build scripts: --no-updater, --allow-dirty, -h/--help.
#
# Updater signatures: with TAURI_SIGNING_PRIVATE_KEY set, or the key file at
# ${TAURI_SIGNING_PRIVATE_KEY_PATH:-~/.tauri/remotedesktop.key}, the build also makes the signed
# updater artifacts (.sig) that scripts/release-publish.sh needs for latest.json; the key's password comes
# from TAURI_SIGNING_PRIVATE_KEY_PASSWORD or is asked for once.

cd "$(dirname "$0")/.."

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
die() { echo "error: $*" >&2; exit 1; }

updater=auto dirty=0
for arg in "$@"; do
  case "$arg" in
    --no-updater) updater=off ;;
    --allow-dirty) dirty=1 ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d'; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

# cmake, ninja and perl build FreeRDP the first time (scripts/build-freerdp.sh).
for tool in cargo rustup node pnpm git cmake ninja perl; do
  command -v "$tool" >/dev/null || die "$tool not found"
done

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || die "no [workspace.package] version in Cargo.toml"
commit=$(git rev-parse HEAD)
out="dist/release/$version"

if [ "$dirty" = 0 ] && [ -n "$(git status --porcelain)" ]; then
  die "uncommitted changes; commit them first (or --allow-dirty for a test build)"
fi

key_file=${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.tauri/remotedesktop.key}
sign=false
if [ "$updater" != off ]; then
  if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -f "$key_file" ]; then
    TAURI_SIGNING_PRIVATE_KEY=$(cat "$key_file")
    export TAURI_SIGNING_PRIVATE_KEY
  fi
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    sign=true
    if [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+x}" ]; then
      read -rsp "Updater key password: " TAURI_SIGNING_PRIVATE_KEY_PASSWORD
      echo
      export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    fi
  else
    echo "note: no updater key ($key_file); building without updater signatures (.sig)." >&2
  fi
fi
tauri_args=(--ci)
if [ "$sign" = true ]; then
  tauri_args+=(--config '{"bundle":{"createUpdaterArtifacts":true}}')
else
  unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
fi

mkdir -p "$out"
echo "RemoteDesktop $version from ${commit:0:12} → $out (updater signatures: $sign)"

step "UI dependencies"
pnpm -C app/ui install --frozen-lockfile

# build_target <triple> [extra tauri args…]: builds, collects the bundles into $out and records the commit
# they came from (release-publish.sh checks it).
build_target() {
  local target=$1
  shift
  step "Build $target"
  rustup target add "$target" >/dev/null
  pnpm -C app/ui tauri build --target "$target" "${tauri_args[@]}" "$@"
  step "Collect $target"
  UPDATER=$sign scripts/collect-bundles.sh "$target" "$version" "$out"
  printf '%s\nupdater=%s\n' "$commit" "$sign" >"$out/.build-$target"
}
