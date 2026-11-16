#!/usr/bin/env bash
# Compila o strongSwan para o engine embutido (roda DENTRO do builder).
#
# Uso: build-strongswan VERSAO CAMINHO SHA256
#   /keys     (ro)  chave pública de release do strongSwan
#   /patches  (ro)  patches do IPsec Client (native-engine/patches/*.patch)
#   /out      (rw)  destino do tarball
#
# Garantias:
#   - o tarball é conferido por SHA-256 (fixado em native-engine/versions.txt)
#     E pela assinatura GPG oficial;
#   - os patches de segurança do IPsec Client são aplicados DEPOIS da
#     conferência, e todos precisam aplicar (ADR-0019);
#   - gmp entra estático; só a glibc fica dinâmica (resolução de nomes);
#   - plugins próprios, sem OpenSSL (políticas de cripto do host não afetam);
#   - tarball determinístico (ordem, datas e donos fixos).

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

mkdir -p "$SRC" "$STAGE" /build/gmp-static
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

# Diretório só com a libgmp estática: o linker acha libgmp.a antes da .so.
cp "$(gcc -print-file-name=libgmp.a)" /build/gmp-static/

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
    --enable-gcm --enable-chapoly --enable-curve25519 --enable-gmp
    --enable-pem --enable-pkcs1 --enable-pkcs8 --enable-x509 --enable-pubkey
    --enable-xauth-generic --enable-updown --enable-unity
    --disable-pki --disable-scepclient
)
# 6.x: PRF+ para o IKEv2 sem OpenSSL fica no plugin kdf.
[[ $version == 6.* ]] && configure_flags+=(--enable-kdf)

cd "$SRC"
log "configure (${#configure_flags[@]} options)"
# 5.8.x é anterior ao -fno-common, padrão do gcc >= 10 (definições duplicadas).
cflags="-O2"
[[ $version == 5.8.* ]] && cflags+=" -fcommon"
CFLAGS="$cflags" LDFLAGS="-L/build/gmp-static" ./configure "${configure_flags[@]}" >/build/configure.log 2>&1 ||
    { tail -n 40 /build/configure.log; exit 1; }
log "make -j$(nproc)"
make -j"$(nproc)" >/build/make.log 2>&1 || { tail -n 60 /build/make.log; exit 1; }
make install DESTDIR="$STAGE" >/build/install.log 2>&1 || { tail -n 30 /build/install.log; exit 1; }

root="$STAGE$PREFIX"
find "$root" -type f -perm -u+x -exec sh -c 'file -b "$1" | grep -q ELF && strip "$1"' _ {} \;
# Build estático: as bibliotecas .a e os headers não servem em tempo de execução.
rm -rf "${root:?}/share/man" "${root:?}/lib" "${root:?}/include"

# Conformidade (ADR-0017): textos de licença do strongSwan e do GMP no pacote.
mkdir -p "$root/share/licenses/strongswan" "$root/share/licenses/gmp"
cp "$SRC/COPYING" "$SRC/LICENSE" "$root/share/licenses/strongswan/"
cp /usr/share/doc/libgmp10/copyright "$root/share/licenses/gmp/copyright"
gmp_version=$(dpkg-query -W -f='${Version}' libgmp-dev)

charon="$root/libexec/ipsec/charon"
[[ -x $charon ]] || { echo "charon was not built" >&2; exit 1; }
{
    echo "strongswan=$version"
    echo "sha256=$sha256"
    echo "builder=ubuntu:22.04 (glibc $(ldd --version | head -n 1 | awk '{print $NF}'))"
    echo "gmp=$gmp_version (static; source: Ubuntu 22.04 gmp package)"
    echo "patches=${patches[*]}"
    echo "configure=${configure_flags[*]}"
    echo "charon_deps=$(ldd "$charon" | awk '{print $1}' | tr '\n' ' ')"
} >"$root/BUILDINFO"

out="/out/ipsec-client-engine-$version-x86_64.tar.gz"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
    -C "$STAGE" -cf - opt | gzip -n -9 >"$out"
log "done: $(basename "$out") ($(du -h "$out" | cut -f1))"
log "charon dependencies: $(ldd "$charon" | awk '{print $1}' | tr '\n' ' ')"
