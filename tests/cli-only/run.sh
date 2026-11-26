#!/usr/bin/env bash
# The CLI-only build (ADR-0033): ikebox-cli and ikebox-helper on a clean
# Debian 13 with only Rust, a C compiler, pkg-config and the D-Bus headers.
# No Node, no WebKitGTK, no GTK, no Docker inside: if either crate ever
# needed them, this build would fail. Their tests run there too, and their
# dependency tree must not contain the app's (Tauri, wry, WebKitGTK, GTK).
#
#   tests/cli-only/run.sh
#
# rustup-init is the release image's pinned one (packaging/release/Dockerfile),
# checked against the same SHA-256. The build and the tests run as an
# unprivileged user, as distribution build systems do. The engine is not
# built here: it has its own test without Docker (tests/engine-host/run.sh).

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
readonly BASE="debian@sha256:9c6e381421d0f48d1b8133e573f33c185ee16cbf505eda9c4a8a4791b25ea8a6" # debian:13
dockerfile="$repo/packaging/release/Dockerfile"
rust=$(awk -F'"' '/^channel/ {print $2}' "$repo/rust-toolchain.toml")
rustup_version=$(sed -n 's/^ARG RUSTUP_VERSION=//p' "$dockerfile")
rustup_sha256=$(sed -n 's/^ARG RUSTUP_SHA256=//p' "$dockerfile")
[[ -n $rust && -n $rustup_version && $rustup_sha256 =~ ^[0-9a-f]{64}$ ]] ||
    { echo "could not read the Rust and rustup pins" >&2; exit 1; }

docker run --rm -i --security-opt label=disable \
    -v "$repo:/src:ro" -v ikebox-cli-only-target:/target -v ikebox-cli-only-cargo:/opt/cargo/registry \
    -e CARGO_TARGET_DIR=/target -e CARGO_HOME=/opt/cargo -e RUSTUP_HOME=/opt/rustup -e "RUST=$rust" -e "RUSTUP_VERSION=$rustup_version" \
    -e "RUSTUP_SHA256=$rustup_sha256" -w /src "$BASE" bash -euo pipefail -s <<'INSIDE'
step() { printf '\n==> %s\n' "$*"; }
step "Debian 13 with only Rust's needs: curl, a C compiler, pkg-config, D-Bus headers"
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
    ca-certificates curl gcc libc6-dev pkg-config libdbus-1-dev >/dev/null
base="https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu"
curl --proto '=https' --tlsv1.2 -fsSLo /tmp/rustup-init "$base/rustup-init"
echo "$RUSTUP_SHA256  /tmp/rustup-init" | sha256sum -c --quiet -
chmod +x /tmp/rustup-init
/tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST" >/dev/null 2>&1
export PATH=/opt/cargo/bin:$PATH
useradd -m builder
chown -R builder /target /opt/cargo /opt/rustup
as_builder() { runuser -u builder -- env PATH="$PATH" CARGO_HOME=/opt/cargo RUSTUP_HOME=/opt/rustup CARGO_TARGET_DIR=/target "$@"; }

step "Nothing of the desktop app is installed"
for lib in libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1; do
    if dpkg-query -W -f '${Status}' "$lib" 2>/dev/null | grep -q installed; then
        echo "unexpected: $lib is installed" >&2; exit 1
    fi
done
! command -v node >/dev/null || { echo "unexpected: node is installed" >&2; exit 1; }
! command -v docker >/dev/null || { echo "unexpected: docker is installed" >&2; exit 1; }
echo "no WebKitGTK, GTK, AppIndicator, Node or Docker"

step "The dependency tree has nothing of the app"
tree=$(as_builder cargo tree --locked -e normal -p ikebox-cli -p ikebox-helper --prefix none)
if grep -E '^(tauri|wry|tao|webkit2gtk|gtk|gdk) ' <<<"$tree"; then
    echo "the CLI or the helper depends on the desktop app's libraries" >&2; exit 1
fi
echo "$(sort -u <<<"$tree" | wc -l) crates, none of them Tauri, wry, WebKitGTK or GTK"

step "Build (release, as packaged) and tests, as an unprivileged user"
as_builder cargo build -q --release --locked -p ikebox-cli -p ikebox-helper
as_builder cargo test -q --locked -p ikebox-cli -p ikebox-helper -p ikebox-core -p ikebox-proto 2>&1 | grep -E '^test result' | sort | uniq -c
/target/release/ikebox-cli --version
/target/release/ikebox-helper --version
INSIDE
