#!/usr/bin/env bash
# Builds strongSwan for the embedded engine. The same script runs in two
# places (ADR-0032):
#   - inside the Alpine builder (native-engine/build.sh): the official,
#     reproducible path that releases use;
#   - straight on a computer with a musl toolchain (native-engine/build.sh
#     --host), for distribution packagers and builds without Docker.
#
# Usage: build-strongswan [options] VERSION PATH SHA256
#   --keys DIR       release public keys (strongSwan's, OpenSSL's)     [/keys]
#   --patches DIR    Ikebox patches (native-engine/patches)             [/patches]
#   --licenses DIR   OpenSSL and musl license texts                     [/licenses]
#   --out DIR        where the tarballs go                              [/out]
#   --work DIR       scratch directory                                  [/build]
#   --cap-header F   capget/capset declarations for musl
#                                    [/usr/local/include/musl-capabilities.h]
#   --sources DIR    tarballs and signatures already downloaded: nothing is
#                    fetched (offline builds). With --fetch-only, where the
#                    downloads go.
#   --openssl MODE   auto: the toolchain's own static OpenSSL if it links
#                    (the builder's Alpine package, Nix's pkgsStatic),
#                    otherwise "source" [default]; system: only that one;
#                    source: OpenSSL compiled here, from --openssl-pin
#   --openssl-pin F  version, URL and SHA-256 of that OpenSSL (native-engine/openssl.txt)
#   --fetch-only     download and check the sources into --sources, then stop
#   CC               the musl C compiler (default: cc when it is musl, else musl-gcc)
#
# Guarantees:
#   - every source tarball is checked by SHA-256 (pinned in
#     native-engine/versions.txt and openssl.txt) AND by its official GPG
#     signature, each against a keyring holding only its own project's key;
#   - the Ikebox security patches are applied AFTER the check, and all
#     of them must apply (ADR-0019);
#   - one fully static executable (musl and OpenSSL inside, ADR-0020,
#     ADR-0027): it does not depend on the glibc of the computer it runs on,
#     and the host's crypto policies do not affect it;
#   - the toolchain must be musl and give a static-pie charon with full RELRO,
#     BIND_NOW and a non-executable stack, or the build stops: Alpine's gcc
#     does that from -static, other toolchains are asked explicitly;
#   - two tarballs: the engine (only what production runs) and the test tools
#     (swanctl), which never go to users;
#   - deterministic tarballs (fixed order, dates and owners). Inside the
#     builder, the same inputs give the same bytes.

set -euo pipefail

keys=/keys
patches_dir=/patches
licenses=/licenses
out=/out
work=/build
cap_header=/usr/local/include/musl-capabilities.h
sources=""
openssl_mode=auto
openssl_pin=""
fetch_only=0
while (($# > 0)); do
    case $1 in
    --keys) keys=${2:?}; shift 2 ;;
    --patches) patches_dir=${2:?}; shift 2 ;;
    --licenses) licenses=${2:?}; shift 2 ;;
    --out) out=${2:?}; shift 2 ;;
    --work) work=${2:?}; shift 2 ;;
    --cap-header) cap_header=${2:?}; shift 2 ;;
    --sources) sources=${2:?}; shift 2 ;;
    --openssl) openssl_mode=${2:?}; shift 2 ;;
    --openssl-pin) openssl_pin=${2:?}; shift 2 ;;
    --fetch-only) fetch_only=1; shift ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) break ;;
    esac
done

version=${1:?version}
path=${2:?path on the server}
sha256=${3:?sha256}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "invalid version" >&2; exit 2; }
[[ $sha256 =~ ^[0-9a-f]{64}$ ]] || { echo "invalid sha256" >&2; exit 2; }
[[ $path =~ ^/([a-z0-9.]+/)*$ ]] || { echo "invalid path" >&2; exit 2; }
[[ $openssl_mode =~ ^(auto|system|source)$ ]] || { echo "invalid --openssl: $openssl_mode" >&2; exit 2; }
((fetch_only == 0)) || [[ -n $sources ]] || { echo "--fetch-only needs --sources DIR" >&2; exit 2; }

