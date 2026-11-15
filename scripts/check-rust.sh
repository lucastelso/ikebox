#!/usr/bin/env bash
# check-rust.sh — verificações do workspace Rust (as mesmas da CI).
#   fmt → clippy (-D warnings) → testes → cargo-deny (licenças/vulnerabilidades)
#
# Uso: scripts/check-rust.sh [--fmt]   (--fmt: só a formatação, para o pre-commit)
# Carrega ~/.cargo/env quando o cargo não está no PATH (hooks de git, IDEs).
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v cargo >/dev/null 2>&1 && [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
fi
command -v cargo >/dev/null 2>&1 || {
    printf 'check-rust: cargo não encontrado (veja README, "Pré-requisitos").\n' >&2
    exit 1
}

cargo fmt --all --check
[[ ${1:-} == --fmt ]] && exit 0

cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny --log-level error check
else
    printf 'check-rust: cargo-deny ausente; instale com: cargo install --locked cargo-deny\n' >&2
    exit 1
fi
