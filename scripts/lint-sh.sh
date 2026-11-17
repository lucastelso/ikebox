#!/usr/bin/env bash
# lint-sh.sh — roda o shellcheck nos scripts shell do repositório.
#
# The shellcheck version is pinned (SHELLCHECK_VERSION): codes and checks
# change between versions (the "unreachable function" warning is SC2317 in
# 0.9 and SC2329 in 0.11), and the CI runner ships an older one than
# developers' computers. The system shellcheck is used only when it is that
# version; otherwise the official Docker image of it, pinned by digest, with
# no network and the repository read-only.
#
# Uso: scripts/lint-sh.sh [arquivo...]   (sem argumentos: todos os *.sh do repo)

set -euo pipefail

readonly SHELLCHECK_VERSION=0.11.0
readonly SHELLCHECK_IMAGE="koalaman/shellcheck:v$SHELLCHECK_VERSION@sha256:61862eba1fcf09a484ebcc6feea46f1782532571a34ed51fedf90dd25f925a8d"

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

if (($# > 0)); then
    files=("$@")
else
    # Rastreados + novos (não ignorados).
    mapfile -t files < <(git ls-files --cached --others --exclude-standard -- '*.sh')
fi
((${#files[@]} > 0)) || exit 0

system_version=""
if command -v shellcheck >/dev/null 2>&1; then
    system_version=$(shellcheck --version | awk '/^version:/ { print $2 }')
fi
if [[ $system_version == "$SHELLCHECK_VERSION" ]]; then
    exec shellcheck "${files[@]}"
elif command -v docker >/dev/null 2>&1; then
    printf 'lint-sh: shellcheck %s not found (system: %s); using %s\n' \
        "$SHELLCHECK_VERSION" "${system_version:-none}" "${SHELLCHECK_IMAGE%%@*}" >&2
    exec docker run --rm --network none \
        -v "$repo_root:/mnt:ro" -w /mnt \
        "$SHELLCHECK_IMAGE" "${files[@]}"
else
    printf 'lint-sh: install shellcheck %s (or Docker).\n' "$SHELLCHECK_VERSION" >&2
    exit 1
fi
