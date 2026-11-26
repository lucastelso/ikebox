#!/usr/bin/env bash
# Builds the embedded engine's strongSwan for one (or all) of the versions in
# native-engine/versions.txt (ADR-0032).
#
# By default it runs native-engine/build/build-strongswan.sh inside the
# pinned Alpine builder: the official, reproducible path that releases use.
# With --host, the same script runs on this computer with its musl toolchain
# (no Docker), for distribution packagers.
#
# Usage: native-engine/build.sh [options] [VERSION|all]  (default: the first in the list)
#        native-engine/build.sh --fetch DIR [VERSION|all]
#   --host            build here instead of in the builder (needs a musl toolchain)
#   --out DIR         where the tarballs go             [native-engine/dist]
#   --sources DIR     tarballs and signatures already downloaded (offline build)
#   --openssl MODE    auto, system or source (see build/build-strongswan.sh)
#   --fetch DIR       only download and check strongSwan and OpenSSL into DIR
# Output: <out>/ikebox-engine-<version>-x86_64.tar.gz (engine)
#         <out>/ikebox-engine-tools-<version>-x86_64.tar.gz (swanctl, tests only)

set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readonly BUILDER=ikebox-engine-builder:local
host=0
out=$here/dist
sources=""
fetch=""
openssl_args=()
wanted=""
while (($# > 0)); do
    case $1 in
    --host) host=1; shift ;;
    --out) out=${2:?}; shift 2 ;;
    --sources) sources=${2:?}; shift 2 ;;
    --openssl) openssl_args=(--openssl "${2:?}"); shift 2 ;;
    --fetch) fetch=${2:?}; shift 2 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) wanted=$1; shift ;;
    esac
done

mapfile -t entries < <(grep -vE '^\s*(#|$)' "$here/versions.txt")
((${#entries[@]} > 0)) || { echo "versions.txt is empty" >&2; exit 1; }
[[ -n $wanted ]] || wanted=$(awk '{print $1}' <<<"${entries[0]}")

script=$here/build/build-strongswan.sh
common=(--keys "$here/keys" --patches "$here/patches" --licenses "$here/licenses"
    --cap-header "$here/build/musl-capabilities.h" --openssl-pin "$here/openssl.txt")

if [[ -n $fetch ]]; then
    mkdir -p "$fetch"
    fetch=$(cd "$fetch" && pwd)
else
    mkdir -p "$out"
    out=$(cd "$out" && pwd)
    ((host)) || docker build -q -t "$BUILDER" "$here/build" >/dev/null
fi
source_args=()
if [[ -n $sources ]]; then
    sources=$(cd "$sources" && pwd)
    source_args=(--sources "$sources")
fi

built=0
for entry in "${entries[@]}"; do
    read -r version path sha256 <<<"$entry"
    [[ $wanted == all || $wanted == "$version" ]] || continue
    if [[ -n $fetch ]]; then
        echo "==> strongSwan $version: sources"
        work=$(mktemp -d "${TMPDIR:-/tmp}/ikebox-fetch.XXXXXX")
        "$script" "${common[@]}" --work "$work" --sources "$fetch" --fetch-only \
            "$version" "$path" "$sha256"
        rm -rf "$work"
    elif ((host)); then
        echo "==> strongSwan $version (this computer's musl toolchain)"
        work=$(mktemp -d "${TMPDIR:-/tmp}/ikebox-engine.XXXXXX")
        "$script" "${common[@]}" --work "$work" --out "$out" \
            "${source_args[@]}" "${openssl_args[@]}" "$version" "$path" "$sha256"
        rm -rf "$work"
    else
        echo "==> strongSwan $version"
        mounts=(-v "$here/keys:/keys:ro" -v "$here/patches:/patches:ro"
            -v "$here/licenses:/licenses:ro" -v "$out:/out")
        extra=()
        if [[ -n $sources ]]; then
            mounts+=(-v "$sources:/sources:ro")
            extra+=(--sources /sources)
        fi
        if ((${#openssl_args[@]} > 0)); then
            mounts+=(-v "$here/openssl.txt:/pins/openssl.txt:ro")
            extra+=("${openssl_args[@]}" --openssl-pin /pins/openssl.txt)
        fi
        docker run --rm --network bridge "${mounts[@]}" \
            --user "$(id -u):$(id -g)" -e HOME=/build \
            --tmpfs /build:exec,uid="$(id -u)",gid="$(id -g)" \
            "$BUILDER" "${extra[@]}" "$version" "$path" "$sha256"
    fi
    built=$((built + 1))
done
((built > 0)) || { echo "version '$wanted' is not in versions.txt" >&2; exit 1; }
