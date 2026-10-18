#!/usr/bin/env bash
# CI: puts rustup on PATH for this and the later steps, installing it when missing (a self-hosted runner
# service doesn't read ~/.profile; the hosted images have it), then the pinned toolchain from
# rust-toolchain.toml. Extra arguments are rustup targets to add.
set -euo pipefail

cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
export PATH="$cargo_bin:$PATH"
if ! command -v rustup >/dev/null; then
  echo "rustup not found; installing it into $cargo_bin"
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
      init="${RUNNER_TEMP:-/tmp}/rustup-init.exe"
      curl -sSfL -o "$init" https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe
      "$init" -y --no-modify-path --default-toolchain none
      ;;
    *) curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none ;;
  esac
fi
[ -n "${GITHUB_PATH:-}" ] && echo "$cargo_bin" >>"$GITHUB_PATH"

rustup show active-toolchain || rustup toolchain install
for target in "$@"; do rustup target add "$target"; done
