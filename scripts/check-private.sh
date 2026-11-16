#!/usr/bin/env bash
# check-private.sh — impede que dados privados (do mantenedor ou da organização
# dele) entrem no repositório.
#
# Os padrões NÃO ficam no repositório: são lidos de um arquivo local, por padrão
# `.git/info/private-patterns` (uma ERE por linha, `#` para comentários), ou do
# caminho em $IPSEC_CLIENT_PRIVATE_PATTERNS.
#
# Uso:
#   scripts/check-private.sh              # verifica o conteúdo staged (o índice)
#   scripts/check-private.sh --tracked    # verifica a árvore de trabalho rastreada
#   scripts/check-private.sh --history    # verifica todo o histórico (antes de push)
#   scripts/check-private.sh --message F  # verifica uma mensagem de commit
#   scripts/check-private.sh --artifacts D  # varre os pacotes de release em D
#                                           # (AppImage, deb, rpm), por dentro
#
# Saída: 0 = limpo (ou sem padrões configurados), 1 = encontrou dado privado,
#        2 = erro.
# Nunca imprime o trecho que casou, só a localização, para não espalhar o dado
# privado por logs de terminal ou de CI.

set -euo pipefail

die() {
    printf 'check-private: %s\n' "$*" >&2
    exit 2
}

repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || die "não é um repositório git"
cd "$repo_root"

patterns_file=${IPSEC_CLIENT_PRIVATE_PATTERNS:-$(git rev-parse --git-path info/private-patterns)}

mode=${1:---staged}

# Remove comentários e linhas vazias num arquivo temporário privado.
umask 077
clean_patterns=$(mktemp)
work=""
trap 'rm -f "$clean_patterns"; [[ -n $work ]] && rm -rf "$work"' EXIT
if [[ -s "$patterns_file" ]]; then
    grep -vE '^[[:space:]]*(#|$)' "$patterns_file" >"$clean_patterns" || true
fi

if [[ ! -s "$clean_patterns" && $mode != --artifacts ]]; then
    # A CI não tem (nem deve ter) os padrões privados: sai em silêncio.
    if [[ -z "${CI:-}" ]]; then
        printf 'check-private: aviso: nenhum padrão em %s (veja CONTRIBUTING.md)\n' \
            "$patterns_file" >&2
    fi
    exit 0
fi

fail() {
    printf '\ncheck-private: DADO PRIVADO ENCONTRADO (%s).\n' "$1" >&2
    printf 'Troque por um valor genérico (ex.: 192.0.2.0/24, vpn.example.com).\n' >&2
    exit 1
}

# scan <rótulo> <campos-de-localização> <args do git grep...>
# git grep sai com 0 quando acha, 1 quando não acha e >1 em erro.
scan() {
    local label=$1 fields=$2 out rc=0
    shift 2
    out=$(git grep -I -n -i -E -f "$clean_patterns" "$@" 2>&1) || rc=$?
    case "$rc" in
    0)
        printf '%s\n' "$out" | cut -d: -f"$fields" | sort -u | sed 's/^/  - /' >&2
        fail "$label"
        ;;
    1) ;;
    *) die "git grep falhou (padrão inválido em $patterns_file?)" ;;
    esac
}

# extract_package <pacote> <destino>: o conteúdo inteiro, inclusive o
# strongSwan (engine.tar.gz) e o helper (.gz) que vão compactados dentro.
extract_package() {
    local pkg=$1 dest=$2 data nested
    mkdir -p "$dest"
    case $pkg in
    *.deb)
        (cd "$dest" && ar x "$pkg" && for data in control.tar.* data.tar.*; do
            tar -xf "$data" && rm -f "$data"
        done) || die "não consegui extrair $pkg"
        ;;
    *.rpm)
        # O rpm2cpio do Ubuntu 22.04 sai com 1 mesmo extraindo tudo.
        (cd "$dest" && { rpm2cpio "$pkg" || true; } | cpio -idm --quiet) ||
            die "não consegui extrair $pkg"
        ;;
    *.AppImage)
        (cd "$dest" && "$pkg" --appimage-extract >/dev/null) || die "não consegui extrair $pkg"
        ;;
    esac
    while IFS= read -r -d '' nested; do
        mkdir -p "$nested.d"
        tar -xzf "$nested" -C "$nested.d" || die "não consegui extrair $nested"
    done < <(find "$dest" -type f -name '*.tar.gz' -print0)
    while IFS= read -r -d '' nested; do
        gzip -dc "$nested" >"${nested%.gz}.descompactado" || die "não consegui descompactar $nested"
    done < <(find "$dest" -type f -name '*.gz' ! -name '*.tar.gz' -print0)
}

