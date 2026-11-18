#!/usr/bin/env bash
# Is the network component reproducible? Builds it twice from the same commit,
# each time in a fresh container of the release image (the engine twice too),
# and compares the bundle's SHA256SUMS, which is how the app tells whether the
# installed component is current: a rebuild of the same version must not look
# like an update.
#
# Usage: scripts/check-reproducible.sh [ref]     (default: HEAD)
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
ref=${1:-HEAD}
commit=$(git -C "$repo" rev-parse --verify "$ref^{commit}")

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
fail() {
    printf '\033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

strongswan=$(git -C "$repo" show "$commit:native-engine/versions.txt" |
    grep -vE '^\s*(#|$)' | head -n 1 | awk '{print $1}')
engine="$repo/native-engine/dist/ipsec-client-engine-$strongswan-x86_64.tar.gz"
rust=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$repo/rust-toolchain.toml")
image="ipsec-client-release:rust$rust-$(sha256sum "$repo/packaging/release/Dockerfile" | cut -c1-12)"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

step "engine, twice (strongSwan $strongswan)"
"$repo/native-engine/build.sh" "$strongswan" >/dev/null
first_engine=$(sha256sum "$engine" | cut -c1-64)
"$repo/native-engine/build.sh" "$strongswan" >/dev/null
second_engine=$(sha256sum "$engine" | cut -c1-64)
[[ $first_engine == "$second_engine" ]] ||
    fail "the engine differs between two builds: $first_engine / $second_engine"
printf '  ✔ engine %s\n' "$first_engine"

step "release image"
docker build -q --build-arg "RUST_VERSION=$rust" -t "$image" "$repo/packaging/release" >/dev/null

for round in 1 2; do
    step "network component, build $round (commit ${commit:0:12})"
    mkdir -p "$work/$round"
    git -C "$repo" archive --format=tar "$commit" |
        docker run --rm -i \
            -v "$engine:/dist/engine.tar.gz:ro" -v "$work/$round:/out" \
            -v ipsec-client-release-cargo:/root/.cargo/registry \
            -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
            "$image" bash -euo pipefail -c '
                tar -x -C /build
                scripts/stage-network-component.sh --out /out/component \
                    --engine /dist/engine.tar.gz >/dev/null
                chown -R "$HOST_UID:$HOST_GID" /out
            '
    sha256sum "$work/$round/component/SHA256SUMS" | cut -c1-64 >"$work/$round.sum"
done

if ! cmp -s "$work/1.sum" "$work/2.sum"; then
    diff "$work/1/component/SHA256SUMS" "$work/2/component/SHA256SUMS" >&2 || true
    fail "the network component differs between two builds"
fi
printf '  ✔ the same network component twice: SHA256SUMS %s\n' "$(cat "$work/1.sum")"
