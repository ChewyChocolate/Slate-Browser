#!/usr/bin/env python3
"""
Heavy-Scenario Benchmark (Task 1b Extension):
Loads:
  1. Long YouTube streaming video: https://www.youtube.com/watch?v=aqz-KE-bpKQ (Big Buck Bunny 10-min 4K/60fps)
  2. Heavy ad-laden news site: https://www.theverge.com
Monitors continuous playback and memory over a 30-second hold.
"""

import sys, os, time, subprocess, tempfile, shutil, json

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')

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

def measure_heavy_slate():
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    # Slate runs with 2 tabs
    cmd = [
        CEF_BIN,
        '--real-sites',
        '--tabs', '2',
        '--hold-ms', '30000',
        '--allow-autoplay',
        '--opt-level', '3'
    ]
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(25) # Hold for 25s of streaming playback
    pss_kb, rss_kb, pcount = sample_process_tree(p.pid)
    p.terminate()
    try: p.wait(timeout=3)
    except: p.kill()
    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount
    }

def measure_heavy_brave():
    prof = tempfile.mkdtemp(prefix='brave-heavy-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    cache_src = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'
    if os.path.exists(cache_src):
        shutil.copytree(cache_src, os.path.join(prof, 'Default/adblock_cache'))

    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--autoplay-policy=no-user-gesture-required'
    ] + HEAVY_SITES

    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(25) # Hold for 25s of streaming playback
    pss_kb, rss_kb, pcount = sample_process_tree(p.pid, prof)
    p.terminate()
    try: p.wait(timeout=3)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    subprocess.run(['rm', '-rf', prof], check=False)
    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount
    }

def main():
    print("================================================================================")
    print(" HEAVY-SCENARIO BENCHMARK: LONG YOUTUBE STREAM + THE VERGE (30s HOLD)")
    print("================================================================================")
    print("  -> Running Slate...", end="", flush=True)
    s = measure_heavy_slate()
    print(f" done (PSS: {s['pss_mb']} MB, RSS: {s['rss_mb']} MB, Procs: {s['process_count']})")

    print("  -> Running Brave...", end="", flush=True)
    b = measure_heavy_brave()
    print(f" done (PSS: {b['pss_mb']} MB, RSS: {b['rss_mb']} MB, Procs: {b['process_count']})")

    res = {'slate': s, 'brave': b}
    out_file = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/heavy_scenario_results.json')
    with open(out_file, 'w') as f:
        json.dump(res, f, indent=2)
    print(f"\nSaved heavy scenario results to: {out_file}")

if __name__ == '__main__':
    main()
