#!/usr/bin/env bash
# Compila o strongSwan para o engine embutido (roda DENTRO do builder).
#
# Uso: build-strongswan VERSAO CAMINHO SHA256
#   /keys      (ro)  chave pública de release do strongSwan
#   /patches   (ro)  patches do IPsec Client (native-engine/patches/*.patch)
#   /licenses  (ro)  textos de licença do GMP e da musl (native-engine/licenses)
#   /out       (rw)  destino dos tarballs
#
# Garantias:
#   - o tarball é conferido por SHA-256 (fixado em native-engine/versions.txt)
#     E pela assinatura GPG oficial;
#   - os patches de segurança do IPsec Client são aplicados DEPOIS da
#     conferência, e todos precisam aplicar (ADR-0019);
#   - one fully static executable (musl and GMP inside, ADR-0020): it does
#     not depend on the glibc of the computer it runs on;
#   - plugins próprios, sem OpenSSL (políticas de cripto do host não afetam);
#   - two tarballs: the engine (only what production runs) and the test tools
#     (swanctl), which never go to users;
#   - tarballs determinísticos (ordem, datas e donos fixos).

set -euo pipefail

version=${1:?versão}
path=${2:?caminho no servidor}
sha256=${3:?sha256}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid version" >&2; exit 2; }
[[ $sha256 =~ ^[0-9a-f]{64}$ ]] || { echo "invalid sha256" >&2; exit 2; }
[[ $path =~ ^/([a-z0-9.]+/)*$ ]] || { echo "invalid path" >&2; exit 2; }

readonly PREFIX=/opt/ipsec-client/engine
readonly SRC=/build/src
readonly STAGE=/build/stage
tarball="strongswan-$version.tar.bz2"
url="https://download.strongswan.org${path}${tarball}"

log() { printf '[build] %s\n' "$*" >&2; }

mkdir -p "$SRC" "$STAGE"
cd /build

log "downloading $url"
curl -sSfL --proto '=https' --tlsv1.2 -o "$tarball" "$url"
curl -sSfL --proto '=https' --tlsv1.2 -o "$tarball.sig" "$url.sig"
echo "$sha256  $tarball" | sha256sum -c --quiet
log "SHA-256 verified"
export GNUPGHOME=/build/gnupg
mkdir -m 0700 "$GNUPGHOME"
gpg --batch --quiet --import /keys/strongswan-release.asc
gpg --batch --verify "$tarball.sig" "$tarball" 2>&1 | grep -q "Good signature" ||
    { echo "invalid GPG signature" >&2; exit 1; }
log "GPG signature verified"

tar -xjf "$tarball" -C "$SRC" --strip-components=1

