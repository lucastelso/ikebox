#!/usr/bin/env bash
# ikebox updown (ADR-0004).
#
# charon calls this script (through /bin/sh -c) for every pair of traffic
# selectors that goes up or down. It runs as root in the host's network
# namespace and:
#   - installs/removes the route of the remote TS on the dedicated XFRM
#     interface;
#   - sets the interface's split DNS in the host's systemd-resolved.
#
# Arguments (validated again here). In the embedded engine, charon calls
# `updown --args-file FILE`, and the arguments come, one per line, from that
# file written by the entrypoint (root): whoever speaks VICI chooses neither
# the command nor the arguments (ADR-0019). In the Docker engine, they come
# from the app on the command line:
#   --iface NAME            the session's XFRM interface (required)
#   --dns IP                fixed DNS server (repeatable)
#   --domain DOMAIN         search/routing domain; "~domain" only routes
#                           queries, "~." sends all of them (repeatable)
#   --use-received-dns      also use the DNS servers sent by the gateway
#   --dns-default-route     use these DNS servers for queries outside the
#                           domains
#   --dns-fifo PATH         embedded engine, charon without root (ADR-0028):
#                           DNS needs root, so the request goes to the
#                           entrypoint through this FIFO instead of resolvectl
#
# Environment provided by charon: PLUTO_VERB, PLUTO_PEER, PLUTO_PEER_CLIENT,
# PLUTO_MY_SOURCEIP*, PLUTO_DNS4_<n>, PLUTO_DNS6_<n>.
#
# The output goes to charon's log. This script never receives secrets.

set -euo pipefail

# Tags the session's routes; the entrypoint removes what is left at cleanup.
readonly RT_PROTO=241

log() { printf 'ikebox-updown: %s\n' "$*"; }
die() {
    log "error: $*"
    exit 1
}

is_ipv4() {
    local IFS=. octet
    [[ $1 =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}$ ]] || return 1
    # shellcheck disable=SC2086 # deliberate splitting on the dots
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

# --- arguments --------------------------------------------------------------
if [[ ${1:-} == --args-file ]]; then
    (($# == 2)) || die "--args-file takes only the file path"
    args_file=$2
    [[ -f $args_file && ! -L $args_file ]] || die "invalid arguments file"
    [[ $(stat -c '%u' -- "$args_file") == 0 ]] || die "the arguments file is not owned by root"
    mapfile -t file_args <"$args_file"
    set -- "${file_args[@]}"
fi

iface=""
static_dns=()
domains=()
use_received_dns=0
dns_default_route=0
dns_fifo=""

while (($# > 0)); do
    case $1 in
    --iface | --dns | --domain | --dns-fifo)
        (($# >= 2)) || die "$1 requires a value"
        case $1 in
        --iface) iface=$2 ;;
        --dns-fifo)
            [[ $2 =~ ^/[A-Za-z0-9/._-]+$ && $2 != *..* ]] || die "invalid --dns-fifo"
            dns_fifo=$2
            ;;
        --dns)
            is_ip "$2" || die "invalid --dns: $2"
            static_dns+=("$2")
            ;;
        --domain)
            is_domain "$2" || die "invalid --domain: $2"
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
    *) die "unknown argument: $1" ;;
    esac
done

[[ $iface =~ ^[a-z][a-z0-9_-]{0,14}$ ]] || die "missing or invalid --iface"

# --- charon's environment ---------------------------------------------------
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
    die "invalid PLUTO_MY_SOURCEIP: $src"
fi

src_args=()
[[ -n $src ]] && src_args=(src "$src")

is_full_tunnel() { [[ $peer_client == "0.0.0.0/0" || $peer_client == "::/0" ]]; }

