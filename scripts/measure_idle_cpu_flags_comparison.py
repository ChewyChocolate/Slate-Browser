#!/usr/bin/env python3
"""
Comprehensive 60-Second Idle CPU Comparison (Harness/DevTools Excluded):
1. Slate on about:blank (Level 3 flags ON) - 60s
2. Slate on about:blank (Level 0 flags OFF) - 60s
3. Brave on about:blank with Slate flags - 60s
4. Brave on about:blank with Rewards/News/Wallet/Leo disabled - 60s
5. Brave on 10 static pages with Slate flags - 60s
6. Brave on 10 static pages with Rewards/News/Wallet/Leo disabled - 60s

All scenarios include 15-second pre-measurement settle window, DevTools closed,
and break down CPU ticks and % of 1 core by process kind.
Outputs saved incrementally to benchmarks/results/realworld/idle_cpu_comparison_verified.json.
"""

import os, sys, time, subprocess, tempfile, json

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
CLK_TCK = os.sysconf(os.sysconf_names['SC_CLK_TCK'])
RESULTS_FILE = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/idle_cpu_comparison_verified.json')

STATIC_10_SITES = [
    "https://news.ycombinator.com",
    "https://en.wikipedia.org/wiki/Rust_(programming_language)",
    "https://slashdot.org",
    "https://docs.google.com/document/d/1hXalsO6oixyfHXQIo9C6iozYBHcXmDL_/preview",
    "https://www.openstreetmap.org",
    "https://github.com/rust-lang/rust",
    "https://www.bbc.com/news",
    "https://www.chase.com",
    "https://www.theverge.com",
    "https://lobste.rs"
]

def get_process_tree(root_pid, profile_filter=None):
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

    proc_info = {}
    for pid in visited:
        p_kind = "browser"
        try:
            with open(f'/proc/{pid}/cmdline', 'rb') as f:
                c = f.read().decode('utf-8', errors='ignore')
                if '--type=renderer' in c: p_kind = "renderer"
                elif '--type=gpu-process' in c: p_kind = "gpu-process"
                elif '--type=utility' in c:
                    if 'network.mojom' in c: p_kind = "utility:network"
                    elif 'storage.mojom' in c: p_kind = "utility:storage"
                    else: p_kind = "utility"
                elif '--type=zygote' in c: p_kind = "zygote"
        except Exception: pass

        ticks = 0
        try:
            with open(f'/proc/{pid}/stat', 'r') as f:
                fields = f.read().split(')')[-1].split()
                ticks = int(fields[11]) + int(fields[12])
        except Exception: pass

        proc_info[pid] = {'kind': p_kind, 'ticks': ticks}
    return proc_info

def measure_run(cmd, env=None, prof=None, window_sec=60, settle_sec=15):
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(settle_sec)

    t0 = time.time()
    info0 = get_process_tree(proc.pid, prof)
    time.sleep(window_sec)
    t1 = time.time()
    info1 = get_process_tree(proc.pid, prof)

    proc.terminate()
    try: proc.wait(timeout=3)
    except: proc.kill()

    if prof:
        subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
        subprocess.run(['rm', '-rf', prof], check=False)

    dt = t1 - t0
    kind_ticks = {}
    total_dticks = 0

    for pid, data1 in info1.items():
        k = data1['kind']
        t1_val = data1['ticks']
        t0_val = info0.get(pid, {}).get('ticks', t1_val)
        delta = max(0, t1_val - t0_val)
        kind_ticks[k] = kind_ticks.get(k, 0) + delta
        total_dticks += delta

    kind_cpu = {}
    for k, ticks in kind_ticks.items():
        kind_cpu[k] = round((ticks / CLK_TCK) / dt * 100.0, 2)

    total_cpu = round((total_dticks / CLK_TCK) / dt * 100.0, 2)
    return {
        'total_cpu_pct': total_cpu,
        'by_process_kind': kind_cpu,
        'process_count': len(info1),
        'duration_sec': round(dt, 2)
    }

