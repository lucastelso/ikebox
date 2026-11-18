#!/usr/bin/env bash
# End-to-end test of ikebox-cli against the test IPsec gateway.
#
# Runs a real user's flow: doctor → profiles import → connect (wrong password,
# then the right one) → status → logs → disconnect. Uses a temporary
# XDG_CONFIG_HOME (does not touch the user's profiles) and FAKE credentials via
# --credentials-stdin (does not touch the keyring).
#
# Uses the production names (ikebox-engine container, ipsc0 interface):
# aborts if a connection is already active or UDP 500/4500 is in use.
# On a computer with an older network component installed, run it with
# IKEBOX_BACKEND=docker.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
readonly NET=ikebox-cli-e2e
readonly GW=ikebox-cli-e2e-gw
readonly IMAGE_LOCAL=ikebox-engine:e2e
readonly TEST_SECRETS_RE='ikebox-test-psk|senha-de-teste'

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

workdir=""
cli=""

cleanup() {
    local rc=$?
    set +e
    [[ -n $cli ]] && "$cli" disconnect >/dev/null 2>&1
    docker rm -f "$GW" >/dev/null 2>&1
    docker network rm "$NET" >/dev/null 2>&1
    [[ -n $workdir ]] && rm -rf "$workdir"
    exit "$rc"
}

