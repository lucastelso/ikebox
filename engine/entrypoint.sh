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
# Embedded engine (ADR-0028): this script stays root, inside the sandbox, for
# what needs it (the interface, handing the socket over, DNS, cleanup), and
# charon runs as IPSEC_CLIENT_ENGINE_USER with only the network capabilities.
# DNS needs root (systemd-resolved does not accept CAP_NET_ADMIN), so the
# updown, unprivileged, only asks for it through a FIFO; this script checks
# the request and applies it to the session's interface only.
#
# Variables (see docs/architecture.md):
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
#   IPSEC_CLIENT_ENGINE_USER system user charon runs as (embedded engine,
#                            ADR-0028); empty: charon runs as root (Docker)

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
ENGINE_USER=${IPSEC_CLIENT_ENGINE_USER:-}
ENGINE_UID=""
ENGINE_GID=""
# Argumentos do updown forçado (diretório só do root).
ARGS_DIR=${IPSEC_CLIENT_STATE_DIR:-/run/ipsec-client-engine}
UPDOWN_ARGS="$ARGS_DIR/updown.args"
# DNS requests from the unprivileged updown (ADR-0028).
DNS_FIFO="$ARGS_DIR/dns.requests"
# Where the session user's key agent listens, for keys on a smartcard or token
# (ADR-0029): a directory of the user, readable by charon's group.
AGENT_DIR="$SOCKET_DIR/agent"
# Capabilities charon keeps when it does not run as root.
readonly CHARON_CAPS=net_admin,net_bind_service,net_raw

charon_pid=""
dns_fd=""

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
    if [[ -n $ENGINE_USER ]]; then
        [[ $ENGINE_USER =~ ^[a-z_][a-z0-9_-]{0,31}$ ]] || die "invalid IPSEC_CLIENT_ENGINE_USER"
        [[ -n $STATE_DIR && -n $SOCKET_UID && -n $UPDOWN ]] ||
            die "IPSEC_CLIENT_ENGINE_USER needs the embedded engine (state dir, socket uid, updown)"
        command -v setpriv >/dev/null || die "setpriv (util-linux) is needed to run charon without root"
        ENGINE_UID=$(id -u "$ENGINE_USER" 2>/dev/null) ||
            die "user $ENGINE_USER does not exist; reinstall the network component"
        ENGINE_GID=$(id -g "$ENGINE_USER")
        ((ENGINE_UID != 0)) || die "$ENGINE_USER must not be root"
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
        if [[ -n $ENGINE_USER ]]; then
            printf '%s\n' --dns-fifo "$DNS_FIFO"
        fi
    } >"$UPDOWN_ARGS.tmp"
    chmod 0600 "$UPDOWN_ARGS.tmp"
    # Still root's file (the updown checks it), readable by charon's group.
    if [[ -n $ENGINE_USER ]]; then
        chgrp "$ENGINE_GID" "$UPDOWN_ARGS.tmp"
        chmod 0640 "$UPDOWN_ARGS.tmp"
    fi
    mv -f "$UPDOWN_ARGS.tmp" "$UPDOWN_ARGS"
}

# Remove SAs e políticas XFRM marcadas com o nosso if_id que tenham sobrado de
# uma sessão encerrada de forma abrupta (SIGKILL). Nunca faz flush global:
# outros IPsec do host não são afetados. A saída do `ip xfrm state` contém
# chaves; ela só é lida pelo awk e nunca é impressa.
#
# Policies are only removed with an iproute2 that accepts `if_id` on delete
# (the host's, in the embedded engine). With the iproute2 5.5 of Ubuntu 20.04
# (Docker image) they stay: they are inert, a session with the same VIP
# replaces them and they are gone after a reboot. See docs/development-log.md.
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

# Is there a link with the session's name, and is it an XFRM interface?
# The output is read in full before matching (with pipefail, an early exit of
# grep would kill `ip` with SIGPIPE).
iface_exists() { ip link show dev "$IFACE" >/dev/null 2>&1; }
iface_is_xfrm() {
    local details
    details=$(ip -d link show dev "$IFACE" 2>/dev/null) || return 1
    [[ $details =~ (^|$'\n')[[:space:]]+xfrm([[:space:]]|$) ]]
}

