# SPIKE: simulates the user's host (Fedora 45: glibc 2.44, modern iproute2)
# running the embedded engine. The container only stands in for root in this
# test; in the product, the helper runs charon (phase D of the roadmap).
FROM fedora@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c

RUN set -eux; \
    dnf -y install --setopt=install_weak_deps=False iproute systemd-resolved gawk iputils; \
    dnf clean all

# The build tarball (native-engine/dist) is extracted to /opt/ikebox/engine.
ADD engine.tar.gz /
# swanctl (the E2E drives charon with it) comes from the test tools tarball.
ADD engine-tools.tar.gz /
COPY zz-ikebox.conf /opt/ikebox/engine/etc/strongswan.d/zz-ikebox.conf
COPY entrypoint.sh /usr/local/sbin/ikebox-entrypoint
COPY updown.sh /usr/local/libexec/ikebox/updown
RUN chmod 0755 /usr/local/sbin/ikebox-entrypoint /usr/local/libexec/ikebox/updown && \
    mkdir -p /run/ikebox

ENV PATH=/opt/ikebox/engine/sbin:$PATH \
    IKEBOX_CHARON=/opt/ikebox/engine/libexec/ipsec/charon \
    IKEBOX_STRONGSWAN_D=/opt/ikebox/engine/etc/strongswan.d
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/sbin/ikebox-entrypoint"]
