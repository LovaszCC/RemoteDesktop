#!/usr/bin/env bash
# Publishes the bundles made by scripts/build-macos.sh, build-linux.sh and build-windows.ps1 as a DRAFT
# GitHub release, like the "draft release" job of .github/workflows/release.yml.
#
#   scripts/release-publish.sh                 tag v<version> (Cargo.toml) on HEAD if missing, push it,
#                                              then a draft release with the bundles, the checksums and
#                                              latest.json
#   scripts/release-publish.sh --no-updater    allow bundles without updater signatures (no latest.json;
#                                              installed apps won't see this release)
#   scripts/release-publish.sh --yes           don't ask before tagging/pushing/uploading
#
# Checks first: HEAD is origin/main and clean, every bundle in dist/release/<version>/ was built from HEAD,
# the updater pubkey isn't the placeholder (unless --no-updater). Publishing the draft stays a manual step
# on GitHub; only then does the in-app updater see it.
set -euo pipefail
cd "$(dirname "$0")/.."

no_updater=0 yes=0
for arg in "$@"; do
  case "$arg" in
    --no-updater) no_updater=1 ;;
    --yes) yes=1 ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }
confirm() {
  [ "$yes" = 1 ] && return 0
  read -rp "$1 [y/N] " answer
  [ "$answer" = y ] || [ "$answer" = Y ] || die "cancelled"
}

repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || die "no [workspace.package] version in Cargo.toml"
tag="v$version"
dir="dist/release/$version"
commit=$(git rev-parse HEAD)

# --- checks --------------------------------------------------------------------------------------------
[ -z "$(git status --porcelain)" ] || die "uncommitted changes"
git fetch -q origin main --tags
[ "$commit" = "$(git rev-parse origin/main)" ] || die "HEAD isn't origin/main; release from an up-to-date main"
[ -d "$dir" ] || die "$dir doesn't exist; run scripts/build-macos.sh / build-linux.sh / build-windows.ps1 first"
if gh release view "$tag" -R "$repo" >/dev/null 2>&1; then
  die "release $tag already exists on GitHub; bump the version in Cargo.toml"
fi

shopt -s nullglob
builds=("$dir"/.build-*)
[ "${#builds[@]}" -gt 0 ] || die "no builds in $dir"
for build in "${builds[@]}"; do
  target=${build##*/.build-}
  [ "$(head -n 1 "$build")" = "$commit" ] || die "$target was built from $(head -n 1 "$build"), not HEAD $commit; rebuild it"
  if [ "$no_updater" = 0 ] && ! grep -qx 'updater=true' "$build"; then
    die "$target has no updater signatures (no key at build time); rebuild with the key, or --no-updater"
  fi
done
if [ "$no_updater" = 0 ] && grep -q REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY app/src-tauri/tauri.conf.json; then
  die "tauri.conf.json still has the placeholder updater pubkey; this release could never update itself"
fi

assets=()
for f in "$dir"/RemoteDesktop_"${version}"_*; do
  case "$f" in *_SHA256SUMS.txt) ;; *) assets+=("$f") ;; esac
done
[ "${#assets[@]}" -gt 0 ] || die "no bundles in $dir"

for platform in macos_aarch64 windows_x64 linux_amd64; do
  if ! printf '%s\n' "${assets[@]}" | grep -q "_${platform}"; then
    echo "warning: no $platform bundle; this release won't have one." >&2
  fi
done

echo "Release $tag of $repo from $commit:"
printf '  %s\n' "${assets[@]##*/}"

# --- tag -----------------------------------------------------------------------------------------------
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  [ "$(git rev-parse "$tag^{commit}")" = "$commit" ] || die "tag $tag exists on another commit"
else
  confirm "Create tag $tag on HEAD and push it?"
  git tag -a "$tag" -m "RemoteDesktop $version"
fi
git push -q origin "$tag"

# --- draft release -------------------------------------------------------------------------------------
sums="$dir/RemoteDesktop_${version}_SHA256SUMS.txt"
(cd "$dir" && shasum -a 256 -- "${assets[@]##*/}") >"$sums"

confirm "Upload ${#assets[@]} files as draft release $tag?"
gh release create "$tag" -R "$repo" "${assets[@]}" "$sums" --draft --verify-tag \
  --title "RemoteDesktop $version" --generate-notes

if [ "$no_updater" = 0 ]; then
  notes=$(mktemp)
  gh release view "$tag" -R "$repo" --json body --jq .body >"$notes"
  node scripts/make-latest-json.mjs "$dir" "$version" \
    "https://github.com/$repo/releases/download/$tag" "$notes" >"$dir/latest.json"
  rm -f "$notes"
  gh release upload "$tag" -R "$repo" "$dir/latest.json"
fi

echo
echo "Draft release $tag created: $(gh release view "$tag" -R "$repo" --json url --jq .url)"
echo "Review it and Publish it on GitHub (not as pre-release); only then do installed apps see it."
