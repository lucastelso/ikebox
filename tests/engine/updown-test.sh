#!/usr/bin/env bash
# Offline test of engine/updown.sh: no root, no Docker. Fake `ip` and
# `resolvectl` record every call, so the test checks which routes the script
# asks for and in which order.
#
# Covers the gateway host route (audit of 0.2.0, P11): when the remote
# network contains the gateway, the route to the gateway through the original
# path comes before the route of the network, and goes away with it. And the
# profiles without a virtual IP (phase 6): charon says "up-host" when our
# side is this computer's own address, and the remote network still needs
# its route.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# IPSEC_CLIENT_UPDOWN_UNDER_TEST: another copy of the script (to check that the
# test catches an old version).
readonly UPDOWN=${IPSEC_CLIENT_UPDOWN_UNDER_TEST:-"$repo/engine/updown.sh"}

ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/bin"
# Fake ip: logs the call; `route get` answers with the physical path, and
# `route show` with nothing (no route of ours yet).
cat >"$work/bin/ip" <<'STUB'
#!/usr/bin/env bash
printf 'ip %s\n' "$*" >>"$CALLS"
if [[ " $* " == *" route get "* ]]; then
    printf '%s via 10.0.0.1 dev eth0 src 10.0.0.2 uid 1000\n' "${*: -1}"
fi
exit 0
STUB
printf '#!/bin/sh\nexit 0\n' >"$work/bin/resolvectl"
chmod 0755 "$work/bin/ip" "$work/bin/resolvectl"

# run VERB PEER PEER_CLIENT [SOURCE_IP]: runs updown and leaves its ip calls
# in $CALLS. SOURCE_IP is the virtual IP (default 198.51.100.10; "" = none).
run() {
    export CALLS="$work/calls-$RANDOM"
    : >"$CALLS"
    PATH="$work/bin:$PATH" PLUTO_VERB=$1 PLUTO_PEER=$2 PLUTO_PEER_CLIENT=$3 \
        PLUTO_MY_SOURCEIP4_1=${4-198.51.100.10} bash "$UPDOWN" --iface ipsc0 >/dev/null
}
# line_of PATTERN: number of the first call that matches (0 if none).
line_of() { grep -n -F -- "$1" "$CALLS" | head -n 1 | cut -d: -f1 || true; }

echo "updown: routes and the gateway host route"

run up-client 192.0.2.1 192.0.2.0/24
host=$(line_of "route replace 192.0.2.1/32 via 10.0.0.1 dev eth0 proto 241")
net=$(line_of "route replace 192.0.2.0/24 dev ipsc0 src 198.51.100.10 proto 241")
[[ -n $host && -n $net ]] || fail "network containing the gateway: missing routes: $(cat "$CALLS")"
((host < net)) || fail "the gateway route must come before the network route"
ok "remote network containing the gateway: host route through the original path, first"

run down-client 192.0.2.1 192.0.2.0/24
[[ -n $(line_of "route del 192.0.2.0/24 dev ipsc0 proto 241") ]] || fail "network route not removed"
[[ -n $(line_of "route del 192.0.2.1/32 proto 241") ]] || fail "gateway route not removed"
ok "going down removes both routes"

run up-client 203.0.113.7 192.0.2.0/24
[[ -z $(line_of "203.0.113.7/32") ]] || fail "host route for a gateway outside the network: $(cat "$CALLS")"
[[ -n $(line_of "route replace 192.0.2.0/24 dev ipsc0") ]] || fail "network route missing"
run down-client 203.0.113.7 192.0.2.0/24
[[ -z $(line_of "203.0.113.7/32") ]] || fail "removing a gateway route that was never added"
ok "gateway outside the remote network: no host route"

run up-client 192.0.2.1 192.0.2.1/32
[[ -z $(line_of "route replace") ]] || fail "the remote TS is the gateway itself: nothing to route"
ok "remote TS is the gateway itself: no route"

run up-client 203.0.113.7 0.0.0.0/0
host=$(line_of "route replace 203.0.113.7/32 via 10.0.0.1 dev eth0 proto 241")
half=$(line_of "route replace 0.0.0.0/1 dev ipsc0")
if [[ -z $host || -z $half ]] || ((host > half)); then
    fail "full tunnel: $(cat "$CALLS")"
fi
ok "full tunnel: host route first, as before"

run up-client 198.51.100.200 198.51.100.128/25
[[ -n $(line_of "route replace 198.51.100.200/32") ]] || fail "/25 containing the gateway"
run up-client 198.51.100.100 198.51.100.128/25
[[ -z $(line_of "198.51.100.100/32") ]] || fail "/25 not containing the gateway"
ok "prefix boundaries (/25 in and out)"

echo "updown: no virtual IP (up-host)"

run up-host 203.0.113.7 192.0.2.0/24 ""
[[ -n $(line_of "route replace 192.0.2.0/24 dev ipsc0 proto 241") ]] ||
    fail "no virtual IP: network route missing: $(cat "$CALLS")"
[[ -z $(line_of " src ") ]] || fail "no virtual IP: the route must not carry a source: $(cat "$CALLS")"
run down-host 203.0.113.7 192.0.2.0/24 ""
[[ -n $(line_of "route del 192.0.2.0/24 dev ipsc0 proto 241") ]] || fail "no virtual IP: route not removed"
ok "our side is this computer's address: the remote network is routed, and removed on down"

run up-host 203.0.113.7 203.0.113.7/32 ""
[[ -z $(line_of "route replace") ]] || fail "host-to-host: nothing to route"
ok "true host-to-host (remote TS is the gateway): no route"
