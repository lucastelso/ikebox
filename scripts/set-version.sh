#!/usr/bin/env bash
# Sets the version where the build reads it (docs/release.md, step 1): the
# Cargo workspace, the core's dependency on the protocol crate, the Tauri
# config and package.json, and refreshes the lock files.
#
# Usage: scripts/set-version.sh <X.Y.Z>     (e.g. scripts/set-version.sh 1.0.1)
set -euo pipefail

version=${1:-}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "usage: $0 <X.Y.Z>, with the real numbers (e.g. 1.0.1)" >&2
    exit 2
}
cd "$(dirname "${BASH_SOURCE[0]}")/.."

sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
sed -i "s/ikebox-proto = { version = \"[^\"]*\"/ikebox-proto = { version = \"$version\"/" crates/core/Cargo.toml
sed -i "s/^  \"version\": \".*\",/  \"version\": \"$version\",/" src-tauri/tauri.conf.json
npm version "$version" --no-git-tag-version --allow-same-version >/dev/null
# shellcheck source=/dev/null
[[ -f $HOME/.cargo/env ]] && . "$HOME/.cargo/env"
cargo update -q -w

echo "version $version set. Still by hand (docs/release.md): the file names in"
echo "docs/installation.md and docs/pt-BR/instalacao.md, the CHANGELOG, the AppStream"
echo "<release> and the release notes."