def run_all():
    env_slate = os.environ.copy()
    env_slate['WAYLAND_DISPLAY'] = 'wayland-1'
    env_slate['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env_slate['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')

    results = {}

    def checkpoint():
        with open(RESULTS_FILE, 'w') as f:
            json.dump(results, f, indent=2)

    # 1. Slate about:blank Level 3 (flags ON)
    print("Measuring Slate about:blank (flags ON, Level 3)...", flush=True)
    cmd = [CEF_BIN, '--url', 'about:blank', '--hold-ms', '90000', '--opt-level', '3']
    results['slate_blank_flags_on'] = measure_run(cmd, env=env_slate, window_sec=60)
    checkpoint()
    print("  -> Done:", results['slate_blank_flags_on']['total_cpu_pct'], "%")

    # 2. Slate about:blank Level 0 (flags OFF)
    print("Measuring Slate about:blank (flags OFF, Level 0)...", flush=True)
    cmd = [CEF_BIN, '--url', 'about:blank', '--hold-ms', '90000', '--opt-level', '0']
    results['slate_blank_flags_off'] = measure_run(cmd, env=env_slate, window_sec=60)
    checkpoint()
    print("  -> Done:", results['slate_blank_flags_off']['total_cpu_pct'], "%")

    # 3. Brave about:blank with Slate flags
    print("Measuring Brave about:blank with Slate flags...", flush=True)
    prof = tempfile.mkdtemp(prefix='brave-flags-')
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--disable-background-networking',
        '--disable-domain-reliability',
        '--metrics-recording-only',
        '--no-pings',
        '--disable-features=Translate,OptimizationHints,AutofillServerCommunication,InterestFeedContentSuggestions,BrowsingTopics,Fledge,LiveCaption',
        'about:blank'
    ]
    results['brave_blank_slate_flags'] = measure_run(cmd, prof=prof, window_sec=60)
    checkpoint()
    print("  -> Done:", results['brave_blank_slate_flags']['total_cpu_pct'], "%")

    # 4. Brave about:blank with Rewards/News/Wallet/Leo disabled
    print("Measuring Brave about:blank with Rewards/News/Wallet/Leo disabled...", flush=True)
    prof = tempfile.mkdtemp(prefix='brave-policy-')
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--disable-features=BraveRewards,BraveNews,BraveWallet,BraveLeo',
        'about:blank'
    ]
    results['brave_blank_bloat_disabled'] = measure_run(cmd, prof=prof, window_sec=60)
    checkpoint()
    print("  -> Done:", results['brave_blank_bloat_disabled']['total_cpu_pct'], "%")

    # 5. Brave 10 static pages with Slate flags
    print("Measuring Brave 10 static pages with Slate flags...", flush=True)
    prof = tempfile.mkdtemp(prefix='brave-flags-10-')
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--disable-background-networking',
        '--disable-domain-reliability',
        '--metrics-recording-only',
        '--no-pings',
        '--disable-features=Translate,OptimizationHints,AutofillServerCommunication,InterestFeedContentSuggestions,BrowsingTopics,Fledge,LiveCaption'
    ] + STATIC_10_SITES
    results['brave_10static_slate_flags'] = measure_run(cmd, prof=prof, window_sec=60)
    checkpoint()
    print("  -> Done:", results['brave_10static_slate_flags']['total_cpu_pct'], "%")

    # 6. Brave 10 static pages with Rewards/News/Wallet/Leo disabled
    print("Measuring Brave 10 static pages with Rewards/News/Wallet/Leo disabled...", flush=True)
    prof = tempfile.mkdtemp(prefix='brave-policy-10-')
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--disable-features=BraveRewards,BraveNews,BraveWallet,BraveLeo'
    ] + STATIC_10_SITES
    results['brave_10static_bloat_disabled'] = measure_run(cmd, prof=prof, window_sec=60)
    checkpoint()
    print("  -> Done:", results['brave_10static_bloat_disabled']['total_cpu_pct'], "%")

    print(f"\nSaved all results to: {RESULTS_FILE}")

if __name__ == '__main__':
    run_all()
