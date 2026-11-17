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
# helper USER JSON [--hold SECONDS | --drip SECONDS]: raw request to the helper.
helper() { runuser -u "$1" -- python3 -I /test/helper_client.py "${@:2}"; }
# The output of `ip xfrm state` has keys: it is only counted, never printed.
count_xfrm_state() { ip -o xfrm state 2>/dev/null | grep -c "if_id $1" || true; }

host_is_clean() {
    ip link show "$IFACE" >/dev/null 2>&1 && fail "$IFACE was left over"
    [[ -z $(ip -4 route show proto "$RT_PROTO") ]] || fail "proto $RT_PROTO routes were left over"
    [[ ! -e $ALICE_SOCKET ]] || fail "the VICI socket was left over"
    ok "nothing left: interface, routes and socket"
}

engine_prop() { systemctl show ipsec-client-engine -p "$1" --value; }

# Canary sweep (audit of 0.2.0): the PSK and the password must exist nowhere
# outside charon's memory, and the username only in the system journal.
# The patterns go in files under /dev (left out of the sweep), so that grep's
# own command line does not match. Only places are printed, never contents.
readonly SECRETS_FILE=/dev/shm/canary-secrets
readonly USER_FILE=/dev/shm/canary-user
printf '%s\n' ipsec-client-test-psk senha-de-teste >"$SECRETS_FILE"
printf '%s\n' usuario.teste >"$USER_FILE"

# files_with PATTERNS_FILE ROOT...: regular files under the roots that contain
# a pattern. /proc, /sys, /dev and the test material (/test) are left out.
files_with() {
    local patterns=$1
    shift
    find "$@" \( -path /proc -o -path /sys -o -path /dev -o -path /test \) -prune \
        -o -type f -print0 2>/dev/null |
        xargs -0 -r grep -l -a -F -f "$patterns" 2>/dev/null || true
}
# procs_with PATTERNS_FILE: /proc/<pid>/{cmdline,environ} that contain a pattern.
procs_with() {
    grep -l -a -F -f "$1" /proc/[0-9]*/cmdline /proc/[0-9]*/environ 2>/dev/null || true
}
# text_has PATTERNS_FILE TEXT: does the text contain a pattern?
text_has() { grep -q -a -F -f "$1" <<<"$2"; }
# must_be_empty WHAT LIST: fails listing the places (never the contents).
must_be_empty() {
    [[ -z $2 ]] && return 0
    printf '%s\n' "$2" | sed 's/^/    /' >&2
    fail "$1"
}

step "Test computer"
# shellcheck source=/dev/null
ok "$(. /etc/os-release && printf '%s' "$PRETTY_NAME"): systemd $(systemctl --version | awk 'NR == 1 { print $2 }'), iproute2 $(ip -V | grep -oE 'iproute2-[^, ]+' | cut -d- -f2), bash ${BASH_VERSION%%(*}, glibc $(ldd --version | awk 'NR == 1 { print $NF }')"

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
# Some base images come with a user of their own on uid 1000 (Ubuntu 24.04).
for uid in 1000 1001; do
    existing=$(getent passwd "$uid" | cut -d: -f1 || true)
    [[ -z $existing ]] || userdel -r "$existing" 2>/dev/null
done
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

step "Canary sweep while connected: credentials only in charon's memory"
must_be_empty "PSK or password in a file" "$(files_with "$SECRETS_FILE" /)"
must_be_empty "PSK or password in a process command line or environment" "$(procs_with "$SECRETS_FILE")"
ok "PSK and password: in no file and in no process command line or environment"
journal=$(journalctl --no-pager -o cat)
text_has "$SECRETS_FILE" "$journal" && fail "PSK or password in the journal"
units=$(systemctl show ipsec-client-engine ipsec-client-helper.service ipsec-client-helper.socket)
text_has "$SECRETS_FILE" "$units" && fail "PSK or password in the unit properties"
text_has "$USER_FILE" "$units" && fail "username in the unit properties"
ok "PSK and password: not in the journal nor in the unit properties (systemctl show)"
# The username is an identity: charon logs it, so it exists in the system
# journal (root and the journal groups) and nowhere else.
must_be_empty "username in a file outside the journal" \
    "$(files_with "$USER_FILE" / | grep -v -E '^/(var|run)/log/journal/' || true)"
must_be_empty "username in a process command line or environment" "$(procs_with "$USER_FILE")"
ok "username: only in the system journal"
for cmd in "status" "logs -n 500" "logs -n 500 --anonymize" "profiles show gateway-teste"; do
    # shellcheck disable=SC2086  # $cmd is a list of arguments
    out=$(as alice "$CLI" $cmd 2>&1) || true
    text_has "$SECRETS_FILE" "$out" && fail "PSK or password in the output of '$cmd'"
    text_has "$USER_FILE" "$out" && fail "raw username in the output of '$cmd'"
