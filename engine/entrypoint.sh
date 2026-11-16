#!/usr/bin/env bash
# Entrypoint do engine ipsec-client.
#
# 1. Valida o ambiente (só dados não sensíveis: os segredos chegam via VICI).
# 2. Limpa restos de uma sessão anterior que tenha morrido de forma abrupta.
# 3. Cria a interface XFRM dedicada (ADR-0004) e escreve a config de runtime.
# 4. Sobe o charon e libera o socket VICI para o usuário (root:<gid>, 0660).
# 5. No SIGTERM (docker stop): encerra o charon e desfaz tudo.
#
# Variáveis (ver docs/arquitetura.md):
#   IPSEC_CLIENT_IFACE       nome da interface XFRM          (padrão: ipsc0)
#   IPSEC_CLIENT_IF_ID       if_id das SAs/políticas, 1..2^32-1 (obrigatória)
#   IPSEC_CLIENT_MTU         MTU da interface, 576..9000      (padrão: 1400)
#   IPSEC_CLIENT_SOCKET_GID  gid que pode usar o socket VICI  (obrigatória)
#   IPSEC_CLIENT_LOG_LEVEL   nível de log do charon, 0..2     (padrão: 1)
#   IPSEC_CLIENT_CHARON      caminho do charon   (padrão: o do pacote do Ubuntu)
#   IPSEC_CLIENT_STRONGSWAN_D diretório strongswan.d (padrão: /etc/strongswan.d)

set -euo pipefail

readonly SOCKET_DIR=/run/ipsec-client
readonly SOCKET="$SOCKET_DIR/charon.vici"
readonly CHARON=${IPSEC_CLIENT_CHARON:-/usr/lib/ipsec/charon}
readonly RUNTIME_CONF=${IPSEC_CLIENT_STRONGSWAN_D:-/etc/strongswan.d}/zzz-ipsec-client-runtime.conf
# Protocolo de rota usado pelo updown para marcar as rotas da sessão.
readonly RT_PROTO=241

IFACE=${IPSEC_CLIENT_IFACE:-ipsc0}
IF_ID=${IPSEC_CLIENT_IF_ID:-}
MTU=${IPSEC_CLIENT_MTU:-1400}
SOCKET_GID=${IPSEC_CLIENT_SOCKET_GID:-}
LOG_LEVEL=${IPSEC_CLIENT_LOG_LEVEL:-1}

charon_pid=""

log() { printf '[engine] %s\n' "$*" >&2; }
die() {
    log "erro: $*"
    exit 1
}

