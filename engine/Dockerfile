# ipsec-client Docker engine and test gateway: Debian's strongSwan in a
# container. Development and tests only (ADR-0001, amended by ADR-0020):
# release builds of the app have no Docker engine, and users get the embedded
# engine. See docs/architecture.md.
#
# The container runs with --network host and NET_ADMIN only. charon installs
# XFRM SAs and policies in the host kernel, bound to the dedicated interface
# created by the entrypoint.

# debian:13-slim pinned by digest: the build fails instead of silently changing.
FROM debian:13-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

# Upstream version expected from Debian. Security revisions of Debian
# (6.0.1-6+deb13uN) are taken; another upstream version fails the build, so
# that the tests never change strongSwan without notice.
ARG STRONGSWAN_VERSION=6.0.1

ENV DEBIAN_FRONTEND=noninteractive

# charon + swanctl, XAuth (extauth plugins: xauth-generic) and the extra
# plugins (unity, among others). systemd-resolved only for resolvectl (DNS in
# the host's systemd-resolved, over D-Bus).
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        strongswan-charon strongswan-swanctl \
        libcharon-extauth-plugins libcharon-extra-plugins libstrongswan-extra-plugins \
        iproute2 iputils-ping systemd-resolved; \
    rm -rf /var/lib/apt/lists/*; \
    test -x /usr/lib/ipsec/charon; \
    test -f /usr/lib/ipsec/plugins/libstrongswan-xauth-generic.so; \
    /usr/lib/ipsec/charon --version | grep -q "strongSwan ${STRONGSWAN_VERSION}\b"; \
    command -v resolvectl

COPY strongswan.d/zz-ipsec-client.conf /etc/strongswan.d/zz-ipsec-client.conf
COPY entrypoint.sh /usr/local/sbin/ipsec-client-entrypoint
COPY updown.sh /usr/local/libexec/ipsec-client/updown

RUN set -eux; \
    chmod 0644 /etc/strongswan.d/zz-ipsec-client.conf; \
    chmod 0755 /usr/local/sbin/ipsec-client-entrypoint /usr/local/libexec/ipsec-client/updown; \
    mkdir -p /run/ipsec-client

LABEL org.opencontainers.image.title="ipsec-client-engine" \
      org.opencontainers.image.description="Debian strongSwan for ipsec-client development and tests" \
      io.github.ipsec-client.strongswan="${STRONGSWAN_VERSION}"

STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/sbin/ipsec-client-entrypoint"]
