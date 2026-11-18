# Embedded engine builder: compiles strongSwan from source into one fully
# static executable (ADR-0020).
#
# Alpine (musl): charon links musl and GMP statically, so the engine does not
# depend on the glibc of the computer it runs on. Pinned by tag and digest; Alpine
# 3.24.4, musl 1.2.6, GMP 6.3.0. When it changes, update the license texts in
# native-engine/licenses/ (they must match the GMP and musl versions).
FROM alpine:3.24.4@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf

# What ends up inside charon is pinned to its upstream version: GMP and musl
# (and the license texts must match them). Alpine keeps only the newest
# revision of a package, so "~" accepts its security revisions (-rN) but
# stops the build when the version itself changes. The other packages are
# build tools; BUILDINFO records what was used.
RUN set -eux; \
    apk add --no-cache \
        bash build-base linux-headers patch pkgconf \
        'gmp-dev~6.3.0' 'gmp-static~6.3.0' 'musl-dev~1.2.6' \
        ca-certificates curl gnupg bzip2 file python3 tar gzip coreutils findutils

COPY build-strongswan.sh /usr/local/bin/build-strongswan
ENTRYPOINT ["/usr/local/bin/build-strongswan"]
