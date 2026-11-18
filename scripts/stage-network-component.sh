#!/usr/bin/env bash
# Builds the network component bundle: the directory that goes inside the app
# (a Tauri resource, ADR-0018) and that the installer uses with --bundle.
#
# Usage:
#   scripts/stage-network-component.sh [--out <dir>] [--engine <tarball>] [--helper <binary>]
#
# Without --engine/--helper, it builds what is missing (the embedded
# strongSwan through Docker, the helper with cargo). Default output:
# target/network-component/
#
# Content (paths relative to the bundle):
#   VERSION                         version=<app> and strongswan=<version>
#   SHA256SUMS                      hash of every other file
#   install-native.sh               installer (runs as root)
#   engine.tar.gz                   compiled strongSwan
#   ikebox-helper.gz          privileged helper, compressed: linuxdeploy
#                                   (AppImage) rewrites the RPATH of every
#                                   loose ELF in the bundle, and the hash
#                                   would no longer match
#   engine/{entrypoint.sh,updown.sh,strongswan.d/zz-ikebox.conf}
#   packaging/systemd/ikebox-helper.{socket,service}
#   packaging/polkit/io.github.lucastelso.ikebox.policy

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
readonly POLICY=io.github.lucastelso.ikebox.policy

die() {
    printf 'stage-network-component: error: %s\n' "$*" >&2
    exit 1
}

out="$repo/target/network-component"
engine=""
helper=""
while (($# > 0)); do
    case $1 in
    --out) out=${2:?}; shift 2 ;;
    --engine) engine=${2:?}; shift 2 ;;
    --helper) helper=${2:?}; shift 2 ;;
    *) die "argumento desconhecido: $1" ;;
    esac
done

strongswan=$(grep -vE '^\s*(#|$)' "$repo/native-engine/versions.txt" | head -n 1 | awk '{print $1}')
app_version=$(awk -F'"' '/^\[workspace.package\]/ {s = 1} s && /^version *=/ {print $2; exit}' "$repo/Cargo.toml")
[[ -n $strongswan && -n $app_version ]] || die "could not read the versions"

if [[ -z $engine ]]; then
    engine="$repo/native-engine/dist/ikebox-engine-$strongswan-x86_64.tar.gz"
    [[ -f $engine ]] || "$repo/native-engine/build.sh" "$strongswan"
fi
if [[ -z $helper ]]; then
    # rustup's cargo (it honors rust-toolchain.toml), even with another one in
    # PATH.
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
    fi
    (cd "$repo" && cargo build --release --locked -p ikebox-helper)
    helper="$repo/target/release/ikebox-helper"
fi
[[ -f $engine ]] || die "engine tarball not found: $engine"
[[ -f $helper ]] || die "helper not found: $helper"

# The engine must have exactly the security patches of this commit
# (ADR-0019): an old tarball in native-engine/dist/ would leave the app
# vulnerable.
expected_patches=()
for patch_file in "$repo"/native-engine/patches/*.patch; do
    [[ -f $patch_file ]] || continue
    expected_patches+=("$(basename "$patch_file"):$(sha256sum "$patch_file" | cut -c1-64)")
done
engine_patches=$(tar -xzOf "$engine" opt/ikebox/engine/BUILDINFO 2>/dev/null |
    sed -n 's/^patches=//p')
[[ $engine_patches == "${expected_patches[*]}" ]] ||
    die "the engine $engine lacks the patches of this commit (native-engine/patches): rebuild it with native-engine/build.sh"

rm -rf "${out:?}"
install -d -m 0755 "$out" "$out/engine/strongswan.d" "$out/packaging/systemd" "$out/packaging/polkit"
install -m 0644 "$engine" "$out/engine.tar.gz"
gzip -9 -n -c "$helper" >"$out/ikebox-helper.gz"
chmod 0644 "$out/ikebox-helper.gz"
install -m 0755 "$repo/packaging/install-native.sh" "$out/install-native.sh"
install -m 0755 "$repo/engine/entrypoint.sh" "$repo/engine/updown.sh" "$out/engine/"
install -m 0644 "$repo/engine/strongswan.d/zz-ikebox.conf" "$out/engine/strongswan.d/"
install -m 0644 "$repo/packaging/systemd/ikebox-helper.socket" \
    "$repo/packaging/systemd/ikebox-helper.service" "$out/packaging/systemd/"
install -m 0644 "$repo/packaging/polkit/$POLICY" "$out/packaging/polkit/"
printf 'version=%s\nstrongswan=%s\n' "$app_version" "$strongswan" >"$out/VERSION"

# Fixed order: SHA256SUMS also identifies the bundle (the app compares it with
# the installed one to know whether there is an update).
# shellcheck disable=SC2094 # find excludes SHA256SUMS itself
(
    cd "$out"
    find . -type f ! -name SHA256SUMS -printf '%P\n' | LC_ALL=C sort |
        xargs -d '\n' sha256sum >SHA256SUMS
)
echo "network component bundle in $out (app $app_version, strongSwan $strongswan)"
