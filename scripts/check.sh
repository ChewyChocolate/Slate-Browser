#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "Stages: [1] fmt  [2] clippy  [3] cargo test  [4] cargo audit  [5] spike CEF smoke  [6] real app smoke  [7] site isolation  [8] filter cache  [9] fake-counter/fake-flag guard  [10] third-party cookie live test"
export CEF_PATH="${CEF_PATH:-$HOME/.cache/slate-cef}"

echo "==> [1/10] Checking Rust formatting (cargo fmt --all -- --check)"
cargo fmt --all -- --check

echo "==> [2/10] Running Clippy with warnings as errors (cargo clippy --workspace --all-targets -- -D warnings)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> [3/10] Running automated test suite (cargo test --workspace)"
cargo test --workspace

echo "==> [4/10] Running dependency security audit (cargo audit; FAILS if cargo-audit is missing)"
if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "ERROR: cargo-audit is not installed. Install with 'cargo install cargo-audit --locked' or 'pacman -S cargo-audit'." >&2
    echo "       The audit must never be silently skipped." >&2
    exit 1
fi
cargo audit

echo "==> [5/10] Running live CEF launch smoke test on the spike (slate-spike-cef --tabs 1 --exit-after-load)"
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

echo "==> [6/10] Running live smoke test on the REAL app binary (target/release/slate --exit-after-load)"
cargo build --release -p slate-app
set +e
APP_OUT=$(LD_LIBRARY_PATH="$ROOT_DIR/target/release:${LD_LIBRARY_PATH:-}" \
    timeout 60 "$ROOT_DIR/target/release/slate" --exit-after-load)
APP_RC=$?
set -e
echo "    Real app exit code: $APP_RC"
echo "    Real app output: $APP_OUT"
if [ "$APP_RC" -ne 0 ]; then
    echo "ERROR: target/release/slate did not start and exit cleanly (exit code $APP_RC; 124 means timeout)." >&2
    exit 1
fi
python3 -c '
import sys, json
data = json.loads(sys.argv[1])
assert data.get("engine") == "cef-156", f"unexpected engine: {data}"
assert data.get("pss_tree_kb", 0) > 0, f"missing pss_tree_kb: {data}"
assert data.get("process_count", 0) >= 2, f"too few processes: {data}"
' "$APP_OUT"

echo "==> [7/10] Running live Site Isolation test (slate-spike-cef --test-site-isolation --tabs 2 --real-sites)"
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

echo "==> [8/10] Verifying filters.dat cache rule count matches configured lists"
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

echo "==> [9/10] Guarding against hardcoded/simulated tracker data and fake flags"
if grep -nE "blocked *= *[0-9]+|theverge\.com|Simulating" crates/app/resources/ui/shell.js; then
    echo "ERROR: shell.js contains hardcoded or simulated blocked-tracker data. Counters must come from the real pipeline." >&2
    exit 1
fi
echo "    shell.js contains no hardcoded counters"
if grep -nE "third_party_cookies_blocked: *true" crates/engine-adapter/src/cef_backend.rs; then
    echo "ERROR: third_party_cookies_blocked is hardcoded to true (it must be read from real filter state)." >&2
    exit 1
fi
echo "    no hardcoded third_party_cookies_blocked literal"

echo "==> [10/10] Live third-party cookie test (top-level site resolved from the tab; scripts/test_third_party_cookies.py)"
python3 scripts/test_third_party_cookies.py

echo "==> All checks passed!"
