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

echo "==> [4/5] Running dependency security audit (cargo audit)"
if command -v cargo-audit >/dev/null 2>&1; then
    cargo audit
else
    echo "NOTE: cargo-audit binary not installed in PATH; install with 'cargo install cargo-audit' or 'pacman -S cargo-audit'."
fi

echo "==> [5/6] Running live CEF launch smoke test (slate-spike-cef --tabs 1 --exit-after-load)"
cargo build --release -p slate-spike-cef
SMOKE_OUT=$(LD_LIBRARY_PATH="$ROOT_DIR/target/release:${LD_LIBRARY_PATH:-}" \
    "$ROOT_DIR/target/release/slate-spike-cef" --tabs 1 --exit-after-load)
echo "    Smoke output: $SMOKE_OUT"
python3 -c '
import sys, json
data = json.loads(sys.argv[1])
assert data.get("engine") == "cef-156", f"unexpected engine: {data}"
assert data.get("tabs") == 1, f"unexpected tabs: {data}"
assert data.get("pss_tree_kb", 0) > 0, f"missing pss_tree_kb: {data}"
assert data.get("document_start_injected") is True, f"document_start_injected failed: {data}"
' "$SMOKE_OUT"

echo "==> [6/7] Running live Site Isolation test (slate-spike-cef --test-site-isolation --tabs 2 --real-sites)"
SITE_ISO_OUT=$(LD_LIBRARY_PATH="$ROOT_DIR/target/release:${LD_LIBRARY_PATH:-}" \
    "$ROOT_DIR/target/release/slate-spike-cef" --test-site-isolation --tabs 2 --real-sites)
echo "    Site isolation output: $SITE_ISO_OUT"
python3 -c '
import sys, json
data = json.loads(sys.argv[1])
assert data.get("strict_one_process_per_site_preserved") is True, f"site isolation failed: {data}"
assert data.get("renderer_process_count", 0) >= 2, f"fewer than 2 renderers: {data}"
pids = data.get("renderer_pids", [])
assert len(pids) >= 2 and len(set(pids)) == len(pids), f"renderer pids not unique: {pids}"
' "$SITE_ISO_OUT"

echo "==> [7/7] Verifying filters.dat cache rule count matches configured lists"
python3 -c "
import json, os
meta_path = os.path.expanduser('~/.local/share/slate/filters.meta.json')
dat_path = os.path.expanduser('~/.local/share/slate/filters.dat')
assert os.path.exists(dat_path), f'filters.dat missing at {dat_path}'
assert os.path.exists(meta_path), f'filters.meta.json missing at {meta_path}'
with open(meta_path) as f:
    d = json.load(f)
assert d.get('list_count') == 8, f'expected 8 lists, got {d.get(\"list_count\")}'
assert d.get('rule_count', 0) >= 250000, f'rule count too low: {d.get(\"rule_count\")}'
print(f'    Filter cache verified: {d.get(\"rule_count\")} active rules from {d.get(\"list_count\")} lists ({os.path.getsize(dat_path)} bytes)')
"

echo "==> All checks passed!"
