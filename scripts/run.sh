#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CEF_PATH="${HOME}/.cache/slate-cef"
export LD_LIBRARY_PATH="${ROOT_DIR}/target/release:${CEF_PATH}:${LD_LIBRARY_PATH:-}"

# Check for release binary
if [ ! -x "${ROOT_DIR}/target/release/slate" ]; then
    echo "Building release binary..."
    cargo build --release -p slate-app
fi

exec "${ROOT_DIR}/target/release/slate" "$@"