main() {
    step "Prerequisites"
    if [[ -f "$HOME/.cargo/env" ]] && ! command -v cargo >/dev/null 2>&1; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
    command -v cargo >/dev/null || fail "cargo not found"
    docker info >/dev/null 2>&1 || fail "Docker is not reachable"
    if [[ -n $(ss -Hlun '( sport = :500 or sport = :4500 )') ]]; then
        fail "UDP 500/4500 in use (another IPsec VPN connected?)"
    fi
    if docker ps -q --filter name=^ikebox-engine$ | grep -q .; then
        fail "an ikebox connection is already active"
    fi
    trap cleanup EXIT
    workdir=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/ikebox-cli-e2e.XXXXXX")
    export XDG_CONFIG_HOME="$workdir/config"
    ok "temporary XDG_CONFIG_HOME: $XDG_CONFIG_HOME"

    step "CLI build"
    # With the Docker engine (feature docker): this test runs where the
    # network component may not be installed (CI).
    cargo build -q -p ikebox-cli --features docker --manifest-path "$repo_root/Cargo.toml"
    cli="$repo_root/target/debug/ikebox-cli"
    ok "$("$cli" --version)"

    step "Test IPsec gateway"
    docker build -q -t "$IMAGE_LOCAL" "$repo_root/engine" >/dev/null
    docker network rm "$NET" >/dev/null 2>&1 || true
    docker network create "$NET" >/dev/null
    docker run -d --name "$GW" --network "$NET" --cap-add NET_ADMIN \
        --security-opt label=disable \
        -v "$repo_root/tests/engine/gateway:/test:ro" --entrypoint /test/gateway.sh \
        "$IMAGE_LOCAL" >/dev/null
    local _ gw_ip
    for _ in $(seq 1 150); do
        grep -q '\[test-gateway\] ready' < <(docker logs "$GW" 2>&1) && break
        sleep 0.1
    done
    gw_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$GW")
    [[ $gw_ip =~ ^[0-9]+(\.[0-9]+){3}$ ]] || fail "the test gateway did not start"
    ok "gateway at $gw_ip"

    step "doctor"
    "$cli" doctor >"$workdir/doctor.txt" 2>&1 || {
        cat "$workdir/doctor.txt"
        fail "doctor rejected the environment"
    }
    ok "$(tail -n 1 "$workdir/doctor.txt")"

    step "profiles import / list / validate"
    sed "s/\"gw.ikebox.test\"/\"$gw_ip\"/" \
        "$repo_root/crates/core/tests/fixtures/test-gateway.json" >"$workdir/perfil.json"
    "$cli" profiles validate "$workdir/perfil.json" >/dev/null || fail "invalid profile"
    "$cli" profiles import "$workdir/perfil.json" >/dev/null
    "$cli" profiles list | grep -q '^gateway-teste ' || fail "profile not listed"
    [[ $(stat -c '%a' "$XDG_CONFIG_HOME/ikebox/profiles/gateway-teste.json") == 600 ]] ||
        fail "profile without 0600 permissions"
    ok "profile imported (0600) and listed"

    step "profiles import-swanctl (from a working swanctl.conf)"
    sed -e "s|@GATEWAY@|$gw_ip|" -e "s|@IFACE@|ipsc0|" -e "s|@IF_ID@|1|" \
        -e "s|@REMOTE_TS@|192.0.2.0/24|" -e "s|@UPDOWN_EXTRA@||" \
        "$repo_root/tests/engine/initiator.conf.in" >"$workdir/swanctl.conf"
    "$cli" profiles import-swanctl "$workdir/swanctl.conf" --id via-swanctl \
        --domain '~ikebox.test' --use-received-dns >"$workdir/import.txt" ||
        fail "import-swanctl failed"
    if grep -qE "$TEST_SECRETS_RE|usuario\.teste" < <("$cli" profiles show via-swanctl); then
        fail "secret or XAuth username copied into the profile"
    fi
    ok "profile 'via-swanctl' created without secrets or XAuth username"

    step "connect with a wrong password → clear message, nothing left running"
    local out rc=0
    out=$(printf 'psk=ikebox-test-psk\nxauth-user=usuario.teste\nxauth-password=errada\n' |
        "$cli" connect gateway-teste --detach --credentials-stdin 2>&1) || rc=$?
    ((rc != 0)) || fail "connect with a wrong password should fail"
    [[ $out == *"wrong XAuth username or password"* ]] || {
        printf '%s\n' "$out"
        fail "unexpected error message"
    }
    [[ $out != *usuario.teste* ]] || fail "unmasked XAuth username in the output"
    ip link show ipsc0 >/dev/null 2>&1 && fail "ipsc0 left over after the failure"
    ok "\"wrong XAuth username or password\", no identity exposed, host clean"

    step "connect --detach (profile imported from swanctl.conf)"
    out=$(printf 'psk=ikebox-test-psk\nxauth-user=usuario.teste\nxauth-password=senha-de-teste\n' |
        "$cli" connect via-swanctl --detach --credentials-stdin 2>&1) || {
        printf '%s\n' "$out"
        fail "connect failed"
    }
    [[ $out == *"Connected: via-swanctl"* ]] || fail "unexpected connect output"
    if grep -qE "$TEST_SECRETS_RE|usuario\.teste" <<<"$out"; then
        fail "secret or identity in the connect output"
    fi
    ok "connected (output without secrets or identities)"

    step "status / ping / logs"
    out=$("$cli" status) || fail "status should exit with 0 when connected"
    [[ $out == *"Virtual IP: 198.51.100."* ]] || fail "status without VIP: $out"
    ping -c 2 -W 2 -q 192.0.2.1 >/dev/null || fail "ping through the tunnel failed"
    out=$("$cli" logs -n 200)
    [[ $out == *"ready"* ]] || fail "logs without the readiness line"
    if grep -qE "$TEST_SECRETS_RE" <<<"$out"; then fail "secret in the logs"; fi
    ok "status with VIP, ping through the tunnel, logs without secrets"

    step "logs --anonymize (for bug reports)"
    out=$("$cli" logs -n 200 --anonymize)
    [[ $out == *"<gateway>"* ]] || fail "the gateway did not become <gateway>"
    if grep -qF -e "$gw_ip" -e "198.51.100." -e "ikebox.test" -e "grupo-teste" <<<"$out"; then
        fail "gateway, VIP, domain or group in the anonymized log"
    fi
    ok "gateway, VIP, domain and group replaced with placeholders"

    step "disconnect"
    "$cli" disconnect | grep -q "Disconnected" || fail "disconnect"
    rc=0
    "$cli" status >/dev/null || rc=$?
    ((rc == 3)) || fail "status should exit with 3 when disconnected (got $rc)"
    ip link show ipsc0 >/dev/null 2>&1 && fail "ipsc0 left over"
    ok "disconnected, status = 3, host clean"

    step "Result"
    ok "full CLI flow"
}

main "$@"
