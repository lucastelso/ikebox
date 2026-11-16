#!/usr/bin/env bash
# Confere os pacotes de release antes de publicar:
#
# - em cada pacote (AppImage, deb e rpm), o componente de rede embutido
#   precisa bater com o próprio SHA256SUMS. O app recusa instalar um
#   componente que não bate, e o empacotamento já alterou arquivos antes (o
#   linuxdeploy reescreve o RPATH de executáveis dentro do AppImage);
# - o AppImage não pode exigir uma glibc mais nova que a do Ubuntu 22.04.
#
# Uso: scripts/verify-release.sh <diretório com os pacotes>
# Precisa de: binutils (ar, objdump), rpm2cpio e cpio. Roda sozinho no fim do
# scripts/build-release.sh.

set -euo pipefail

readonly MAX_GLIBC=2.35

ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

dir=$(cd "${1:?uso: verify-release.sh <diretório com os pacotes>}" && pwd)
shopt -s nullglob
appimages=("$dir"/*.AppImage)
debs=("$dir"/*.deb)
rpms=("$dir"/*.rpm)
((${#appimages[@]} == 1 && ${#debs[@]} == 1 && ${#rpms[@]} == 1)) ||
    fail "esperava um AppImage, um deb e um rpm em $dir"

work=$(mktemp -d)
# shellcheck disable=SC2064 # caminho fixado agora
trap "rm -rf '$work'" EXIT

component_ok() {
    local name=$2 net="$1/usr/lib/ipsec-client/network"
    [[ -f $net/SHA256SUMS ]] || fail "$name: componente de rede ausente"
    (cd "$net" && sha256sum --check --strict --quiet SHA256SUMS) ||
        fail "$name: o componente de rede não bate com o SHA256SUMS"
    ok "$name: componente de rede íntegro ($(grep -h '^version=' "$net/VERSION"))"
}

mkdir "$work/appimage" "$work/deb" "$work/rpm"
(cd "$work/appimage" && "${appimages[0]}" --appimage-extract >/dev/null) ||
    fail "AppImage: não consegui extrair o pacote"
component_ok "$work/appimage/squashfs-root" "AppImage"
glibc=$(objdump -T "$work/appimage/squashfs-root/usr/bin/ipsec-client" |
    grep -oE 'GLIBC_[0-9.]+' | sed 's/GLIBC_//' | sort -Vu | tail -n 1)
printf '%s\n%s\n' "$glibc" "$MAX_GLIBC" | sort -VC ||
    fail "AppImage: exige glibc $glibc (o limite é $MAX_GLIBC, do Ubuntu 22.04)"
ok "AppImage: exige glibc $glibc (limite $MAX_GLIBC)"

(cd "$work/deb" && ar x "${debs[0]}" && tar -xf data.tar.*) ||
    fail "deb: não consegui extrair o pacote"
component_ok "$work/deb" "deb"

# O rpm2cpio do Ubuntu 22.04 extrai os pacotes do Tauri por inteiro, mas sai
# com 1. Quem decide é a conferência do componente logo abaixo: uma extração
# incompleta falha nela.
(cd "$work/rpm" && { rpm2cpio "${rpms[0]}" || true; } | cpio -idm --quiet) ||
    fail "rpm: não consegui extrair o pacote"
component_ok "$work/rpm" "rpm"
