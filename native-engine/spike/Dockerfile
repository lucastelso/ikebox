# SPIKE: simula o host do usuário (Fedora 45: glibc 2.44, iproute2 moderno)
# rodando o engine embutido. O container é só um substituto do root neste
# teste; no produto, quem roda o charon é o helper (fase D do roadmap).
FROM fedora@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c

RUN set -eux; \
    dnf -y install --setopt=install_weak_deps=False iproute systemd-resolved gawk iputils; \
    dnf clean all

# O tarball do build (native-engine/dist) é extraído em /opt/ipsec-client/engine.
ADD engine.tar.gz /
COPY zz-ipsec-client.conf /opt/ipsec-client/engine/etc/strongswan.d/zz-ipsec-client.conf
COPY entrypoint.sh /usr/local/sbin/ipsec-client-entrypoint
COPY updown.sh /usr/local/libexec/ipsec-client/updown
RUN chmod 0755 /usr/local/sbin/ipsec-client-entrypoint /usr/local/libexec/ipsec-client/updown && \
    mkdir -p /run/ipsec-client

ENV PATH=/opt/ipsec-client/engine/sbin:$PATH \
    IPSEC_CLIENT_CHARON=/opt/ipsec-client/engine/libexec/ipsec/charon \
    IPSEC_CLIENT_STRONGSWAN_D=/opt/ipsec-client/engine/etc/strongswan.d
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/sbin/ipsec-client-entrypoint"]
