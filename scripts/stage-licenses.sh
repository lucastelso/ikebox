#!/usr/bin/env bash
# Prepares the license folder that goes into every package (release builds,
# src-tauri/tauri.release.conf.json), installed as /usr/lib/ikebox/
# licenses (inside the AppImage too):
#   LICENSE                     the project's license (GPL-3.0-or-later)
#   THIRD_PARTY_NOTICES.md      what is distributed, and where its source is
#   THIRD_PARTY_LICENSES.txt    texts of the crates and npm packages compiled
#                               into the app, the CLI and the helper
#   engine/                     strongSwan, OpenSSL and musl texts, taken from
#                               the staged engine (the one being shipped)
# Runs after scripts/stage-network-component.sh.
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
out="$repo/target/licenses"
engine="$repo/target/network-component/engine.tar.gz"

[[ -f $engine ]] || {
    echo "error: $engine not found; run scripts/stage-network-component.sh first" >&2
    exit 1
}

cd "$repo"
rm -rf "$out"
install -d "$out/engine"
install -m 0644 LICENSE THIRD_PARTY_NOTICES.md "$out/"
node scripts/third-party-licenses.mjs "$out/THIRD_PARTY_LICENSES.txt"
tar -xzf "$engine" -C "$out/engine" --strip-components=5 --no-same-owner \
    opt/ikebox/engine/share/licenses
[[ -n $(find "$out/engine" -type f -print -quit) ]] || {
    echo "error: the engine carries no license texts" >&2
    exit 1
}
echo "licenses staged in $out"