# Patches do IPsec Client (ADR-0019). Um patch que não aplica derruba o build:
# o engine sem ele seria inseguro.
patches=()
for patch_file in /patches/*.patch; do
    [[ -f $patch_file ]] || continue
    patch -d "$SRC" -p1 --forward --quiet <"$patch_file" ||
        { echo "patch $(basename "$patch_file") does not apply to strongSwan $version" >&2; exit 1; }
    patches+=("$(basename "$patch_file"):$(sha256sum "$patch_file" | cut -c1-64)")
    log "patch applied: $(basename "$patch_file")"
done
((${#patches[@]} > 0)) || { echo "no patch in /patches" >&2; exit 1; }

configure_flags=(
    --prefix="$PREFIX"
    --sysconfdir="$PREFIX/etc"
    --libexecdir="$PREFIX/libexec"
    --with-piddir=/run/ipsec-client-engine
    --disable-shared --enable-static --enable-monolithic
    --disable-defaults
    --enable-charon --enable-swanctl --enable-vici
    --enable-ikev1 --enable-ikev2
    --enable-kernel-netlink --enable-socket-default
    --enable-nonce --enable-random --enable-drbg
    --enable-aes --enable-des --enable-sha1 --enable-sha2 --enable-md5 --enable-hmac
    --enable-gcm --enable-chapoly --enable-curve25519
    # OpenSSL (ADR-0027): RSA, ECDSA, the MODP and ECP DH groups. It replaces
    # GMP. The symmetric plugins above load first and keep their algorithms;
    # single DES and MD4 stay in strongSwan's own plugins (OpenSSL 3 has them
    # only in its legacy provider, which a static build cannot load).
    --enable-openssl
    # charon runs without root with ambient capabilities (ADR-0028): it must
    # see them (capget, no libcap), or it disables its sockets and netlink.
    --with-capabilities=native
    --enable-pem --enable-pkcs1 --enable-pkcs8 --enable-x509 --enable-pubkey
    --enable-xauth-generic --enable-updown --enable-unity
    # IKEv2 + EAP-MSCHAPv2 (ADR-0023): the EAP identity exchange, MSCHAPv2
    # itself and the MD4 hash it needs (DES is already above).
    --enable-eap-identity --enable-eap-mschapv2 --enable-md4
    --disable-pki --disable-scepclient
)
# 6.x: PRF+ para o IKEv2 sem OpenSSL fica no plugin kdf.
[[ $version == 6.* ]] && configure_flags+=(--enable-kdf)

cd "$SRC"
log "configure (${#configure_flags[@]} options)"
# 5.8.x é anterior ao -fno-common, padrão do gcc >= 10 (definições duplicadas).
# Hardening stated, not left to the compiler's defaults: stack protector and
# FORTIFY (Alpine's fortify-headers, part of build-base). PIE, full RELRO and a
# non-executable stack come from the static-pie link.
cflags="-O2 -fstack-protector-strong -D_FORTIFY_SOURCE=2 -fstack-clash-protection"
[[ $version == 5.8.* ]] && cflags+=" -fcommon"
CFLAGS="$cflags" LDFLAGS="-static" ./configure "${configure_flags[@]}" >/build/configure.log 2>&1 ||
    { tail -n 40 /build/configure.log; exit 1; }
log "make -j$(nproc)"
# -all-static: libtool's way of linking the executables fully static (its
# -static would still take libc as a shared library).
# capget/capset declarations for the native capability backend (ADR-0028),
# only when compiling (CPPFLAGS adds to configure's flags): in configure they
# would clash with its own test of capset.
make -j"$(nproc)" LDFLAGS="-all-static" \
    CPPFLAGS="-include /usr/local/include/musl-capabilities.h" >/build/make.log 2>&1 || { tail -n 60 /build/make.log; exit 1; }
make install DESTDIR="$STAGE" >/build/install.log 2>&1 || { tail -n 30 /build/install.log; exit 1; }

root="$STAGE$PREFIX"
find "$root" -type f -perm -u+x -exec sh -c 'file -b "$1" | grep -q ELF && strip "$1"' _ {} \;
# Build estático: as bibliotecas .a e os headers não servem em tempo de execução.
rm -rf "${root:?}/share/man" "${root:?}/lib" "${root:?}/include"

charon="$root/libexec/ipsec/charon"
[[ -x $charon ]] || { echo "charon was not built" >&2; exit 1; }
# Fully static, or it is not the engine of ADR-0020.
if [[ -n $(readelf -d "$charon" 2>/dev/null | grep NEEDED || true) ]]; then
    echo "charon is not fully static: $(readelf -d "$charon" | grep NEEDED)" >&2
    exit 1
fi

# Test tools: swanctl goes to its own tarball, never to users. What
# production does not run stays out of the engine (audit of 0.2.0, P7):
# strongSwan's stock _updown, the xfrmi tool, swanctl's configuration and the
# configuration templates.
TOOLS=/build/tools
mkdir -p "$TOOLS$PREFIX/sbin"
mv "$root/sbin/swanctl" "$TOOLS$PREFIX/sbin/swanctl"
rmdir "$root/sbin"
rm -rf "${root:?}/libexec/ipsec/_updown" "${root:?}/libexec/ipsec/xfrmi" \
    "${root:?}/share/strongswan" "${root:?}/etc/swanctl" "${root:?}/etc/strongswan.d/swanctl.conf"

# Compliance (ADR-0017): license texts of strongSwan, OpenSSL and musl.
mkdir -p "$root/share/licenses/strongswan"
cp "$SRC/COPYING" "$SRC/LICENSE" "$root/share/licenses/strongswan/"
cp -R /licenses/openssl /licenses/musl "$root/share/licenses/"
openssl_version=$(apk list -I openssl-libs-static 2>/dev/null | awk '{print $1}' | sed 's/^openssl-libs-static-//')
musl_version=$(apk list -I musl 2>/dev/null | awk '{print $1}' | sed 's/^musl-//')
[[ -n $openssl_version && -n $musl_version ]] || { echo "could not read the OpenSSL/musl versions" >&2; exit 1; }

{
    echo "strongswan=$version"
    echo "sha256=$sha256"
    echo "builder=alpine:$(cat /etc/alpine-release) (musl $musl_version)"
    echo "openssl=$openssl_version (static; Alpine package openssl-libs-static)"
    echo "musl=$musl_version (static)"
    echo "linkage=static"
    echo "patches=${patches[*]}"
    echo "configure=${configure_flags[*]}"
} >"$root/BUILDINFO"

pack() {
    tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
        -C "$1" -cf - opt | gzip -n -9 >"$2"
    log "done: $(basename "$2") ($(du -h "$2" | cut -f1))"
}
pack "$STAGE" "/out/ipsec-client-engine-$version-x86_64.tar.gz"
pack "$TOOLS" "/out/ipsec-client-engine-tools-$version-x86_64.tar.gz"
log "charon: $(file -b "$charon" | cut -d, -f1-2)"
