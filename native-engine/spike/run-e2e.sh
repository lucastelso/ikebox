#!/usr/bin/env bash
# SPIKE: roda o E2E do engine (tests/engine/e2e.sh) usando o strongSwan
# compilado por native-engine/build.sh, num "host" Fedora 45 simulado.
#
# Uso: native-engine/spike/run-e2e.sh VERSAO     (ex.: 6.1.0)

set -euo pipefail

version=${1:?versão (ex.: 6.1.0)}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tarball="$repo_root/native-engine/dist/ipsec-client-engine-$version-x86_64.tar.gz"
[[ -f $tarball ]] || { echo "rode antes: native-engine/build.sh $version" >&2; exit 1; }

ctx=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/ipsec-client-spike.XXXXXX")
trap 'rm -rf "$ctx"' EXIT
cp "$tarball" "$ctx/engine.tar.gz"
cp "$repo_root/native-engine/spike/Dockerfile" "$ctx/"
cp "$repo_root/engine/entrypoint.sh" "$repo_root/engine/updown.sh" "$ctx/"
cp "$repo_root/engine/strongswan.d/zz-ipsec-client.conf" "$ctx/"

image="ipsec-client-native-spike:$version"
docker build -q -t "$image" "$ctx" >/dev/null
echo "test image: $image"

# O engine embutido tem o patch do IPsec Client: updown e if_id forçados
# (cenário 5 do E2E).
IPSEC_CLIENT_E2E_ENGINE_IMAGE="$image" IPSEC_CLIENT_E2E_EXPECT_VERSION="$version" \
    IPSEC_CLIENT_E2E_FORCED=1 "$repo_root/tests/engine/e2e.sh"
