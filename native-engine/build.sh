#!/usr/bin/env bash
# Builds the embedded engine's strongSwan for one (or all) of the versions in
# native-engine/versions.txt. It uses Docker only as a reproducible build
# environment: the end user gets the result ready inside the app.
#
# Usage: native-engine/build.sh [VERSION|all]     (default: the first in the list)
# Output: native-engine/dist/ikebox-engine-<version>-x86_64.tar.gz (engine)
#         native-engine/dist/ikebox-engine-tools-<version>-x86_64.tar.gz (swanctl, tests only)

set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readonly BUILDER=ikebox-engine-builder:local
wanted=${1:-}

mapfile -t entries < <(grep -vE '^\s*(#|$)' "$here/versions.txt")
((${#entries[@]} > 0)) || { echo "versions.txt vazio" >&2; exit 1; }
[[ -n $wanted ]] || wanted=$(awk '{print $1}' <<<"${entries[0]}")

docker build -q -t "$BUILDER" "$here/build" >/dev/null
mkdir -p "$here/dist"

built=0
for entry in "${entries[@]}"; do
    read -r version path sha256 <<<"$entry"
    [[ $wanted == all || $wanted == "$version" ]] || continue
    echo "==> strongSwan $version"
    docker run --rm --network bridge \
        -v "$here/keys:/keys:ro" -v "$here/patches:/patches:ro" \
        -v "$here/licenses:/licenses:ro" -v "$here/dist:/out" \
        --user "$(id -u):$(id -g)" -e HOME=/build \
        --tmpfs /build:exec,uid="$(id -u)",gid="$(id -g)" \
        "$BUILDER" "$version" "$path" "$sha256"
    built=$((built + 1))
done
((built > 0)) || { echo "version '$wanted' is not in versions.txt" >&2; exit 1; }
