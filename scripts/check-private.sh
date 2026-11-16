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

repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || die "not a git repository"
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
        printf 'check-private: warning: no patterns in %s (see CONTRIBUTING.md)\n' \
            "$patterns_file" >&2
    fi
    exit 0
fi

fail() {
    printf '\ncheck-private: PRIVATE DATA FOUND (%s).\n' "$1" >&2
    printf 'Replace it with a generic value (e.g. 192.0.2.0/24, vpn.example.com).\n' >&2
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
    *) die "git grep failed (invalid pattern in $patterns_file?)" ;;
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
        done) || die "could not extract $pkg"
        ;;
    *.rpm)
        # O rpm2cpio do Ubuntu 22.04 sai com 1 mesmo extraindo tudo.
        (cd "$dest" && { rpm2cpio "$pkg" || true; } | cpio -idm --quiet) ||
            die "could not extract $pkg"
        ;;
    *.AppImage)
        (cd "$dest" && "$pkg" --appimage-extract >/dev/null) || die "could not extract $pkg"
        ;;
    esac
    while IFS= read -r -d '' nested; do
        mkdir -p "$nested.d"
        tar -xzf "$nested" -C "$nested.d" || die "could not extract $nested"
    done < <(find "$dest" -type f -name '*.tar.gz' -print0)
    while IFS= read -r -d '' nested; do
        gzip -dc "$nested" >"${nested%.gz}.decompressed" || die "could not decompress $nested"
    done < <(find "$dest" -type f -name '*.gz' ! -name '*.tar.gz' -print0)
}

# scan_patterns <caminho>...: arquivos em que os padrões privados aparecem.
# Em executáveis ELF, só o texto legível (strings de 4+ caracteres) conta: um
# padrão curto casa por acaso com bytes de código de máquina (aconteceu na
# 0.1.1: 3 bytes isolados na seção .text do app). Um dado de verdade embutido
# no binário fica em texto legível.
scan_patterns() {
    local file
    while IFS= read -r -d '' file; do
        if [[ $(head -c 4 "$file" | od -An -tx1 | tr -d ' \n') == 7f454c46 ]]; then
            # Process substitution: only grep decides. A pipe under pipefail
            # would turn a match into SIGPIPE on strings and hide the file.
            grep -q -i -E -f "$clean_patterns" < <(strings -a -n 4 "$file") && printf '%s\n' "$file"
        else
            grep -q -a -i -E -f "$clean_patterns" "$file" && printf '%s\n' "$file"
        fi
    done < <(find "$@" -type f -print0)
    return 0
}

case "$mode" in
--staged)
    scan "staged content" 1,2 --cached -- .
    ;;
--tracked)
    scan "tracked files" 1,2 -- .
    ;;
--history)
    mapfile -t revs < <(git rev-list --all 2>/dev/null || true)
    ((${#revs[@]} > 0)) || exit 0
    scan "git history" 1-3 "${revs[@]}" -- .
    bad_commits=()
    for rev in "${revs[@]}"; do
        msg=$(git log -1 --format=%B "$rev")
        if grep -q -i -E -f "$clean_patterns" <<<"$msg"; then
            bad_commits+=("$rev")
        fi
    done
    if ((${#bad_commits[@]} > 0)); then
        printf '  - commit message %s\n' "${bad_commits[@]}" >&2
        fail "commit messages"
    fi
    ;;
--message)
    msg_file=${2:-}
    [[ -f "$msg_file" ]] || die "usage: $0 --message <file>"
    # Ignora as linhas de comentário que o git coloca no editor.
    msg=$(grep -v '^#' "$msg_file" || true)
    if grep -q -i -E -f "$clean_patterns" <<<"$msg"; then
        fail "commit message"
    fi
    ;;
--artifacts)
    dir=$(cd "${2:?usage: $0 --artifacts <directory with the packages>}" && pwd)
    shopt -s nullglob
    packages=("$dir"/*.AppImage "$dir"/*.deb "$dir"/*.rpm)
    ((${#packages[@]} > 0)) || die "no packages in $dir"
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
            mapfile -t -O "${#hits[@]}" hits < <(scan_patterns "${own[@]}")
        fi
        mapfile -t -O "${#hits[@]}" hits < <(grep -r -a -l -F -i "${fixed[@]/#/-e}" "$dest" || true)
        if ((${#hits[@]} > 0)); then
            printf '%s\n' "${hits[@]#"$work"/}" | sort -u | sed 's/^/  - /' >&2
            found+=("$(basename "$pkg")")
        else
            printf '  ✔ %s: no private data\n' "$(basename "$pkg")"
        fi
    done
    if [[ ! -s $clean_patterns ]] && [[ -z ${CI:-} ]]; then
        printf 'check-private: warning: no patterns in %s; only the home path and the hostname were searched\n' \
            "$patterns_file" >&2
    fi
    ((${#found[@]} == 0)) || fail "inside the packages: ${found[*]}"
    ;;
-h | --help)
    sed -n '2,20p' "$0"
    ;;
*)
    die "unknown option: $mode (use --help)"
    ;;
esac
