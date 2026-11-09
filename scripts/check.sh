#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export CEF_PATH="${CEF_PATH:-$HOME/.cache/slate-cef}"

echo "==> [1/4] Checking Rust formatting (cargo fmt --all -- --check)"
cargo fmt --all -- --check

echo "==> [2/4] Running Clippy with warnings as errors (cargo clippy --workspace --all-targets -- -D warnings)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> [3/4] Running automated test suite (cargo test --workspace)"
cargo test --workspace

echo "==> [4/4] Running dependency security audit (cargo audit)"
if command -v cargo-audit >/dev/null 2>&1; then
    cargo audit
else
    echo "NOTE: cargo-audit binary not installed in PATH; install with 'cargo install cargo-audit' or 'pacman -S cargo-audit'."
fi

echo "==> All checks passed!"
