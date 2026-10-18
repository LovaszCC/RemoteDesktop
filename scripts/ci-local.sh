#!/usr/bin/env bash
# Local CI: the same checks as .github/workflows/ci.yml, run on this machine.
#
#   scripts/ci-local.sh            UI lint/test/build, fmt, clippy, tests, Tauri compile check
#   scripts/ci-local.sh --lab      also the lab integration tests (needs `docker compose -f
#                                  tests/lab/docker-compose.yml up -d`)
#   scripts/ci-local.sh --fuzz     also 30 s of fuzzing (needs nightly + cargo-fuzz)
#   scripts/ci-local.sh --fast     skip the Tauri compile check
#
# Only covers the host OS. Windows/Linux checks run on GitHub via `gh workflow run ci.yml`.
set -euo pipefail
cd "$(dirname "$0")/.."

lab=0 fuzz=0 fast=0
for arg in "$@"; do
  case "$arg" in
    --lab) lab=1 ;;
    --fuzz) fuzz=1 ;;
    --fast) fast=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

export CARGO_TERM_COLOR=always CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

step "Forbidden strings"
if git grep -nIiE 'alpha.?vet' -- . ':!scripts/ci-local.sh' ':!.github' ; then
  echo "forbidden string found" >&2; exit 1
fi

step "UI install";  pnpm -C app/ui install --frozen-lockfile
step "UI lint";     pnpm -C app/ui lint
step "UI tests";    pnpm -C app/ui test
step "UI build";    pnpm -C app/ui build
step "rustfmt";     cargo fmt --all --check
step "clippy";      cargo clippy --workspace --all-targets -- -D warnings
step "Tests";       cargo test --workspace
step "latest.json"; node --test scripts/make-latest-json.test.mjs

if [ "$fast" = 0 ]; then
  step "Tauri compile check"; pnpm -C app/ui tauri build --debug --no-bundle
fi

if [ "$lab" = 1 ]; then
  step "Lab: background SSH and file browser"
  RD_TEST_SSHD=127.0.0.1:2221 RD_TEST_SSHD_BUSYBOX=127.0.0.1:2222 \
    cargo test -p rd-ssh --test background --test files
  RD_TEST_SSHD=127.0.0.1:2221 cargo test -p rd-app --lib lab_sshd
  step "Lab: embedded RDP against xrdp"
  RD_TEST_XRDP=127.0.0.1:3390 RD_TEST_XRDP_CONTAINER=lab-xrdp-1 cargo test -p rd-rdp --test live
fi

if [ "$fuzz" = 1 ]; then
  step "Fuzz winbox .cdb (30 s)"
  cargo +nightly fuzz run winbox_cdb --sanitizer none --fuzz-dir crates/rd-core/fuzz -- \
    -max_total_time=30 -dict=crates/rd-core/fuzz/winbox.dict
fi

step "Local CI passed"
