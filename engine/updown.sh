#!/usr/bin/env bash
# updown do ipsec-client (ADR-0004).
#
# O charon chama este script (via /bin/sh -c) a cada par de traffic selectors
# que sobe ou desce. Ele roda como root no namespace de rede do host e:
#   - instala/remove a rota do TS remoto na interface XFRM dedicada;
#   - configura o DNS split da interface no systemd-resolved do host.
#
# Argumentos (gerados e validados pelo app; validados de novo aqui):
#   --iface NOME            interface XFRM da sessão (obrigatório)
#   --dns IP                servidor DNS fixo (repetível)
#   --domain DOMINIO        domínio de busca/roteamento; "~dominio" só roteia
#                           consultas, "~." manda todas (repetível)
#   --use-received-dns      usa também os DNS enviados pelo gateway
#   --dns-default-route     usa estes DNS para consultas fora dos domínios
#
# Ambiente fornecido pelo charon: PLUTO_VERB, PLUTO_PEER, PLUTO_PEER_CLIENT,
# PLUTO_MY_SOURCEIP*, PLUTO_DNS4_<n>, PLUTO_DNS6_<n>.
#
# A saída vai para o log do charon. Este script nunca recebe segredos.

set -euo pipefail

# Marca as rotas da sessão; o entrypoint remove o que sobrar na limpeza.
readonly RT_PROTO=241

log() { printf 'ipsec-client-updown: %s\n' "$*"; }
die() {
    log "erro: $*"
    exit 1
}

