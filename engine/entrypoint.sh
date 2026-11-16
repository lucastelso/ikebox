#!/usr/bin/env bash
# Entrypoint do engine ipsec-client.
#
# 1. Valida o ambiente (só dados não sensíveis: os segredos chegam via VICI).
# 2. Limpa restos de uma sessão anterior que tenha morrido de forma abrupta.
# 3. Cria a interface XFRM dedicada (ADR-0004) e escreve a config de runtime.
# 4. Sobe o charon e libera o socket VICI para o usuário (dono <uid>, 0600;
#    ou root:<gid>, 0660, no engine Docker).
# 5. No SIGTERM (docker stop): encerra o charon e desfaz tudo.
#
# Variáveis (ver docs/arquitetura.md):
#   IPSEC_CLIENT_IFACE       nome da interface XFRM          (padrão: ipsc0)
#   IPSEC_CLIENT_IF_ID       if_id das SAs/políticas, 1..2^32-1 (obrigatória)
#   IPSEC_CLIENT_MTU         MTU da interface, 576..9000      (padrão: 1400)
#   IPSEC_CLIENT_SOCKET_UID  uid dono do socket VICI (0600). Engine embutido.
#   IPSEC_CLIENT_SOCKET_GID  gid que pode usar o socket VICI (0660). Engine
#                            Docker; obrigatória se não houver SOCKET_UID
#   IPSEC_CLIENT_LOG_LEVEL   nível de log do charon, 0..2     (padrão: 1)
#   IPSEC_CLIENT_CHARON      caminho do charon   (padrão: o do pacote do Ubuntu)
#   IPSEC_CLIENT_STRONGSWAN_D diretório strongswan.d (padrão: /etc/strongswan.d)
#   IPSEC_CLIENT_SOCKET_DIR  diretório do socket VICI   (padrão: /run/ipsec-client)
#   IPSEC_CLIENT_STATE_DIR   modo nativo (systemd, raiz somente leitura): diretório
#                            gravável onde vai a config de runtime; o charon a lê
#                            via STRONGSWAN_CONF, que inclui a config base
#   IPSEC_CLIENT_STRONGSWAN_CONF  config base incluída no modo nativo
#   IPSEC_CLIENT_UPDOWN      updown FORÇADO (ADR-0019): o charon embutido
#                            (com o patch do IPsec Client) ignora o updown e o
#                            if_id enviados pelo cliente VICI e usa estes. Os
#                            argumentos vão num arquivo só do root.
#   IPSEC_CLIENT_DNS_SERVERS IPs dos DNS fixos, separados por vírgula
#   IPSEC_CLIENT_DNS_DOMAINS domínios (~dominio, ~.), separados por vírgula
#   IPSEC_CLIENT_DNS_FLAGS   use-received e/ou default-route, por vírgula

set -euo pipefail

readonly SOCKET_DIR=${IPSEC_CLIENT_SOCKET_DIR:-/run/ipsec-client}
readonly SOCKET="$SOCKET_DIR/charon.vici"
readonly STATE_DIR=${IPSEC_CLIENT_STATE_DIR:-}
readonly BASE_CONF=${IPSEC_CLIENT_STRONGSWAN_CONF:-/etc/strongswan.conf}
readonly CHARON=${IPSEC_CLIENT_CHARON:-/usr/lib/ipsec/charon}
readonly RUNTIME_CONF=${IPSEC_CLIENT_STRONGSWAN_D:-/etc/strongswan.d}/zzz-ipsec-client-runtime.conf
# Protocolo de rota usado pelo updown para marcar as rotas da sessão.
readonly RT_PROTO=241

IFACE=${IPSEC_CLIENT_IFACE:-ipsc0}
IF_ID=${IPSEC_CLIENT_IF_ID:-}
MTU=${IPSEC_CLIENT_MTU:-1400}
SOCKET_GID=${IPSEC_CLIENT_SOCKET_GID:-}
SOCKET_UID=${IPSEC_CLIENT_SOCKET_UID:-}
LOG_LEVEL=${IPSEC_CLIENT_LOG_LEVEL:-1}
UPDOWN=${IPSEC_CLIENT_UPDOWN:-}
DNS_SERVERS=${IPSEC_CLIENT_DNS_SERVERS:-}
DNS_DOMAINS=${IPSEC_CLIENT_DNS_DOMAINS:-}
DNS_FLAGS=${IPSEC_CLIENT_DNS_FLAGS:-}
# Argumentos do updown forçado (diretório só do root).
ARGS_DIR=${IPSEC_CLIENT_STATE_DIR:-/run/ipsec-client-engine}
UPDOWN_ARGS="$ARGS_DIR/updown.args"

