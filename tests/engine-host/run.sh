#!/usr/bin/env bash
# Host build of the embedded engine (ADR-0032), on plain distribution
# containers standing in for a packager's computer.
#
#   tests/engine-host/run.sh                  every distribution of distros.txt
#   tests/engine-host/run.sh fedora-45 ...    only the ones named
#   IKEBOX_ENGINE_HOST_E2E=0                  build only, skip the E2E
#
# For each distribution: an image with only that distribution's packages (a
# musl toolchain, the kernel headers, the build tools); then
# `native-engine/build.sh --host` inside it, as an unprivileged user, with no
# network, from the sources fetched and checked once here (SHA-256 + GPG).
# The engine must come out static-pie, record the host build in BUILDINFO,
# and pass the engine's E2E (native-engine/spike/run-e2e.sh), the same as the
# builder's engine.
#
# Requirements: Docker, curl and gpg (to fetch the sources), the
# xfrm_interface module (for the E2E).

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
readonly LIST="$repo/tests/engine-host/distros.txt"
readonly WORK="$repo/target/engine-host"

step() { printf '\n\033[1;35m#### %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '\033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

# The packages each family needs, and nothing else.
install_command() {
    case $1 in
    fedora-*)
        echo "dnf -y -q install --setopt=install_weak_deps=False musl-gcc musl-libc-static kernel-headers make pkgconf-pkg-config python3 perl bzip2 patch file binutils gnupg2 diffutils && dnf clean all"
        ;;
    ubuntu-* | debian-*)
        echo "apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends build-essential musl-tools linux-libc-dev pkg-config python3 perl bzip2 patch file gnupg && rm -rf /var/lib/apt/lists/*"
        ;;
    arch)
        echo "pacman -Syu --noconfirm --needed base-devel musl python perl bzip2 patch file gnupg && pacman -Scc --noconfirm"
        ;;
    *) return 1 ;;
    esac
}

# build_one NAME BASE VERSION: the image, the build, the checks.
build_one() {
    local name=$1 base=$2 version=$3 cmd image ctx out info charon
    cmd=$(install_command "$name") || fail "$name: no package list"
    image="ikebox-engine-host:$name-$(printf '%s\n%s\n' "$base" "$cmd" | sha256sum | cut -c1-12)"
    if ! docker image inspect "$image" >/dev/null 2>&1; then
        ctx=$(mktemp -d)
        printf 'FROM %s\nRUN %s\n' "$base" "$cmd" >"$ctx/Dockerfile"
        docker build -q -t "$image" "$ctx" >/dev/null || fail "$name: could not prepare the image"
        rm -rf "$ctx"
    fi
    out="$WORK/$name"
    rm -rf "$out"
    mkdir -p "$out"
    docker run --rm --network none --security-opt label=disable \
        --user "$(id -u):$(id -g)" -e HOME=/tmp -e TMPDIR=/tmp \
        -v "$repo:/src:ro" -v "$WORK/sources:/sources:ro" -v "$out:/out" \
        "$image" /src/native-engine/build.sh --host --sources /sources --out /out "$version" \
        >"$out/build.log" 2>&1 || {
        tail -n 25 "$out/build.log"
        fail "$name: the host build failed (log: $out/build.log)"
    }
    [[ -f $out/ikebox-engine-$version-x86_64.tar.gz ]] || fail "$name: no engine tarball"
    info=$(tar -xzOf "$out/ikebox-engine-$version-x86_64.tar.gz" opt/ikebox/engine/BUILDINFO)
    [[ $info == *$'\nbuilder=host:'* && $info == *$'\nlinkage=static\n'* ]] ||
        fail "$name: BUILDINFO does not record a static host build"
    grep -q '^patches=.*0001-vici-forced-updown' <<<"$info" || fail "$name: BUILDINFO without the patches"
    charon="$out/charon"
    tar -xzOf "$out/ikebox-engine-$version-x86_64.tar.gz" opt/ikebox/engine/libexec/ipsec/charon >"$charon"
    [[ $(file -b "$charon") == *"static-pie linked"* ]] || fail "$name: charon is not static-pie: $(file -b "$charon")"
    [[ $(readelf -dW "$charon") == *BIND_NOW* ]] || fail "$name: charon without BIND_NOW"
    ok "$name: $(sed -n 's/^builder=//p' <<<"$info")"
    ok "$name: OpenSSL $(sed -n 's/^openssl=//p' <<<"$info" | sed 's/, SHA-256 .*/)/'), charon static-pie with BIND_NOW"
    if [[ ${IKEBOX_ENGINE_HOST_E2E:-1} != 0 ]]; then
        "$repo/native-engine/spike/run-e2e.sh" \
            --engine "$out/ikebox-engine-$version-x86_64.tar.gz" \
            --tools "$out/ikebox-engine-tools-$version-x86_64.tar.gz" "$version" \
            >"$out/e2e.log" 2>&1 || {
            tail -n 25 "$out/e2e.log"
            fail "$name: the engine's E2E failed with this engine (log: $out/e2e.log)"
        }
        ok "$name: the engine's E2E passes with this engine"
    fi
}

main() {
    docker info >/dev/null 2>&1 || fail "Docker is not reachable"
    local -a wanted=("$@") names=() bases=() failed=()
    local name base version i
    while read -r name base; do
        [[ -n $name && $name != \#* ]] || continue
        if ((${#wanted[@]} == 0)) || [[ " ${wanted[*]} " == *" $name "* ]]; then
            names+=("$name")
            bases+=("$base")
        fi
    done <"$LIST"
    ((${#names[@]} > 0)) || fail "no distribution selected (see $LIST)"
    version=$(grep -vE '^\s*(#|$)' "$repo/native-engine/versions.txt" | head -n 1 | awk '{print $1}')

    step "Sources of strongSwan $version and OpenSSL, fetched and checked here"
    mkdir -p "$WORK"
    "$repo/native-engine/build.sh" --fetch "$WORK/sources" "$version" >"$WORK/fetch.log" 2>&1 || {
        cat "$WORK/fetch.log"
        fail "could not fetch the sources"
    }
    ok "$(find "$WORK/sources" -maxdepth 1 -type f | wc -l) files in $WORK/sources"

    for i in "${!names[@]}"; do
        step "${names[$i]}"
        if ! (build_one "${names[$i]}" "${bases[$i]}" "$version"); then
            failed+=("${names[$i]}")
        fi
    done

    step "Result"
    for name in "${names[@]}"; do
        if [[ " ${failed[*]} " == *" $name "* ]]; then
            printf '  \033[0;31m✘\033[0m %s\n' "$name"
        else
            ok "$name"
        fi
    done
    ((${#failed[@]} == 0))
}

main "$@"
