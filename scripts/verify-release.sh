#!/usr/bin/env bash
# Confere os pacotes de release antes de publicar:
#
# - em cada pacote (AppImage, deb e rpm), o componente de rede embutido
#   precisa bater com o próprio SHA256SUMS. O app recusa instalar um
#   componente que não bate, e o empacotamento já alterou arquivos antes (o
#   linuxdeploy reescreve o RPATH de executáveis dentro do AppImage);
# - o strongSwan embutido precisa ter os patches de segurança do commit
#   (native-engine/patches, ADR-0019) e ser um executável totalmente estático
#   (ADR-0020), com as versões de OpenSSL e musl registradas no BUILDINFO;
# - o AppImage não pode exigir uma glibc mais nova que a do Ubuntu 22.04.
#
# Uso: scripts/verify-release.sh <diretório com os pacotes>
# Precisa de: binutils (ar, objdump), rpm2cpio e cpio. Roda sozinho no fim do
# scripts/build-release.sh.

set -euo pipefail

readonly MAX_GLIBC=2.35

ok() { printf '  \033[0;32m✔\033[0m %s\n' "$*"; }
fail() {
    printf '  \033[0;31m✘ %s\033[0m\n' "$*" >&2
    exit 1
}

dir=$(cd "${1:?usage: verify-release.sh <directory with the packages>}" && pwd)
shopt -s nullglob
appimages=("$dir"/*.AppImage)
debs=("$dir"/*.deb)
rpms=("$dir"/*.rpm)
((${#appimages[@]} == 1 && ${#debs[@]} == 1 && ${#rpms[@]} == 1)) ||
    fail "expected one AppImage, one deb and one rpm in $dir"

work=$(mktemp -d)
# shellcheck disable=SC2064 # caminho fixado agora
trap "rm -rf '$work'" EXIT

# Patches de segurança do strongSwan esperados (ADR-0019), como no BUILDINFO.
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
expected_patches=()
for patch_file in "$repo"/native-engine/patches/*.patch; do
    [[ -f $patch_file ]] || continue
    expected_patches+=("$(basename "$patch_file"):$(sha256sum "$patch_file" | cut -c1-64)")
done
((${#expected_patches[@]} > 0)) || fail "no patch in native-engine/patches"

component_ok() {
    local name=$2 net="$1/usr/lib/ipsec-client/network" patches info charon
    [[ -f $net/SHA256SUMS ]] || fail "$name: network component missing"
    (cd "$net" && sha256sum --check --strict --quiet SHA256SUMS) ||
        fail "$name: the network component does not match its SHA256SUMS"
    info=$(tar -xzOf "$net/engine.tar.gz" opt/ipsec-client/engine/BUILDINFO)
    patches=$(sed -n 's/^patches=//p' <<<"$info")
    [[ $patches == "${expected_patches[*]}" ]] ||
        fail "$name: the embedded strongSwan lacks the security patches (ADR-0019)"
    # One fully static executable (ADR-0020), with what went into it recorded.
    charon="$work/charon-$name"
    tar -xzOf "$net/engine.tar.gz" opt/ipsec-client/engine/libexec/ipsec/charon >"$charon"
    [[ -z $(readelf -d "$charon" 2>/dev/null | grep NEEDED || true) ]] ||
        fail "$name: charon is not fully static"
    if ! grep -q '^linkage=static$' <<<"$info" || ! grep -q '^openssl=' <<<"$info" ||
        ! grep -q '^musl=' <<<"$info"; then
        fail "$name: the engine BUILDINFO does not record the static build (linkage, openssl, musl)"
    fi
    ok "$name: network component intact ($(grep -h '^version=' "$net/VERSION"), strongSwan with the patches, static: OpenSSL $(sed -n 's/^openssl=\([^ ]*\).*/\1/p' <<<"$info"), musl $(sed -n 's/^musl=\([^ ]*\).*/\1/p' <<<"$info"))"
}

# The license folder every package carries (scripts/stage-licenses.sh).
licenses_ok() {
    local dir=$1/usr/lib/ipsec-client/licenses name=$2 f
    for f in LICENSE THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.txt; do
        [[ -s $dir/$f ]] || fail "$name: licenses/$f missing"
    done
    [[ -s $dir/engine/strongswan/COPYING && -s $dir/engine/openssl/LICENSE.txt &&
        -n $(find "$dir/engine/musl" -type f -print -quit) ]] ||
        fail "$name: the strongSwan, OpenSSL or musl license texts are missing"
    ok "$name: license texts (project, $(grep -m1 -oE '^[0-9]+ components' "$dir/THIRD_PARTY_LICENSES.txt"), strongSwan, OpenSSL, musl)"
}

