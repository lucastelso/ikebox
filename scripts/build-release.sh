#!/usr/bin/env bash
# Gera os pacotes de release (AppImage, deb e rpm) num container Ubuntu 22.04.
#
# Uso:
#   scripts/build-release.sh [<tag ou commit>]     (padrão: HEAD)
#
# - O código vem do commit indicado (git archive): mudanças não commitadas
#   ficam de fora, e nenhum caminho do seu computador entra nos binários.
# - O strongSwan embutido vem de native-engine/dist/ (compilado aqui, com o
#   código-fonte conferido por SHA-256 + GPG, se ainda não existir).
# - Saída: target/release-artifacts/<versão>/ com os três pacotes e o
#   SHA256SUMS, depois de conferidos por scripts/verify-release.sh e varridos
#   por scripts/check-private.sh --artifacts (dados privados, no host).
#
# Ver docs/release.md para publicar no GitHub.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"

die() {
    printf 'build-release: error: %s\n' "$*" >&2
    exit 1
}
step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

ref=${1:-HEAD}
commit=$(git rev-parse --verify --quiet "$ref^{commit}") || die "commit/tag not found: $ref"
version=$(git show "$commit:Cargo.toml" |
    awk -F'"' '/^\[workspace.package\]/ {s = 1} s && /^version *=/ {print $2; exit}')
[[ -n $version ]] || die "could not read the version in the Cargo.toml of $ref"
if [[ $ref == v* && $ref != "v$version" ]]; then
    die "tag $ref does not match version $version in Cargo.toml"
fi
if [[ $ref == HEAD && -n $(git status --porcelain) ]]; then
    echo "warning: there are uncommitted changes; they are NOT part of the build" >&2
fi

strongswan=$(git show "$commit:native-engine/versions.txt" | grep -vE '^\s*(#|$)' | head -n 1 | awk '{print $1}')
engine="native-engine/dist/ipsec-client-engine-$strongswan-x86_64.tar.gz"
# Patches de segurança do commit (ADR-0019), no formato do BUILDINFO.
expected_patches=$(git ls-tree --name-only "$commit" native-engine/patches/ | grep '\.patch$' | sort |
    while read -r path; do
        printf '%s:%s\n' "$(basename "$path")" "$(git show "$commit:$path" | sha256sum | cut -c1-64)"
    done | paste -sd ' ')
engine_patches=""
[[ -f $engine ]] && engine_patches=$(tar -xzOf "$engine" opt/ipsec-client/engine/BUILDINFO | sed -n 's/^patches=//p')
if [[ ! -f $engine || $engine_patches != "$expected_patches" ]]; then
    step "building strongSwan $strongswan (with the patches of the commit)"
    native-engine/build.sh "$strongswan"
fi

rust=$(git show "$commit:rust-toolchain.toml" | awk -F'"' '/^channel/ {print $2}')
image="ipsec-client-release:rust$rust-$(sha256sum packaging/release/Dockerfile | cut -c1-12)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
    step "preparing the build image ($image)"
    docker build --build-arg "RUST_VERSION=$rust" -t "$image" packaging/release
fi

out="$repo/target/release-artifacts/$version"
rm -rf "$out"
mkdir -p "$out"

step "building AppImage, deb and rpm for version $version (commit ${commit:0:12})"
# Volumes nomeados guardam só downloads (crates, npm, ferramentas do AppImage);
# a compilação é sempre do zero.
git archive --format=tar "$commit" |
    docker run --rm -i \
        -v "$repo/$engine:/dist/engine.tar.gz:ro" \
        -v "$out:/out" \
        -v ipsec-client-release-cargo:/root/.cargo/registry \
        -v ipsec-client-release-npm:/root/.npm \
        -v ipsec-client-release-tauri:/root/.cache/tauri \
        -e "ENGINE_FILE=$(basename "$engine")" \
        -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
        "$image" bash -euo pipefail -c '
            tar -x -C /build
            install -D -m 0644 /dist/engine.tar.gz "native-engine/dist/$ENGINE_FILE"
            npm ci --ignore-scripts --no-audit --no-fund
            npm run bundle
            cp target/release/bundle/appimage/*.AppImage \
               target/release/bundle/deb/*.deb \
               target/release/bundle/rpm/*.rpm /out/
            chown -R "$HOST_UID:$HOST_GID" /out
            echo "==> checking the packages"
            scripts/verify-release.sh /out
        '

(cd "$out" && sha256sum -- *.AppImage *.deb *.rpm >SHA256SUMS)

# Aqui no host, onde estão os padrões privados (o container não os vê):
# nada do mantenedor pode ter entrado nos pacotes.
step "looking for private data inside the packages"
scripts/check-private.sh --artifacts "$out"
step "packages ready in ${out#"$repo"/}"
ls -lh "$out"
