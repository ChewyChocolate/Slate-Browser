#!/usr/bin/env python3
"""
7-Run Heavy Scenario Benchmark:
Loads:
  1. Long YouTube video (Big Buck Bunny 10-min 4K): https://www.youtube.com/watch?v=aqz-KE-bpKQ
  2. Heavy news site: https://www.theverge.com
Records start and end currentTime for YouTube, PSS, RSS, and process count.
Run 1 discarded as warmup; Runs 2-7 analyzed with Median and IQR.
"""

import sys, os, time, subprocess, tempfile, shutil, json, statistics, urllib.request

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
sys.path.insert(0, os.path.join(ROOT_DIR, 'scripts'))
from verify_readiness_gates import ws_connect, cdp_eval

HEAVY_SITES = [
    "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
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
        except Exception: pass

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

def run_heavy_slate():
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    cmd = [
        CEF_BIN,
        '--real-sites',
        '--tabs', '2',
        '--remote-debugging-port=9222',
        '--hold-ms', '30000',
        '--allow-autoplay',
        '--opt-level', '3'
    ]
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(4)

    t_start = -1
    t_end = -1
    sock = None
    try:
        tabs = json.loads(urllib.request.urlopen('http://127.0.0.1:9222/json', timeout=2).read().decode())
        yt_tab = [t for t in tabs if t.get('type') == 'page' and 'youtube' in t.get('url', '').lower()][0]
        sock = ws_connect(yt_tab['webSocketDebuggerUrl'])
        t_start = cdp_eval(sock, "Boolean(document.querySelector('video')) ? document.querySelector('video').currentTime : -1", 1) or -1
    except Exception: pass

    time.sleep(20) # Hold for 20s

    try:
        if sock:
            t_end = cdp_eval(sock, "Boolean(document.querySelector('video')) ? document.querySelector('video').currentTime : -1", 2) or -1
            sock.close()
    except Exception: pass

    pss_kb, rss_kb, pcount = sample_process_tree(p.pid)
    p.terminate()
    try: p.wait(timeout=3)
    except: p.kill()

    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount,
        'yt_time_start': round(float(t_start), 2) if t_start is not None else -1,
        'yt_time_end': round(float(t_end), 2) if t_end is not None else -1
    }

def run_heavy_brave():
    prof = tempfile.mkdtemp(prefix='brave-heavy-7run-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    cache_src = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'
    if os.path.exists(cache_src):
        shutil.copytree(cache_src, os.path.join(prof, 'Default/adblock_cache'))

    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--remote-debugging-port=9222',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--autoplay-policy=no-user-gesture-required'
    ] + HEAVY_SITES

    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(4)

    t_start = -1
    t_end = -1
    sock = None
    try:
        tabs = json.loads(urllib.request.urlopen('http://127.0.0.1:9222/json', timeout=2).read().decode())
        yt_tab = [t for t in tabs if t.get('type') == 'page' and 'youtube' in t.get('url', '').lower()][0]
        sock = ws_connect(yt_tab['webSocketDebuggerUrl'])
        t_start = cdp_eval(sock, "Boolean(document.querySelector('video')) ? document.querySelector('video').currentTime : -1", 1) or -1
    except Exception: pass

    time.sleep(20)

    try:
        if sock:
            t_end = cdp_eval(sock, "Boolean(document.querySelector('video')) ? document.querySelector('video').currentTime : -1", 2) or -1
            sock.close()
    except Exception: pass

    pss_kb, rss_kb, pcount = sample_process_tree(p.pid, prof)
    p.terminate()
    try: p.wait(timeout=3)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    subprocess.run(['rm', '-rf', prof], check=False)

    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount,
        'yt_time_start': round(float(t_start), 2) if t_start is not None else -1,
        'yt_time_end': round(float(t_end), 2) if t_end is not None else -1
    }

def compute_iqr(vals):
    s = sorted(vals)
    n = len(s)
    med = statistics.median(s)
    q1 = statistics.median(s[:n//2])
    q3 = statistics.median(s[(n+1)//2:]) if n%2 != 0 else statistics.median(s[n//2:])
    return {
        'median': round(med, 1),
        'q1': round(q1, 1),
        'q3': round(q3, 1),
        'iqr': round(q3 - q1, 1),
        'min': round(min(s), 1),
        'max': round(max(s), 1),
        'raw': [round(x, 1) for x in s]
    }

def main():
    print("================================================================================")
    print(" 7-RUN HEAVY SCENARIO BENCHMARK (LONG YOUTUBE STREAM + THE VERGE)")
    print("================================================================================")
    slate_runs = []
    for r in range(1, 8):
        print(f"  [Slate] Run {r}/7...", end="", flush=True)
        res = run_heavy_slate()
        slate_runs.append(res)
        print(f" done (PSS: {res['pss_mb']} MB, YT: {res['yt_time_start']}s -> {res['yt_time_end']}s)")
        time.sleep(1)

    brave_runs = []
    for r in range(1, 8):
        print(f"  [Brave] Run {r}/7...", end="", flush=True)
        res = run_heavy_brave()
        brave_runs.append(res)
        print(f" done (PSS: {res['pss_mb']} MB, YT: {res['yt_time_start']}s -> {res['yt_time_end']}s)")
        time.sleep(1)

    # Discard run 1 as warmup
    s_analyzed = slate_runs[1:]
    b_analyzed = brave_runs[1:]

    s_pss = compute_iqr([x['pss_mb'] for x in s_analyzed])
    b_pss = compute_iqr([x['pss_mb'] for x in b_analyzed])

    s_rss = compute_iqr([x['rss_mb'] for x in s_analyzed])
    b_rss = compute_iqr([x['rss_mb'] for x in b_analyzed])

    iqr_overlap = not (s_pss['q3'] < b_pss['q1'] or b_pss['q3'] < s_pss['q1'])

    summary = {
        'slate': {
            'pss': s_pss,
            'rss': s_rss,
            'process_count_median': int(statistics.median([x['process_count'] for x in s_analyzed])),
            'runs': slate_runs
        },
        'brave': {
            'pss': b_pss,
            'rss': b_rss,
            'process_count_median': int(statistics.median([x['process_count'] for x in b_analyzed])),
            'runs': brave_runs
        },
        'iqr_overlap': iqr_overlap
    }

    out_file = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/heavy_scenario_7run.json')
    with open(out_file, 'w') as f:
        json.dump(summary, f, indent=2)
    print(f"\nSaved heavy 7-run results to: {out_file}")

if __name__ == '__main__':
    main()
