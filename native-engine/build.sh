#!/usr/bin/env bash
# Compila o strongSwan do engine embutido para uma (ou todas as) versões de
# native-engine/versions.txt. Usa Docker só como ambiente de build
# reproduzível: o usuário final recebe o resultado pronto dentro do app.
#
# Uso: native-engine/build.sh [VERSAO|all]     (padrão: a primeira da lista)
# Saída: native-engine/dist/ipsec-client-engine-<versão>-x86_64.tar.gz (engine)
#        native-engine/dist/ipsec-client-engine-tools-<versão>-x86_64.tar.gz (swanctl, tests only)

set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readonly BUILDER=ipsec-client-engine-builder:local
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