is_ipv4() {
    local IFS=. octet
    [[ $1 =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]] || return 1
    # shellcheck disable=SC2086 # divisão proposital pelos pontos
    for octet in $1; do
        ((10#$octet <= 255)) || return 1
    done
}
is_ipv6() { [[ $1 =~ ^[0-9A-Fa-f:.]{2,45}$ && $1 == *:* ]]; }
is_ip() { is_ipv4 "$1" || is_ipv6 "$1"; }
is_cidr() {
    local addr=${1%/*} prefix=${1##*/}
    [[ $1 == */* && $prefix =~ ^[0-9]{1,3}$ ]] || return 1
    if is_ipv4 "$addr"; then
        ((10#$prefix <= 32))
    else
        is_ipv6 "$addr" && ((10#$prefix <= 128))
    fi
}
is_domain() {
    [[ $1 == "~." ]] && return 0
    [[ $1 =~ ^~?[A-Za-z0-9]([A-Za-z0-9-]{0,62})?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,62})?)*\.?$ ]] &&
        ((${#1} <= 254))
}

# --- argumentos -------------------------------------------------------------
iface=""
static_dns=()
domains=()
use_received_dns=0
dns_default_route=0

while (($# > 0)); do
    case $1 in
    --iface | --dns | --domain)
        (($# >= 2)) || die "$1 exige um valor"
        case $1 in
        --iface) iface=$2 ;;
        --dns)
            is_ip "$2" || die "--dns inválido: $2"
            static_dns+=("$2")
            ;;
        --domain)
            is_domain "$2" || die "--domain inválido: $2"
            domains+=("$2")
            ;;
        esac
        shift 2
        ;;
    --use-received-dns)
        use_received_dns=1
        shift
        ;;
    --dns-default-route)
        dns_default_route=1
        shift
        ;;
    *) die "argumento desconhecido: $1" ;;
    esac
done

[[ $iface =~ ^[a-z][a-z0-9_-]{0,14}$ ]] || die "--iface ausente ou inválido"

# --- ambiente do charon -----------------------------------------------------
verb=${PLUTO_VERB:-}
peer=${PLUTO_PEER:-}
peer_client=${PLUTO_PEER_CLIENT:-}

if [[ $verb == *-v6 ]]; then
    family=-6
    src=${PLUTO_MY_SOURCEIP6_1:-}
    full_tunnel_routes=("::/1" "8000::/1")
    host_prefix=128
else
    family=-4
    src=${PLUTO_MY_SOURCEIP4_1:-${PLUTO_MY_SOURCEIP:-}}
    full_tunnel_routes=("0.0.0.0/1" "128.0.0.0/1")
    host_prefix=32
fi
if [[ -n $src ]] && ! is_ip "$src"; then
    die "PLUTO_MY_SOURCEIP inválido: $src"
fi

src_args=()
[[ -n $src ]] && src_args=(src "$src")

is_full_tunnel() { [[ $peer_client == "0.0.0.0/0" || $peer_client == "::/0" ]]; }

# --- rotas ------------------------------------------------------------------

# No túnel completo, o próprio gateway precisa continuar alcançável pelo
# caminho original. Sem isso, o tráfego IKE/ESP entraria no túnel (loop).
add_gateway_host_route() {
    is_ip "$peer" || die "PLUTO_PEER inválido: $peer"
    if [[ -n $(ip "$family" route show "$peer/$host_prefix" proto "$RT_PROTO") ]]; then
        return 0 # já existe (rekey/reconexão)
    fi
    local route via dev
    route=$(ip "$family" route get "$peer" 2>/dev/null | head -n 1)
    via=$(awk '{ for (i = 1; i < NF; i++) if ($i == "via") { print $(i + 1); exit } }' <<<"$route")
    dev=$(awk '{ for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit } }' <<<"$route")
    [[ -n $dev && $dev != "$iface" ]] || die "não achei o caminho original até o gateway"
    local args=("$peer/$host_prefix")
    [[ -n $via ]] && args+=(via "$via")
    args+=(dev "$dev" proto "$RT_PROTO")
    ip "$family" route replace "${args[@]}"
    log "rota de host para o gateway $peer via ${via:-link} dev $dev"
}

route_up() {
    is_cidr "$peer_client" || die "PLUTO_PEER_CLIENT inválido: $peer_client"
    if [[ ${peer_client%/*} == "$peer" ]]; then
        log "TS remoto é o próprio gateway ($peer_client): rota ignorada"
        return 0
    fi
    if is_full_tunnel; then
        add_gateway_host_route
        local r
        for r in "${full_tunnel_routes[@]}"; do
            ip "$family" route replace "$r" dev "$iface" "${src_args[@]}" proto "$RT_PROTO"
        done
        log "túnel completo: rotas ${full_tunnel_routes[*]} via $iface"
    else
        ip "$family" route replace "$peer_client" dev "$iface" "${src_args[@]}" proto "$RT_PROTO"
        log "rota $peer_client via $iface${src:+ (src $src)}"
    fi
}

route_down() {
    is_cidr "$peer_client" || return 0
    if is_full_tunnel; then
        local r
        for r in "${full_tunnel_routes[@]}"; do
            ip "$family" route del "$r" dev "$iface" proto "$RT_PROTO" 2>/dev/null || true
        done
        if is_ip "$peer"; then
            ip "$family" route del "$peer/$host_prefix" proto "$RT_PROTO" 2>/dev/null || true
        fi
    else
        ip "$family" route del "$peer_client" dev "$iface" proto "$RT_PROTO" 2>/dev/null || true
    fi
    log "rota $peer_client removida"
}

# --- DNS --------------------------------------------------------------------

dns_up() {
    local servers=("${static_dns[@]}") i var value
    if ((use_received_dns)); then
        for var in PLUTO_DNS4_ PLUTO_DNS6_; do
            for i in $(seq 1 8); do
                value=$(printenv "$var$i" || true)
                [[ -n $value ]] || break
                if is_ip "$value"; then
                    servers+=("$value")
                else
                    log "aviso: DNS recebido inválido ignorado"
                fi
            done
        done
    fi

    if ((${#servers[@]} == 0 && ${#domains[@]} == 0)); then
        return 0
    fi

    # Falha de DNS não derruba o túnel: só registra.
    if ((${#servers[@]} > 0)); then
        resolvectl dns "$iface" "${servers[@]}" || log "aviso: falha ao configurar servidores DNS"
    fi
    if ((${#domains[@]} > 0)); then
        resolvectl domain "$iface" "${domains[@]}" || log "aviso: falha ao configurar domínios DNS"
    fi
    if ((dns_default_route)); then
        resolvectl default-route "$iface" yes || true
    else
        resolvectl default-route "$iface" no || true
    fi
    resolvectl flush-caches || true
    log "DNS em $iface: servidores [${servers[*]}] domínios [${domains[*]}]"
}

dns_down() {
    # Com vários TS por conexão, só reverte quando a última rota sair.
    if [[ -z "$(ip -4 route show dev "$iface" proto "$RT_PROTO" 2>/dev/null)$(ip -6 route show dev "$iface" proto "$RT_PROTO" 2>/dev/null)" ]]; then
        resolvectl revert "$iface" 2>/dev/null || true
        resolvectl flush-caches 2>/dev/null || true
        log "DNS de $iface revertido"
    fi
}

# --- principal --------------------------------------------------------------
case $verb in
up-client | up-client-v6)
    route_up
    dns_up
    ;;
down-client | down-client-v6)
    route_down
    dns_down
    ;;
up-host | up-host-v6 | down-host | down-host-v6)
    log "túnel host-a-host ($verb): nada a fazer"
    ;;
*)
    log "verbo ignorado: ${verb:-<vazio>}"
    ;;
esac
