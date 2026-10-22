#!/usr/bin/env bash
# CI (Linux): installs the system packages of a job. `test` is what ci.yml compiles and tests against;
# `build` adds what release.yml's bundling and smoke test need.
#
# The .deb files are downloaded into ~/apt-cache, which the workflows keep with actions/cache: the Ubuntu
# mirror the hosted runners use is sometimes very slow (62.8 MB took 16 minutes), and with the cache apt
# only fetches packages that changed since. The package lists are part of the cache key (hashFiles of this
# script), so editing them starts a fresh cache.
set -euo pipefail

packages=(libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev cmake ninja-build)
case "${1:-}" in
  test) ;;
  build) packages+=(file xvfb xauth) ;;
  *)
    echo "usage: $0 test|build" >&2
    exit 2
    ;;
esac

cache="$HOME/apt-cache"
mkdir -p "$cache/partial"
# The man-db trigger rebuilds the manual page index after every install; nothing here reads man pages.
sudo rm -f /var/lib/man-db/auto-update
sudo apt-get update
sudo apt-get install -y -o Dir::Cache::Archives="$cache" "${packages[@]}"
# apt downloads as root; actions/cache saves as the runner user.
sudo chown -R "$(id -u):$(id -g)" "$cache"
rm -f "$cache/lock"
