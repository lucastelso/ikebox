# Embedded engine builder: compiles strongSwan from source.
#
# Ubuntu 22.04 (glibc 2.35): the same base used for the Tauri AppImage; the
# binary runs on distributions with the same or a newer glibc.
FROM ubuntu:22.04@sha256:9abcaffc92a5279fff3a6a41635c0800ea33e528425fb389658806476e7defde

ENV DEBIAN_FRONTEND=noninteractive
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        build-essential patch pkg-config libgmp-dev ca-certificates curl gnupg bzip2 file python3; \
    rm -rf /var/lib/apt/lists/*

COPY build-strongswan.sh /usr/local/bin/build-strongswan
ENTRYPOINT ["/usr/local/bin/build-strongswan"]
