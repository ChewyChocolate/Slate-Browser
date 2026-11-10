#!/usr/bin/env python3
"""
7-Run Benchmark Suite (Slate vs. Seeded Brave)
- 7 runs per scenario, run 1 discarded as warmup.
- Evaluates:
  1_tab (YouTube)
  5_tabs
  10_tabs
  10_tabs_sleep (exact same 9 background tabs discarded in both)
- Symmetrical Brave profile seeded with adblock_cache
- Calculates Median, Q1, Q3, IQR, and Min-Max range for PSS (primary) and RSS (secondary).
"""

import sys, os, time, json, subprocess, tempfile, shutil, statistics, urllib.request

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
RESULTS_DIR = os.path.join(ROOT_DIR, 'benchmarks/results/realworld')
BRAVE_ADBLOCK_CACHE = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'

SITES = [
    "https://www.youtube.com/watch?v=jNQXAC9IVRw",
    "https://docs.google.com/document/d/1hXalsO6oixyfHXQIo9C6iozYBHcXmDL_/preview",
    "https://news.ycombinator.com",
    "https://en.wikipedia.org/wiki/Rust_(programming_language)",
    "https://slashdot.org",
    "https://www.bbc.com/news",
    "https://www.openstreetmap.org",
    "https://github.com/rust-lang/rust",
    "https://www.chase.com",
    "https://www.theverge.com"
]

def sample_process_tree(root_pid, profile_filter=None):
    children = {}
    seed_pids = {root_pid}
    for entry in os.listdir('/proc'):
        if not entry.isdigit(): continue
        pid = int(entry)
        try:
            with open(f'/proc/{pid}/status', 'r', errors='ignore') as f:
                for line in f:
                    if line.startswith('PPid:'):
                        children.setdefault(int(line.split()[1]), []).append(pid)
                        break
            if profile_filter:
                with open(f'/proc/{pid}/cmdline', 'rb') as f:
                    if profile_filter.encode() in f.read():
                        seed_pids.add(pid)
        except Exception:
            pass

    visited = set()
    queue = list(seed_pids)
    while queue:
        cur = queue.pop()
        if cur in visited: continue
        visited.add(cur)
        queue.extend(children.get(cur, []))

    rss_kb = 0
    pss_kb = 0
    pcount = 0
    for pid in visited:
        p_rss = 0
        p_pss = 0
        try:
            with open(f'/proc/{pid}/status', 'r', errors='ignore') as f:
                for line in f:
                    if line.startswith('VmRSS:'):
                        p_rss = int(line.split()[1])
                        break
        except Exception: pass
        try:
            with open(f'/proc/{pid}/smaps_rollup', 'r', errors='ignore') as f:
                for line in f:
                    if line.startswith('Pss:'):
                        p_pss = int(line.split()[1])
                        break
        except Exception: pass

        if p_rss > 0:
            rss_kb += p_rss
            pss_kb += p_pss
            pcount += 1
    return pss_kb, rss_kb, pcount

def run_slate(tabs, sleep_tabs, hold_ms=10000):
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    cmd = [
        CEF_BIN,
        '--real-sites',
        '--tabs', str(tabs),
        '--hold-ms', str(hold_ms),
        '--allow-autoplay',
        '--opt-level', '3'
    ]
    if sleep_tabs:
        cmd.append('--sleep-background-tabs')

    p = subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    out, _ = p.communicate()
    for line in out.splitlines():
        if line.startswith('{') and 'engine' in line:
            try:
                d = json.loads(line)
                pss = (d.get('pss_after_suspend_kb') or d['pss_tree_kb']) / 1024
                rss = (d.get('rss_after_suspend_kb') or d['rss_tree_kb']) / 1024
                procs = d.get('process_count_after_suspend') or d['process_count']
                return {
                    'pss_mb': round(pss, 1),
                    'rss_mb': round(rss, 1),
                    'process_count': procs,
                    'blocked_requests': d.get('blocked_requests', 0),
                    'evaluated_requests': d.get('evaluated_requests', 0)
                }
            except Exception: pass
    raise RuntimeError(f"Slate failed to produce JSON output: {out}")

def run_brave(tabs, sleep_tabs, hold_sec=10):
    prof = tempfile.mkdtemp(prefix='brave-7run-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    if os.path.exists(BRAVE_ADBLOCK_CACHE):
        shutil.copytree(BRAVE_ADBLOCK_CACHE, os.path.join(prof, 'Default/adblock_cache'))

    urls = SITES[:tabs]
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--remote-debugging-port=9222',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--autoplay-policy=no-user-gesture-required'
    ] + urls

    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(hold_sec)

    if sleep_tabs and tabs > 1:
        # Symmetrically discard the exact same 9 background tabs (leaving Tab 0 / YouTube active)
        try:
            tab_data = json.loads(urllib.request.urlopen('http://127.0.0.1:9222/json', timeout=3).read().decode())
            page_tabs = [t for t in tab_data if t.get('type') == 'page']
            for bg_tab in page_tabs[1:]:
                tid = bg_tab['id']
                urllib.request.urlopen(f'http://127.0.0.1:9222/json/close/{tid}', timeout=2)
            time.sleep(1.5)
        except Exception:
            pass

    pss_kb, rss_kb, pcount = sample_process_tree(p.pid, prof)

    # Read Brave's blocked count from Preferences
    pref_blocked = None
    pref_file = os.path.join(prof, 'Default/Preferences')
    if os.path.exists(pref_file):
        try:
            with open(pref_file) as f:
                pd = json.load(f)
            pref_blocked = int(pd.get('brave', {}).get('stats', {}).get('ads_blocked', 0))
        except Exception: pass

    p.terminate()
    try: p.wait(timeout=3)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    subprocess.run(['rm', '-rf', prof], check=False)

    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount,
        'blocked_requests': pref_blocked,
        'evaluated_requests': None
    }

