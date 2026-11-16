# Computador de teste com systemd de verdade (Fedora 45), para o
# tests/systemd/run.sh: o componente de rede é instalado pelo instalador real
# e o helper sobe o engine com systemd-run e o sandbox de produção.
FROM fedora@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c

RUN set -eux; \
    dnf -y install --setopt=install_weak_deps=False \
        systemd systemd-resolved dbus-broker dbus-libs iproute iputils gawk python3 util-linux \
        procps-ng gzip tar findutils; \
    dnf clean all; \
    systemctl enable systemd-resolved; \
    systemctl mask systemd-firstboot.service systemd-homed.service \
        systemd-userdbd.service systemd-userdbd.socket getty.target

STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
