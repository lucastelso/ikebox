# Embedded engine builder: compiles strongSwan from source into one fully
# static executable (ADR-0020).
#
# Alpine (musl): charon links musl and GMP statically, so the engine does not
# depend on the glibc of the computer it runs on. Pinned by digest; Alpine
# 3.24.4, musl 1.2.6, GMP 6.3.0. When it changes, update the license texts in
# native-engine/licenses/ (they must match the GMP and musl versions).
FROM alpine@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf

RUN set -eux; \
    apk add --no-cache \
        bash build-base linux-headers patch pkgconf gmp-dev gmp-static \
        ca-certificates curl gnupg bzip2 file python3 tar gzip coreutils findutils

COPY build-strongswan.sh /usr/local/bin/build-strongswan
ENTRYPOINT ["/usr/local/bin/build-strongswan"]
