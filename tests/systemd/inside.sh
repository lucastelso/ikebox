#!/usr/bin/env bash
# The part of tests/systemd/run.sh that runs INSIDE the test "computer"
# (Fedora container with systemd), as root. Credentials and addresses belong
# to the test gateway (FAKE, RFC 5737).
#
# Usage: inside.sh <test gateway IP>

set -euo pipefail

readonly GW_IP=${1:?gateway ip}
readonly CREDS='psk=ikebox-test-psk
xauth-user=usuario.teste
xauth-password=senha-de-teste'

# shellcheck source=tests/systemd/lib.sh
. /test/lib.sh

# helper USER JSON [--hold SECONDS | --drip SECONDS]: raw request to the helper.
helper() { runuser -u "$1" -- python3 -I /test/helper_client.py "${@:2}"; }

engine_prop() { systemctl show ikebox-engine -p "$1" --value; }

# Canary sweep (audit of 0.2.0): the PSK and the password must exist nowhere
# outside charon's memory, and the username only in the system journal.
# The patterns go in files under /dev (left out of the sweep), so that grep's
# own command line does not match. Only places are printed, never contents.
readonly SECRETS_FILE=/dev/shm/canary-secrets
readonly USER_FILE=/dev/shm/canary-user
printf '%s\n' ikebox-test-psk senha-de-teste >"$SECRETS_FILE"
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

describe_computer
install_component

step "The gateway is named, not numbered: charon resolves it through systemd-resolved"
# As on a user's computer: /etc/resolv.conf points at the resolved stub, and
# resolved forwards the test domain to a DNS server that knows the gateway.
# charon resolves the name inside the engine sandbox, with its own C library
# (musl, statically linked: no NSS modules).
# A unit of its own, not a background job: later scenarios `wait` for theirs.
systemd-run --quiet --unit=test-dns python3 -I /test/dns_responder.py 127.0.0.2 \
    gw.ikebox.test "$GW_IP"
install -d /etc/systemd/resolved.conf.d
printf '[Resolve]\nDNS=127.0.0.2\nDomains=~ikebox.test\n' >/etc/systemd/resolved.conf.d/test.conf
systemctl restart systemd-resolved
printf 'nameserver 127.0.0.53\noptions edns0 trust-ad\n' >/etc/resolv.conf
# A few tries: resolved was just restarted. `|| true`: awk stops reading at the
# first line, and under pipefail resolvectl's SIGPIPE would end the script
# without a word (it happened once).
resolved_ip=""
for _ in $(seq 1 20); do
    resolved_ip=$(resolvectl query --legend=no -4 gw.ikebox.test 2>/dev/null |
        awk '{print $2; exit}' || true)
    [[ $resolved_ip == "$GW_IP" ]] && break
    sleep 0.25
done
[[ $resolved_ip == "$GW_IP" ]] || fail "systemd-resolved does not resolve the test gateway (got '$resolved_ip')"
ok "gw.ikebox.test resolves to $GW_IP through systemd-resolved"

step "alice connects with ikebox-cli (the profile DNS goes through the helper)"
cp /test/test-gateway.json /tmp/profile.json
chmod 0644 /tmp/profile.json
as alice "$CLI" profiles import /tmp/profile.json >/dev/null
out=$(as alice "$CLI" connect gateway-teste --detach --credentials-stdin <<<"$CREDS" 2>&1) || {
    printf '%s\n' "$out"
    fail "connect failed"
}
[[ $out == *"Connected: gateway-teste"* ]] || fail "unexpected connect output: $out"
ok "connected, with the gateway given by name"

[[ $(stat -c '%a %U' "$ALICE_SOCKET") == "600 alice" ]] || fail "VICI socket without 0600/alice"
# During the session charon's group may create files there (its socket);
# alice only traverses it (ADR-0028). Root's again after the session.
[[ $(stat -c '%a %U %G' "$(dirname "$ALICE_SOCKET")") == "1731 root ikebox-engine" ]] ||
    fail "socket directory not 1731 root:ikebox-engine during the session"
ok "VICI socket 0600 owned by alice, directory owned by root (charon's group may create its socket, alice only traverses)"

route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
[[ $route == *"192.0.2.0/24"* ]] || fail "route missing: '$route'"
eventually 10 domain_has "$IFACE" "~ikebox.test" || fail "profile domain missing"
eventually 10 dns_has "$IFACE" 192.0.2.53 || fail "DNS received from the gateway missing"
ping -c 2 -W 2 -q 192.0.2.1 >/dev/null || fail "ping through the tunnel failed"
ok "route 192.0.2.0/24, DNS ~ikebox.test + 192.0.2.53 (through the helper) and ping through the tunnel"

