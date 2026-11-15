# Engine do ipsec-client: strongSwan 5.8.2 (Ubuntu 20.04) isolado num container.
# Ver docs/adr/0001-engine-docker-strongswan-5.8.2.md e docs/arquitetura.md.
#
# O container roda com --network host e só NET_ADMIN. O charon instala SAs e
# políticas XFRM no kernel do host, ligadas à interface dedicada criada pelo
# entrypoint.

# ubuntu:20.04 fixado por digest: o build falha em vez de mudar em silêncio.
FROM ubuntu:20.04@sha256:8feb4d8ca5354def3d8fce243717141ce31e2c428701f6682bd2fafe15388214

ARG STRONGSWAN_VERSION=5.8.2-1ubuntu3.6

ENV DEBIAN_FRONTEND=noninteractive

# Mesma lista de pacotes do fluxo manual validado (com recommends), com o
# strongSwan fixado na versão validada. O systemd entra só pelo resolvectl
# (DNS no systemd-resolved do host via D-Bus), sem recommends.
RUN set -eux; \
    apt-get update; \
    apt-get install -y \
        strongswan="${STRONGSWAN_VERSION}" \
        strongswan-swanctl="${STRONGSWAN_VERSION}" \
        libcharon-extra-plugins="${STRONGSWAN_VERSION}" \
        libstrongswan-extra-plugins="${STRONGSWAN_VERSION}" \
        iptables iproute2 iputils-ping; \
    apt-get install -y --no-install-recommends systemd; \
    rm -rf /var/lib/apt/lists/*; \
    test -x /usr/lib/ipsec/charon; \
    command -v resolvectl

COPY strongswan.d/zz-ipsec-client.conf /etc/strongswan.d/zz-ipsec-client.conf
COPY entrypoint.sh /usr/local/sbin/ipsec-client-entrypoint
COPY updown.sh /usr/local/libexec/ipsec-client/updown

RUN set -eux; \
    chmod 0644 /etc/strongswan.d/zz-ipsec-client.conf; \
    chmod 0755 /usr/local/sbin/ipsec-client-entrypoint /usr/local/libexec/ipsec-client/updown; \
    mkdir -p /run/ipsec-client

LABEL org.opencontainers.image.title="ipsec-client-engine" \
      org.opencontainers.image.description="strongSwan 5.8.2 isolado para o ipsec-client" \
      io.github.ipsec-client.strongswan="${STRONGSWAN_VERSION}"

STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/sbin/ipsec-client-entrypoint"]
