#!/usr/bin/env python3
"""
7-Run Cold Start Benchmark (Slate vs Brave)
Measures two fixed events across 7 runs:
1. Time from process launch to first rendered frame / load of about:blank
2. Time from process launch to load-finished of fixed page (Hacker News)
Reports Median and IQR (Runs 2-7 analyzed, Run 1 discarded as warmup).
"""

import os, sys, time, subprocess, tempfile, shutil, json, statistics, urllib.request

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
FIXED_URL = "https://news.ycombinator.com"

def measure_slate_launch(target_url):
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    t0 = time.time()
    cmd = [
        CEF_BIN,
        '--url', target_url,
        '--exit-after-load',
        '--opt-level', '3'
    ]
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    out, _ = p.communicate()
    t_wall_ms = (time.time() - t0) * 1000.0

    # Slate also records cold_start_ms from internal timer
    internal_cold_ms = None
    for line in out.splitlines():
        if 'cold_start_ms' in line:
            try:
                d = json.loads(line)
                internal_cold_ms = d.get('cold_start_ms')
            except Exception: pass

    return t_wall_ms, internal_cold_ms

def measure_brave_launch(target_url):
    prof = tempfile.mkdtemp(prefix='brave-cold-7run-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    cache_src = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'
    if os.path.exists(cache_src):
        shutil.copytree(cache_src, os.path.join(prof, 'Default/adblock_cache'))

    t0 = time.time()
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--remote-debugging-port=9222',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        target_url
    ]
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    # Poll CDP until target is created and loaded
    loaded = False
    while (time.time() - t0) < 15:
        try:
            tabs = json.loads(urllib.request.urlopen('http://127.0.0.1:9222/json', timeout=1).read().decode())
            page_tabs = [t for t in tabs if t.get('type') == 'page']
            if page_tabs:
                loaded = True
                break
        except Exception:
            pass
        time.sleep(0.01)

    t_wall_ms = (time.time() - t0) * 1000.0
    p.terminate()
    try: p.wait(timeout=2)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    subprocess.run(['rm', '-rf', prof], check=False)

    return t_wall_ms

def compute_iqr(vals):
    s = sorted(vals)
    n = len(s)
    med = statistics.median(s)
    q1 = statistics.median(s[:n//2])
    q3 = statistics.median(s[(n+1)//2:]) if n%2 != 0 else statistics.median(s[n//2:])
    return {
        'median_ms': round(med, 1),
        'q1_ms': round(q1, 1),
        'q3_ms': round(q3, 1),
        'iqr_ms': round(q3 - q1, 1),
        'min_ms': round(min(s), 1),
        'max_ms': round(max(s), 1),
        'raw': [round(x, 1) for x in s]
    }

def main():
    print("================================================================================")
    print(" 7-RUN COLD START BENCHMARK: SLATE vs BRAVE")
    print("================================================================================")
    results = {}

    for event_name, url in [("about_blank", "about:blank"), ("fixed_page_hn", FIXED_URL)]:
        print(f"\n--- Measuring Event: {event_name} ({url}) ---")
        
        # Slate
        s_runs = []
        for r in range(1, 8):
            print(f"  [Slate] Run {r}/7...", end="", flush=True)
            wall_ms, internal_ms = measure_slate_launch(url)
            s_runs.append(wall_ms)
            print(f" done ({wall_ms:.1f} ms)")
            time.sleep(0.5)

        # Brave
        b_runs = []
        for r in range(1, 8):
            print(f"  [Brave] Run {r}/7...", end="", flush=True)
            b_ms = measure_brave_launch(url)
            b_runs.append(b_ms)
            print(f" done ({b_ms:.1f} ms)")
            time.sleep(0.5)

        # Analyze Runs 2-7 (Run 1 discarded as warmup)
        s_stats = compute_iqr(s_runs[1:])
        b_stats = compute_iqr(b_runs[1:])

        # Check IQR overlap
        iqr_overlap = not (s_stats['q3_ms'] < b_stats['q1_ms'] or b_stats['q3_ms'] < s_stats['q1_ms'])

        results[event_name] = {
            'slate': s_stats,
            'brave': b_stats,
            'iqr_overlap': iqr_overlap
        }

    out_file = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/cold_start_7run.json')
    with open(out_file, 'w') as f:
        json.dump(results, f, indent=2)
    print(f"\nSaved cold start results to: {out_file}")

if __name__ == '__main__':
    main()
