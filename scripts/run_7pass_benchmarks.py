#!/usr/bin/env python3
"""
7-Run Fair Benchmark Suite (Slate vs. Seeded Brave)
- 7 runs per scenario.
- Writes raw run data directly to `benchmarks/results/realworld/benchmark_7runs_raw.json` AS IT GOES.
- Scenarios:
  1_tab (YouTube)
  5_tabs (YouTube, Docs, HN, Wikipedia, Slashdot)
  10_tabs (All 10 sites)
  10_tabs_sleep (10 sites loaded, 9 inactive tabs closed in both browsers)
"""

import sys, os, time, json, subprocess, tempfile, shutil, urllib.request

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
RESULTS_DIR = os.path.join(ROOT_DIR, 'benchmarks/results/realworld')
RAW_OUT_FILE = os.path.join(RESULTS_DIR, 'benchmark_7runs_raw.json')
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
            with open(f'/proc/{pid}/smaps_rollup', 'r') as f:
                for line in f:
                    if line.startswith('Rss:'):
                        p_rss = int(line.split()[1])
                    elif line.startswith('Pss:'):
                        p_pss = int(line.split()[1])
        except Exception:
            try:
                with open(f'/proc/{pid}/status', 'r') as f:
                    for line in f:
                        if line.startswith('VmRSS:'):
                            p_rss = int(line.split()[1])
                            p_pss = p_rss
                            break
            except Exception: pass

        if p_rss > 0:
            rss_kb += p_rss
            pss_kb += p_pss
            pcount += 1
    return pss_kb, rss_kb, pcount

def run_slate(tabs, sleep_tabs, hold_ms=8000):
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

def run_brave(tabs, sleep_tabs, hold_sec=8):
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
        try:
            req = urllib.request.urlopen('http://127.0.0.1:9222/json', timeout=2)
            tab_list = json.loads(req.read().decode('utf-8'))
            page_tabs = [t for t in tab_list if t.get('type') == 'page']
            for bg_tab in page_tabs[1:]:
                tid = bg_tab['id']
                urllib.request.urlopen(f'http://127.0.0.1:9222/json/close/{tid}', timeout=2)
            time.sleep(1.5)
        except Exception:
            pass

    pss_kb, rss_kb, pcount = sample_process_tree(p.pid, prof)

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
    shutil.rmtree(prof, ignore_errors=True)

    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount,
        'blocked_requests': pref_blocked,
        'evaluated_requests': None
    }

def save_raw_progress(raw_store):
    with open(RAW_OUT_FILE, 'w') as f:
        json.dump(raw_store, f, indent=2)

def main():
    configs = [
        ("1_tab", 1, False, 8000),
        ("5_tabs", 5, False, 9000),
        ("10_tabs", 10, False, 10000),
        ("10_tabs_sleep", 10, True, 10000)
    ]

    raw_store = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "description": "7 runs per scenario. Raw run data written to disk as it executes.",
        "scenarios": {}
    }

    print("================================================================================")
    print(" 7-RUN FAIR BENCHMARK RUNNER (STREAMING TO RAW DISK FILE)")
    print(f" Target file: {RAW_OUT_FILE}")
    print("================================================================================")

    for name, tabs, sleep_tabs, hold_ms in configs:
        print(f"\n--- Scenario: {name} (tabs={tabs}, sleep={sleep_tabs}) ---")
        raw_store["scenarios"][name] = {
            "tabs": tabs,
            "sleep_tabs": sleep_tabs,
            "slate_runs": [],
            "brave_runs": []
        }
        save_raw_progress(raw_store)

        for r in range(1, 8):
            print(f"  [Slate] Run {r}/7...", end="", flush=True)
            s_res = run_slate(tabs, sleep_tabs, hold_ms)
            s_res["run_index"] = r
            raw_store["scenarios"][name]["slate_runs"].append(s_res)
            save_raw_progress(raw_store)
            print(f" done (PSS: {s_res['pss_mb']} MB, Procs: {s_res['process_count']})")
            time.sleep(1)

        for r in range(1, 8):
            print(f"  [Brave] Run {r}/7...", end="", flush=True)
            b_res = run_brave(tabs, sleep_tabs, hold_ms // 1000)
            b_res["run_index"] = r
            raw_store["scenarios"][name]["brave_runs"].append(b_res)
            save_raw_progress(raw_store)
            print(f" done (PSS: {b_res['pss_mb']} MB, Procs: {b_res['process_count']})")
            time.sleep(1)

    print("\n================================================================================")
    print(f" Raw runs fully saved to: {RAW_OUT_FILE}")
    print("================================================================================")

if __name__ == '__main__':
    main()