# uint_in_range VALOR MIN MAX: inteiro decimal sem sinal dentro do intervalo.
uint_in_range() {
    [[ $1 =~ ^[0-9]{1,10}$ ]] || return 1
    ((10#$1 >= $2 && 10#$1 <= $3))
}

validate_env() {
    [[ $IFACE =~ ^[a-z][a-z0-9_-]{0,14}$ ]] ||
        die "IPSEC_CLIENT_IFACE inválido (use ^[a-z][a-z0-9_-]{0,14}\$)"
    uint_in_range "$IF_ID" 1 4294967295 ||
        die "IPSEC_CLIENT_IF_ID inválido (inteiro 1-4294967295)"
    uint_in_range "$MTU" 576 9000 ||
        die "IPSEC_CLIENT_MTU inválido (inteiro 576-9000)"
    uint_in_range "$SOCKET_GID" 0 2147483647 ||
        die "IPSEC_CLIENT_SOCKET_GID inválido"
    [[ $LOG_LEVEL =~ ^[0-2]$ ]] ||
        die "IPSEC_CLIENT_LOG_LEVEL deve ser 0, 1 ou 2"
    [[ -d $SOCKET_DIR ]] ||
        die "$SOCKET_DIR não está montado (bind do diretório do socket VICI)"
    # Normaliza zeros à esquerda.
    IF_ID=$((10#$IF_ID))
    MTU=$((10#$MTU))
    SOCKET_GID=$((10#$SOCKET_GID))
}

# Remove SAs e políticas XFRM marcadas com o nosso if_id que tenham sobrado de
# uma sessão encerrada de forma abrupta (SIGKILL). Nunca faz flush global:
# outros IPsec do host não são afetados. A saída do `ip xfrm state` contém
# chaves; ela só é lida pelo awk e nunca é impressa.
#
# As políticas só saem com um iproute2 que aceite `if_id` no delete (o do host,
# no engine embutido). Com o iproute2 5.5 do Ubuntu 20.04 (imagem Docker),
# elas ficam: são inertes, uma sessão com o mesmo VIP as substitui e elas
# somem no reboot. Ver docs/desenvolvimento.md.
flush_stale_xfrm() {
    local hex_id src dst proto spi stale_policies
    hex_id=$(printf '0x%x' "$IF_ID")

    # `ip -o` separa os blocos com "\": troca por espaço antes de ler os campos.
    while read -r src dst proto spi; do
        [[ -n $spi ]] || continue
        ip xfrm state delete src "$src" dst "$dst" proto "$proto" spi "$spi" 2>/dev/null || true
    done < <(ip -o xfrm state 2>/dev/null | awk -v want="$hex_id" '
        {
            gsub(/\\/, " ")
            ok = 0; s = ""; d = ""; p = ""; spi = ""
            for (i = 1; i < NF; i++) {
                if ($i == "if_id" && tolower($(i + 1)) == want) ok = 1
                if ($i == "src" && s == "") s = $(i + 1)
                if ($i == "dst" && d == "") d = $(i + 1)
                if ($i == "proto" && p == "") p = $(i + 1)
                if ($i == "spi" && spi == "") spi = $(i + 1)
            }
            if (ok) print s, d, p, spi
        }')

    # Políticas: o iproute2 moderno (host) aceita if_id no delete; o 5.5 do
    # Ubuntu 20.04 não, e aí elas ficam (inertes) e só geram o aviso abaixo.
    while read -r src dst dir; do
        [[ -n $dir ]] || continue
        ip xfrm policy delete src "$src" dst "$dst" dir "$dir" if_id "$IF_ID" 2>/dev/null || true
    done < <(ip -o xfrm policy 2>/dev/null | awk -v want="$hex_id" '
        {
            gsub(/\\/, " ")
            ok = 0; s = ""; d = ""; dir = ""
            for (i = 1; i < NF; i++) {
                if ($i == "if_id" && tolower($(i + 1)) == want) ok = 1
                if ($i == "src" && s == "") s = $(i + 1)
                if ($i == "dst" && d == "") d = $(i + 1)
                if ($i == "dir" && dir == "") dir = $(i + 1)
            }
            if (ok) print s, d, dir
        }')

    stale_policies=$(ip -o xfrm policy 2>/dev/null |
        grep -ciE "if_id ${hex_id}([^0-9a-f]|$)" || true)
    if ((stale_policies > 0)); then
        log "aviso: $stale_policies política(s) XFRM órfã(s) com if_id $hex_id (inertes; somem no reboot)"
    fi
}

remove_iface() {
    resolvectl revert "$IFACE" >/dev/null 2>&1 || true
    ip link del "$IFACE" >/dev/null 2>&1 || true
}

# Desfaz tudo o que a sessão criou no host.
cleanup() {
    remove_iface
    # Rotas fora da interface (ex.: rota de host para o gateway no modo túnel
    # completo) são marcadas pelo updown com o protocolo RT_PROTO.
    ip -4 route flush proto "$RT_PROTO" >/dev/null 2>&1 || true
    ip -6 route flush proto "$RT_PROTO" >/dev/null 2>&1 || true
    flush_stale_xfrm
    rm -f "$SOCKET"
}

create_iface() {
    if ! ip link add "$IFACE" type xfrm if_id "$IF_ID" 2>/dev/null; then
        # Kernels/iproute2 mais antigos exigem um dispositivo subjacente.
        local dev
        dev=$(ip -4 route show default | awk '{ for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit } }')
        [[ -n $dev ]] || die "não foi possível criar $IFACE (o módulo xfrm_interface está disponível?)"
        ip link add "$IFACE" type xfrm dev "$dev" if_id "$IF_ID" ||
            die "não foi possível criar $IFACE"
    fi
    ip link set "$IFACE" mtu "$MTU" up
    log "interface $IFACE criada (if_id $IF_ID, MTU $MTU)"
}

write_runtime_conf() {
    cat >"$RUNTIME_CONF" <<EOF
# Gerado pelo entrypoint do ipsec-client. Não editar.
charon {
    install_virtual_ip_on = $IFACE
    filelog {
        stderr {
            default = $LOG_LEVEL
        }
    }
}
EOF
}

wait_for_socket() {
    local _
    for _ in $(seq 1 100); do
        [[ -S $SOCKET ]] && return 0
        kill -0 "$charon_pid" 2>/dev/null || die "o charon terminou antes de abrir o socket VICI"
        sleep 0.1
    done
    die "o socket VICI não apareceu em 10 s"
}

# Chamada pelo trap de TERM/INT (docker stop).
# shellcheck disable=SC2329
shutdown() {
    trap - TERM INT
    log "encerrando..."
    if [[ -n $charon_pid ]] && kill -0 "$charon_pid" 2>/dev/null; then
        # O charon encerra as SAs (envia DELETE ao gateway) e remove o VIP.
        kill -TERM "$charon_pid" 2>/dev/null || true
        wait "$charon_pid" 2>/dev/null || true
    fi
    cleanup
    log "limpeza concluída"
    exit 0
}

main() {
    validate_env
    trap shutdown TERM INT

    local version
    version=$("$CHARON" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true)
    log "preparando (strongSwan ${version:-?})"
    cleanup
    create_iface
    write_runtime_conf
    rm -f /var/run/charon.pid /run/charon.pid

    "$CHARON" &
    charon_pid=$!

    wait_for_socket
    chgrp "$SOCKET_GID" "$SOCKET"
    chmod 0660 "$SOCKET"
    log "pronto: socket VICI liberado para o gid $SOCKET_GID"

    local status=0
    wait "$charon_pid" || status=$?
    # Só chega aqui se o charon morrer sozinho (o SIGTERM sai pelo trap).
    log "o charon terminou inesperadamente (status $status)"
    charon_pid=""
    cleanup
    exit "$status"
}

main "$@"