charon_pid=""

log() { printf '[engine] %s\n' "$*" >&2; }
die() {
    log "error: $*"
    exit 1
}

# uint_in_range VALOR MIN MAX: inteiro decimal sem sinal dentro do intervalo.
uint_in_range() {
    [[ $1 =~ ^[0-9]{1,10}$ ]] || return 1
    ((10#$1 >= $2 && 10#$1 <= $3))
}

validate_env() {
    [[ $IFACE =~ ^[a-z][a-z0-9_-]{0,14}$ ]] ||
        die "invalid IPSEC_CLIENT_IFACE (use ^[a-z][a-z0-9_-]{0,14}\$)"
    uint_in_range "$IF_ID" 1 4294967295 ||
        die "invalid IPSEC_CLIENT_IF_ID (integer 1-4294967295)"
    uint_in_range "$MTU" 576 9000 ||
        die "invalid IPSEC_CLIENT_MTU (integer 576-9000)"
    if [[ -n $SOCKET_UID ]]; then
        uint_in_range "$SOCKET_UID" 0 2147483647 || die "invalid IPSEC_CLIENT_SOCKET_UID"
        SOCKET_UID=$((10#$SOCKET_UID))
    else
        uint_in_range "$SOCKET_GID" 0 2147483647 || die "invalid IPSEC_CLIENT_SOCKET_GID"
        SOCKET_GID=$((10#$SOCKET_GID))
    fi
    [[ $LOG_LEVEL =~ ^[0-2]$ ]] ||
        die "IPSEC_CLIENT_LOG_LEVEL must be 0, 1 or 2"
    local path
    for path in "$SOCKET_DIR" ${STATE_DIR:+"$STATE_DIR"} "$BASE_CONF" "$ARGS_DIR" ${UPDOWN:+"$UPDOWN"}; do
        [[ $path =~ ^/[A-Za-z0-9/._-]+$ && $path != *..* ]] || die "invalid path: $path"
    done
    [[ -d $SOCKET_DIR ]] ||
        die "$SOCKET_DIR does not exist (VICI socket directory)"
    if [[ -n $STATE_DIR ]]; then
        [[ -d $STATE_DIR ]] || die "$STATE_DIR does not exist"
        [[ -f $BASE_CONF ]] || die "base config $BASE_CONF does not exist"
    fi
    if [[ -n $UPDOWN ]]; then
        [[ -f $UPDOWN && -x $UPDOWN ]] || die "updown not found: $UPDOWN"
        validate_dns_env
    fi
    # Normaliza zeros à esquerda.
    IF_ID=$((10#$IF_ID))
    MTU=$((10#$MTU))
}

# Primeira barreira para o DNS vindo do helper (o updown valida de novo).
# Os valores nunca passam por um shell: vão, um por linha, para um arquivo
# que o updown lê como dados.
validate_dns_env() {
    local item n=0
    local -a list
    IFS=, read -r -a list <<<"$DNS_SERVERS"
    for item in "${list[@]}"; do
        [[ $item =~ ^[0-9A-Fa-f:.]{2,45}$ ]] || die "invalid IPSEC_CLIENT_DNS_SERVERS"
        n=$((n + 1))
    done
    ((n <= 8)) || die "more than 8 DNS servers"
    n=0
    IFS=, read -r -a list <<<"$DNS_DOMAINS"
    for item in "${list[@]}"; do
        [[ $item == "~." || $item =~ ^~?[A-Za-z0-9][A-Za-z0-9.-]{0,252}$ ]] ||
            die "invalid IPSEC_CLIENT_DNS_DOMAINS"
        n=$((n + 1))
    done
    ((n <= 16)) || die "more than 16 DNS domains"
    IFS=, read -r -a list <<<"$DNS_FLAGS"
    for item in "${list[@]}"; do
        [[ $item == use-received || $item == default-route ]] || die "invalid IPSEC_CLIENT_DNS_FLAGS"
    done
}

# Argumentos do updown forçado, um por linha (ADR-0019).
write_updown_args() {
    local item
    local -a list
    install -d -m 0700 "$ARGS_DIR"
    {
        printf '%s\n' --iface "$IFACE"
        IFS=, read -r -a list <<<"$DNS_SERVERS"
        for item in "${list[@]}"; do printf '%s\n' --dns "$item"; done
        IFS=, read -r -a list <<<"$DNS_DOMAINS"
        for item in "${list[@]}"; do printf '%s\n' --domain "$item"; done
        if [[ ,$DNS_FLAGS, == *,use-received,* ]]; then
            printf '%s\n' --use-received-dns
        fi
        if [[ ,$DNS_FLAGS, == *,default-route,* ]]; then
            printf '%s\n' --dns-default-route
        fi
    } >"$UPDOWN_ARGS.tmp"
    chmod 0600 "$UPDOWN_ARGS.tmp"
    mv -f "$UPDOWN_ARGS.tmp" "$UPDOWN_ARGS"
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
        log "warning: $stale_policies orphan XFRM policies with if_id $hex_id (inert; gone after a reboot)"
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
    rm -f "$SOCKET" "$UPDOWN_ARGS"
}

create_iface() {
    if ! ip link add "$IFACE" type xfrm if_id "$IF_ID" 2>/dev/null; then
        # Kernels/iproute2 mais antigos exigem um dispositivo subjacente.
        local dev
        dev=$(ip -4 route show default | awk '{ for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit } }')
        [[ -n $dev ]] || die "could not create $IFACE (is the xfrm_interface module available?)"
        ip link add "$IFACE" type xfrm dev "$dev" if_id "$IF_ID" ||
            die "could not create $IFACE"
    fi
    ip link set "$IFACE" mtu "$MTU" up
    log "interface $IFACE created (if_id $IF_ID, MTU $MTU)"
}

write_runtime_conf() {
    local overrides forced=""
    # Lidos só pelo charon com o patch do IPsec Client (engine embutido); um
    # charon sem o patch ignora as opções.
    if [[ -n $UPDOWN ]]; then
        write_updown_args
        forced="
            force_updown = $UPDOWN --args-file $UPDOWN_ARGS"
    fi
    overrides="charon {
    install_virtual_ip_on = $IFACE
    plugins {
        vici {
            socket = unix://$SOCKET
            force_if_id = $IF_ID$forced
        }
    }
    filelog {
        stderr {
            default = $LOG_LEVEL
        }
    }
}"
    if [[ -n $STATE_DIR ]]; then
        # Modo nativo: a raiz é somente leitura; a config de runtime inclui a
        # base e sobrescreve o que depende da sessão.
        printf '# Gerado pelo entrypoint do ipsec-client. Não editar.\ninclude %s\n%s\n' \
            "$BASE_CONF" "$overrides" >"$STATE_DIR/strongswan.conf"
        export STRONGSWAN_CONF="$STATE_DIR/strongswan.conf"
    else
        printf '# Gerado pelo entrypoint do ipsec-client. Não editar.\n%s\n' "$overrides" >"$RUNTIME_CONF"
    fi
}

wait_for_socket() {
    local _
    for _ in $(seq 1 100); do
        [[ -S $SOCKET ]] && return 0
        kill -0 "$charon_pid" 2>/dev/null || die "charon exited before opening the VICI socket"
        sleep 0.1
    done
    die "the VICI socket did not show up within 10 s"
}

# Chamada pelo trap de TERM/INT (docker stop).
# shellcheck disable=SC2329
shutdown() {
    trap - TERM INT
    log "shutting down..."
    if [[ -n $charon_pid ]] && kill -0 "$charon_pid" 2>/dev/null; then
        # O charon encerra as SAs (envia DELETE ao gateway) e remove o VIP.
        kill -TERM "$charon_pid" 2>/dev/null || true
        wait "$charon_pid" 2>/dev/null || true
    fi
    cleanup
    log "cleanup done"
    exit 0
}

main() {
    validate_env
    trap shutdown TERM INT

    local version
    version=$("$CHARON" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true)
    log "preparing (strongSwan ${version:-?})"
    cleanup
    create_iface
    write_runtime_conf
    rm -f /var/run/charon.pid /run/charon.pid 2>/dev/null || true

    "$CHARON" &
    charon_pid=$!

    wait_for_socket
    if [[ -n $SOCKET_UID ]]; then
        # Só o dono da sessão conecta (o diretório do socket é do root). O
        # chmod vem antes: depois do chown, o root sem CAP_FOWNER (sandbox do
        # engine embutido) não pode mais mudar a permissão.
        chmod 0600 "$SOCKET"
        chown "$SOCKET_UID:0" "$SOCKET"
        log "ready: VICI socket handed over to uid $SOCKET_UID"
    else
        chgrp "$SOCKET_GID" "$SOCKET"
        chmod 0660 "$SOCKET"
        log "ready: VICI socket handed over to gid $SOCKET_GID"
    fi

    local status=0
    wait "$charon_pid" || status=$?
    # Só chega aqui se o charon morrer sozinho (o SIGTERM sai pelo trap).
    log "charon exited unexpectedly (status $status)"
    charon_pid=""
    cleanup
    exit "$status"
}

main "$@"
