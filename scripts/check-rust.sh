#!/usr/bin/env bash
# check-rust.sh — checks of the Rust workspace (the same as CI).
#   fmt → clippy (-D warnings) → tests → cargo-deny (licenses/vulnerabilities)
#
# Usage: scripts/check-rust.sh [--fmt | --docker-tests]
#   --fmt           formatting only (pre-commit)
#   --docker-tests  integration tests with the real Docker (they change the
#                   host's network temporarily; see crates/core/tests/)
# Loads ~/.cargo/env (git hooks, IDEs): rustup's cargo honors
# rust-toolchain.toml, even with another (the system's) cargo in PATH.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
fi
command -v cargo >/dev/null 2>&1 || {
    printf 'check-rust: cargo not found (see README, "Prerequisites").\n' >&2
    exit 1
}

if [[ ${1:-} == --docker-tests ]]; then
    exec cargo test -p ipsec-client-core --features docker-tests --locked --test '*' -- --test-threads=1 --nocapture
fi

cargo fmt --all --check
[[ ${1:-} == --fmt ]] && exit 0

# Both builds: the release one (no Docker engine, the default) and the
# development one (feature docker). Each must compile without warnings.
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked --features ipsec-client-core/docker,ipsec-client-cli/docker,ipsec-client-app/docker

if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny --log-level error check
else
    printf 'check-rust: cargo-deny missing; install it with: cargo install --locked cargo-deny\n' >&2
    exit 1
fi