readonly PREFIX=/opt/ikebox/engine
readonly SRC=$work/src
readonly STAGE=$work/stage
tarball="strongswan-$version.tar.bz2"
url="https://download.strongswan.org${path}${tarball}"

log() { printf '[build] %s\n' "$*" >&2; }
die() { printf '[build] error: %s\n' "$*" >&2; exit 1; }

need() {
    local tool missing=()
    for tool in "$@"; do command -v "$tool" >/dev/null 2>&1 || missing+=("$tool"); done
    ((${#missing[@]} == 0)) || die "missing tools: ${missing[*]} (see native-engine/README.md)"
}

mkdir -p "$work"
cd "$work"

# --- Sources: fetched (or taken from --sources), then checked ----------------

# fetch_file URL NAME: prints where NAME is, downloading it unless --sources
# already has it (and never downloading when --sources is given without
# --fetch-only: an offline build stays offline).
fetch_file() {
    local from=$1 name=$2 dest
    if [[ -n $sources ]] && ((fetch_only == 0)); then
        [[ -f $sources/$name ]] || die "$name is not in $sources (native-engine/build.sh --fetch downloads it)"
        printf '%s\n' "$sources/$name"
        return
    fi
    dest=$work/$name
    if ((fetch_only)); then
        dest=$sources/$name
        # Already fetched: checked again below, not downloaded again.
        [[ ! -f $dest ]] || { printf '%s\n' "$dest"; return; }
    fi
    log "downloading $from"
    curl -sSfL --proto '=https' --tlsv1.2 -o "$dest" "$from"
    printf '%s\n' "$dest"
}

# verify FILE SIGNATURE KEYFILE SHA256 LABEL: the pinned SHA-256 and a good
# signature from the one key in KEYFILE (a keyring of its own, so another
# project's key can never vouch for this file). The status lines are read,
# not gpg's text, which follows the computer's language.
verify() {
    local file=$1 sig=$2 keyfile=$3 expected=$4 label=$5 home status
    [[ $(sha256sum "$file" | cut -c1-64) == "$expected" ]] || die "$label: wrong SHA-256"
    log "$label: SHA-256 verified"
    home=$work/gnupg-$label
    rm -rf "$home"
    mkdir -m 0700 "$home"
    GNUPGHOME=$home gpg --batch --quiet --import "$keyfile" 2>/dev/null || die "$label: cannot import $keyfile"
    status=$(GNUPGHOME=$home gpg --batch --status-fd 1 --verify "$sig" "$file" 2>/dev/null || true)
    GNUPGHOME=$home gpgconf --kill all >/dev/null 2>&1 || true
    [[ $status == *"[GNUPG:] VALIDSIG "* ]] || die "$label: invalid GPG signature"
    log "$label: GPG signature verified"
}

openssl_ver="" openssl_url="" openssl_sha256=""
read_openssl_pin() {
    [[ -n $openssl_pin && -f $openssl_pin ]] || die "OpenSSL from source needs --openssl-pin (native-engine/openssl.txt)"
    read -r openssl_ver openssl_url openssl_sha256 < <(grep -vE '^\s*(#|$)' "$openssl_pin" | head -n 1)
    [[ $openssl_ver =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "invalid OpenSSL version in $openssl_pin"
    [[ $openssl_sha256 =~ ^[0-9a-f]{64}$ ]] || die "invalid OpenSSL sha256 in $openssl_pin"
    [[ $openssl_url == "https://github.com/openssl/openssl/releases/download/openssl-$openssl_ver/openssl-$openssl_ver.tar.gz" ]] ||
        die "unexpected OpenSSL URL in $openssl_pin"
}

fetch_strongswan() {
    local file sig
    file=$(fetch_file "$url" "$tarball")
    sig=$(fetch_file "$url.sig" "$tarball.sig")
    verify "$file" "$sig" "$keys/strongswan-release.asc" "$sha256" strongswan
    strongswan_file=$file
}

fetch_openssl() {
    local file sig
    read_openssl_pin
    file=$(fetch_file "$openssl_url" "openssl-$openssl_ver.tar.gz")
    sig=$(fetch_file "$openssl_url.asc" "openssl-$openssl_ver.tar.gz.asc")
    verify "$file" "$sig" "$keys/openssl-release.asc" "$openssl_sha256" openssl
    openssl_file=$file
}

if ((fetch_only)); then
    need curl gpg sha256sum
    mkdir -p "$sources"
    fetch_strongswan
    [[ -z $openssl_pin ]] || fetch_openssl
    log "sources checked in $sources"
    exit 0
fi

# python3: strongSwan's configure resolves the static plugin constructors with it.
need gpg sha256sum tar bzip2 patch make pkg-config python3 file readelf strip gzip
[[ -n $sources ]] || need curl
[[ -f $cap_header ]] || die "capabilities header not found: $cap_header"

# --- The musl toolchain ---------------------------------------------------------

probe=$work/probe
mkdir -p "$probe"
cc_for_configure=""   # empty: configure picks the toolchain's own compiler
cc_argv=()

# is_static_pie FILE [needs-hardening]: one static-pie executable, with full
# RELRO, BIND_NOW and a non-executable stack when asked.
is_static_pie() {
    local h l d
    h=$(readelf -hW "$1" 2>/dev/null) || return 1
    l=$(readelf -lW "$1" 2>/dev/null) || return 1
    d=$(readelf -dW "$1" 2>/dev/null) || true
    [[ $h == *"DYN ("* && $l != *INTERP* && $d != *NEEDED* ]] || return 1
    [[ ${2:-} == needs-hardening ]] || return 0
    [[ $d == *BIND_NOW* && $l == *GNU_RELRO* && $l == *GNU_STACK* ]] || return 1
    [[ ! $l =~ GNU_STACK[^$'\n']*RWE ]]
}

# The builder's Alpine gcc makes -static a static-pie with RELRO and BIND_NOW
# (its defaults): its flags stay as they always were. Other toolchains get the
# same result asked for explicitly.
toolchain_mode=builder
cflags="-O2 -fstack-protector-strong -D_FORTIFY_SOURCE=2 -fstack-clash-protection"
configure_ldflags="-static"
make_ldflags="-all-static"
readonly HOST_LDFLAGS="-static-pie -Wl,-z,relro,-z,now"

try_link() { # OUTPUT FLAGS-AND-LIBRARIES... < source
    local output=$1
    shift
    # The source before the libraries: a static link resolves left to right.
    # shellcheck disable=SC2086 # cflags is a flag list
    "${cc_argv[@]}" $cflags -x c - -x none "$@" -o "$output" >"$probe/cc.log" 2>&1
}

# A musl-gcc wrapper whose specs file cannot link static-pie (Arch's): the
# same specs, with musl's rcrt1.o and the static-pie link line added.
patch_musl_specs() {
    local wrapper specs realgcc libdir
    wrapper=$(command -v "${cc_argv[0]}") || return 1
    [[ $(head -c 2 "$wrapper") == '#!' ]] || return 1
    specs=$(sed -n 's/.*-specs[ =]"\{0,1\}\([^" ]*\)"\{0,1\}.*/\1/p' "$wrapper" | head -n 1)
    # shellcheck disable=SC2016 # the wrapper's literal ${REALGCC:-gcc}
    realgcc=$(sed -n 's/.*\${REALGCC:-\([^}]*\)}.*/\1/p' "$wrapper" | head -n 1)
    [[ -f $specs ]] || return 1
    libdir=$(dirname "$specs")
    [[ -f $libdir/rcrt1.o ]] || return 1
    mkdir -p "$work/toolchain"
    sed -e "s|%{!shared: \([^ }]*\)/Scrt1\.o}|%{!shared: %{static-pie:\1/rcrt1.o;:\1/Scrt1.o}}|" \
        -e "/^\*link:/{n;s|-dynamic-linker \([^ ]*\)|%{!static-pie:-dynamic-linker \1}|;s|\$| %{static-pie:-static -pie --no-dynamic-linker -z text}|;}" \
        "$specs" >"$work/toolchain/musl-gcc.specs"
    # A wrapper like musl-gcc itself: configure, libtool and OpenSSL's
    # Configure all want one program as CC.
    printf '#!/bin/sh\nexec %s "$@" -specs %s\n' "${realgcc:-gcc}" "$work/toolchain/musl-gcc.specs" \
        >"$work/toolchain/musl-cc"
    chmod 0755 "$work/toolchain/musl-cc"
    cc_for_configure=$work/toolchain/musl-cc
    cc_argv=("$cc_for_configure")
    log "musl-gcc's specs cannot link static-pie: using a patched copy ($work/toolchain)"
}

setup_toolchain() {
    local hello='#include <stdio.h>
int main(void) { puts("ok"); return 0; }'
    if [[ -n ${CC:-} ]]; then
        cc_for_configure=$CC
    elif command -v cc >/dev/null 2>&1 &&
        printf '#include <stdio.h>\n#ifdef __GLIBC__\n#error glibc\n#endif\n' | cc -E -x c - >/dev/null 2>&1; then
        cc_for_configure=""
    elif command -v musl-gcc >/dev/null 2>&1; then
        cc_for_configure=musl-gcc
    elif [[ -x /usr/lib/musl/bin/musl-gcc ]]; then
        cc_for_configure=/usr/lib/musl/bin/musl-gcc
    else
        die "no musl C compiler: install musl-gcc (Fedora: musl-gcc musl-libc-static; Debian/Ubuntu: musl-tools; Arch: musl) or set CC"
    fi
    read -ra cc_argv <<<"${cc_for_configure:-cc}"

    # musl, not glibc: a static glibc charon would break name resolution.
    printf '#include <stdio.h>\n#ifdef __GLIBC__\n#error glibc\n#endif\n' |
        "${cc_argv[@]}" -E -x c - >/dev/null 2>"$probe/cc.log" ||
        die "${cc_argv[*]} is not a musl compiler (glibc headers): set CC to a musl one"

    # The kernel's UAPI headers (XFRM, netlink, capabilities). musl-gcc on a
    # glibc distribution does not search the system ones: link them in.
    if ! printf '#include <linux/xfrm.h>\n#include <linux/capability.h>\n' |
        "${cc_argv[@]}" -E -x c - >/dev/null 2>&1; then
        local kh=$work/kernel-headers dir base multiarch
        multiarch=$(gcc -dumpmachine 2>/dev/null || true)
        mkdir -p "$kh"
        for dir in linux asm asm-generic; do
            for base in /usr/include ${multiarch:+/usr/include/$multiarch}; do
                if [[ -d $base/$dir ]]; then
                    ln -sfn "$base/$dir" "$kh/$dir"
                    break
                fi
            done
        done
        cflags+=" -isystem $kh"
        # shellcheck disable=SC2086 # a flag list
        printf '#include <linux/xfrm.h>\n#include <linux/capability.h>\n' |
            "${cc_argv[@]}" $cflags -E -x c - >/dev/null 2>"$probe/cc.log" ||
            die "the kernel headers are missing (Fedora: kernel-headers; Debian/Ubuntu: linux-libc-dev; Arch: linux-api-headers)"
        log "kernel headers linked for musl: $kh"
    fi

    if try_link "$probe/hello" -static <<<"$hello" && is_static_pie "$probe/hello" needs-hardening; then
        toolchain_mode=builder
    else
        toolchain_mode=host
        cflags+=" -fPIE"
        configure_ldflags=$HOST_LDFLAGS
        make_ldflags=$HOST_LDFLAGS
        # shellcheck disable=SC2086 # a flag list
        if ! { try_link "$probe/hello" $HOST_LDFLAGS <<<"$hello" &&
            is_static_pie "$probe/hello" needs-hardening; }; then
            patch_musl_specs || die "${cc_argv[*]} cannot link a static-pie executable ($(tail -n 3 "$probe/cc.log" | tr '\n' ' '))"
            # shellcheck disable=SC2086 # a flag list
            { try_link "$probe/hello" $HOST_LDFLAGS <<<"$hello" &&
                is_static_pie "$probe/hello" needs-hardening; } ||
                die "${cc_argv[*]} cannot link a static-pie executable, even with patched specs"
        fi
    fi
    "$probe/hello" >/dev/null || die "the probe built by ${cc_argv[*]} does not run here"
    log "toolchain: ${cc_argv[*]} ($toolchain_mode flags)"
}

setup_toolchain
[[ $version == 5.8.* ]] && cflags+=" -fcommon"

# --- OpenSSL: the toolchain's own, or compiled here from the pin ------------------

openssl_prefix=""
evp_probe='#include <openssl/evp.h>
int main(void) { return EVP_CIPHER_CTX_new() == 0; }'
if [[ $openssl_mode != source ]]; then
    # shellcheck disable=SC2086 # a flag list
    if try_link "$probe/evp" $configure_ldflags -lcrypto <<<"$evp_probe"; then
        openssl_mode=system
    elif [[ $openssl_mode == system ]]; then
        die "the toolchain has no static OpenSSL for musl (use --openssl source)"
    else
        openssl_mode=source
    fi
fi

if [[ $openssl_mode == source ]]; then
    need perl
    fetch_openssl
    openssl_src=$work/openssl-src
    openssl_prefix=$work/openssl
    rm -rf "$openssl_src"
    mkdir -p "$openssl_src"
    tar -xzf "$openssl_file" -C "$openssl_src" --strip-components=1
    log "OpenSSL $openssl_ver: configure (static, no modules, no legacy provider)"
    # The same choices as the builder's package where they matter: one static
    # libcrypto, its configuration directory at /etc/ssl, and no legacy
    # provider (a static charon cannot load it; patch 0003, ADR-0027).
    (
        cd "$openssl_src"
        CC=${cc_for_configure:-cc} CFLAGS="$cflags" ./Configure linux-x86_64 \
            --prefix="$openssl_prefix" --libdir=lib --openssldir=/etc/ssl \
            no-shared no-module no-legacy no-engine no-afalgeng no-tests no-docs \
            >"$work/openssl-configure.log" 2>&1 || { tail -n 30 "$work/openssl-configure.log"; exit 1; }
        log "OpenSSL $openssl_ver: make -j$(nproc)"
        make -j"$(nproc)" build_libs >"$work/openssl-make.log" 2>&1 || { tail -n 40 "$work/openssl-make.log"; exit 1; }
        make install_dev >"$work/openssl-install.log" 2>&1 || { tail -n 20 "$work/openssl-install.log"; exit 1; }
    )
    cflags+=" -I$openssl_prefix/include"
    configure_ldflags+=" -L$openssl_prefix/lib"
    make_ldflags+=" -L$openssl_prefix/lib"
    # shellcheck disable=SC2086 # a flag list
    try_link "$probe/evp" $configure_ldflags -lcrypto <<<"$evp_probe" ||
        die "the OpenSSL built here does not link: $(tail -n 3 "$probe/cc.log" | tr '\n' ' ')"
fi
log "OpenSSL: $openssl_mode"

# --- strongSwan -----------------------------------------------------------------

mkdir -p "$SRC" "$STAGE"
fetch_strongswan
tar -xjf "$strongswan_file" -C "$SRC" --strip-components=1

# Ikebox patches (ADR-0019). A patch that does not apply fails the
# build: the engine without it would be insecure.
patches=()
for patch_file in "$patches_dir"/*.patch; do
    [[ -f $patch_file ]] || continue
    patch -d "$SRC" -p1 --forward --quiet <"$patch_file" ||
        { echo "patch $(basename "$patch_file") does not apply to strongSwan $version" >&2; exit 1; }
    patches+=("$(basename "$patch_file"):$(sha256sum "$patch_file" | cut -c1-64)")
    log "patch applied: $(basename "$patch_file")"
done
((${#patches[@]} > 0)) || { echo "no patch in $patches_dir" >&2; exit 1; }

configure_flags=(
    --prefix="$PREFIX"
    --sysconfdir="$PREFIX/etc"
    --libexecdir="$PREFIX/libexec"
    --with-piddir=/run/ikebox-engine
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
    # Smartcards and tokens (ADR-0029): charon asks the person's process to
    # sign over the ssh-agent protocol (agent), which sends keys in the SSH
    # format (sshkey). The token never reaches charon.
    --enable-agent --enable-sshkey
    --disable-pki --disable-scepclient
)
# 6.x: PRF+ for IKEv2 without OpenSSL lives in the kdf plugin.
[[ $version == 6.* ]] && configure_flags+=(--enable-kdf)

cd "$SRC"
log "configure (${#configure_flags[@]} options)"
# 5.8.x predates -fno-common, gcc >= 10's default (duplicate definitions).
# Hardening stated, not left to the compiler's defaults: stack protector and
# FORTIFY (Alpine's fortify-headers, part of build-base; a musl toolchain
# without them ignores it). PIE, full RELRO and a non-executable stack come
# from the static-pie link, checked on charon below.
cc_env=()
[[ -z $cc_for_configure ]] || cc_env=(CC="$cc_for_configure")
env "${cc_env[@]}" CFLAGS="$cflags" LDFLAGS="$configure_ldflags" ./configure "${configure_flags[@]}" >"$work/configure.log" 2>&1 ||
    { tail -n 40 "$work/configure.log"; exit 1; }
log "make -j$(nproc)"
# Builder: -all-static, libtool's way of linking the executables fully static
# (its -static would still take libc as a shared library). Elsewhere the
# compiler gets -static-pie itself: libtool's -static would pick musl's
# non-PIE start file.
# capget/capset declarations for the native capability backend (ADR-0028),
# only when compiling (CPPFLAGS adds to configure's flags): in configure they
# would clash with its own test of capset.
make -j"$(nproc)" LDFLAGS="$make_ldflags" \
    CPPFLAGS="-include $cap_header" >"$work/make.log" 2>&1 || { tail -n 60 "$work/make.log"; exit 1; }
make install DESTDIR="$STAGE" >"$work/install.log" 2>&1 || { tail -n 30 "$work/install.log"; exit 1; }

root="$STAGE$PREFIX"
find "$root" -type f -perm -u+x -exec sh -c 'file -b "$1" | grep -q ELF && strip "$1"' _ {} \;
# Static build: the .a libraries and the headers are of no use at run time.
rm -rf "${root:?}/share/man" "${root:?}/lib" "${root:?}/include"

charon="$root/libexec/ipsec/charon"
[[ -x $charon ]] || { echo "charon was not built" >&2; exit 1; }
# Fully static, or it is not the engine of ADR-0020.
if [[ -n $(readelf -d "$charon" 2>/dev/null | grep NEEDED || true) ]]; then
    echo "charon is not fully static: $(readelf -d "$charon" | grep NEEDED)" >&2
    exit 1
fi
# And hardened like the release engine: static-pie (ASLR), full RELRO,
# BIND_NOW, no executable stack.
is_static_pie "$charon" needs-hardening ||
    die "charon is not a hardened static-pie executable ($(file -b "$charon" | cut -d, -f1-4))"

# Test tools: swanctl goes to its own tarball, never to users. What
# production does not run stays out of the engine (audit of 0.2.0, P7):
# strongSwan's stock _updown, the xfrmi tool, swanctl's configuration and the
# configuration templates.
TOOLS=$work/tools
mkdir -p "$TOOLS$PREFIX/sbin"
mv "$root/sbin/swanctl" "$TOOLS$PREFIX/sbin/swanctl"
rmdir "$root/sbin"
rm -rf "${root:?}/libexec/ipsec/_updown" "${root:?}/libexec/ipsec/xfrmi" \
    "${root:?}/share/strongswan" "${root:?}/etc/swanctl" "${root:?}/etc/strongswan.d/swanctl.conf"

# Compliance (ADR-0017): license texts of strongSwan, OpenSSL and musl.
mkdir -p "$root/share/licenses/strongswan"
cp "$SRC/COPYING" "$SRC/LICENSE" "$root/share/licenses/strongswan/"
cp -R "$licenses/openssl" "$licenses/musl" "$root/share/licenses/"

# What went in, for BUILDINFO. In the builder (and on any Alpine), the
# packages say it; elsewhere, the sources and the toolchain do.
musl_from_libc() {
    local libc banner
    libc=$("${cc_argv[@]}" -print-file-name=libc.so 2>/dev/null || true)
    if [[ -f $libc ]]; then
        banner=$("$libc" 2>&1 || true)
        banner=$(sed -n 's/^Version \([0-9.]*\).*/\1/p' <<<"$banner" | head -n 1)
        [[ -z $banner ]] || { printf '%s\n' "$banner"; return; }
    fi
    # The distribution's package, then.
    if command -v rpm >/dev/null 2>&1 && rpm -q musl-libc >/dev/null 2>&1; then
        rpm -q --qf '%{VERSION}\n' musl-libc
    elif command -v dpkg-query >/dev/null 2>&1 && dpkg-query -W musl-dev >/dev/null 2>&1; then
        dpkg-query -W -f '${Version}\n' musl-dev
    elif command -v pacman >/dev/null 2>&1 && pacman -Q musl >/dev/null 2>&1; then
        pacman -Q musl | awk '{print $2}'
    fi
}
if [[ -f /etc/alpine-release && $openssl_mode == system ]] && command -v apk >/dev/null 2>&1; then
    openssl_version=$(apk list -I openssl-libs-static 2>/dev/null | awk '{print $1}' | sed 's/^openssl-libs-static-//')
    musl_version=$(apk list -I musl 2>/dev/null | awk '{print $1}' | sed 's/^musl-//')
    [[ -n $openssl_version && -n $musl_version ]] || { echo "could not read the OpenSSL/musl versions" >&2; exit 1; }
    builder="alpine:$(cat /etc/alpine-release) (musl $musl_version)"
    openssl_line="$openssl_version (static; Alpine package openssl-libs-static)"
else
    musl_version=$(musl_from_libc)
    [[ -n $musl_version ]] || musl_version=unknown
    # shellcheck disable=SC1091 # the computer's own file
    os=$(. /etc/os-release 2>/dev/null && printf '%s %s' "${ID:-linux}" "${VERSION_ID:-}")
    builder="host:${os% } (musl $musl_version; $("${cc_argv[@]}" --version 2>/dev/null | head -n 1))"
    if [[ $openssl_mode == source ]]; then
        openssl_line="$openssl_ver (static; built here from openssl-$openssl_ver.tar.gz, SHA-256 $openssl_sha256)"
    else
        # shellcheck disable=SC2086 # a flag list
        openssl_version=$(printf '#include <openssl/opensslv.h>\nOPENSSL_VERSION_STR\n' |
            "${cc_argv[@]}" $cflags -E -P -x c - 2>/dev/null | tail -n 1 | tr -d '" ')
        openssl_line="${openssl_version:-unknown} (static; the toolchain's own)"
    fi
fi

{
    echo "strongswan=$version"
    echo "sha256=$sha256"
    echo "builder=$builder"
    echo "openssl=$openssl_line"
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
mkdir -p "$out"
pack "$STAGE" "$out/ikebox-engine-$version-x86_64.tar.gz"
pack "$TOOLS" "$out/ikebox-engine-tools-$version-x86_64.tar.gz"
log "charon: $(file -b "$charon" | cut -d, -f1-2)"
