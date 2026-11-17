#!/usr/bin/env bash
# The part of tests/modes/run.sh that runs INSIDE the test "computer" (the
# container of tests/systemd, with the real helper and the embedded engine),
# as root. Each scenario imports a profile, connects with ipsec-client-cli as
# alice, checks what was negotiated and disconnects.
#
# Usage: inside.sh MODE=GATEWAY_IP...   (modes: the files in tests/modes/gateways)
#
# What was negotiated is read from charon (swanctl --list-sas over alice's own
# VICI socket: no key material in that output) and from the kernel (counts
# only: `ip xfrm state` has keys).

set -euo pipefail

# shellcheck source=tests/systemd/lib.sh
. /test/lib.sh

readonly PSK='psk=ipsec-client-test-psk'
readonly XAUTH='psk=ipsec-client-test-psk
xauth-user=usuario.teste
xauth-password=senha-de-teste'

declare -A GW
for arg in "$@"; do
    GW[${arg%%=*}]=${arg#*=}
done

sas() { runuser -u alice -- "$SWANCTL" --list-sas --uri "unix://$ALICE_SOCKET" 2>/dev/null; }
# child_line: the CHILD_SA line of the profile child ("rede: #1, ... ESP:...").
child_line() { sas | grep -E '^ +rede: #[0-9]+, .*INSTALLED' | tail -n 1; }

# connect ID CREDENTIALS key=value...: writes the profile, imports and connects.
connect() {
    local id=$1 creds=$2 out
    shift 2
    python3 -I /test/modes/make_profile.py "/tmp/modes/$id.json" id="$id" "$@"
    as alice "$CLI" profiles import /tmp/modes/"$id".json >/dev/null
    out=$(as alice "$CLI" connect "$id" --detach --credentials-stdin <<<"$creds" 2>&1) || {
        printf '%s\n' "$out"
        as alice "$CLI" logs -n 80 || true
        fail "$id: connect failed"
    }
    [[ $out == *"Connected: $id"* ]] || fail "$id: unexpected connect output: $out"
}

# expect_ike VERSION MODE: IKE version from charon, mode from the session log.
expect_ike() {
    local version=$1 mode=$2 text
    # Whole outputs first: `cmd | grep -q` under pipefail fails when grep
    # stops reading early.
    text=$(sas)
    [[ $text =~ ESTABLISHED,\ IKEv$version, ]] || fail "IKE_SA is not IKEv$version"
    if [[ -n $mode ]]; then
        text=$(as alice "$CLI" logs -n 300)
        [[ $text == *"initiating $mode Mode IKE_SA"* ]] || fail "the log does not show $mode Mode"
    fi
}

# expect_vip yes|no
expect_vip() {
    local addr
    addr=$(ip -4 -o addr show dev "$IFACE" | awk '{ print $4 }')
    if [[ $1 == yes ]]; then
        [[ $addr == 198.51.100.* ]] || fail "virtual IP missing on $IFACE ('$addr')"
    else
        [[ -z $addr ]] || fail "virtual IP on $IFACE without asking for one ('$addr')"
    fi
}

# expect_routes split|full GATEWAY
expect_routes() {
    local routes
    routes=$(ip -4 route show proto "$RT_PROTO")
    if [[ $1 == split ]]; then
        [[ $routes == *"192.0.2.0/24 dev $IFACE"* ]] || fail "split route missing: $routes"
        [[ $routes != *"0.0.0.0/1"* ]] || fail "full tunnel routes in a split tunnel: $routes"
    else
        [[ $routes == *"0.0.0.0/1 dev $IFACE"* && $routes == *"128.0.0.0/1 dev $IFACE"* ]] ||
            fail "full tunnel /1 routes missing: $routes"
        [[ $routes == *"$2 "*"dev eth0"* ]] || fail "host route to the gateway missing: $routes"
    fi
}

# expect_esp udp|native GROUP|none: encapsulation and the PFS group of phase 2.
expect_esp() {
    local line
    line=$(child_line)
    [[ -n $line ]] || fail "no installed CHILD_SA"
    if [[ $1 == udp ]]; then
        [[ $line == *TUNNEL-in-UDP* ]] || fail "ESP is not in UDP (NAT-T): $line"
    else
        [[ $line == *"TUNNEL,"* ]] || fail "ESP should not be in UDP: $line"
    fi
    if [[ $2 == none ]]; then
        [[ $line != *MODP_* && $line != *CURVE_* && $line != *ECP_* ]] ||
            fail "phase 2 negotiated a DH group without PFS: $line"
    else
        [[ $line == *"/$2"* ]] || fail "phase 2 without the PFS group $2: $line"
    fi
    (($(count_xfrm_state "$ENGINE_IF_ID") >= 2)) || fail "no SAs with the engine if_id"
}

# expect_failure ID CREDENTIALS MESSAGE key=value...: the connection must fail
# with MESSAGE (the diagnosis of the CLI) and leave nothing behind. A wrong
# diagnosis is counted and printed with charon's lines, and the next cases
# still run: one run shows every line to learn from.
wrong_diagnoses=0
expect_failure() {
    local id=$1 creds=$2 message=$3 out
    shift 3
    python3 -I /test/modes/make_profile.py "/tmp/modes/$id.json" id="$id" "$@"
    as alice "$CLI" profiles import --force /tmp/modes/"$id".json >/dev/null
    if out=$(as alice "$CLI" connect "$id" --detach --credentials-stdin <<<"$creds" 2>&1); then
        as alice "$CLI" disconnect >/dev/null || true
        fail "$id: should not connect"
    fi
    host_is_clean
    if [[ $out != *"$message"* ]]; then
        printf '%s\n' "$out" | grep -vF 'keyring is not available' >&2
        printf '  \033[0;31m✘ %s\033[0m\n' "$id: expected the diagnosis \"$message\"" >&2
        wrong_diagnoses=$((wrong_diagnoses + 1))
        return 0
    fi
    ok "$id: $(grep -F '✘' <<<"$out" | head -n 1)"
}

traffic() { ping -c 2 -W 2 -q 192.0.2.1 >/dev/null || fail "no traffic through the tunnel"; }

disconnect() {
    as alice "$CLI" disconnect >/dev/null
    host_is_clean
}

install -d -m 0755 /tmp/modes
describe_computer
install_component

# ---------------------------------------------------------------------------
step "IKEv1 main mode + group PSK + XAuth: virtual IP, split tunnel, PFS, plain ESP"
gw=${GW[ikev1-main-xauth]}
connect v1-main-xauth "$XAUTH" gateway="$gw" version=1 method=psk-xauth \
    ike=aes256-sha256-modp2048 esp=aes256-sha256-modp2048 remote_ts=192.0.2.0/24
expect_ike 1 Main
expect_vip yes
expect_routes split
expect_esp native MODP_2048
traffic
ok "main mode, XAuth, VIP $(ip -4 -o addr show dev "$IFACE" | awk '{ print $4 }'), PFS MODP_2048, ESP without UDP"
disconnect

step "IKEv1 main mode + group PSK + XAuth: full tunnel, forced NAT-T, no PFS"
connect v1-main-xauth-full "$XAUTH" gateway="$gw" version=1 method=psk-xauth encap=yes \
    ike=aes256-sha256-modp2048 esp=aes256-sha256 remote_ts=0.0.0.0/0
expect_ike 1 Main
expect_vip yes
expect_routes full "$gw"
expect_esp udp none
traffic
ok "full tunnel (0.0.0.0/1 + 128.0.0.0/1, gateway via eth0), ESP in UDP, phase 2 without DH"
disconnect

# ---------------------------------------------------------------------------
step "IKEv1 main mode + PSK only: virtual IP, PFS, plain ESP"
gw=${GW[ikev1-main-psk]}
connect v1-main-psk "$PSK" gateway="$gw" version=1 method=psk \
    ike=aes256-sha256-modp2048 esp=aes256-sha256-modp2048 remote_ts=192.0.2.0/24
expect_ike 1 Main
expect_vip yes
expect_routes split
expect_esp native MODP_2048
traffic
ok "main mode without XAuth: only the PSK was asked, VIP, PFS MODP_2048"
disconnect

step "IKEv1 main mode + PSK only: no virtual IP, forced NAT-T"
connect v1-main-psk-novip "$PSK" gateway="$gw" version=1 method=psk encap=yes vip=no \
    ike=aes256-sha256-modp2048 esp=aes256-sha256 remote_ts=192.0.2.0/24
expect_ike 1 Main
expect_vip no
expect_routes split
expect_esp udp none
traffic
ok "tunnel from the computer's own address (no mode config), ESP in UDP"
disconnect

# ---------------------------------------------------------------------------
step "IKEv1 aggressive mode + group PSK only (no XAuth), legacy algorithms"
gw=${GW[ikev1-aggr-psk]}
connect v1-aggr-psk "$PSK" gateway="$gw" version=1 aggressive=yes method=psk encap=yes \
    local_id=keyid:grupo-teste ike=aes256-sha1-modp1024 esp=aes256-sha1-modp1024 \
    remote_ts=192.0.2.0/24
expect_ike 1 Aggressive
expect_vip yes
expect_routes split
expect_esp udp MODP_1024
traffic
ok "aggressive mode with the group id, PSK only, PFS MODP_1024"
disconnect

# ---------------------------------------------------------------------------
step "IKEv2 + PSK: gateway identity checked, virtual IP, PFS on rekey, plain ESP"
gw=${GW[ikev2-psk]}
connect v2-psk "$PSK" gateway="$gw" version=2 method=psk remote_id=gw.ipsec-client.test \
    ike=aes256gcm16-prfsha256-curve25519 esp=aes256gcm16-curve25519 remote_ts=192.0.2.0/24
expect_ike 2 ""
expect_vip yes
expect_routes split
# The first CHILD_SA comes from IKE_AUTH: no DH yet.
expect_esp native none
traffic
runuser -u alice -- "$SWANCTL" --rekey --child rede --uri "unix://$ALICE_SOCKET" >/dev/null 2>&1 ||
    fail "CHILD_SA rekey failed"
for _ in $(seq 1 50); do
    [[ $(child_line) == *"/CURVE_25519"* ]] && break
    sleep 0.2
done
expect_esp native CURVE_25519
expect_routes split
traffic
ok "IKEv2, gateway id checked, after the rekey: PFS CURVE_25519, route kept, traffic flows"
disconnect

step "IKEv2 + PSK: full tunnel, forced NAT-T, no PFS"
connect v2-psk-full "$PSK" gateway="$gw" version=2 method=psk encap=yes \
    ike=aes256-sha256-modp2048 esp=aes256gcm16 remote_ts=0.0.0.0/0
expect_ike 2 ""
expect_vip yes
expect_routes full "$gw"
expect_esp udp none
traffic
ok "IKEv2 full tunnel, ESP in UDP"
disconnect

step "IKEv2 + PSK: no virtual IP"
connect v2-psk-novip "$PSK" gateway="$gw" version=2 method=psk vip=no \
    local_id=novip.ipsec-client.test \
    ike=aes256gcm16-prfsha256-curve25519 esp=aes256gcm16 remote_ts=192.0.2.0/24
expect_ike 2 ""
expect_vip no
expect_routes split
expect_esp native none
traffic
ok "IKEv2 from the computer's own address"
disconnect

# ---------------------------------------------------------------------------
step "Failures are diagnosed (IKEv2 and IKEv1 main mode)"
gw=${GW[ikev2-psk]}
v2=(gateway="$gw" version=2 method=psk remote_ts=192.0.2.0/24
    ike=aes256gcm16-prfsha256-curve25519 esp=aes256gcm16)
expect_failure v2-bad-psk "psk=wrong-psk" "rejected phase 1 authentication" "${v2[@]}"
# A strongSwan gateway refuses an IDr that is not its own with the same
# AUTHENTICATION_FAILED as a wrong PSK: the diagnosis names both.
expect_failure v2-bad-gateway-id "$PSK" "auth.remote_id" "${v2[@]}" remote_id=other.ipsec-client.test
expect_failure v2-bad-ike "$PSK" "none of the phase 1 proposals" \
    "${v2[@]/#ike=*/ike=aes128-sha1-modp1024}"
expect_failure v2-bad-esp "$PSK" "none of the phase 2 proposals" "${v2[@]/#esp=*/esp=aes128-sha1}"
# The connection without a pool has no full tunnel child to narrow into.
expect_failure v2-bad-ts "$PSK" "rejected the requested networks" \
    "${v2[@]/#remote_ts=*/remote_ts=203.0.113.0/24}" vip=no local_id=novip.ipsec-client.test
expect_failure v2-vip-required "$PSK" "ask for a virtual IP" "${v2[@]}" vip=no
gw=${GW[ikev1-main-psk]}
expect_failure v1-main-bad-psk "psk=wrong-psk" "rejected phase 1 authentication" \
    gateway="$gw" version=1 method=psk remote_ts=192.0.2.0/24 \
    ike=aes256-sha256-modp2048 esp=aes256-sha256
((wrong_diagnoses == 0)) || fail "$wrong_diagnoses failures without the right diagnosis"

step "Result"
ok "IPsec modes: IKEv1 main (PSK, PSK + XAuth), IKEv1 aggressive PSK, IKEv2 PSK; NAT-T, PFS, VIP, split and full tunnel; failures diagnosed"