# The command line the .rpm/.deb carry: the binary (same version as the
# component), its manual page and the shell completions.
cli_ok() {
    local root=$1 name=$2 version
    [[ -x $root/usr/bin/ipsec-client-cli ]] || fail "$name: ipsec-client-cli missing"
    version=$(sed -n 's/^version=//p' "$root/usr/lib/ipsec-client/network/VERSION")
    [[ $("$root/usr/bin/ipsec-client-cli" --version) == "ipsec-client-cli $version" ]] ||
        fail "$name: ipsec-client-cli is not version $version"
    gzip -t "$root/usr/share/man/man1/ipsec-client-cli.1.gz" ||
        fail "$name: manual page missing or corrupted"
    [[ -s $root/usr/share/bash-completion/completions/ipsec-client-cli &&
        -s $root/usr/share/fish/vendor_completions.d/ipsec-client-cli.fish ]] ||
        fail "$name: shell completions missing"
    ok "$name: ipsec-client-cli $version, manual page and completions"
    local meta=$root/usr/share/metainfo/io.github.lucastelso.ipsecclient.metainfo.xml
    [[ -f $meta ]] || fail "$name: AppStream metainfo missing"
    grep -q "<release version=\"$version\"" "$meta" ||
        fail "$name: the AppStream metainfo has no <release> for $version (docs/release.md)"
    ok "$name: AppStream metainfo with release $version"
}

mkdir "$work/appimage" "$work/deb" "$work/rpm"
(cd "$work/appimage" && "${appimages[0]}" --appimage-extract >/dev/null) ||
    fail "AppImage: could not extract the package"
component_ok "$work/appimage/squashfs-root" "AppImage"
# One version everywhere: the component inside and the three file names
# (from tauri.conf.json, while the component's comes from Cargo.toml).
version=$(sed -n 's/^version=//p' "$work/appimage/squashfs-root/usr/lib/ipsec-client/network/VERSION")
[[ ${appimages[0]##*/} == *"_${version}_amd64.AppImage" && ${debs[0]##*/} == *"_${version}_amd64.deb" &&
    ${rpms[0]##*/} == *"-${version}-1.x86_64.rpm" ]] ||
    fail "the package names do not all carry version $version (Cargo.toml and tauri.conf.json differ?)"
ok "one version: $version in the component and in the three packages"
licenses_ok "$work/appimage/squashfs-root" "AppImage"
# Every ELF file inside it, not only the app: the bundled libraries
# (WebKitGTK, among others) are what require the newest glibc.
glibc=$(find "$work/appimage/squashfs-root" -type f -exec sh -c \
    'for f; do if head -c 4 "$f" | grep -q "ELF"; then objdump -T "$f" 2>/dev/null; fi; done; exit 0' _ {} + |
    grep -oE 'GLIBC_[0-9.]+' | sed 's/GLIBC_//' | sort -Vu | tail -n 1)
[[ -n $glibc ]] || fail "AppImage: could not read the glibc versions"
printf '%s\n%s\n' "$glibc" "$MAX_GLIBC" | sort -VC ||
    fail "AppImage: requires glibc $glibc (the limit is $MAX_GLIBC, from Ubuntu 22.04)"
ok "AppImage: requires glibc $glibc, counting the bundled libraries (limit $MAX_GLIBC)"

(cd "$work/deb" && ar x "${debs[0]}" && tar -xf data.tar.*) ||
    fail "deb: could not extract the package"
component_ok "$work/deb" "deb"
cli_ok "$work/deb" "deb"
licenses_ok "$work/deb" "deb"
# The maintainer scripts that install and remove the component (ADR-0018).
mkdir "$work/deb-control"
tar -xf "$work"/deb/control.tar.* -C "$work/deb-control" ||
    fail "deb: could not read the control archive"
[[ -f $work/deb-control/postinst && -f $work/deb-control/prerm ]] ||
    fail "deb: postinst/prerm missing"
if ! grep -q 'install-native.sh" install --bundle' "$work/deb-control/postinst" ||
    ! grep -q 'install-native.sh" uninstall' "$work/deb-control/prerm"; then
    fail "deb: postinst/prerm do not manage the network component"
fi
ok "deb: postinst installs and prerm removes the network component"

# Ubuntu 22.04's rpm2cpio extracts Tauri's packages completely but exits
# with 1. The component check right below decides: an incomplete extraction
# fails there.
(cd "$work/rpm" && { rpm2cpio "${rpms[0]}" || true; } | cpio -idm --quiet) ||
    fail "rpm: could not extract the package"
component_ok "$work/rpm" "rpm"
cli_ok "$work/rpm" "rpm"
licenses_ok "$work/rpm" "rpm"
