#!/usr/bin/env bash
# Gateway IPsec de TESTE (responder). Roda na mesma imagem do engine, numa rede
# Docker isolada, e imita uma VPN legada: IKEv1 agressivo, PSK de grupo +
# XAuth, modp1024/modp768.
#
# Rede protegida: 192.0.2.0/24 (RFC 5737). O gateway responde em 192.0.2.1.
# Nenhum dado real: credenciais falsas em swanctl.conf.

set -euo pipefail

log() { printf '[gateway-teste] %s\n' "$*" >&2; }

# Rede "interna" atrás do gateway.
ip link add dum0 type dummy
ip addr add 192.0.2.1/24 dev dum0
ip link set dum0 up

# Overrides só do gateway de teste (o responder precisa aceitar aggressive+PSK).
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
    log "socket VICI não apareceu"
    exit 1
}

swanctl --load-all --file /test/swanctl.conf
log "pronto"

wait "$charon_pid" || true
