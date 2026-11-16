#!/usr/bin/env bash
# lint-sh.sh — roda o shellcheck nos scripts shell do repositório.
#
# Usa o shellcheck instalado no sistema. Se não houver, usa a imagem Docker
# oficial, fixada por versão e digest, sem rede e com o repositório só leitura.
#
# Uso: scripts/lint-sh.sh [arquivo...]   (sem argumentos: todos os *.sh do repo)

set -euo pipefail

SHELLCHECK_IMAGE="koalaman/shellcheck:v0.11.0@sha256:61862eba1fcf09a484ebcc6feea46f1782532571a34ed51fedf90dd25f925a8d"

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

if (($# > 0)); then
    files=("$@")
else
    # Rastreados + novos (não ignorados).
    mapfile -t files < <(git ls-files --cached --others --exclude-standard -- '*.sh')
fi
((${#files[@]} > 0)) || exit 0

if command -v shellcheck >/dev/null 2>&1; then
    exec shellcheck "${files[@]}"
elif command -v docker >/dev/null 2>&1; then
    printf 'lint-sh: shellcheck not found; using %s\n' "${SHELLCHECK_IMAGE%%@*}" >&2
    exec docker run --rm --network none \
        -v "$repo_root:/mnt:ro" -w /mnt \
        "$SHELLCHECK_IMAGE" "${files[@]}"
else
    printf 'lint-sh: instale o shellcheck (ou o Docker).\n' >&2
    exit 1
fi
