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
#   IPSEC_CLIENT_TEST_SUITE    systemd (default, the scenarios above) or modes
#                              (tests/modes/run.sh: one test gateway per IPsec
#                              mode in tests/modes/gateways, and
#                              tests/modes/inside.sh instead)

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
readonly SUITE=${IPSEC_CLIENT_TEST_SUITE:-systemd}
# Password of the test client certificates (.p12): FAKE.
readonly P12_PASSWORD=senha-p12-teste

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
    remove_mode_gateways
    docker network rm "$NET" >/dev/null 2>&1
    [[ -n $work ]] && rm -rf "$work"
    exit "$rc"
}

remove_mode_gateways() {
    docker ps -aq --filter "name=^$GW-mode-" | xargs -r docker rm -f >/dev/null 2>&1 || true
}

# make_pki DIR: a test CA, the gateway certificate it signs (RSA, SAN
# gw.ipsec-client.test) and a second, unrelated CA, for the certificate
# modes (EAP, client certificates). DIR/gateway/{x509ca,x509,private} goes to
# the gateway; DIR/client/ has the CA certificates a profile may carry and
# the person's certificate as .p12 files (password P12_PASSWORD): alice.p12
# (modern), alice-legacy.p12 (3DES/SHA-1, as older Windows exports) and
# mallory.p12, signed by the unrelated CA. The CA keys are deleted right
# away. Fake, short-lived, made at test time: nothing of it is kept.
make_pki() {
    local dir=$1 ca_ext=(-addext "basicConstraints=critical,CA:TRUE"
        -addext "keyUsage=critical,keyCertSign,cRLSign")
    mkdir -p "$dir/gateway/x509ca" "$dir/gateway/x509" "$dir/gateway/private" "$dir/client" "$dir/tmp"
    openssl req -x509 -newkey rsa:2048 -nodes -days 2 -keyout "$dir/tmp/ca.key" \
        -subj "/O=Example/CN=IPsec Client Mode Test CA" "${ca_ext[@]}" \
        -out "$dir/gateway/x509ca/ca.pem" 2>/dev/null
    openssl req -newkey rsa:2048 -nodes -keyout "$dir/gateway/private/gateway.key" \
        -subj "/O=Example/CN=gw.ipsec-client.test" -out "$dir/tmp/gateway.csr" 2>/dev/null
    printf 'subjectAltName=DNS:gw.ipsec-client.test\nextendedKeyUsage=serverAuth,1.3.6.1.5.5.8.2.2\n' \
        >"$dir/tmp/gateway.ext"
    openssl x509 -req -in "$dir/tmp/gateway.csr" -CA "$dir/gateway/x509ca/ca.pem" \
        -CAkey "$dir/tmp/ca.key" -CAcreateserial -days 2 -extfile "$dir/tmp/gateway.ext" \
        -out "$dir/gateway/x509/gateway.pem" 2>/dev/null
    openssl req -x509 -newkey rsa:2048 -nodes -days 2 -keyout "$dir/tmp/other.key" \
        -subj "/O=Example/CN=IPsec Client Unrelated Test CA" "${ca_ext[@]}" \
        -out "$dir/client/other-ca.pem" 2>/dev/null
    cp "$dir/gateway/x509ca/ca.pem" "$dir/client/ca.pem"
    # client_p12 NAME CN CA: a client certificate signed by CA, as NAME.p12.
    client_p12() {
        openssl req -newkey rsa:2048 -nodes -keyout "$dir/tmp/$1.key" \
            -subj "/O=Example Org/CN=$2" -out "$dir/tmp/$1.csr" 2>/dev/null
        printf 'subjectAltName=email:%s@example.com\nextendedKeyUsage=clientAuth\n' "$1" \
            >"$dir/tmp/$1.ext"
        openssl x509 -req -in "$dir/tmp/$1.csr" -CA "$dir/$3.pem" -CAkey "$dir/tmp/$3.key" \
            -CAcreateserial -days 2 -extfile "$dir/tmp/$1.ext" -out "$dir/tmp/$1.pem" 2>/dev/null
        openssl pkcs12 -export -inkey "$dir/tmp/$1.key" -in "$dir/tmp/$1.pem" \
            -passout "pass:$P12_PASSWORD" "${@:4}" -out "$dir/client/$1.p12" 2>/dev/null
    }
    cp "$dir/gateway/x509ca/ca.pem" "$dir/ca.pem"
    cp "$dir/client/other-ca.pem" "$dir/other.pem"
    client_p12 alice "Alice Teste" ca
    client_p12 alice-legacy "Alice Teste" ca \
        -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1
    client_p12 mallory "Mallory Teste" other
    [[ -s $dir/client/alice.p12 && -s $dir/client/alice-legacy.p12 && -s $dir/client/mallory.p12 ]] ||
        fail "could not make the test client certificates"
    rm -f "$dir/ca.pem" "$dir/other.pem"
    rm -rf "$dir/tmp"
    chmod -R a+rX "$dir/client"
}