step "Canary sweep while connected: credentials only in charon's memory"
must_be_empty "PSK or password in a file" "$(files_with "$SECRETS_FILE" /)"
must_be_empty "PSK or password in a process command line or environment" "$(procs_with "$SECRETS_FILE")"
ok "PSK and password: in no file and in no process command line or environment"
journal=$(journalctl --no-pager -o cat)
text_has "$SECRETS_FILE" "$journal" && fail "PSK or password in the journal"
units=$(systemctl show ikebox-engine ikebox-helper.service ikebox-helper.socket)
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
[[ $(systemctl show ikebox-helper.service -p LimitCORE --value) == 0 ]] ||
    fail "the helper can leave a core dump (LimitCORE)"
ok "no core dumps: LimitCORE=0 in the engine and in the helper"
helper_prop() { systemctl show ikebox-helper.service -p "$1" --value; }
for prop in ProtectKernelLogs=yes NoNewPrivileges=yes ProtectSystem=strict UMask=0077; do
    [[ $(helper_prop "${prop%%=*}") == "${prop#*=}" ]] || fail "helper: $prop not applied"
done
[[ -n $(helper_prop SystemCallFilter) ]] || fail "helper: SystemCallFilter not applied"
ok "helper sandbox: ProtectKernelLogs, UMask and SystemCallFilter applied (it served the requests above)"
# Sandbox score (information for the audit report; lower is better).
for unit in ikebox-helper.service ikebox-engine.service; do
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

step "charon runs without root (ADR-0028), with the network capabilities only"
engine_uid=$(id -u ikebox-engine) || fail "system user ikebox-engine missing"
cpid=$(pgrep -x charon | head -n 1)
[[ -n $cpid ]] || fail "charon not found"
# Real, effective, saved and filesystem uid all the engine user's.
read -r _ r e sv fs < <(grep '^Uid:' "/proc/$cpid/status")
[[ $r == "$engine_uid" && $e == "$engine_uid" && $sv == "$engine_uid" && $fs == "$engine_uid" ]] ||
    fail "charon runs as uid $r/$e/$sv/$fs, not as ikebox-engine ($engine_uid)"