done
as alice "$CLI" profiles export gateway-teste /tmp/alice-config/exported.json >/dev/null
exported=$(cat /tmp/alice-config/exported.json)
text_has "$SECRETS_FILE" "$exported" && fail "PSK or password in the exported profile"
text_has "$USER_FILE" "$exported" && fail "username in the exported profile"
ok "CLI output (status, logs, anonymized logs, profile) and exported profile: no credentials, username masked"
[[ $(engine_prop LimitCORE) == 0 ]] || fail "the engine can leave a core dump (LimitCORE)"
[[ $(systemctl show ipsec-client-helper.service -p LimitCORE --value) == 0 ]] ||
    fail "the helper can leave a core dump (LimitCORE)"
ok "no core dumps: LimitCORE=0 in the engine and in the helper"
helper_prop() { systemctl show ipsec-client-helper.service -p "$1" --value; }
for prop in ProtectKernelLogs=yes NoNewPrivileges=yes ProtectSystem=strict UMask=0077; do
    [[ $(helper_prop "${prop%%=*}") == "${prop#*=}" ]] || fail "helper: $prop not applied"
done
[[ -n $(helper_prop SystemCallFilter) ]] || fail "helper: SystemCallFilter not applied"
ok "helper sandbox: ProtectKernelLogs, UMask and SystemCallFilter applied (it served the requests above)"
# Sandbox score (information for the audit report; lower is better).
for unit in ipsec-client-helper.service ipsec-client-engine.service; do
    score=$(systemd-analyze security --no-pager "$unit" 2>/dev/null | tail -n 1 || true)
    ok "systemd-analyze security: ${score:-not available}"
done

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

# bob opens more connections than the helper serves at once, half of them
# idle and half dripping one byte per second (each byte inside a per-read
# timeout): alice must still be served, and the drip must be cut by the
# request deadline (3 s), not kept until it ends (20 s).
for _ in 1 2 3 4 5; do helper bob '{}' --hold 6 & done
for _ in 1 2 3 4 5; do helper bob '{}' --drip 20 & done
sleep 0.3
timeout 2 runuser -u alice -- python3 -I /test/helper_client.py '{"op":"status"}' >/dev/null ||
    fail "bob's idle and slow connections blocked the helper for alice"
started=$SECONDS
wait
((SECONDS - started < 12)) || fail "a dripping client held its connection for $((SECONDS - started)) s"
ok "ten idle or dripping connections from bob do not block alice; the drip is cut by the deadline"

step "alice: status, log and disconnection"
out=$(as alice "$CLI" status) || fail "status should exit with 0 when connected"
[[ $out == *"Virtual IP: 198.51.100."* ]] || fail "status without VIP"
out=$(as alice "$CLI" logs -n 200)
[[ $out == *ready* ]] || fail "log without the readiness line"
out=$(as alice "$CLI" disconnect)
[[ $out == *Disconnected* ]] || fail "disconnect failed"
host_is_clean
must_be_empty "credentials left in a file after disconnecting" "$(files_with "$SECRETS_FILE" /)"
[[ ! -e /run/ipsec-client-engine ]] || fail "the engine runtime directory was left over"
ok "after disconnecting: no credential in any file, engine runtime directory removed"

step "bob connects after alice: he must not get the log of her session"
reply=$(helper bob '{"op":"start","iface":"ipsc0","if_id":777,"mtu":1400,"log_level":1,"profile_id":"bob","dns":{"servers":[],"domains":[],"use_received":false,"default_route":false}}')
[[ $reply == *'"result":"started"'* ]] || fail "bob's start failed: $reply"
# The journal may take a moment to have the last line.
for _ in 1 2 3 4 5 6 7 8 9 10; do
    reply=$(helper bob '{"op":"logs","tail":2000}')
    [[ $reply == *"ready: VICI socket handed over to uid 1001"* ]] && break
    sleep 0.3
done
[[ $reply == *"ready: VICI socket handed over to uid 1001"* ]] || fail "bob does not get his own log: $reply"
text_has "$USER_FILE" "$reply" && fail "bob's log has alice's username (lines of her session)"
[[ $reply != *"uid 1000"* && $reply != *"$GW_IP"* ]] || fail "bob's log has lines of alice's session"
# The whole unit journal does have her session: the filter is what hides it.
text_has "$USER_FILE" "$(journalctl -u ipsec-client-engine --no-pager -o cat)" ||
    fail "the test lost its reference: alice's session is not in the unit journal"
