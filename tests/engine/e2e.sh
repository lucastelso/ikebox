#!/usr/bin/env bash
# End-to-end test of the engine, WITHOUT any real VPN.
#
# Starts a test IPsec gateway (IKEv1 aggressive, group PSK + XAuth,
# modp1024/768) on an isolated Docker network and connects the engine to it the
# same way the app does (--network host, NET_ADMIN only). Checks:
#
#   scenario 1, normal cycle:
#     XFRM interface + MTU, VICI socket (owner/permissions, `version` as a
#     regular user), docker inspect without secrets, VIP, route, split DNS,
#     ping through the tunnel; after `docker stop`, that nothing is left
#     (interface, routes, XFRM policies/SAs with the if_id, socket).
#   scenario 2, recovery after SIGKILL:
#     the engine dies without cleaning up and the next engine removes the leftovers.
#   scenario 3, full tunnel (0.0.0.0/0) on an isolated network.
#   scenario 4, PFS with modp768 (DH1) only in phase 2.
#   scenario 5 (embedded engine only, IPSEC_CLIENT_E2E_FORCED=1): a malicious
#     VICI client tries to pick the updown (which runs as root) and the if_id;
#     charon with the IPsec Client patch ignores both (ADR-0019).
#
# Changes the host only temporarily: creates the ipsct0 interface (name and
# if_id of its own, so it does not collide with a real session), routes to
# 192.0.2.0/24 (RFC 5737) and DNS for ~ipsec-client.test on that interface.
#
# Requirements: Docker (user in the docker group), python3, iproute2, ping,
# systemd-resolved and free UDP ports 500/4500.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
here="$repo_root/tests/engine"