# The bounding set is NET_ADMIN (12), NET_BIND_SERVICE (10) and NET_RAW (13),
# 0x3400: no way back to anything else. charon itself drops what it no longer
# needs once its sockets are open, so its effective and ambient sets (the
# ambient ones pass to the updown) are within that, and always NET_ADMIN.
cap_set() { awk -v k="$1:" '$1 == k { print $2 }' "/proc/$cpid/status"; }
[[ $((16#$(cap_set CapBnd))) == $((16#3400)) ]] ||
    fail "charon CapBnd is $(cap_set CapBnd), expected 0000000000003400"
for set in CapEff CapAmb; do
    value=$((16#$(cap_set "$set")))
    ((value & ~16#3400)) && fail "charon $set has more than the network capabilities: $(cap_set "$set")"
    ((value & 16#1000)) || fail "charon $set lacks NET_ADMIN: $(cap_set "$set")"
done
effective=$(cap_set CapEff)
grep -q '^NoNewPrivs:[[:space:]]*1' "/proc/$cpid/status" || fail "charon may gain privileges"
[[ $(stat -c "%U %a" "$ALICE_SOCKET") == "alice 600" ]] || fail "the VICI socket is not alice's (0600)"
ok "charon: uid $engine_uid (ikebox-engine), effective capabilities $effective (network only, bounding set NET_ADMIN/NET_BIND_SERVICE/NET_RAW), no new privileges; socket handed to alice"

step "Attack: what charon's user writes to the DNS FIFO cannot reach further"
fifo=/run/ikebox-engine/dns.requests
[[ -p $fifo ]] || fail "no DNS FIFO"
# As charon's user (what a compromised charon could do): shell syntax, other
# interfaces, junk and an oversized line. Only addresses are ever read.
# shellcheck disable=SC2016 # the $(...) must reach the FIFO unexpanded
setpriv --reuid=ikebox-engine --regid=ikebox-engine --clear-groups bash -c '
    printf "%s\n" "up 198.51.100.250 \$(id >/tmp/fifo-pwned) lo; touch /tmp/fifo-pwned2" \
        "down lo" "revert eth0" "$(head -c 5000 /dev/zero | tr "\0" a)" >'"$fifo"
sleep 2.5
[[ ! -e /tmp/fifo-pwned && ! -e /tmp/fifo-pwned2 ]] || fail "a request from the FIFO ran a command"
[[ $(resolvectl dns lo 2>/dev/null) != *198.51.100.250* ]] || fail "a FIFO request changed the DNS of lo"
[[ $(engine_prop ActiveState) == active ]] || fail "the engine died on junk in the DNS FIFO"
# A well-formed "up" may change the servers received for $IFACE: charon is the
# one that reports them anyway. Nothing else can be reached.
[[ $(resolvectl dns "$IFACE") == *198.51.100.250* ]] || fail "the valid address in the request was not applied to $IFACE"
ok "shell syntax, other interfaces and junk in the FIFO: nothing ran, only $IFACE is ever touched, the engine keeps running"

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
[[ ! -e /run/ikebox-engine ]] || fail "the engine runtime directory was left over"
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
text_has "$USER_FILE" "$(journalctl -u ikebox-engine --no-pager -o cat)" ||
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
    -e "s|updown = .*|updown = /bin/touch /run/ikebox/1000/PWNED|" \
    /test/initiator.conf.in >/tmp/attack.conf
chmod 0644 /tmp/attack.conf
reply=$(helper alice '{"op":"start","iface":"ipsc0","if_id":4242,"mtu":1400,"log_level":1,"profile_id":"attack","dns":{"servers":["192.0.2.54"],"domains":["~ikebox.test"],"use_received":false,"default_route":false}}')
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
[[ ! -e /run/ikebox/1000/PWNED ]] || fail "charon ran the client's command as root"
# Read in full before grep: with pipefail, `grep -q` would exit early and
# journalctl would die with SIGPIPE.
journal=$(journalctl -u ikebox-engine --no-pager)
grep -q "ignoring updown command supplied by client" <<<"$journal" ||
    fail "charon did not log that it ignored the client updown"
ok "the client's command did not run (charon ignored its updown)"
route=$(ip -4 route show dev "$IFACE" proto "$RT_PROTO")
[[ $route == *"192.0.2.0/24"* ]] || fail "the forced updown did not run: '$route'"
eventually 10 dns_has "$IFACE" 192.0.2.54 || fail "the helper DNS was not applied"
# The helper's own if_id (0x49504331, P10): neither the one in the VICI
# load-conn (999 = 0x3e7) nor the one in the request to the helper
# (4242 = 0x1092).
forced=$(count_xfrm_state 0x49504331)
evil=$(count_xfrm_state 0x3e7)
asked=$(count_xfrm_state 0x1092)
((forced >= 2 && evil == 0 && asked == 0)) ||
    fail "if_id not the helper's ($forced with 0x49504331, $evil with 0x3e7, $asked with 0x1092)"
ok "the forced updown ran (route and helper DNS) and the SAs use the helper's own if_id"

# Audit of 0.2.0, P9: VICI can name a certificate by path ("file"), which root
# charon would open, telling the user whether the file exists. Refused.
cat >/tmp/probe.conf <<'CONF'
connections {
    probe {
        remote_addrs = 192.0.2.250
        local {
            auth = pubkey
            cert-probe {
                file = /etc/hostname
            }
        }
        remote {
            auth = pubkey
        }
    }
}
authorities {
    probe {
        file = /etc/hostname
    }
}
CONF
chmod 0644 /tmp/probe.conf
out=$(runuser -u alice -- "$SWANCTL" --load-conns --uri "unix://$ALICE_SOCKET" --file /tmp/probe.conf 2>&1 || true)
[[ $out == *"certificate files and tokens are not accepted"* ]] ||
    fail "charon accepted a certificate by path in load-conn: $out"
out=$(runuser -u alice -- "$SWANCTL" --load-authorities --uri "unix://$ALICE_SOCKET" --file /tmp/probe.conf 2>&1 || true)
[[ $out == *"CA certificate files and tokens are not accepted"* ]] ||
    fail "charon accepted a CA certificate by path in load-authority: $out"
journal=$(journalctl -u ikebox-engine --no-pager)
if ! grep -q "refusing certificate file or token" <<<"$journal" ||
    ! grep -q "refusing CA certificate file or token" <<<"$journal"; then
    fail "charon did not log the refusals"
fi
ok "certificates by path are refused in load-conn and load-authority (P9)"
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
# in /run/systemd/resolve stay: the diagnostics must not trust them. Without
# resolved, the gateway name comes from /etc/hosts.
systemctl mask --now systemd-resolved.service >/dev/null 2>&1
printf '%s gw.ikebox.test\n' "$GW_IP" >>/etc/hosts
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
rm /etc/systemd/system/ikebox-helper.service.d/test.conf
systemctl daemon-reload
systemctl stop ikebox-helper.service 2>/dev/null || true
# The output decides: pkaction 0.105 (Ubuntu 22.04) exits with 1 even when it
# finds the action.
out=$(pkaction --action-id io.github.lucastelso.ikebox.connect 2>&1 || true)
[[ $out == io.github.lucastelso.ikebox.connect* ]] ||
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
