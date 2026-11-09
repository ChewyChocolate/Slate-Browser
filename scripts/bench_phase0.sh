#!/usr/bin/env bash
# Reproducible Phase 0 Benchmark & Compatibility Suite for Slate
# Compares:
#   - Spike A: Rust shell + CEF (slate-spike-cef)
#   - Spike B: Rust shell + WebKitGTK (slate-spike-webkit)
#   - Baseline: Brave (/usr/bin/brave) on the same Arch Linux machine

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export CEF_PATH="${CEF_PATH:-$HOME/.cache/slate-cef}"
export LD_LIBRARY_PATH="$ROOT_DIR/target/release:${LD_LIBRARY_PATH:-}"
RESULTS_DIR="$ROOT_DIR/benchmarks/results"
mkdir -p "$RESULTS_DIR"

echo "==> Building release binaries for Spike A (CEF) and Spike B (WebKitGTK)..."
cargo build --release -p slate-spike-webkit -p slate-spike-cef

WEBKIT_BIN="$ROOT_DIR/target/release/slate-spike-webkit"
CEF_BIN="$ROOT_DIR/target/release/slate-spike-cef"

# 1. Measure Install Sizes on Disk
WEBKIT_BIN_BYTES=$(stat -c%s "$WEBKIT_BIN")
WEBKIT_SYS_SO_BYTES=$(stat -L -c%s /usr/lib/libwebkit2gtk-4.1.so 2>/dev/null || echo 0)
WEBKIT_JSC_SO_BYTES=$(stat -L -c%s /usr/lib/libjavascriptcoregtk-4.1.so 2>/dev/null || echo 0)
WEBKIT_TOTAL_SHARED_BYTES=$((WEBKIT_BIN_BYTES + WEBKIT_SYS_SO_BYTES + WEBKIT_JSC_SO_BYTES))

CEF_BIN_BYTES=$(stat -c%s "$CEF_BIN")
CEF_RUNTIME_BYTES=$(du -sb "$CEF_PATH" 2>/dev/null | awk '{print $1}' || echo 0)
CEF_STRIPPED_BUNDLE_BYTES=0
if [ -f "$ROOT_DIR/target/release/libcef.so" ]; then
    CEF_ASSETS_BYTES=$(( \
        $(stat -c%s "$CEF_BIN") + \
        $(stat -c%s "$ROOT_DIR/target/release/libEGL.so" 2>/dev/null || echo 0) + \
        $(stat -c%s "$ROOT_DIR/target/release/libGLESv2.so" 2>/dev/null || echo 0) + \
        $(stat -c%s "$ROOT_DIR/target/release/icudtl.dat" 2>/dev/null || echo 0) + \
        $(stat -c%s "$ROOT_DIR/target/release/resources.pak" 2>/dev/null || echo 0) + \
        $(stat -c%s "$ROOT_DIR/target/release/chrome_100_percent.pak" 2>/dev/null || echo 0) + \
        $(stat -c%s "$ROOT_DIR/target/release/chrome_200_percent.pak" 2>/dev/null || echo 0) + \
        $(stat -c%s "$ROOT_DIR/target/release/v8_context_snapshot.bin" 2>/dev/null || echo 0) \
    ))
    CEF_BUNDLE_BYTES=$(( CEF_ASSETS_BYTES + $(stat -c%s "$ROOT_DIR/target/release/libcef.so" 2>/dev/null || echo 0) ))
    TMP_STRIPPED=$(mktemp /tmp/slate-libcef-stripped-XXXXXX.so)
    if strip -s -o "$TMP_STRIPPED" "$ROOT_DIR/target/release/libcef.so" 2>/dev/null; then
        CEF_STRIPPED_BUNDLE_BYTES=$(( CEF_ASSETS_BYTES + $(stat -c%s "$TMP_STRIPPED") ))
    fi
    rm -f "$TMP_STRIPPED"
else
    CEF_BUNDLE_BYTES=$CEF_RUNTIME_BYTES
fi

BRAVE_DIR_BYTES=$(du -sb /opt/brave-bin 2>/dev/null | awk '{print $1}' || echo 0)

echo "==> Running Spike B (WebKitGTK) benchmarks..."
"$WEBKIT_BIN" --tabs 1 --exit-after-load > "$RESULTS_DIR/webkit_cold_1tab.json"
"$WEBKIT_BIN" --tabs 1 --hold-ms 2500 > "$RESULTS_DIR/webkit_idle_1tab.json"
"$WEBKIT_BIN" --tabs 10 --hold-ms 3000 > "$RESULTS_DIR/webkit_idle_10tabs.json"
"$WEBKIT_BIN" --tabs 2 --hold-ms 2500 --verify-capabilities > "$RESULTS_DIR/webkit_capabilities.json"

echo "==> Running Spike A (CEF) benchmarks..."
"$CEF_BIN" --tabs 1 --exit-after-load > "$RESULTS_DIR/cef_cold_1tab.json"
"$CEF_BIN" --tabs 1 --hold-ms 2500 > "$RESULTS_DIR/cef_idle_1tab.json"
"$CEF_BIN" --tabs 10 --hold-ms 3000 > "$RESULTS_DIR/cef_idle_10tabs.json"
"$CEF_BIN" --tabs 2 --hold-ms 2500 --verify-capabilities > "$RESULTS_DIR/cef_capabilities.json"

