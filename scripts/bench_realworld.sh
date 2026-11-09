#!/usr/bin/env bash
# Reproducible Real-World 10-Site Benchmark Suite for Slate vs. Brave
# Sites tested:
#   1. https://www.youtube.com
#   2. https://docs.google.com
#   3. https://news.ycombinator.com
#   4. https://en.wikipedia.org/wiki/Rust_(programming_language)
#   5. https://www.reddit.com/r/linux/
#   6. https://www.bbc.com/news
#   7. https://mail.proton.me
#   8. https://github.com/rust-lang/rust
#   9. https://www.chase.com
#  10. https://www.nytimes.com

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export CEF_PATH="${CEF_PATH:-$HOME/.cache/slate-cef}"
export LD_LIBRARY_PATH="$ROOT_DIR/target/release:${LD_LIBRARY_PATH:-}"
RESULTS_DIR="$ROOT_DIR/benchmarks/results/realworld"
mkdir -p "$RESULTS_DIR"

rm -f "$ROOT_DIR/target/release/WidevineCdm"

echo "==> Building release binary for Slate (CEF 156)..."
cargo build --release -p slate-spike-cef
if [ -f "$ROOT_DIR/target/release/libcef.so" ]; then
    strip --strip-unneeded "$ROOT_DIR/target/release/libcef.so" 2>/dev/null || true
fi

CEF_BIN="$ROOT_DIR/target/release/slate-spike-cef"

REAL_SITES=(
    "https://www.youtube.com"
    "https://docs.google.com"
    "https://news.ycombinator.com"
    "https://en.wikipedia.org/wiki/Rust_(programming_language)"
    "https://www.reddit.com/r/linux/"
    "https://www.bbc.com/news"
    "https://mail.proton.me"
    "https://github.com/rust-lang/rust"
    "https://www.chase.com"
    "https://www.nytimes.com"
)

echo "==> [1/6] Running Slate (CEF 156, Opt Level 3) — 1 Real Site (YouTube)..."
"$CEF_BIN" --real-sites --tabs 1 --hold-ms 6000 --opt-level 3 | tee "$RESULTS_DIR/slate_real_1tab.json"

echo "==> [2/6] Running Slate (CEF 156, Opt Level 3) — 5 Real Sites..."
"$CEF_BIN" --real-sites --tabs 5 --hold-ms 8000 --opt-level 3 | tee "$RESULTS_DIR/slate_real_5tabs.json"

echo "==> [3/6] Running Slate (CEF 156, Opt Level 3) — 10 Real Sites + Tab Sleeping..."
"$CEF_BIN" --real-sites --tabs 10 --hold-ms 10000 --sleep-background-tabs --opt-level 3 | tee "$RESULTS_DIR/slate_real_10tabs_and_sleep.json"

measure_brave_real() {
    local num_tabs=$1
    local hold_sec=$2
    local out_file=$3
    local profile_dir
    profile_dir=$(mktemp -d /tmp/slate-brave-real-XXXXXX)

    local urls=()
    for ((i=0; i<num_tabs; i++)); do
        urls+=("${REAL_SITES[$i]}")
    done

    /usr/bin/brave \
        --user-data-dir="$profile_dir" \
        --no-first-run \
        --no-default-browser-check \
        --disable-sync \
        "${urls[@]}" >/dev/null 2>&1 &
    local brave_pid=$!

    sleep "$hold_sec"

    # Walk full /proc descendant tree from brave_pid (plus any进程 matching profile_dir)
    # using the exact same RSS + PSS summation as Rust's measure_process_tree_memory.
    python3 - "$brave_pid" "$profile_dir" "$num_tabs" > "$out_file" <<'PY'
import os, sys, json

root_pid = int(sys.argv[1])
profile_dir = sys.argv[2]
num_tabs = int(sys.argv[3])

children = {}
seed_pids = {root_pid}

for entry in os.listdir('/proc'):
    if not entry.isdigit():
        continue
    pid = int(entry)
    try:
        with open(f'/proc/{pid}/status', 'r', errors='ignore') as f:
            for line in f:
                if line.startswith('PPid:'):
                    ppid = int(line.split()[1])
                    children.setdefault(ppid, []).append(pid)
                    break
        with open(f'/proc/{pid}/cmdline', 'rb') as f:
            cmd = f.read().decode('utf-8', errors='ignore')
            if profile_dir in cmd:
                seed_pids.add(pid)
    except Exception:
        pass

visited = set()
queue = list(seed_pids)
while queue:
    p = queue.pop()
    if p in visited:
        continue
    visited.add(p)
    queue.extend(children.get(p, []))

rss_kb = 0
pss_kb = 0
proc_count = 0
for pid in visited:
    try:
        with open(f'/proc/{pid}/status', 'r', errors='ignore') as f:
            for line in f:
                if line.startswith('VmRSS:'):
                    rss_kb += int(line.split()[1])
                    proc_count += 1
                    break
    except Exception:
        pass
    try:
        with open(f'/proc/{pid}/smaps_rollup', 'r', errors='ignore') as f:
            for line in f:
                if line.startswith('Pss:'):
                    pss_kb += int(line.split()[1])
                    break
    except Exception:
        pass

print(json.dumps({
    "engine": "brave-1.99.55",
    "tabs": num_tabs,
    "real_sites": True,
    "rss_tree_kb": rss_kb,
    "pss_tree_kb": pss_kb,
    "process_count": proc_count
}))
PY

    local pids
    pids=$(pgrep -f "$profile_dir" || true)
    kill -TERM "$brave_pid" $pids 2>/dev/null || true
    sleep 0.5
    kill -KILL "$brave_pid" $pids 2>/dev/null || true
    wait "$brave_pid" 2>/dev/null || true
    rm -rf "$profile_dir"

    cat "$out_file"
}

if [ -x /usr/bin/brave ]; then
    echo "==> [4/6] Running Brave 1.99.55 — 1 Real Site (YouTube)..."
    measure_brave_real 1 6 "$RESULTS_DIR/brave_real_1tab.json"
    echo "==> [5/6] Running Brave 1.99.55 — 5 Real Sites..."
    measure_brave_real 5 8 "$RESULTS_DIR/brave_real_5tabs.json"
    echo "==> [6/6] Running Brave 1.99.55 — 10 Real Sites..."
    measure_brave_real 10 10 "$RESULTS_DIR/brave_real_10tabs.json"
fi

echo "==> Real-world benchmark results saved in $RESULTS_DIR"