ok "bob's log has only his own session (alice's is in the journal, out of his reach)"
reply=$(helper bob '{"op":"stop"}')
[[ $reply == *'"was_running":true'* ]] || fail "bob's stop failed: $reply"
host_is_clean

step "A client asks root to take over an interface that is not the VPN's"
ip link add br-audit type bridge
ip link add ipsc0 type bridge
for name in br-audit eth0 lo; do
    reply=$(helper alice "{\"op\":\"start\",\"iface\":\"$name\",\"if_id\":4242,\"mtu\":1400,\"log_level\":1,\"profile_id\":\"attack\"}")
    [[ $reply == *'"result":"error"'* && $reply == *"invalid interface name"* ]] ||
        fail "the helper accepted the interface $name: $reply"
done
ip link show br-audit >/dev/null 2>&1 || fail "root deleted br-audit for the client"
ok "the helper refuses any interface name that is not ipsc<N> (br-audit, eth0, lo)"
# The right name, but the link is something else (not an XFRM interface):
# the engine must leave it alone and give up.
reply=$(helper alice '{"op":"start","iface":"ipsc0","if_id":4242,"mtu":1400,"log_level":1,"profile_id":"attack"}')
[[ $reply == *'"result":"error"'* && $reply == *"not an XFRM interface"* ]] ||
    fail "the engine started over a foreign ipsc0: $reply"
[[ $(ip -d link show dev ipsc0) == *bridge* ]] || fail "the engine deleted a foreign ipsc0"
ok "an existing ipsc0 that is not an XFRM interface is left alone, and the engine does not start"
ip link del ipsc0
ip link del br-audit

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

step "Without systemd-resolved (the default on Debian and openSUSE)"
out=$(as alice "$CLI" doctor 2>&1 || true)
grep -q '^✔ systemd-resolved: ' <<<"$out" || {
    printf '%s\n' "$out"
    fail "doctor does not see systemd-resolved running"
}
# Masked, so that resolvectl cannot start it again through D-Bus. Its files
# in /run/systemd/resolve stay: the diagnostics must not trust them.
systemctl mask --now systemd-resolved.service >/dev/null 2>&1
out=$(as alice "$CLI" doctor 2>&1 || true)
grep -q '^⚠ systemd-resolved: ' <<<"$out" || {
    printf '%s\n' "$out"
    fail "doctor does not warn that systemd-resolved is missing"
}
ok "doctor warns: $(grep '^⚠ systemd-resolved: ' <<<"$out" | head -n 1)"
out=$(as alice "$CLI" connect gateway-teste --detach --credentials-stdin <<<"$CREDS" 2>&1) || {
    printf '%s\n' "$out"
    fail "connect failed without systemd-resolved"
}
ping -c 2 -W 2 -q 192.0.2.1 >/dev/null || fail "no traffic through the tunnel without systemd-resolved"
out=$(as alice "$CLI" logs -n 500)
grep -q "warning: failed to set the DNS servers" <<<"$out" ||
    fail "the log does not say that the VPN DNS was not applied"
as alice "$CLI" disconnect >/dev/null
host_is_clean
ok "the tunnel works by IP, the log says the DNS was not applied, and nothing is left"

step "Authorization by the real polkit of this distribution"
# Until here the helper used a uid list. Without it, every request goes to
# polkit: the policy must be known to it, and the question the helper asks
# (process, start time, uid) must be understood by this polkit version.
rm /etc/systemd/system/ipsec-client-helper.service.d/test.conf
systemctl daemon-reload
systemctl stop ipsec-client-helper.service 2>/dev/null || true
# The output decides: pkaction 0.105 (Ubuntu 22.04) exits with 1 even when it
# finds the action.
out=$(pkaction --action-id io.github.lucastelso.ipsecclient.connect 2>&1 || true)
[[ $out == io.github.lucastelso.ipsecclient.connect* ]] ||
    fail "polkit does not know the action of the installed policy: $out"
# alice has no local active session here (no login manager): a clean "no"
# from polkit, not a failure to ask.
reply=$(helper alice '{"op":"status"}')
[[ $reply == *'"result":"error"'* && $reply == *"only the user of the active local session"* ]] ||
    fail "polkit should have refused alice (no active session): $reply"
# root is always authorized by polkit.
reply=$(python3 -I /test/helper_client.py '{"op":"status"}')
[[ $reply == *'"result":"status"'* && $reply == *'"running":false'* ]] || fail "polkit should have authorized root: $reply"
ok "polkit $(pkaction --version | awk '{ print $NF }'): policy registered, a user without an active session is refused, root is authorized"

step "Result"
ok "embedded engine with systemd: tunnel, sandbox, user isolation and attack blocked"
