#!/usr/bin/env bash
# SPIKE: runs the engine's E2E (tests/engine/e2e.sh) with the strongSwan
# built by native-engine/build.sh, on a simulated Fedora 45 "host".
#
# Usage: native-engine/spike/run-e2e.sh VERSION     (e.g. 6.1.0)

set -euo pipefail

version=${1:?versão (ex.: 6.1.0)}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tarball="$repo_root/native-engine/dist/ikebox-engine-$version-x86_64.tar.gz"
tools="$repo_root/native-engine/dist/ikebox-engine-tools-$version-x86_64.tar.gz"
[[ -f $tarball && -f $tools ]] || { echo "rode antes: native-engine/build.sh $version" >&2; exit 1; }

ctx=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/ikebox-spike.XXXXXX")
trap 'rm -rf "$ctx"' EXIT
cp "$tarball" "$ctx/engine.tar.gz"
cp "$tools" "$ctx/engine-tools.tar.gz"
cp "$repo_root/native-engine/spike/Dockerfile" "$ctx/"
cp "$repo_root/engine/entrypoint.sh" "$repo_root/engine/updown.sh" "$ctx/"
cp "$repo_root/engine/strongswan.d/zz-ikebox.conf" "$ctx/"

image="ikebox-native-spike:$version"
docker build -q -t "$image" "$ctx" >/dev/null
echo "test image: $image"

# The embedded engine has the Ikebox patch: forced updown and if_id
# (scenario 5 of the E2E).
IKEBOX_E2E_ENGINE_IMAGE="$image" IKEBOX_E2E_EXPECT_VERSION="$version" \
    IKEBOX_E2E_FORCED=1 "$repo_root/tests/engine/e2e.sh"
