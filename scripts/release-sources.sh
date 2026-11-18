#!/usr/bin/env bash
# Builds the corresponding-source archive of what the release packages
# distribute in binary form under the GPL: the embedded
# strongSwan (GPL-2.0-or-later) with the project's patches, and OpenSSL
# (Apache-2.0) and musl (MIT), linked statically into it (ADR-0027). It is attached to the release next to the packages (the
# project's own source is the tagged repository itself).
#
# Every download is checked against a hash recorded in the repository:
# native-engine/versions.txt (strongSwan) and native-engine/licenses/README.md
# (OpenSSL and musl). Alpine's packaging of both (APKBUILD and patches)
# comes from fixed commits of aports, the ones of the versions in BUILDINFO.
#
# Usage: scripts/release-sources.sh <version>
#   → target/release-sources/<version>/ikebox-<version>-sources.tar
set -euo pipefail

readonly OPENSSL_VERSION=3.5.9
readonly OPENSSL_SHA256=603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a
readonly MUSL_VERSION=1.2.6
readonly MUSL_SHA256=d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a
# aports (3.24-stable) commits of openssl 3.5.9-r0 and musl 1.2.6-r3.
readonly APORTS_OPENSSL=29b9ec24b1b5b39aeef51fa2a044210e2ec5258e
readonly APORTS_MUSL=8f79d39391b46d6dbf1800fb68b8212a21d5ee2e

version=${1:?usage: $0 <version>}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]] || {
    echo "error: invalid version: $version" >&2
    exit 1
}
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
out="$repo/target/release-sources/$version"
name="ikebox-$version-sources"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
dir="$work/$name"
mkdir -p "$dir/patches" "$dir/alpine"

fetch() { # fetch <file> <url> [sha256]
    curl --proto '=https' --tlsv1.2 -fsSL -o "$dir/$1" "$2"
    if [[ -n ${3:-} ]]; then
        echo "$3  $dir/$1" | sha256sum -c --quiet - || {
            echo "error: $1 does not match the recorded SHA-256" >&2
            exit 1
        }
    fi
}

read -r ss_version ss_path ss_sha < <(grep -vE '^\s*(#|$)' "$repo/native-engine/versions.txt" | head -n 1)
fetch "strongswan-$ss_version.tar.bz2" \
    "https://download.strongswan.org${ss_path}strongswan-$ss_version.tar.bz2" "$ss_sha"
fetch "strongswan-$ss_version.tar.bz2.sig" \
    "https://download.strongswan.org${ss_path}strongswan-$ss_version.tar.bz2.sig"
cp "$repo"/native-engine/patches/*.patch "$dir/patches/"
cp "$repo/native-engine/build/build-strongswan.sh" "$dir/"

ossl="https://github.com/openssl/openssl/releases/download/openssl-$OPENSSL_VERSION"
fetch "openssl-$OPENSSL_VERSION.tar.gz" "$ossl/openssl-$OPENSSL_VERSION.tar.gz" "$OPENSSL_SHA256"
fetch "openssl-$OPENSSL_VERSION.tar.gz.asc" "$ossl/openssl-$OPENSSL_VERSION.tar.gz.asc"
fetch "musl-$MUSL_VERSION.tar.gz" "https://musl.libc.org/releases/musl-$MUSL_VERSION.tar.gz" "$MUSL_SHA256"
fetch "musl-$MUSL_VERSION.tar.gz.asc" "https://musl.libc.org/releases/musl-$MUSL_VERSION.tar.gz.asc"
for pkg in openssl:$APORTS_OPENSSL musl:$APORTS_MUSL; do
    fetch "alpine/aports-${pkg%%:*}-${pkg#*:}.tar.gz" \
        "https://gitlab.alpinelinux.org/alpine/aports/-/archive/${pkg#*:}/aports-${pkg#*:}.tar.gz?path=main/${pkg%%:*}"
done

cat >"$dir/README.txt" <<README
Corresponding source of the third-party code in Ikebox $version

The packages of Ikebox $version carry a compiled strongSwan
(GPL-2.0-or-later), statically linked with OpenSSL (Apache-2.0) and musl
(MIT). This archive holds their source:

  strongswan-$ss_version.tar.bz2 (+ .sig)   official release, SHA-256 $ss_sha
  patches/                           Ikebox's changes to strongSwan
  build-strongswan.sh                how it is configured and built
  openssl-$OPENSSL_VERSION.tar.gz (+ .asc)    official release
  musl-$MUSL_VERSION.tar.gz (+ .asc)          official release
  alpine/                            Alpine's packaging of OpenSSL and musl
                                     (aports commits $APORTS_OPENSSL
                                     and $APORTS_MUSL)

To rebuild the engine as shipped, use native-engine/build.sh from the
Ikebox repository at tag v$version. See THIRD_PARTY_NOTICES.md there.
README

mkdir -p "$out"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -C "$work" -cf "$out/$name.tar" "$name"
(cd "$out" && sha256sum "$name.tar" >"$name.tar.sha256")
echo "corresponding source: $out/$name.tar"
