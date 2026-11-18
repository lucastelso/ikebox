#!/usr/bin/env bash
# Builds the release packages (AppImage, deb and rpm) in an Ubuntu 22.04
# container.
#
# Usage:
#   scripts/build-release.sh [<tag or commit>]     (default: HEAD)
#
# - The code comes from the given commit (git archive): uncommitted changes
#   stay out, and no path of your computer gets into the binaries.
# - The embedded strongSwan comes from native-engine/dist/ (built here, with
#   the source checked by SHA-256 + GPG, if it does not exist yet).
# - Output: target/release-artifacts/<version>/ with the three packages and
#   SHA256SUMS, after being checked by scripts/verify-release.sh and scanned
#   by scripts/check-private.sh --artifacts (private data, on the host).
#   Built from anything that is not the version tag (HEAD, a commit), the
#   output goes to target/release-artifacts/<version>-dev-<commit>/: a test
#   build never replaces the packages of a published version.
#
# See docs/release.md to publish on GitHub.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"

die() {
    printf 'build-release: error: %s\n' "$*" >&2
    exit 1
}
step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

ref=${1:-HEAD}
commit=$(git rev-parse --verify --quiet "$ref^{commit}") || die "commit/tag not found: $ref"
version=$(git show "$commit:Cargo.toml" |
    awk -F'"' '/^\[workspace.package\]/ {s = 1} s && /^version *=/ {print $2; exit}')
[[ -n $version ]] || die "could not read the version in the Cargo.toml of $ref"
if [[ $ref == v* && $ref != "v$version" ]]; then
    die "tag $ref does not match version $version in Cargo.toml"
fi
if [[ $ref == HEAD && -n $(git status --porcelain) ]]; then
    echo "warning: there are uncommitted changes; they are NOT part of the build" >&2
fi

strongswan=$(git show "$commit:native-engine/versions.txt" | grep -vE '^\s*(#|$)' | head -n 1 | awk '{print $1}')
engine="native-engine/dist/ikebox-engine-$strongswan-x86_64.tar.gz"
# Security patches of the commit (ADR-0019), in the BUILDINFO format.
expected_patches=$(git ls-tree --name-only "$commit" native-engine/patches/ | grep '\.patch$' | sort |
    while read -r path; do
        printf '%s:%s\n' "$(basename "$path")" "$(git show "$commit:$path" | sha256sum | cut -c1-64)"
    done | paste -sd ' ')
engine_patches=""
[[ -f $engine ]] && engine_patches=$(tar -xzOf "$engine" opt/ikebox/engine/BUILDINFO | sed -n 's/^patches=//p')
if [[ ! -f $engine || $engine_patches != "$expected_patches" ]]; then
    step "building strongSwan $strongswan (with the patches of the commit)"
    native-engine/build.sh "$strongswan"
fi

rust=$(git show "$commit:rust-toolchain.toml" | awk -F'"' '/^channel/ {print $2}')
image="ikebox-release:rust$rust-$(sha256sum packaging/release/Dockerfile | cut -c1-12)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
    step "preparing the build image ($image)"
    docker build --build-arg "RUST_VERSION=$rust" -t "$image" packaging/release
fi

if [[ $ref == "v$version" ]]; then
    out="$repo/target/release-artifacts/$version"
else
    out="$repo/target/release-artifacts/$version-dev-${commit:0:12}"
fi
rm -rf "$out"
mkdir -p "$out"

step "building AppImage, deb and rpm for version $version (commit ${commit:0:12})"
# Named volumes keep only downloads (crates, npm); the build always starts
# from scratch. The AppImage tools come pinned inside the image, so Tauri's
# cache is not a volume: the check after the bundle sees exactly what it used.
git archive --format=tar "$commit" |
    docker run --rm -i \
        -v "$repo/$engine:/dist/engine.tar.gz:ro" \
        -v "$out:/out" \
        -v ikebox-release-cargo:/root/.cargo/registry \
        -v ikebox-release-npm:/root/.npm \
        -e "ENGINE_FILE=$(basename "$engine")" \
        -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
        "$image" bash -euo pipefail -c '
            tar -x -C /build
            install -D -m 0644 /dist/engine.tar.gz "native-engine/dist/$ENGINE_FILE"
            npm ci --ignore-scripts --no-audit --no-fund
            npm run bundle
            echo "==> checking the AppImage tools Tauri used (pinned in the image)"
            (cd /root/.cache/tauri && sha256sum -c --quiet /etc/ikebox-tauri-tools.sha256)
            extra=$(cd /root/.cache/tauri && find . -type f ! -name "linuxdeploy-plugin-g*.sh" |
                sed "s#^\./##" | sort | comm -23 - <(sed "s#.*  \./##" /etc/ikebox-tauri-tools.sha256 | sort))
            if [[ -n $extra ]]; then
                echo "Tauri downloaded tools that are not pinned in the image: $extra" >&2
                exit 1
            fi
            cp target/release/bundle/appimage/*.AppImage \
               target/release/bundle/deb/*.deb \
               target/release/bundle/rpm/*.rpm /out/
            chown -R "$HOST_UID:$HOST_GID" /out
            echo "==> checking the packages"
            scripts/verify-release.sh /out
        '

(cd "$out" && sha256sum -- *.AppImage *.deb *.rpm >SHA256SUMS)

# Here on the host, where the private patterns are (the container does not
# see them): nothing of the maintainer's may have got into the packages.
step "looking for private data inside the packages"
scripts/check-private.sh --artifacts "$out"
step "packages ready in ${out#"$repo"/}"
ls -lh "$out"
