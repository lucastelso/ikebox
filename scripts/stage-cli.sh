#!/usr/bin/env bash
# Prepares the command line for the .rpm/.deb packages (release builds,
# src-tauri/tauri.release.conf.json): the ipsec-client-cli binary, its
# manual page and the shell completions, in target/cli-assets/. The
# AppImage does not carry the CLI (it cannot put it on the PATH).
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
out="$repo/target/cli-assets"

cd "$repo"
cargo build --release --locked -p ipsec-client-cli
cli="$repo/target/release/ipsec-client-cli"

rm -rf "$out"
install -d "$out"
install -m 0755 "$cli" "$out/ipsec-client-cli"
"$cli" manpage | gzip -n -9 >"$out/ipsec-client-cli.1.gz"
"$cli" completions bash >"$out/ipsec-client-cli.bash"
"$cli" completions zsh >"$out/_ipsec-client-cli"
"$cli" completions fish >"$out/ipsec-client-cli.fish"
echo "command line staged in $out"
