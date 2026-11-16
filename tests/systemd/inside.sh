#!/usr/bin/env bash
# The part of tests/systemd/run.sh that runs INSIDE the test "computer"
# (Fedora container with systemd), as root. Credentials and addresses belong
# to the test gateway (FAKE, RFC 5737).
#
# Usage: inside.sh <test gateway IP>

set -euo pipefail

readonly GW_IP=${1:?gateway ip}
readonly CLI=/test/ipsec-client-cli
readonly IFACE=ipsc0
readonly RT_PROTO=241
readonly ALICE_SOCKET=/run/ipsec-client/1000/charon.vici
readonly SWANCTL=/opt/ipsec-client/engine/sbin/swanctl
readonly CREDS='psk=ipsec-client-test-psk
xauth-user=usuario.teste
xauth-password=senha-de-teste'

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

# as USER CMD...: runs as the user, with their own config and the embedded
# engine forced.
as() {
    local user=$1
    shift
    runuser -u "$user" -- env XDG_CONFIG_HOME="/tmp/$user-config" \
        IPSEC_CLIENT_BACKEND=native "$@"
}
# helper USER JSON: raw request to the helper.
helper() { runuser -u "$1" -- python3 -I /test/helper_client.py "$2"; }
# The output of `ip xfrm state` has keys: it is only counted, never printed.
count_xfrm_state() { ip -o xfrm state 2>/dev/null | grep -c "if_id $1" || true; }

host_is_clean() {
    ip link show "$IFACE" >/dev/null 2>&1 && fail "$IFACE was left over"
    [[ -z $(ip -4 route show proto "$RT_PROTO") ]] || fail "proto $RT_PROTO routes were left over"
    [[ ! -e $ALICE_SOCKET ]] || fail "the VICI socket was left over"
    ok "nothing left: interface, routes and socket"
}

engine_prop() { systemctl show ipsec-client-engine -p "$1" --value; }

step "Installation with the real installer"
bash /test/bundle/install-native.sh install --bundle /test/bundle --origin manual \
    >/tmp/install.log 2>&1 || {
    cat /tmp/install.log
    fail "the installer failed"
}
# No polkit in the container: authorization by a uid list. Only root can
# change the helper command line.
install -d /etc/systemd/system/ipsec-client-helper.service.d
printf '[Service]\nExecStart=\nExecStart=/opt/ipsec-client/bin/ipsec-client-helper --allow-uid 1000 --allow-uid 1001\n' \
    >/etc/systemd/system/ipsec-client-helper.service.d/test.conf
systemctl daemon-reload
systemctl stop ipsec-client-helper.service 2>/dev/null || true
useradd -m -u 1000 alice
useradd -m -u 1001 bob
ok "$(tail -n 1 /tmp/install.log)"

step "alice connects with ipsec-client-cli (the profile DNS goes through the helper)"
sed "s/\"gw.ipsec-client.test\"/\"$GW_IP\"/" /test/perfil-gateway-teste.json >/tmp/profile.json
chmod 0644 /tmp/profile.json
as alice "$CLI" profiles import /tmp/profile.json >/dev/null
out=$(as alice "$CLI" connect gateway-teste --detach --credentials-stdin <<<"$CREDS" 2>&1) || {
    printf '%s\n' "$out"
    fail "connect failed"
}
[[ $out == *"Connected: gateway-teste"* ]] || fail "unexpected connect output: $out"
ok "connected"

[[ $(stat -c '%a %U' "$ALICE_SOCKET") == "600 alice" ]] || fail "VICI socket without 0600/alice"
[[ $(stat -c '%a %U %G' "$(dirname "$ALICE_SOCKET")") == "711 root root" ]] ||
    fail "socket directory without 0711 root:root"
ok "VICI socket 0600 owned by alice, directory 0711 owned by root"

route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
[[ $route == *"192.0.2.0/24"* ]] || fail "route missing: '$route'"
[[ $(resolvectl domain "$IFACE") == *"~ipsec-client.test"* ]] || fail "profile domain missing"
[[ $(resolvectl dns "$IFACE") == *192.0.2.53* ]] || fail "DNS received from the gateway missing"
ping -c 2 -W 2 -q 192.0.2.1 >/dev/null || fail "ping through the tunnel failed"
ok "route 192.0.2.0/24, DNS ~ipsec-client.test + 192.0.2.53 (through the helper) and ping through the tunnel"

step "Sandbox of the engine unit (systemd-run)"
for prop in PrivateDevices=yes ProtectKernelTunables=yes ProtectKernelLogs=yes \
    ProtectProc=invisible PrivateIPC=yes NoNewPrivileges=yes ProtectSystem=strict; do
    [[ $(engine_prop "${prop%%=*}") == "${prop#*=}" ]] || fail "$prop not applied"
done
[[ -n $(engine_prop SystemCallFilter) ]] || fail "SystemCallFilter not applied"
pid=$(engine_prop MainPID)
[[ $(awk '$5 == "/proc/sys" { print $6 }' "/proc/$pid/mountinfo") == ro* ]] ||
    fail "/proc/sys writable inside the engine"