# Only an XFRM interface is ever removed or has its DNS reverted: a link with
# the same name that is something else (a bridge, another program's tunnel)
# is not ours.
remove_iface() {
    iface_is_xfrm || return 0
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
    if [[ -n $ENGINE_USER ]]; then
        rm -f "$DNS_FIFO"
        # Back to what the helper made: only root writes there.
        chgrp 0 "$SOCKET_DIR" 2>/dev/null || true
        chmod 0711 "$SOCKET_DIR" 2>/dev/null || true
    fi
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
    if [[ -n $ENGINE_USER ]]; then
        forced+="
            force_agent_socket = $AGENT_DIR/agent.sock"
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
        if [[ -n $ENGINE_USER ]]; then
            chgrp "$ENGINE_GID" "$STATE_DIR/strongswan.conf"
            chmod 0640 "$STATE_DIR/strongswan.conf"
        fi
        export STRONGSWAN_CONF="$STATE_DIR/strongswan.conf"
    else
        printf '# Gerado pelo entrypoint do ipsec-client. Não editar.\n%s\n' "$overrides" >"$RUNTIME_CONF"
    fi
}

# --- charon without root (ADR-0028) -----------------------------------------

is_ipv4() {
    local IFS=. octet
    [[ $1 =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]] || return 1
    # shellcheck disable=SC2086 # split on the dots on purpose
    for octet in $1; do
        ((10#$octet <= 255)) || return 1
    done
}
is_ipv6() { [[ $1 =~ ^[0-9A-Fa-f:.]{2,45}$ && $1 == *:* ]]; }
is_ip() { is_ipv4 "$1" || is_ipv6 "$1"; }

# What charon, as ENGINE_USER, may touch: its runtime directory (sticky:
# it creates its pid file, but cannot remove or replace root's files there),
# the socket directory (to create the VICI socket; the session user still
# only traverses it), and the FIFO where the updown asks for DNS. The FIFO is
# opened here before charon starts, so a writer never blocks and the reads
# never see an end of file.
prepare_engine_access() {
    chgrp "$ENGINE_GID" "$STATE_DIR" "$SOCKET_DIR"
    chmod 1770 "$STATE_DIR"
    chmod 1731 "$SOCKET_DIR"
    prepare_agent_dir
    rm -f "$DNS_FIFO"
    mkfifo -m 0600 "$DNS_FIFO"
    chgrp "$ENGINE_GID" "$DNS_FIFO"
    chmod 0620 "$DNS_FIFO"
    exec {dns_fd}<>"$DNS_FIFO"
}

# The session user's directory for the key agent (ADR-0029): the user creates
# the agent socket there, and only the user and charon's group can enter it
# (no setgid: the sandbox forbids it, RestrictSUIDSGID). Made once (mode
# first, while root still owns it), then left to the user; it lives inside
# the root-owned socket directory, so the user cannot move or replace it.
prepare_agent_dir() {
    if [[ ! -e $AGENT_DIR ]]; then
        mkdir -m 0700 "$AGENT_DIR"
        chmod 0750 "$AGENT_DIR"
        chown "$SOCKET_UID:$ENGINE_GID" "$AGENT_DIR"
    fi
    if [[ -L $AGENT_DIR || $(stat -c '%u:%g:%a' "$AGENT_DIR") != "$SOCKET_UID:$ENGINE_GID:750" ]]; then
        log "warning: $AGENT_DIR is not as expected; keys on a token will not work"
    fi
}

# charon as ENGINE_USER, with the network capabilities only, also for every
# program it starts (the updown, ip): they are ambient, and the bounding set
# keeps the others from ever coming back.
start_charon() {
    if [[ -n $ENGINE_USER ]]; then
        local caps
        caps=+${CHARON_CAPS//,/,+}
        setpriv --reuid="$ENGINE_UID" --regid="$ENGINE_GID" --clear-groups \
            --inh-caps=-all,"$caps" --ambient-caps="$caps" --bounding-set=-all,"$caps" \
            -- "$CHARON" &
    else
        "$CHARON" &
    fi
    charon_pid=$!
}

# The socket is charon's (ENGINE_USER, 0660). Root here has CAP_CHOWN but not
# CAP_FOWNER: the mode is changed as its owner first, then root hands it to
# the session user (0600).
hand_over_socket() {
    if [[ -n $ENGINE_USER ]]; then
        setpriv --reuid="$ENGINE_UID" --regid="$ENGINE_GID" --clear-groups \
            --inh-caps=-all --bounding-set=-all -- chmod 0600 "$SOCKET"
    else
        chmod 0600 "$SOCKET"
    fi
    chown "$SOCKET_UID:0" "$SOCKET"
}

# The DNS the updown asked for, applied as root to the session's interface
# only. The request carries just a verb and the servers the gateway sent;
# the static servers, the domains and the flags come from this script's own
# validated environment, never from the request.
apply_dns() {
    local -a servers=() received=() domains=() list
    local item
    IFS=, read -r -a list <<<"$DNS_SERVERS"
    servers+=("${list[@]}")
    if [[ ,$DNS_FLAGS, == *,use-received,* ]]; then
        read -r -a received <<<"$1"
        for item in "${received[@]:0:16}"; do
            if is_ip "$item"; then
                servers+=("$item")
            else
                log "warning: invalid received DNS ignored"
            fi
        done
    fi
    IFS=, read -r -a domains <<<"$DNS_DOMAINS"
    ((${#servers[@]} > 0 || ${#domains[@]} > 0)) || return 0
    # A DNS failure does not bring the tunnel down: it is only logged.
    if ((${#servers[@]} > 0)); then
        resolvectl dns "$IFACE" "${servers[@]}" || log "warning: failed to set the DNS servers"
    fi
    if ((${#domains[@]} > 0)); then
        resolvectl domain "$IFACE" "${domains[@]}" || log "warning: failed to set the DNS domains"
    fi
    if [[ ,$DNS_FLAGS, == *,default-route,* ]]; then
        resolvectl default-route "$IFACE" yes || true
    else
        resolvectl default-route "$IFACE" no || true
    fi
    resolvectl flush-caches || true
    log "DNS on $IFACE: servers [${servers[*]}] domains [${domains[*]}]"
}

revert_dns() {
    # With several networks per connection, only when the last route is gone.
    if [[ -z "$(ip -4 route show dev "$IFACE" proto "$RT_PROTO" 2>/dev/null)$(ip -6 route show dev "$IFACE" proto "$RT_PROTO" 2>/dev/null)" ]]; then
        resolvectl revert "$IFACE" 2>/dev/null || true
        resolvectl flush-caches 2>/dev/null || true
        log "DNS of $IFACE reverted"
    fi
}

# Serves the updown's DNS requests while charon runs. One line per request,
# at most 1 KiB: "up <servers received from the gateway>" or "down".
serve_dns_requests() {
    local line verb rest
    while kill -0 "$charon_pid" 2>/dev/null; do
        if IFS= read -r -t 1 -n 1024 -u "$dns_fd" line; then
            read -r verb rest <<<"$line"
            case $verb in
            up) apply_dns "$rest" ;;
            down) revert_dns ;;
            *) log "warning: unknown DNS request ignored" ;;
            esac
        fi
    done
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

# Chamada pelo trap de TERM/INT (docker stop). Só o trap a chama: o shellcheck
# acha que é código morto (SC2329 no 0.11, SC2317 nas versões anteriores).
# shellcheck disable=SC2317,SC2329
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
    if iface_exists && ! iface_is_xfrm; then
        die "$IFACE exists and is not an XFRM interface: it belongs to something else and is left alone"
    fi
    cleanup
    create_iface
    write_runtime_conf
    rm -f /var/run/charon.pid /run/charon.pid 2>/dev/null || true
    [[ -z $ENGINE_USER ]] || prepare_engine_access

    start_charon

    wait_for_socket
    if [[ -n $SOCKET_UID ]]; then
        # Só o dono da sessão conecta (o diretório do socket é do root). O
        # chmod vem antes: depois do chown, o root sem CAP_FOWNER (sandbox do
        # engine embutido) não pode mais mudar a permissão.
        hand_over_socket
        log "ready: VICI socket handed over to uid $SOCKET_UID${ENGINE_USER:+ (charon runs as $ENGINE_USER)}"
    else
        chgrp "$SOCKET_GID" "$SOCKET"
        chmod 0660 "$SOCKET"
        log "ready: VICI socket handed over to gid $SOCKET_GID"
    fi

    [[ -z $ENGINE_USER ]] || serve_dns_requests
    local status=0
    wait "$charon_pid" || status=$?
    # Só chega aqui se o charon morrer sozinho (o SIGTERM sai pelo trap).
    log "charon exited unexpectedly (status $status)"
    charon_pid=""
    cleanup
    exit "$status"
}

main "$@"
