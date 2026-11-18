#!/usr/bin/env bash
# Test of the real embedded engine (no Docker in the VPN's path), for the
# maintainer to run on their own computer:
#
#   1. builds the network component bundle, compiling the embedded strongSwan
#      and the helper (as the user);
#   2. installs the bundle, as the app does (ASKS FOR A PASSWORD: sudo);
#   3. runs the CLI's complete E2E forcing the embedded engine
#      (IPSEC_CLIENT_BACKEND=native), against the local test gateway.
#
# Step 3 also tests the real polkit: run it in a terminal of your graphical
# session (active local session). To uninstall afterwards:
#   sudo /opt/ipsec-client/libexec/install-native.sh uninstall

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"
# rustup's cargo (it honors rust-toolchain.toml), even with another one in
# PATH.
if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
fi

bundle="$repo/target/network-component"

echo "==> 1/3 building the network component bundle"
scripts/stage-network-component.sh --out "$bundle"

echo "==> 2/3 installing the network component (sudo)"
sudo "$bundle/install-native.sh" install --bundle "$bundle" --origin manual

echo "==> 3/3 CLI E2E with the embedded engine"
IPSEC_CLIENT_BACKEND=native tests/cli/e2e.sh
echo
echo "✔ embedded engine validated. It stays installed and becomes the app default."
echo "  Para voltar ao Docker: sudo /opt/ipsec-client/libexec/install-native.sh uninstall"
