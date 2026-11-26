# shellcheck shell=bash disable=SC2034 # constants used by the scripts that source it
# Shared by the scripts that run INSIDE the test "computer" of
# tests/systemd/run.sh (tests/systemd/inside.sh, tests/modes/inside.sh), as
# root. Credentials and addresses belong to the test gateways (FAKE,
# RFC 5737).

readonly CLI=/test/ikebox-cli
readonly IFACE=ipsc0
readonly RT_PROTO=241
readonly ALICE_SOCKET=/run/ikebox/1000/charon.vici
# Fixed if_id of the embedded engine (audit P10).
readonly ENGINE_IF_ID=0x49504331
# swanctl comes from the engine's test tools tarball: it is not shipped.
readonly SWANCTL=/test/swanctl

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
        IKEBOX_BACKEND=native "$@"
}
# The output of `ip xfrm state` has keys: it is only counted, never printed.
# eventually <seconds> <command...>: retries the command until it succeeds or
# the time is up. The engine applies the VPN DNS asynchronously: the updown,
# without root, asks for it through a FIFO and returns (ADR-0028).
eventually() {
    local deadline=$((SECONDS + $1))
    shift
    until "$@"; do
        ((SECONDS < deadline)) || return 1
        sleep 0.25
    done
}
dns_has() { [[ $(resolvectl dns "$1" 2>/dev/null) == *"$2"* ]]; }
domain_has() { [[ $(resolvectl domain "$1" 2>/dev/null) == *"$2"* ]]; }
# The routes come from the forced updown. Like the DNS, they may land a
# moment after `swanctl --initiate` returns (seen once on openSUSE Tumbleweed).
route_has() { [[ $(ip -4 route show dev "$IFACE" proto "$RT_PROTO" 2>/dev/null) == *"$1"* ]]; }

count_xfrm_state() { ip -o xfrm state 2>/dev/null | grep -c "if_id $1" || true; }

host_is_clean() {
    ip link show "$IFACE" >/dev/null 2>&1 && fail "$IFACE was left over"
    [[ -z $(ip -4 route show proto "$RT_PROTO") ]] || fail "proto $RT_PROTO routes were left over"
    [[ ! -e $ALICE_SOCKET ]] || fail "the VICI socket was left over"
    ok "nothing left: interface, routes and socket"
}

# describe_computer: distribution and the versions the component depends on.
describe_computer() {
    step "Test computer"
    # shellcheck source=/dev/null
    ok "$(. /etc/os-release && printf '%s' "$PRETTY_NAME"): systemd $(systemctl --version | awk 'NR == 1 { print $2 }'), iproute2 $(ip -V | grep -oE 'iproute2-[^, ]+' | cut -d- -f2), bash ${BASH_VERSION%%(*}, glibc $(ldd --version | awk 'NR == 1 { print $NF }')"
}

# install_component: the real installer, the helper authorizing alice (1000)
# and bob (1001) by uid, and the two users.
install_component() {
    step "Installation with the real installer"
    bash /test/bundle/install-native.sh install --bundle /test/bundle --origin manual \
        >/tmp/install.log 2>&1 || {
        cat /tmp/install.log
        fail "the installer failed"
    }
    # No polkit in the container: authorization by a uid list. Only root can
    # change the helper command line.
    install -d /etc/systemd/system/ikebox-helper.service.d
    printf '[Service]\nExecStart=\nExecStart=/opt/ikebox/bin/ikebox-helper --allow-uid 1000 --allow-uid 1001\n' \
        >/etc/systemd/system/ikebox-helper.service.d/test.conf
    systemctl daemon-reload
    systemctl stop ikebox-helper.service 2>/dev/null || true
    # Some base images come with a user of their own on uid 1000 (Ubuntu 24.04).
    local uid existing
    for uid in 1000 1001; do
        existing=$(getent passwd "$uid" | cut -d: -f1 || true)
        [[ -z $existing ]] || userdel -r "$existing" 2>/dev/null
    done
    useradd -m -u 1000 alice
    useradd -m -u 1001 bob
    ok "$(tail -n 1 /tmp/install.log)"
}
