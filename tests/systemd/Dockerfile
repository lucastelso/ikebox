# Test computer with real systemd, for tests/systemd/run.sh: the network
# component is installed by the real installer and the helper starts the
# engine with systemd-run and the production sandbox.
#
# The default base is Fedora 45. tests/compat/run.sh builds the same image on
# the other distributions of the compatibility matrix (tests/compat/distros.txt)
# by passing BASE: the package manager found in the base decides what to
# install.
ARG BASE=fedora@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c
FROM ${BASE}

# systemd-resolved is its own package on Debian 12+, Ubuntu 24.04+ and
# openSUSE, and part of systemd on Ubuntu 22.04: there the extra install
# finds nothing, and that is fine.
RUN set -eux; \
    if command -v dnf >/dev/null 2>&1; then \
        dnf -y install --setopt=install_weak_deps=False \
            systemd systemd-resolved dbus-broker dbus-libs iproute iputils gawk python3 util-linux \
            procps-ng gzip tar findutils; \
        dnf clean all; \
    elif command -v apt-get >/dev/null 2>&1; then \
        export DEBIAN_FRONTEND=noninteractive; \
        apt-get update; \
        apt-get install -y --no-install-recommends \
            systemd systemd-sysv dbus libdbus-1-3 iproute2 iputils-ping gawk python3 util-linux \
            procps gzip tar findutils; \
        apt-get install -y --no-install-recommends systemd-resolved || true; \
        rm -rf /var/lib/apt/lists/*; \
    elif command -v zypper >/dev/null 2>&1; then \
        zypper -n install --no-recommends \
            systemd systemd-resolved dbus-1 libdbus-1-3 iproute2 iputils gawk python3 util-linux \
            procps gzip tar findutils shadow; \
        zypper clean -a; \
    else \
        echo "unknown package manager in the base image" >&2; exit 1; \
    fi; \
    systemctl enable systemd-resolved; \
    systemctl mask systemd-firstboot.service systemd-homed.service \
        systemd-userdbd.service systemd-userdbd.socket getty.target

STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