# start_gateway NAME [CONF]: a test gateway on the test network; prints its IP.
# CONF is a file of tests/modes/gateways (default: tests/engine/gateway).
start_gateway() {
    local name=$1 conf=${2:-} _ ip
    local args=(-v "$repo/tests/engine/gateway:/test:ro")
    if [[ -n $conf ]]; then
        args+=(-v "$repo/tests/modes/gateways:/modes:ro" -e "GATEWAY_CONF=/modes/$conf"
            -v "$work/pki/gateway:/pki:ro" -e GATEWAY_PKI=/pki)
    fi
    docker run -d --name "$name" --network "$NET" --cap-add NET_ADMIN \
        --security-opt label=disable "${args[@]}" --entrypoint /test/gateway.sh "$GW_IMAGE" >/dev/null
    for _ in $(seq 1 150); do
        grep -q '\[test-gateway\] ready' < <(docker logs "$name" 2>&1) && break
        sleep 0.1
    done
    ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$name")
    if ! grep -q '\[test-gateway\] ready' < <(docker logs "$name" 2>&1) ||
        [[ ! $ip =~ ^[0-9]+(\.[0-9]+){3}$ ]]; then
        docker logs "$name" 2>&1 | tail -n 20 >&2
        fail "the test gateway $name did not start"
    fi
    printf '%s\n' "$ip"
}

main() {
    step "Prerequisites"
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
    docker info >/dev/null 2>&1 || fail "Docker is not reachable"
    local strongswan engine tools
    strongswan=$(grep -vE '^\s*(#|$)' "$repo/native-engine/versions.txt" | head -n 1 | awk '{print $1}')
    engine="$repo/native-engine/dist/ipsec-client-engine-$strongswan-x86_64.tar.gz"
    tools="$repo/native-engine/dist/ipsec-client-engine-tools-$strongswan-x86_64.tar.gz"
    [[ -f $engine && -f $tools ]] || fail "engine missing: run native-engine/build.sh $strongswan"
    grep -q '^patches=.*0001-vici-forced-updown' < <(tar -xzOf "$engine" opt/ipsec-client/engine/BUILDINFO) ||
        fail "the engine in $engine lacks the ADR-0019 patch: rebuild it (native-engine/build.sh)"
    docker rm -f "$HOST" "$GW" >/dev/null 2>&1 || true
    remove_mode_gateways
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
    tar -xzOf "$tools" opt/ipsec-client/engine/sbin/swanctl >"$work/swanctl"
    chmod 0755 "$work/swanctl"
    install -m 0644 "$here/lib.sh" "$here/helper_client.py" "$here/dns_responder.py" \
        "$repo/tests/engine/initiator.conf.in" \
        "$repo/crates/core/tests/fixtures/perfil-gateway-teste.json" "$work/"
    install -m 0755 "$here/inside.sh" "$work/"
    install -d -m 0755 "$work/modes"
    install -m 0644 "$repo/tests/modes/make_profile.py" "$work/modes/"
    install -m 0755 "$repo/tests/modes/inside.sh" "$work/modes/"
    ok "component bundle and ipsec-client-cli ready"

    step "Test gateway and computer with systemd"
    docker build -q -t "$GW_IMAGE" "$repo/engine" >/dev/null
    docker build -q ${BASE:+--build-arg "BASE=$BASE"} -t "$HOST_IMAGE" "$here" >/dev/null
    docker network create "$NET" >/dev/null
    local _ gw_ip conf mode mode_gateways=()
    if [[ $SUITE == modes ]]; then
        command -v openssl >/dev/null || fail "openssl is needed for the test CA of the EAP mode"
        make_pki "$work/pki"
        for conf in "$repo"/tests/modes/gateways/*.conf; do
            mode=$(basename "$conf" .conf)
            gw_ip=$(start_gateway "$GW-mode-$mode" "$mode.conf")
            mode_gateways+=("$mode=$gw_ip")
        done
        gw_ip="${mode_gateways[*]}"
    else
        gw_ip=$(start_gateway "$GW")
    fi
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

    if [[ $SUITE == modes ]]; then
        docker exec "$HOST" bash /test/modes/inside.sh "${mode_gateways[@]}"
    else
        docker exec "$HOST" bash /test/inside.sh "$gw_ip"
    fi
}

main "$@"
