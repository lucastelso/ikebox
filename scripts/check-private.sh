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

if [[ ! -s "$patterns_file" ]]; then
    # A CI não tem (nem deve ter) os padrões privados: sai em silêncio.
    if [[ -z "${CI:-}" ]]; then
        printf 'check-private: aviso: nenhum padrão em %s (veja CONTRIBUTING.md)\n' \
            "$patterns_file" >&2
    fi
    exit 0
fi

# Remove comentários e linhas vazias num arquivo temporário privado.
umask 077
clean_patterns=$(mktemp)
trap 'rm -f "$clean_patterns"' EXIT
grep -vE '^[[:space:]]*(#|$)' "$patterns_file" >"$clean_patterns" || true
[[ -s "$clean_patterns" ]] || exit 0

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

mode=${1:---staged}
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
-h | --help)
    sed -n '2,18p' "$0"
    ;;
*)
    die "opção desconhecida: $mode (use --help)"
    ;;
esac