# has_disks DIR: are there disk nodes (sdX, nvmeN, vdX) in the given /dev?
has_disks() { compgen -G "$1/sd[a-z]" >/dev/null || compgen -G "$1/nvme[0-9]*" >/dev/null ||
    compgen -G "$1/vd[a-z]" >/dev/null; }
if has_disks /dev && has_disks "/proc/$pid/root/dev"; then
    fail "the engine sees the computer's disks"
fi
ok "PrivateDevices, ProtectKernelTunables (read-only /proc/sys), ProtectKernelLogs, ProtectProc, PrivateIPC, SystemCallFilter"

step "bob (another user) tries to touch alice's VPN"
reply=$(helper bob '{"op":"stop"}')
[[ $reply == *'"result":"error"'* && $reply == *"another user"* ]] || fail "bob stopped alice's VPN: $reply"
ip link show "$IFACE" >/dev/null 2>&1 || fail "alice's VPN went down"
reply=$(helper bob '{"op":"logs","tail":50}')
[[ $reply == '{"result":"logs","lines":[]}' ]] || fail "bob read alice's log"
reply=$(helper bob '{"op":"status"}')
[[ $reply == *'"running":true'* && $reply == *'"profile_id":null'* ]] || fail "bob saw alice's profile: $reply"
if runuser -u bob -- python3 -I -c "import socket; socket.socket(socket.AF_UNIX).connect('$ALICE_SOCKET')" 2>/dev/null; then
    fail "bob connected to alice's VICI socket"
fi
ok "bob cannot stop it, read its log, see the profile or reach alice's VICI"

for _ in 1 2 3; do helper bob '{}' --hold 5 & done
sleep 0.3
timeout 2 runuser -u alice -- python3 -I /test/helper_client.py '{"op":"status"}' >/dev/null ||
    fail "idle clients blocked the helper"
wait
ok "three idle connections from bob do not block the helper"

step "alice: status, log and disconnection"
out=$(as alice "$CLI" status) || fail "status should exit with 0 when connected"
[[ $out == *"Virtual IP: 198.51.100."* ]] || fail "status without VIP"
out=$(as alice "$CLI" logs -n 200)
[[ $out == *ready* ]] || fail "log without the readiness line"
out=$(as alice "$CLI" disconnect)
[[ $out == *Disconnected* ]] || fail "disconnect failed"
host_is_clean

step "Attack (ADR-0019): a VICI client picks the updown and the if_id"
sed -e "s|@GATEWAY@|$GW_IP|" -e "s|@IFACE@|$IFACE|" -e "s|@IF_ID@|999|" \
    -e "s|@REMOTE_TS@|192.0.2.0/24|" -e "s|@UPDOWN_EXTRA@||" \
    -e "s|updown = .*|updown = /bin/touch /run/ipsec-client/1000/PWNED|" \
    /test/initiator.conf.in >/tmp/attack.conf
chmod 0644 /tmp/attack.conf
reply=$(helper alice '{"op":"start","iface":"ipsc0","if_id":4242,"mtu":1400,"log_level":1,"profile_id":"attack","dns":{"servers":["192.0.2.54"],"domains":["~ipsec-client.test"],"use_received":false,"default_route":false}}')
[[ $reply == *'"result":"started"'* ]] || fail "start failed: $reply"
runuser -u alice -- "$SWANCTL" --load-all --uri "unix://$ALICE_SOCKET" --file /tmp/attack.conf \
    >/tmp/load.log 2>&1 || {
    cat /tmp/load.log
    fail "swanctl --load-all failed"
}
runuser -u alice -- "$SWANCTL" --initiate --uri "unix://$ALICE_SOCKET" --child rede-teste \
    --timeout 30 >/tmp/initiate.log 2>&1 || {
    cat /tmp/initiate.log
    fail "the negotiation failed"
}
[[ ! -e /run/ipsec-client/1000/PWNED ]] || fail "charon ran the client's command as root"
# Read in full before grep: with pipefail, `grep -q` would exit early and
# journalctl would die with SIGPIPE.
journal=$(journalctl -u ipsec-client-engine --no-pager)
grep -q "ignoring updown command supplied by client" <<<"$journal" ||
    fail "charon did not log that it ignored the client updown"
ok "the client's command did not run (charon ignored its updown)"
route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
[[ $route == *"192.0.2.0/24"* ]] || fail "the forced updown did not run: '$route'"
[[ $(resolvectl dns "$IFACE") == *192.0.2.54* ]] || fail "the helper DNS was not applied"
forced=$(count_xfrm_state 0x1092)
evil=$(count_xfrm_state 0x3e7)
((forced >= 2 && evil == 0)) || fail "client if_id used ($forced with 0x1092, $evil with 0x3e7)"
ok "the forced updown ran (route and helper DNS) and the SAs use the helper's if_id"
reply=$(helper alice '{"op":"stop"}')
[[ $reply == *'"was_running":true'* ]] || fail "stop failed: $reply"
host_is_clean

step "Result"
ok "embedded engine with systemd: tunnel, sandbox, user isolation and attack blocked"