def compute_iqr_stats(runs):
    # runs is a list of 6 values (run 1 discarded)
    sorted_runs = sorted(runs)
    n = len(sorted_runs)
    median = statistics.median(sorted_runs)
    # 25th percentile (Q1) and 75th percentile (Q3)
    q1 = statistics.median(sorted_runs[:n//2])
    q3 = statistics.median(sorted_runs[(n+1)//2:]) if n % 2 != 0 else statistics.median(sorted_runs[n//2:])
    iqr = q3 - q1
    return {
        'median': round(median, 1),
        'q1': round(q1, 1),
        'q3': round(q3, 1),
        'iqr': round(iqr, 1),
        'min': round(min(sorted_runs), 1),
        'max': round(max(sorted_runs), 1)
    }

def main():
    configs = [
        ("1_tab", 1, False, 8000),
        ("5_tabs", 5, False, 10000),
        ("10_tabs", 10, False, 12000),
        ("10_tabs_sleep", 10, True, 12000)
    ]

    all_data = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scenarios": {}
    }

    print("================================================================================")
    print(" 7-RUN FAIR BENCHMARK SUITE: SLATE vs BRAVE (SEEDED)")
    print(" (Run 1 discarded as warmup, Runs 2-7 analyzed)")
    print("================================================================================")

    for name, tabs, sleep_tabs, hold_ms in configs:
        print(f"\n--- Testing Scenario: {name} ({tabs} tabs, sleep={sleep_tabs}) ---")
        
        # Slate 7 runs
        slate_all = []
        for r in range(1, 8):
            print(f"  [Slate] Run {r}/7...", end="", flush=True)
            res = run_slate(tabs, sleep_tabs, hold_ms)
            slate_all.append(res)
            print(f" done (PSS: {res['pss_mb']} MB, Procs: {res['process_count']})")
            time.sleep(1)

        # Brave 7 runs
        brave_all = []
        for r in range(1, 8):
            print(f"  [Brave] Run {r}/7...", end="", flush=True)
            res = run_brave(tabs, sleep_tabs, hold_ms // 1000)
            brave_all.append(res)
            print(f" done (PSS: {res['pss_mb']} MB, Procs: {res['process_count']})")
            time.sleep(1)

        # Discard run 0 (warmup)
        slate_analyzed = slate_all[1:]
        brave_analyzed = brave_all[1:]

        s_pss_stats = compute_iqr_stats([x['pss_mb'] for x in slate_analyzed])
        b_pss_stats = compute_iqr_stats([x['pss_mb'] for x in brave_analyzed])

        s_rss_stats = compute_iqr_stats([x['rss_mb'] for x in slate_analyzed])
        b_rss_stats = compute_iqr_stats([x['rss_mb'] for x in brave_analyzed])

        s_procs = int(statistics.median([x['process_count'] for x in slate_analyzed]))
        b_procs = int(statistics.median([x['process_count'] for x in brave_analyzed]))

        s_blocked = int(statistics.median([x['blocked_requests'] for x in slate_analyzed]))
        b_blocked_vals = [x['blocked_requests'] for x in brave_analyzed if x['blocked_requests'] is not None]
        b_blocked = int(statistics.median(b_blocked_vals)) if b_blocked_vals else 0

        # Check range overlap: do [min, max] ranges overlap?
        pss_overlap = not (s_pss_stats['max'] < b_pss_stats['min'] or b_pss_stats['max'] < s_pss_stats['min'])

        all_data["scenarios"][name] = {
            "slate": {
                "pss": s_pss_stats,
                "rss": s_rss_stats,
                "process_count_median": s_procs,
                "blocked_median": s_blocked,
                "raw_runs": slate_all
            },
            "brave": {
                "pss": b_pss_stats,
                "rss": b_rss_stats,
                "process_count_median": b_procs,
                "blocked_median": b_blocked,
                "raw_runs": brave_all
            },
            "pss_ranges_overlap": pss_overlap
        }

    out_file = os.path.join(RESULTS_DIR, "7run_iqr_summary.json")
    with open(out_file, "w") as f:
        json.dump(all_data, f, indent=2)

    print("\n================================================================================")
    print(f" Summary saved to: {out_file}")
    print("================================================================================")

if __name__ == '__main__':
    main()
