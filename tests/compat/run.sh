#!/usr/bin/env bash
# Compatibility matrix: repeats the test with real systemd (tests/systemd) on
# every distribution of tests/compat/distros.txt.
#
#   tests/compat/run.sh                 every distribution
#   tests/compat/run.sh debian-12 ...   only the ones named
#
# What it checks on each one, with that distribution's own systemd, iproute2,
# bash and glibc: the installer, the helper and its unit, the engine sandbox
# (every property must be known to that systemd), the tunnel, routes and DNS
# (ip, resolvectl), user isolation and the ADR-0019 attack.
#
# The helper and the CLI are built once, in the release image (Ubuntu 22.04):
# the same glibc floor as the published packages. The kernel is this
# computer's: containers share it.
#
# Requirements: Docker, the engine compiled (native-engine/build.sh) and the
# release image (built here if missing; the first time takes a while).

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
readonly LIST="$repo/tests/compat/distros.txt"
readonly BIN="$repo/target/compat/bin"

step() { printf '\n\033[1;35m#### %s\033[0m\n' "$*"; }
fail() {
    printf '\033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

# Builds ikebox-helper and ikebox-cli from the working tree in the
# release image and leaves them in target/compat/bin.
build_portable() {
    local rust image
    rust=$(awk -F'"' '/^channel/ {print $2}' "$repo/rust-toolchain.toml")
    image="ikebox-release:rust$rust-$(sha256sum "$repo/packaging/release/Dockerfile" | cut -c1-12)"
    if ! docker image inspect "$image" >/dev/null 2>&1; then
        step "preparing the build image ($image)"
        docker build --build-arg "RUST_VERSION=$rust" -t "$image" "$repo/packaging/release"
    fi
    mkdir -p "$BIN"
    docker run --rm --security-opt label=disable \
        -v "$repo:/src:ro" -v "$BIN:/out" \
        -v ikebox-release-cargo:/root/.cargo/registry \
        -v ikebox-compat-target:/target \
        -e CARGO_TARGET_DIR=/target -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
        -w /src "$image" bash -euo pipefail -c '
            cargo build -q --release --locked -p ikebox-helper -p ikebox-cli
            install -m 0755 /target/release/ikebox-helper /target/release/ikebox-cli /out/
            chown "$HOST_UID:$HOST_GID" /out/ikebox-helper /out/ikebox-cli
        '
}

main() {
    docker info >/dev/null 2>&1 || fail "Docker is not reachable"
    local -a wanted=("$@") names=() bases=() failed=()
    local name base
    while read -r name base; do
        [[ -n $name && $name != \#* ]] || continue
        if ((${#wanted[@]} == 0)) || [[ " ${wanted[*]} " == *" $name "* ]]; then
            names+=("$name")
            bases+=("$base")
        fi
    done <"$LIST"
    ((${#names[@]} > 0)) || fail "no distribution selected (see $LIST)"

    step "helper and CLI built in the release image (glibc of Ubuntu 22.04)"
    local i
    build_portable

    for i in "${!names[@]}"; do
        step "${names[$i]}"
        if ! IKEBOX_TEST_NAME="${names[$i]}" IKEBOX_TEST_BASE="${bases[$i]}" \
            IKEBOX_TEST_BIN_DIR="$BIN" "$repo/tests/systemd/run.sh"; then
            failed+=("${names[$i]}")
        fi
    done

    step "Result"
    for name in "${names[@]}"; do
        if [[ " ${failed[*]} " == *" $name "* ]]; then
            printf '  \033[0;31m✘\033[0m %s\n' "$name"
        else
            printf '  \033[0;32m✔\033[0m %s\n' "$name"
        fi
    done
    ((${#failed[@]} == 0))
}

main "$@"
