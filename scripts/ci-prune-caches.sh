#!/usr/bin/env bash
# CI: keeps the Actions cache under GitHub's 10 GB per-repository limit. Beyond it GitHub evicts the least
# recently used entries, which used to throw out the FreeRDP and Rust caches the next build needed (a cold
# Windows build takes twice as long).
#
# Deletes the caches of pull request refs (CI no longer runs on PRs, so nothing reads them) and every Rust
# cache (Swatinem/rust-cache, `v0-rust-…`) but the newest per job kind and target: its last two key
# segments are the toolchain/environment and Cargo.lock hashes, so every version bump left the old one
# behind. `--dry-run` only lists what would go. Needs GH_TOKEN with `actions: write`.
set -euo pipefail

dry_run=false
[ "${1:-}" = --dry-run ] && dry_run=true
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}"

caches=$(gh api --paginate "repos/$repo/actions/caches?per_page=100" \
  --jq '.actions_caches[] | [.id, .ref, .key, .created_at, .size_in_bytes] | @tsv')

# Newest first, so the first cache of each group is the one kept.
stale=$(printf '%s\n' "$caches" | sort -t $'\t' -k4,4r | awk -F '\t' '
  NF < 5 { next }
  $2 ~ /^refs\/pull\// { print; next }
  $3 ~ /^v0-rust-/ {
    n = split($3, part, "-")
    group = $2
    for (i = 1; i <= n - 2; i++) group = group "-" part[i]
    if (seen[group]++) print
  }')

if [ -z "$stale" ]; then
  echo "No stale caches."
  exit 0
fi

freed=0
while IFS=$'\t' read -r id ref key _ size; do
  echo "delete $key ($ref, $((size / 1000000)) MB)"
  freed=$((freed + size))
  [ "$dry_run" = true ] || gh api -X DELETE "repos/$repo/actions/caches/$id" --silent
done <<<"$stale"
verb=$([ "$dry_run" = true ] && echo "Would free" || echo "Freed")
echo "$verb $((freed / 1000000)) MB." | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