# IMAGE: test gateway (Debian's strongSwan 6.0.1) and utilities.
# ENGINE_IMAGE: the engine under test (default: the same image; the embedded
# engine spike passes another one, with the strongSwan we compile).
readonly IMAGE=${IPSEC_CLIENT_E2E_IMAGE:-ipsec-client-engine:e2e}
readonly ENGINE_IMAGE=${IPSEC_CLIENT_E2E_ENGINE_IMAGE:-$IMAGE}
readonly EXPECT_VERSION=${IPSEC_CLIENT_E2E_EXPECT_VERSION:-6.0.1}
# 1 = embedded engine: updown/if_id forced by root and a uid-owned socket (0600).
readonly FORCED=${IPSEC_CLIENT_E2E_FORCED:-0}
readonly UPDOWN=/usr/local/libexec/ipsec-client/updown
readonly NET=ipsec-client-e2e
readonly GW=ipsec-client-e2e-gateway
readonly ENGINE=ipsec-client-e2e-engine
readonly IFACE=ipsct0
readonly IF_ID=7341
readonly RT_PROTO=241
readonly VICI_URI=unix:///run/ipsec-client/charon.vici
readonly TEST_SECRETS_RE='ipsec-client-test-psk|senha-de-teste'
HEX_ID=$(printf '0x%x' "$IF_ID")
readonly HEX_ID

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}
need() { command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"; }

workdir=""
rundir=""
confdir=""
gw_ip=""

# Runs `ip` with privileges, in a throwaway container on the host network.
# The output of `ip xfrm state` contains keys: it is only counted, never printed.
host_ip() {
    docker run --rm --network host --cap-add NET_ADMIN --security-opt label=disable \
        --entrypoint ip "$IMAGE" "$@"
}
count_xfrm() { # count_xfrm state|policy
    host_ip -o xfrm "$1" | grep -c "if_id $HEX_ID" || true
}

cleanup() {
    local rc=$?
    set +e
    if ((rc != 0)); then
        printf '\n--- last engine lines ---\n'
        docker logs "$ENGINE" 2>&1 | tail -n 60
        printf '\n--- last gateway lines ---\n'
        docker logs "$GW" 2>&1 | tail -n 30
    fi
    # stop (gracioso) antes do rm, para o entrypoint limpar o host.
    docker stop -t 10 "$ENGINE" >/dev/null 2>&1
    docker rm -f "$ENGINE" "$GW" >/dev/null 2>&1
    docker network rm "$NET" >/dev/null 2>&1
    if ip link show "$IFACE" >/dev/null 2>&1; then
        host_ip link del "$IFACE" >/dev/null 2>&1
    fi
    [[ -n $workdir ]] && rm -rf "$workdir"
    exit "$rc"
}

# start_engine [host|isolado]
#   host:    the way the app starts it (--network host + the host D-Bus).
#   isolado: in the network namespace of the test network and WITHOUT the host
#            D-Bus. resolvectl resolves the interface index in its own
#            namespace and could configure the wrong interface in the host's
#            systemd-resolved.
start_engine() {
    local mode=${1:-host} net_args
    if [[ $mode == host ]]; then
        net_args=(--network host -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket)
    else
        net_args=(--network "$NET")
    fi
    # Um socket velho (de um engine morto com SIGKILL) não indica prontidão:
    # remove antes e espera o entrypoint anunciar "pronto".
    rm -f "$rundir/charon.vici"
    local env_args
    if ((FORCED)); then
        # Como o helper sobe o engine embutido: o DNS vem do root, não do VICI.
        env_args=(
            -e IPSEC_CLIENT_SOCKET_UID="$(id -u)"
            -e IPSEC_CLIENT_UPDOWN="$UPDOWN"
            -e IPSEC_CLIENT_DNS_SERVERS=192.0.2.54
            -e IPSEC_CLIENT_DNS_DOMAINS="~ipsec-client.test"
            -e IPSEC_CLIENT_DNS_FLAGS=use-received
        )
    else
        env_args=(-e IPSEC_CLIENT_SOCKET_GID="$(id -g)")
    fi
    docker run -d --name "$ENGINE" \
        "${net_args[@]}" \
        --cap-add NET_ADMIN \
        --security-opt label=disable \
        -v "$rundir:/run/ipsec-client" \
        -v "$confdir:/test:ro" \
        -e IPSEC_CLIENT_IFACE="$IFACE" \
        -e IPSEC_CLIENT_IF_ID="$IF_ID" \
        "${env_args[@]}" \
        "$ENGINE_IMAGE" >/dev/null

    local sock="$rundir/charon.vici" _
    for _ in $(seq 1 150); do
        if grep -q '^\[engine\] ready' < <(docker logs "$ENGINE" 2>&1); then
            [[ -S $sock ]] || fail "engine ready, but no VICI socket"
            return 0
        fi
        [[ $(docker inspect -f '{{.State.Running}}' "$ENGINE") == true ]] || fail "the engine stopped during startup"
        sleep 0.1
    done
    fail "the engine did not become ready within 15 s"
}

# render_conf ARQUIVO TS_REMOTO ARGS_EXTRAS_DO_UPDOWN
render_conf() {
    sed -e "s|@GATEWAY@|$gw_ip|" -e "s|@IFACE@|$IFACE|" -e "s|@IF_ID@|$IF_ID|" \
        -e "s|@REMOTE_TS@|$2|" -e "s|@UPDOWN_EXTRA@|$3|" \
        "$here/initiator.conf.in" >"$confdir/$1"
}

# connect_tunnel [ARQUIVO]
connect_tunnel() {
    local conf=${1:-initiator.conf}
    docker exec "$ENGINE" swanctl --load-all --uri "$VICI_URI" --file "/test/$conf" \
        >"$workdir/load.log" 2>&1 || {
        cat "$workdir/load.log"
        fail "swanctl --load-all failed"
    }
    docker exec "$ENGINE" swanctl --initiate --uri "$VICI_URI" --child rede-teste --timeout 30 \
        >"$workdir/initiate.log" 2>&1 || {
        cat "$workdir/initiate.log"
        fail "the IKE negotiation failed"
    }
}

main() {
    step "Prerequisites"
    for cmd in docker python3 ip ss ping resolvectl stat; do need "$cmd"; done
    docker info >/dev/null 2>&1 || fail "Docker is not reachable (user in the docker group?)"
    [[ -S /run/dbus/system_bus_socket ]] || fail "system D-Bus socket missing"
    if [[ -n $(ss -Hlun '( sport = :500 or sport = :4500 )') ]]; then
        fail "UDP 500/4500 in use on the host (another IPsec VPN connected?)"
    fi
    ip link show "$IFACE" >/dev/null 2>&1 && fail "the $IFACE interface already exists"
    docker rm -f "$ENGINE" "$GW" >/dev/null 2>&1 || true
    docker network rm "$NET" >/dev/null 2>&1 || true
    trap cleanup EXIT

    workdir=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/ipsec-client-e2e.XXXXXX")
    rundir="$workdir/run"
    confdir="$workdir/conf"
    mkdir -m 0700 "$rundir" "$confdir"
    ok "environment ready"

    step "Engine image build ($IMAGE)"
    docker build -q -t "$IMAGE" "$repo_root/engine" >/dev/null
    ok "image built"

    step "Test IPsec gateway"
    docker network create "$NET" >/dev/null
    docker run -d --name "$GW" --network "$NET" --cap-add NET_ADMIN \
        --security-opt label=disable \
        -v "$here/gateway:/test:ro" --entrypoint /test/gateway.sh "$IMAGE" >/dev/null
    local _
    for _ in $(seq 1 150); do
        grep -q '\[test-gateway\] ready' < <(docker logs "$GW" 2>&1) && break
        sleep 0.1
    done
    grep -q '\[test-gateway\] ready' < <(docker logs "$GW" 2>&1) || fail "the test gateway did not start"
    gw_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$GW")
    [[ $gw_ip =~ ^[0-9]+(\.[0-9]+){3}$ ]] || fail "invalid gateway IP: $gw_ip"
    render_conf initiator.conf 192.0.2.0/24 \
        " --dns '192.0.2.54' --domain '~ipsec-client.test' --use-received-dns"
    render_conf initiator-full.conf 0.0.0.0/0 ""
    render_conf initiator-modp768.conf 203.0.113.0/24 ""
    # Cliente malicioso: tenta rodar um comando como root pelo updown e tirar
    # as SAs da interface da sessão com outro if_id.
    render_conf initiator-evil.conf 192.0.2.0/24 ""
    sed -i -e "s|updown = .*|updown = /bin/touch /run/ipsec-client/PWNED|" \
        -e "s|if_id_in = .*|if_id_in = 999|" -e "s|if_id_out = .*|if_id_out = 999|" \
        "$confdir/initiator-evil.conf"
    # No IKEv1, a fase 2 só leva um grupo DH (o da primeira proposta): para
    # oferecer modp768, ele precisa vir primeiro.
    sed -i 's/esp_proposals = .*/esp_proposals = aes256-sha1-modp768/' "$confdir/initiator-modp768.conf"
    ok "gateway at $gw_ip (protected network 192.0.2.0/24)"

    # ------------------------------------------------------------------ 1 --
    step "Scenario 1: the engine starts the way the app starts it"
    start_engine
    local link
    link=$(ip -d link show "$IFACE")
    [[ $link == *"mtu 1400"* ]] || fail "MTU of $IFACE is not 1400"
    [[ $link == *xfrm* && $link == *"if_id $HEX_ID"* ]] || fail "$IFACE is not XFRM with if_id $HEX_ID"
    ok "$IFACE: XFRM interface, if_id $HEX_ID, MTU 1400"

    if ((FORCED)); then
        [[ $(stat -c '%a %u' "$rundir/charon.vici") == "600 $(id -u)" ]] ||
            fail "VICI socket without 0600/user uid"
        ok "VICI socket 0600, owner $(id -u)"
    else
        [[ $(stat -c '%a %g' "$rundir/charon.vici") == "660 $(id -g)" ]] ||
            fail "VICI socket without 0660/user gid"
        ok "VICI socket 0660, group $(id -g)"
    fi

    local version
    version=$(python3 -I "$here/vici_version.py" "$rundir/charon.vici") ||
        fail "VICI 'version' failed as a regular user"
    [[ $version == *"version=$EXPECT_VERSION"* ]] || fail "unexpected charon version: $version"
    ok "VICI 'version' as a regular user: $(grep '^version=' <<<"$version")"

    grep -qE "$TEST_SECRETS_RE" < <(docker inspect "$ENGINE") && fail "secret visible in docker inspect"
    ok "docker inspect without secrets"

    step "Scenario 1: negotiation (IKEv1 aggressive + PSK + XAuth)"
    connect_tunnel
    ok "tunnel established"

    if grep -qE "$TEST_SECRETS_RE" < <(docker logs "$ENGINE" 2>&1) ||
        grep -qE "$TEST_SECRETS_RE" "$workdir/load.log" "$workdir/initiate.log"; then
        fail "PSK or XAuth password showed up in the charon logs"
    fi
    ok "charon logs (level 1) without PSK or XAuth password"

    local vip
    vip=$(ip -4 -o addr show dev "$IFACE" | awk '{ print $4 }' | cut -d/ -f1 | head -n 1)
    [[ $vip == 198.51.100.* ]] || fail "VIP not installed on $IFACE (found: '${vip:-nothing}')"
    ok "VIP $vip on $IFACE"

    local route
    route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
    [[ $route == *"192.0.2.0/24"* && $route == *"src $vip"* ]] || fail "route 192.0.2.0/24 missing: '$route'"
    ok "route 192.0.2.0/24 dev $IFACE src $vip (proto $RT_PROTO)"

    local dns domains default_route
    dns=$(resolvectl dns "$IFACE")
    domains=$(resolvectl domain "$IFACE")
    default_route=$(resolvectl default-route "$IFACE")
    [[ $dns == *192.0.2.54* ]] || fail "static DNS missing: $dns"
    [[ $dns == *192.0.2.53* ]] || fail "DNS received from the gateway missing: $dns"
    [[ $domains == *"~ipsec-client.test"* ]] || fail "domain missing: $domains"
    [[ $default_route == *no ]] || fail "default-route should be 'no': $default_route"
    ok "split DNS: 192.0.2.54 (static) + 192.0.2.53 (received), ~ipsec-client.test, no default-route"

    ping -c 3 -W 2 -q 192.0.2.1 >/dev/null || fail "ping 192.0.2.1 through the tunnel failed"
    ok "ping 192.0.2.1 through the tunnel"

    local policies states
    policies=$(count_xfrm policy)
    states=$(count_xfrm state)
    ((policies >= 2 && states >= 2)) || fail "XFRM without policies/SAs with the if_id ($policies/$states)"
    ok "XFRM: $policies policies and $states SAs with if_id $HEX_ID"

    grep -q "dev $IFACE" < <(ip -4 route show default) &&
        fail "the default route was changed (split tunnel expected)"
    ok "host default route intact"

    step "Scenario 1: disconnection (docker stop)"
    docker stop -t 10 "$ENGINE" >/dev/null
    [[ $(docker inspect -f '{{.State.ExitCode}}' "$ENGINE") == 0 ]] || fail "the entrypoint did not exit with 0"
    ip link show "$IFACE" >/dev/null 2>&1 && fail "$IFACE was left over"
    [[ -z $(ip -4 route show proto "$RT_PROTO")$(ip -6 route show proto "$RT_PROTO") ]] || fail "proto $RT_PROTO routes were left over"
    [[ ! -e "$rundir/charon.vici" ]] || fail "the VICI socket was left over"
    policies=$(count_xfrm policy)
    states=$(count_xfrm state)
    ((policies == 0 && states == 0)) || fail "XFRM with the if_id left over ($policies policies, $states SAs)"
    ok "nothing left: interface, routes, DNS, XFRM and socket"
    docker rm "$ENGINE" >/dev/null

    # ------------------------------------------------------------------ 2 --
    step "Scenario 2: engine killed with SIGKILL (no cleanup)"
    start_engine
    connect_tunnel
    docker kill -s KILL "$ENGINE" >/dev/null
    docker rm "$ENGINE" >/dev/null
    ip link show "$IFACE" >/dev/null 2>&1 || fail "expected leftovers from the dead session"
    ok "leftovers present: $IFACE, $(count_xfrm policy) policies, $(count_xfrm state) SAs"

    step "Scenario 2: the next engine cleans up the leftovers"
    start_engine
    route=$(ip -4 route show dev "$IFACE")
    [[ -z $route ]] || fail "old routes on $IFACE: $route"
    [[ $(resolvectl dns "$IFACE") != *192.0.2.* ]] || fail "old DNS on $IFACE"
    states=$(count_xfrm state)
    ((states == 0)) || fail "$states SAs with the if_id were left over"
    ok "leftovers removed: interface recreated, routes, DNS and SAs"
    policies=$(count_xfrm policy)
    ok "remaining orphan policies: $policies"

    step "Scenario 2: reconnecting on top of the leftovers"
    # Simula o gateway real descartando a sessão morta (timeout de DPD): o
    # mesmo usuário recebe de volta o mesmo VIP, e o charon substitui as
    # políticas órfãs (que têm o mesmo seletor) e as remove ao sair. Sem isso,
    # elas continuariam inertes no kernel de quem roda o teste.
    docker exec "$GW" swanctl --terminate --ike gateway-teste --force --timeout 2 >/dev/null 2>&1 || true
    connect_tunnel
    [[ $(ip -4 -o addr show dev "$IFACE") == *" $vip/"* ]] || fail "expected the same VIP $vip on reconnection"
    ping -c 3 -W 2 -q 192.0.2.1 >/dev/null || fail "ping through the new tunnel failed"
    ok "new tunnel works (ping 192.0.2.1)"
    docker stop -t 10 "$ENGINE" >/dev/null
    ip link show "$IFACE" >/dev/null 2>&1 && fail "$IFACE was left over"
    policies=$(count_xfrm policy)
    states=$(count_xfrm state)
    ((policies == 0 && states == 0)) || fail "XFRM with the if_id left over ($policies policies, $states SAs)"
    ok "charon replaced the orphan policies and removed everything on exit"

    # ------------------------------------------------------------------ 3 --
    step "Scenario 3: full tunnel (0.0.0.0/0), engine on an isolated network"
    docker rm "$ENGINE" >/dev/null
    start_engine isolado
    connect_tunnel initiator-full.conf
    local routes default_before default_after
    routes=$(docker exec "$ENGINE" ip -4 route show proto "$RT_PROTO")
    [[ $routes == *"0.0.0.0/1 dev $IFACE"* && $routes == *"128.0.0.0/1 dev $IFACE"* ]] ||
        fail "full tunnel /1 routes missing: $routes"
    [[ $routes == *"$gw_ip "*"dev eth0"* ]] || fail "host route to the gateway missing: $routes"
    ok "routes 0.0.0.0/1 + 128.0.0.0/1 via $IFACE and host route to $gw_ip via eth0"
    default_before=$(docker exec "$ENGINE" ip -4 route show default)
    docker exec "$ENGINE" ping -c 3 -W 2 -q 192.0.2.1 >/dev/null || fail "ping through the full tunnel failed"
    ok "ping 192.0.2.1 through the full tunnel (IKE/ESP did not loop)"
    docker exec "$ENGINE" swanctl --terminate --uri "$VICI_URI" --ike teste --timeout 10 >/dev/null 2>&1 ||
        fail "terminate failed"
    routes=$(docker exec "$ENGINE" ip -4 route show proto "$RT_PROTO")
    [[ -z $routes ]] || fail "full tunnel routes left over after down: $routes"
    default_after=$(docker exec "$ENGINE" ip -4 route show default)
    [[ $default_before == "$default_after" ]] || fail "default route changed"
    ok "updown down removed the /1 routes and the host route; default route intact"
    docker stop -t 10 "$ENGINE" >/dev/null

    # ------------------------------------------------------------------ 4 --
    step "Scenario 4: PFS with modp768 (DH1) only in phase 2, like legacy VPNs"
    docker rm "$ENGINE" >/dev/null
    start_engine isolado
    connect_tunnel initiator-modp768.conf
    grep -q "MODP_768" < <(docker exec "$ENGINE" swanctl --list-sas --uri "$VICI_URI" 2>/dev/null) ||
        fail "phase 2 did not negotiate MODP_768"
    docker exec "$ENGINE" ping -c 2 -W 2 -q 203.0.113.1 >/dev/null || fail "ping with modp768 PFS failed"
    ok "phase 2 with MODP_768 PFS negotiated and ping 203.0.113.1 through the tunnel"
    docker stop -t 10 "$ENGINE" >/dev/null

    # ------------------------------------------------------------------ 5 --
    if ((FORCED)); then
        step "Scenario 5: malicious VICI client (its own updown and if_id)"
        docker rm "$ENGINE" >/dev/null
        start_engine
        connect_tunnel initiator-evil.conf
        [[ ! -e "$rundir/PWNED" ]] || fail "charon ran the client updown (as root)"
        grep -q "ignoring updown command supplied by client" < <(docker logs "$ENGINE" 2>&1) ||
            fail "charon did not log that it ignored the client updown"
        ok "client updown ignored: none of its commands ran"
        route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
        [[ $route == *"192.0.2.0/24"* ]] || fail "the forced updown did not install the route: '$route'"
        [[ $(resolvectl domain "$IFACE") == *"~ipsec-client.test"* ]] ||
            fail "the forced updown did not apply the DNS from root"
        ok "the forced updown ran: route 192.0.2.0/24 and DNS ~ipsec-client.test"
        states=$(count_xfrm state)
        local evil
        evil=$(host_ip -o xfrm state | grep -c "if_id 0x3e7" || true)
        ((states >= 2 && evil == 0)) || fail "client if_id used ($states SAs with $HEX_ID, $evil with 0x3e7)"
        ok "SAs with the forced if_id $HEX_ID (none with the client 999)"
        docker stop -t 10 "$ENGINE" >/dev/null
        ip link show "$IFACE" >/dev/null 2>&1 && fail "$IFACE was left over"
        ok "cleanup ok"
    fi

    step "Result"
    ok "all scenarios passed"
}

main "$@"
