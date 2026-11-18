#!/usr/bin/env bash
# Prepares the command line for the .rpm/.deb packages (release builds,
# src-tauri/tauri.release.conf.json): the ikebox-cli binary, its
# manual page and the shell completions, in target/cli-assets/. The
# AppImage does not carry the CLI (it cannot put it on the PATH).
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
out="$repo/target/cli-assets"

cd "$repo"
cargo build --release --locked -p ikebox-cli
cli="$repo/target/release/ikebox-cli"

rm -rf "$out"
install -d "$out"
install -m 0755 "$cli" "$out/ikebox-cli"
"$cli" manpage | gzip -n -9 >"$out/ikebox-cli.1.gz"
"$cli" completions bash >"$out/ikebox-cli.bash"
"$cli" completions zsh >"$out/_ikebox-cli"
"$cli" completions fish >"$out/ikebox-cli.fish"
echo "command line staged in $out"
