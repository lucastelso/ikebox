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
#   SHA256SUMS.
#
# Ver docs/release.md para publicar no GitHub.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"

die() {
    printf 'build-release: erro: %s\n' "$*" >&2
    exit 1
}
step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

ref=${1:-HEAD}
commit=$(git rev-parse --verify --quiet "$ref^{commit}") || die "commit/tag não encontrado: $ref"
version=$(git show "$commit:Cargo.toml" |
    awk -F'"' '/^\[workspace.package\]/ {s = 1} s && /^version *=/ {print $2; exit}')
[[ -n $version ]] || die "não consegui ler a versão no Cargo.toml de $ref"
if [[ $ref == v* && $ref != "v$version" ]]; then
    die "a tag $ref não bate com a versão $version do Cargo.toml"
fi
if [[ $ref == HEAD && -n $(git status --porcelain) ]]; then
    echo "aviso: há mudanças não commitadas; elas NÃO entram no build" >&2
fi

strongswan=$(git show "$commit:native-engine/versions.txt" | grep -vE '^\s*(#|$)' | head -n 1 | awk '{print $1}')
engine="native-engine/dist/ipsec-client-engine-$strongswan-x86_64.tar.gz"
if [[ ! -f $engine ]]; then
    step "compilando o strongSwan $strongswan (uma vez)"
    native-engine/build.sh "$strongswan"
fi

rust=$(git show "$commit:rust-toolchain.toml" | awk -F'"' '/^channel/ {print $2}')
image="ipsec-client-release:rust$rust-$(sha256sum packaging/release/Dockerfile | cut -c1-12)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
    step "preparando a imagem de build ($image)"
    docker build --build-arg "RUST_VERSION=$rust" -t "$image" packaging/release
fi

out="$repo/target/release-artifacts/$version"
rm -rf "$out"
mkdir -p "$out"

step "gerando AppImage, deb e rpm da versão $version (commit ${commit:0:12})"
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
        '

(cd "$out" && sha256sum -- *.AppImage *.deb *.rpm >SHA256SUMS)
step "pacotes prontos em ${out#"$repo"/}"
ls -lh "$out"