# ipv4_int A.B.C.D: the address as a 32-bit integer (already validated).
ipv4_int() {
    local IFS=. a b c d
    read -r a b c d <<<"$1"
    echo $(((10#$a << 24) | (10#$b << 16) | (10#$c << 8) | 10#$d))
}

# Is the gateway inside the remote TS? Then the route of the TS would carry
# the tunnel's own packets (IKE, ESP) into the tunnel, as in the full tunnel
# (audit of 0.2.0, P11). IPv4 only: an IPv6 remote network that contains the
# gateway is not handled yet.
ts_contains_peer() {
    local net=${peer_client%/*} prefix=${peer_client##*/} mask
    is_ipv4 "$peer" && is_ipv4 "$net" || return 1
    mask=$(((0xFFFFFFFF << (32 - 10#$prefix)) & 0xFFFFFFFF))
    ((($(ipv4_int "$peer") & mask) == ($(ipv4_int "$net") & mask)))
}

# The gateway needs a host route of its own: full tunnel, or a remote network
# that contains it.
needs_gateway_route() { is_full_tunnel || ts_contains_peer; }

# --- routes -----------------------------------------------------------------

# In full tunnel mode, the gateway itself must stay reachable through the
# original path. Without this, the IKE/ESP traffic would enter the tunnel
# (a loop).
add_gateway_host_route() {
    is_ip "$peer" || die "invalid PLUTO_PEER: $peer"
    if [[ -n $(ip "$family" route show "$peer/$host_prefix" proto "$RT_PROTO") ]]; then
        return 0 # already exists (rekey or reconnection)
    fi
    local route via dev
    route=$(ip "$family" route get "$peer" 2>/dev/null | head -n 1)
    via=$(awk '{ for (i = 1; i < NF; i++) if ($i == "via") { print $(i + 1); exit } }' <<<"$route")
    dev=$(awk '{ for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit } }' <<<"$route")
    [[ -n $dev && $dev != "$iface" ]] || die "could not find the original path to the gateway"
    local args=("$peer/$host_prefix")
    [[ -n $via ]] && args+=(via "$via")
    args+=(dev "$dev" proto "$RT_PROTO")
    ip "$family" route replace "${args[@]}"
    log "host route to the gateway $peer via ${via:-link} dev $dev"
}

route_up() {
    is_cidr "$peer_client" || die "invalid PLUTO_PEER_CLIENT: $peer_client"
    if [[ ${peer_client%/*} == "$peer" ]]; then
        log "the remote TS is the gateway itself ($peer_client): route skipped"
        return 0
    fi
    # Before the routes that would capture it.
    if needs_gateway_route; then
        add_gateway_host_route
    fi
    if is_full_tunnel; then
        local r
        for r in "${full_tunnel_routes[@]}"; do
            ip "$family" route replace "$r" dev "$iface" "${src_args[@]}" proto "$RT_PROTO"
        done
        log "full tunnel: routes ${full_tunnel_routes[*]} via $iface"
    else
        ip "$family" route replace "$peer_client" dev "$iface" "${src_args[@]}" proto "$RT_PROTO"
        log "route $peer_client via $iface${src:+ (src $src)}"
    fi
}

route_down() {
    is_cidr "$peer_client" || return 0
    if is_full_tunnel; then
        local r
        for r in "${full_tunnel_routes[@]}"; do
            ip "$family" route del "$r" dev "$iface" proto "$RT_PROTO" 2>/dev/null || true
        done
    else
        ip "$family" route del "$peer_client" dev "$iface" proto "$RT_PROTO" 2>/dev/null || true
    fi
    if needs_gateway_route && is_ip "$peer"; then
        ip "$family" route del "$peer/$host_prefix" proto "$RT_PROTO" 2>/dev/null || true
    fi
    log "route $peer_client removed"
}

# --- DNS --------------------------------------------------------------------

# Without root (ADR-0028): the entrypoint applies the DNS. Only the servers
# the gateway sent go in the request (validated here and again there); the
# static servers, the domains and the flags are the entrypoint's own.
dns_request() {
    [[ -p $dns_fifo ]] || { log "warning: failed to set the DNS servers (no DNS FIFO)"; return 0; }
    printf '%s\n' "$*" >"$dns_fifo" || log "warning: failed to set the DNS servers"
}

dns_up() {
    local servers=("${static_dns[@]}") i var value
    local -a received=()
    if [[ -n $dns_fifo ]]; then
        for var in PLUTO_DNS4_ PLUTO_DNS6_; do
            for i in $(seq 1 8); do
                value=$(printenv "$var$i" || true)
                [[ -n $value ]] || break
                if is_ip "$value"; then
                    received+=("$value")
                else
                    log "warning: invalid received DNS ignored"
                fi
            done
        done
        dns_request up "${received[@]}"
        return 0
    fi
    if ((use_received_dns)); then
        for var in PLUTO_DNS4_ PLUTO_DNS6_; do
            for i in $(seq 1 8); do
                value=$(printenv "$var$i" || true)
                [[ -n $value ]] || break
                if is_ip "$value"; then
                    servers+=("$value")
                else
                    log "warning: invalid received DNS ignored"
                fi
            done
        done
    fi

    if ((${#servers[@]} == 0 && ${#domains[@]} == 0)); then
        return 0
    fi

    # A DNS failure does not bring the tunnel down: it is only logged.
    if ((${#servers[@]} > 0)); then
        resolvectl dns "$iface" "${servers[@]}" || log "warning: failed to set the DNS servers"
    fi
    if ((${#domains[@]} > 0)); then
        resolvectl domain "$iface" "${domains[@]}" || log "warning: failed to set the DNS domains"
    fi
    if ((dns_default_route)); then
        resolvectl default-route "$iface" yes || true
    else
        resolvectl default-route "$iface" no || true
    fi
    resolvectl flush-caches || true
    log "DNS on $iface: servers [${servers[*]}] domains [${domains[*]}]"
}

dns_down() {
    if [[ -n $dns_fifo ]]; then
        dns_request down
        return 0
    fi
    # With several TS per connection, only revert when the last route goes.
    if [[ -z "$(ip -4 route show dev "$iface" proto "$RT_PROTO" 2>/dev/null)$(ip -6 route show dev "$iface" proto "$RT_PROTO" 2>/dev/null)" ]]; then
        resolvectl revert "$iface" 2>/dev/null || true
        resolvectl flush-caches 2>/dev/null || true
        log "DNS of $iface reverted"
    fi
}

# --- main -------------------------------------------------------------------
# "-host" says that OUR side of the tunnel is this computer's own address
# (no virtual IP), not that the tunnel is host-to-host: the remote side may
# still be a network that needs a route. route_up skips the true
# host-to-host case (the remote TS is the gateway itself).
case $verb in
up-client | up-client-v6 | up-host | up-host-v6)
    route_up
    dns_up
    ;;
down-client | down-client-v6 | down-host | down-host-v6)
    route_down
    dns_down
    ;;
*)
    log "verb ignored: ${verb:-<empty>}"
    ;;
esac
