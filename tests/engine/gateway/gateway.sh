#!/usr/bin/env bash
# TEST IPsec gateway (responder). Runs in the same image as the engine, on an
# isolated Docker network, and mimics a legacy VPN: IKEv1 aggressive, group PSK
# + XAuth, modp1024/modp768.
#
# Protected networks (RFC 5737): 192.0.2.0/24 (the gateway answers on
# 192.0.2.1) and 203.0.113.0/24 (child with modp768 PFS only; answers on
# 203.0.113.1). No real data: fake credentials in swanctl.conf.
#
# GATEWAY_CONF picks another configuration (the IPsec modes of
# tests/modes/gateways, one gateway container per mode). GATEWAY_PKI is a
# directory with x509ca/, x509/ and private/ (a test CA, the gateway
# certificate and its key, made at test time): they go to swanctl's
# directories, which --load-all reads.

set -euo pipefail

log() { printf '[test-gateway] %s\n' "$*" >&2; }

# "Internal" network behind the gateway.
ip link add dum0 type dummy
ip addr add 192.0.2.1/24 dev dum0
ip addr add 203.0.113.1/24 dev dum0
ip link set dum0 up

# Overrides for the test gateway only (the responder must accept aggressive+PSK).
cat >/etc/strongswan.d/zzz-gateway-teste.conf <<'EOF'
charon {
    i_dont_care_about_security_and_use_aggressive_mode_psk = yes
    plugins {
        vici {
            socket = unix:///var/run/charon.vici
        }
    }
}
EOF

/usr/lib/ipsec/charon &
charon_pid=$!
trap 'kill -TERM "$charon_pid" 2>/dev/null || true' TERM INT

for _ in $(seq 1 100); do
    [[ -S /var/run/charon.vici ]] && break
    sleep 0.1
done
[[ -S /var/run/charon.vici ]] || {
    log "the VICI socket did not show up"
    exit 1
}

if [[ -n ${GATEWAY_PKI:-} ]]; then
    for dir in x509ca x509 private; do
        if [[ -d $GATEWAY_PKI/$dir ]]; then
            cp "$GATEWAY_PKI/$dir"/* "/etc/swanctl/$dir/"
        fi
    done
    chmod 0600 /etc/swanctl/private/* 2>/dev/null || true
fi

# With --file, swanctl reads x509/, x509ca/ and private/ next to the file:
# the configuration goes to /etc/swanctl, where the certificates are.
install -m 0600 "${GATEWAY_CONF:-/test/swanctl.conf}" /etc/swanctl/gateway.conf
swanctl --load-all --file /etc/swanctl/gateway.conf
log "ready"

wait "$charon_pid" || true