echo "==> Running Brave baseline benchmarks..."
measure_brave() {
    local num_tabs=$1
    local out_file=$2
    local profile_dir
    profile_dir=$(mktemp -d /tmp/slate-brave-bench-XXXXXX)

    local bench_html_dir
    bench_html_dir=$(mktemp -d /tmp/slate-brave-html-XXXXXX)
    cat > "$bench_html_dir/index.html" <<'HTML'
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Slate Phase 0 Benchmark Tab</title></head>
<body>
  <h1 id="headline">Slate Phase 0 Benchmark Page</h1>
  <div class="ad-banner" id="test-ad-banner">Sponsored Ad Banner</div>
  <div id="content">Normal page content</div>
</body>
</html>
HTML
    python3 -m http.server 18923 --bind 0.0.0.0 --directory "$bench_html_dir" >/dev/null 2>&1 &
    local srv_pid=$!
    sleep 0.2

    local urls=()
    for ((i=0; i<num_tabs; i++)); do
        local octet=$((i + 1))
        urls+=("http://127.0.0.$octet:18923/?tab=$i")
    done

    local start_ns
    start_ns=$(date +%s%N)
    /usr/bin/brave \
        --user-data-dir="$profile_dir" \
        --no-first-run \
        --no-default-browser-check \
        --disable-sync \
        "${urls[@]}" >/dev/null 2>&1 &
    local brave_pid=$!

    # Wait until renderer process spawns for cold start estimate
    local cold_ms=0
    for _ in $(seq 1 100); do
        if pgrep -P "$brave_pid" >/dev/null 2>&1; then
            local now_ns
            now_ns=$(date +%s%N)
            cold_ms=$(( (now_ns - start_ns) / 1000000 ))
            break
        fi
        sleep 0.02
    done

    sleep 3.0

    # Sum RSS and PSS across all processes using this profile_dir
    local pids
    pids=$(pgrep -f "$profile_dir" || true)
    local rss_kb=0
    local pss_kb=0
    local proc_count=0
    for pid in $pids; do
        if [ -f "/proc/$pid/status" ]; then
            local r
            r=$(awk '/VmRSS:/ {print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)
            rss_kb=$((rss_kb + ${r:-0}))
            proc_count=$((proc_count + 1))
        fi
        if [ -f "/proc/$pid/smaps_rollup" ]; then
            local p
            p=$(awk '/^Pss:/ {print $2}' "/proc/$pid/smaps_rollup" 2>/dev/null || echo 0)
            pss_kb=$((pss_kb + ${p:-0}))
        fi
    done

    kill -TERM $pids "$srv_pid" 2>/dev/null || true
    wait "$brave_pid" 2>/dev/null || true
    rm -rf "$profile_dir" "$bench_html_dir"

    cat > "$out_file" <<EOF
{"engine":"brave-1.99.55","tabs":$num_tabs,"cold_start_ms":$cold_ms,"rss_tree_kb":$rss_kb,"pss_tree_kb":$pss_kb,"process_count":$proc_count}
EOF
}

if [ -x /usr/bin/brave ]; then
    measure_brave 1 "$RESULTS_DIR/brave_idle_1tab.json"
    measure_brave 10 "$RESULTS_DIR/brave_idle_10tabs.json"
fi

cat > "$RESULTS_DIR/disk_sizes.json" <<EOF
{
  "webkit_binary_bytes": $WEBKIT_BIN_BYTES,
  "webkit_with_system_libs_bytes": $WEBKIT_TOTAL_SHARED_BYTES,
  "cef_binary_bytes": $CEF_BIN_BYTES,
  "cef_unstripped_sdk_bundle_bytes": $CEF_BUNDLE_BYTES,
  "cef_stripped_bundle_bytes": $CEF_STRIPPED_BUNDLE_BYTES,
  "brave_install_bytes": $BRAVE_DIR_BYTES
}
EOF

echo "==> Phase 0 Benchmark Results saved to $RESULTS_DIR:"
cat "$RESULTS_DIR/disk_sizes.json"
echo "--- WebKitGTK (cold / 1-tab / 10-tabs / capabilities) ---"
cat "$RESULTS_DIR/webkit_cold_1tab.json"
cat "$RESULTS_DIR/webkit_idle_1tab.json"
cat "$RESULTS_DIR/webkit_idle_10tabs.json"
cat "$RESULTS_DIR/webkit_capabilities.json"
echo "--- CEF (cold / 1-tab / 10-tabs / capabilities) ---"
cat "$RESULTS_DIR/cef_cold_1tab.json"
cat "$RESULTS_DIR/cef_idle_1tab.json"
cat "$RESULTS_DIR/cef_idle_10tabs.json"
cat "$RESULTS_DIR/cef_capabilities.json"
echo "--- Brave Baseline (1-tab / 10-tabs) ---"
cat "$RESULTS_DIR/brave_idle_1tab.json" 2>/dev/null || true
cat "$RESULTS_DIR/brave_idle_10tabs.json" 2>/dev/null || true
