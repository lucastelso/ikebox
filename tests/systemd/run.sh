#!/usr/bin/env bash
# Test of the embedded engine with real systemd, without touching the computer
# that runs it (replaces the sudo of scripts/test-native.sh for the security
# checks):
#
#   1. starts the test IPsec gateway and a Fedora 45 "computer" with systemd
#      (privileged container, its own network);
#   2. installs the network component with the real installer (packaging/);
#   3. as user alice, connects with ipsec-client-cli (embedded engine, the
#      profile DNS going through the helper) and checks the tunnel, the DNS and
#      the sandbox of the engine unit (systemd-run);
#   4. as bob, tries to stop alice's VPN, read its log and see the profile;
#   5. as alice, runs the ADR-0019 attack: a VICI client picks the updown
#      (it would run as root) and the if_id. charon must ignore both.
#
# Requirements: Docker (the container needs --privileged for systemd and XFRM
# in its network namespace), the engine compiled with the patch
# (native-engine/build.sh) and cargo.
#
# Environment (set by tests/compat/run.sh to repeat the test on another
# distribution):
#   IPSEC_CLIENT_TEST_BASE     base image of the test computer (default: the
#                              Fedora pinned in the Dockerfile)
#   IPSEC_CLIENT_TEST_NAME     name of that distribution (image tag, messages)
#   IPSEC_CLIENT_TEST_BIN_DIR  directory with ipsec-client-cli and
#                              ipsec-client-helper already built (for an older
#                              glibc than this computer's); default: build
#                              them here with cargo

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
here="$repo/tests/systemd"
readonly NET=ipsec-client-systemd-test
readonly GW=ipsec-client-systemd-gw
readonly HOST=ipsec-client-systemd-host
readonly GW_IMAGE=ipsec-client-engine:e2e
readonly DISTRO=${IPSEC_CLIENT_TEST_NAME:-local}
readonly HOST_IMAGE=ipsec-client-systemd-test:$DISTRO
readonly BASE=${IPSEC_CLIENT_TEST_BASE:-}
readonly BIN_DIR=${IPSEC_CLIENT_TEST_BIN_DIR:-}

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

work=""
cleanup() {
    local rc=$?
    set +e
    if ((rc != 0)) && docker ps -q --filter "name=^$HOST$" | grep -q .; then
        printf '\n--- engine and helper journal ---\n'
        docker exec "$HOST" journalctl -u ipsec-client-engine -u ipsec-client-helper \
            --no-pager -n 80 2>/dev/null
    fi
    docker rm -f "$HOST" "$GW" >/dev/null 2>&1
    docker network rm "$NET" >/dev/null 2>&1
    [[ -n $work ]] && rm -rf "$work"
    exit "$rc"
}

main() {
    step "Prerequisites"
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
    docker info >/dev/null 2>&1 || fail "Docker is not reachable"
    local strongswan engine
    strongswan=$(grep -vE '^\s*(#|$)' "$repo/native-engine/versions.txt" | head -n 1 | awk '{print $1}')
    engine="$repo/native-engine/dist/ipsec-client-engine-$strongswan-x86_64.tar.gz"
    [[ -f $engine ]] || fail "engine missing: run native-engine/build.sh $strongswan"
    grep -q '^patches=.*0001-vici-forced-updown' < <(tar -xzOf "$engine" opt/ipsec-client/engine/BUILDINFO) ||
        fail "the engine in $engine lacks the ADR-0019 patch: rebuild it (native-engine/build.sh)"
    docker rm -f "$HOST" "$GW" >/dev/null 2>&1 || true
    docker network rm "$NET" >/dev/null 2>&1 || true
    trap cleanup EXIT
    work=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/ipsec-client-systemd.XXXXXX")
    chmod 0755 "$work"
    ok "engine with the patch: $(basename "$engine")"

    step "Network component and CLI"
    if [[ -n $BIN_DIR ]]; then
        [[ -x $BIN_DIR/ipsec-client-cli && -x $BIN_DIR/ipsec-client-helper ]] ||
            fail "ipsec-client-cli and ipsec-client-helper not found in $BIN_DIR"
        "$repo/scripts/stage-network-component.sh" --out "$work/bundle" --engine "$engine" \
            --helper "$BIN_DIR/ipsec-client-helper" >/dev/null
        install -m 0755 "$BIN_DIR/ipsec-client-cli" "$work/ipsec-client-cli"
    else
        "$repo/scripts/stage-network-component.sh" --out "$work/bundle" --engine "$engine" >/dev/null
        cargo build -q --release --locked -p ipsec-client-cli --manifest-path "$repo/Cargo.toml"
        install -m 0755 "$repo/target/release/ipsec-client-cli" "$work/ipsec-client-cli"
    fi
    install -m 0644 "$here/helper_client.py" "$repo/tests/engine/initiator.conf.in" \
        "$repo/crates/core/tests/fixtures/perfil-gateway-teste.json" "$work/"
    install -m 0755 "$here/inside.sh" "$work/"
    ok "component bundle and ipsec-client-cli ready"

    step "Test gateway and computer with systemd"
    docker build -q -t "$GW_IMAGE" "$repo/engine" >/dev/null
    docker build -q ${BASE:+--build-arg "BASE=$BASE"} -t "$HOST_IMAGE" "$here" >/dev/null
    docker network create "$NET" >/dev/null
    docker run -d --name "$GW" --network "$NET" --cap-add NET_ADMIN \
        --security-opt label=disable \
        -v "$repo/tests/engine/gateway:/test:ro" --entrypoint /test/gateway.sh "$GW_IMAGE" >/dev/null
    local _ gw_ip
    for _ in $(seq 1 150); do
        grep -q '\[test-gateway\] ready' < <(docker logs "$GW" 2>&1) && break
        sleep 0.1
    done
    gw_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$GW")
    [[ $gw_ip =~ ^[0-9]+(\.[0-9]+){3}$ ]] || fail "the test gateway did not start"
    docker run -d --name "$HOST" --hostname teste --network "$NET" --privileged \
        --cgroupns=private --security-opt label=disable \
        --tmpfs /run --tmpfs /run/lock --tmpfs /tmp \
        -v "$work:/test:ro" "$HOST_IMAGE" >/dev/null
    local state=""
    for _ in $(seq 1 100); do
        state=$(docker exec "$HOST" systemctl is-system-running 2>/dev/null || true)
        [[ $state == running || $state == degraded ]] && break
        sleep 0.2
    done
    [[ $state == running || $state == degraded ]] || fail "the container systemd did not start ($state)"
    ok "gateway at $gw_ip; container systemd: $state"

    docker exec "$HOST" bash /test/inside.sh "$gw_ip"
}

main "$@"