case "$mode" in
--staged)
    scan "conteúdo staged" 1,2 --cached -- .
    ;;
--tracked)
    scan "arquivos rastreados" 1,2 -- .
    ;;
--history)
    mapfile -t revs < <(git rev-list --all 2>/dev/null || true)
    ((${#revs[@]} > 0)) || exit 0
    scan "histórico do git" 1-3 "${revs[@]}" -- .
    bad_commits=()
    for rev in "${revs[@]}"; do
        msg=$(git log -1 --format=%B "$rev")
        if grep -q -i -E -f "$clean_patterns" <<<"$msg"; then
            bad_commits+=("$rev")
        fi
    done
    if ((${#bad_commits[@]} > 0)); then
        printf '  - mensagem do commit %s\n' "${bad_commits[@]}" >&2
        fail "mensagens de commit"
    fi
    ;;
--message)
    msg_file=${2:-}
    [[ -f "$msg_file" ]] || die "uso: $0 --message <arquivo>"
    # Ignora as linhas de comentário que o git coloca no editor.
    msg=$(grep -v '^#' "$msg_file" || true)
    if grep -q -i -E -f "$clean_patterns" <<<"$msg"; then
        fail "mensagem de commit"
    fi
    ;;
--artifacts)
    dir=$(cd "${2:?uso: $0 --artifacts <diretório com os pacotes>}" && pwd)
    shopt -s nullglob
    packages=("$dir"/*.AppImage "$dir"/*.deb "$dir"/*.rpm)
    ((${#packages[@]} > 0)) || die "nenhum pacote em $dir"
    # Além dos padrões privados: o caminho da home e o nome desta máquina
    # (nomes genéricos ficam de fora, para não dar falso positivo).
    fixed=("/home/$(id -un)")
    host=$(hostname 2>/dev/null || true)
    case ${host,,} in
    "" | localhost* | fedora | ubuntu | debian | linux | toolbox) ;;
    *) ((${#host} >= 4)) && fixed+=("$host") ;;
    esac
    work=$(mktemp -d)
    found=()
    for pkg in "${packages[@]}"; do
        dest="$work/$(basename "$pkg")"
        extract_package "$pkg" "$dest"
        # Os padrões privados vão nos arquivos NOSSOS. O AppImage também traz
        # bibliotecas do Ubuntu 22.04, copiadas pelo linuxdeploy: padrões
        # curtos casam por acaso dentro delas (conferido: as mesmas
        # ocorrências existem nas bibliotecas originais), e nelas só se
        # procura o caminho da home e o hostname.
        own=("$dest")
        if [[ $pkg == *.AppImage ]]; then
            own=()
            for path in usr/bin usr/lib/ipsec-client usr/share/applications usr/share/icons; do
                [[ -e $dest/squashfs-root/$path ]] && own+=("$dest/squashfs-root/$path")
            done
            for path in "$dest"/squashfs-root/*.desktop; do own+=("$path"); done
        fi
        hits=()
        if [[ -s $clean_patterns ]]; then
            mapfile -t -O "${#hits[@]}" hits < <(grep -r -a -l -i -E -f "$clean_patterns" "${own[@]}" || true)
        fi
        mapfile -t -O "${#hits[@]}" hits < <(grep -r -a -l -F -i "${fixed[@]/#/-e}" "$dest" || true)
        if ((${#hits[@]} > 0)); then
            printf '%s\n' "${hits[@]#"$work"/}" | sort -u | sed 's/^/  - /' >&2
            found+=("$(basename "$pkg")")
        else
            printf '  ✔ %s: sem dados privados\n' "$(basename "$pkg")"
        fi
    done
    if [[ ! -s $clean_patterns ]] && [[ -z ${CI:-} ]]; then
        printf 'check-private: aviso: sem padrões em %s; só o caminho da home e o hostname foram procurados\n' \
            "$patterns_file" >&2
    fi
    ((${#found[@]} == 0)) || fail "dentro dos pacotes: ${found[*]}"
    ;;
-h | --help)
    sed -n '2,20p' "$0"
    ;;
*)
    die "opção desconhecida: $mode (use --help)"
    ;;
esac
