#!/usr/bin/env python3
"""
Comprehensive 7-Run Idle CPU Benchmark (DevTools Excluded):
Measures 7 runs each across 8 scenarios:
1. Slate Opt 3 on about:blank
2. Slate Opt 0 on about:blank
3. Brave default on about:blank
4. Brave trimmed (Rewards, News, Wallet, Leo disabled) on about:blank
5. Slate Opt 3 on 10 static pages
6. Slate Opt 0 on 10 static pages
7. Brave default on 10 static pages
8. Brave trimmed on 10 static pages

Settle window: 6s for blank, 12s for 10 static pages.
Measurement window: 10s.
DevTools/CDP is completely closed/disabled to isolate pure browser engine idle CPU.
Saves raw per-run measurements to benchmarks/results/realworld/idle_cpu_7runs_raw.json.
"""

import os, sys, time, subprocess, tempfile, json

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
CLK_TCK = os.sysconf(os.sysconf_names['SC_CLK_TCK'])
RAW_OUT = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/idle_cpu_7runs_raw.json')

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
STATIC_10_URLS_COMMA = ",".join(STATIC_10_SITES)

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

def measure_run(cmd, env=None, prof=None, settle_sec=6, window_sec=10):
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

def main():
    env_slate = os.environ.copy()
    env_slate['WAYLAND_DISPLAY'] = 'wayland-1'
    env_slate['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env_slate['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')

    if os.path.exists(RAW_OUT):
        with open(RAW_OUT, 'r') as f:
            raw_data = json.load(f)
    else:
        raw_data = {}

    def checkpoint():
        with open(RAW_OUT, 'w') as f:
            json.dump(raw_data, f, indent=2)

    scenarios = [
        # (key, label, type, is_blank)
        ("slate_opt3_blank", "Slate (Opt 3) about:blank", "slate_opt3", True),
        ("slate_opt0_blank", "Slate (Opt 0) about:blank", "slate_opt0", True),
        ("brave_default_blank", "Brave (Default) about:blank", "brave_default", True),
        ("brave_trimmed_blank", "Brave (Trimmed) about:blank", "brave_trimmed", True),
        ("slate_opt3_10static", "Slate (Opt 3) 10 Static Sites", "slate_opt3", False),
        ("slate_opt0_10static", "Slate (Opt 0) 10 Static Sites", "slate_opt0", False),
        ("brave_default_10static", "Brave (Default) 10 Static Sites", "brave_default", False),
        ("brave_trimmed_10static", "Brave (Trimmed) 10 Static Sites", "brave_trimmed", False),
    ]

    for key, label, stype, is_blank in scenarios:
        if key not in raw_data:
            raw_data[key] = []
        settle = 6 if is_blank else 12

        print(f"\n=== Scenario: {label} ({len(raw_data[key])}/7 runs done) ===", flush=True)
        while len(raw_data[key]) < 7:
            run_idx = len(raw_data[key]) + 1
            print(f"  Run {run_idx}/7...", end="", flush=True)

            prof = None
            if stype == "slate_opt3":
                if is_blank:
                    cmd = [CEF_BIN, '--url', 'about:blank', '--hold-ms', '30000', '--opt-level', '3']
                else:
                    cmd = [CEF_BIN, '--urls', STATIC_10_URLS_COMMA, '--hold-ms', '35000', '--opt-level', '3']
                res = measure_run(cmd, env=env_slate, settle_sec=settle, window_sec=10)
            elif stype == "slate_opt0":
                if is_blank:
                    cmd = [CEF_BIN, '--url', 'about:blank', '--hold-ms', '30000', '--opt-level', '0']
                else:
                    cmd = [CEF_BIN, '--urls', STATIC_10_URLS_COMMA, '--hold-ms', '35000', '--opt-level', '0']
                res = measure_run(cmd, env=env_slate, settle_sec=settle, window_sec=10)
            elif stype == "brave_default":
                prof = tempfile.mkdtemp(prefix='brave-idle-def-')
                cmd = [
                    '/usr/bin/brave',
                    f'--user-data-dir={prof}',
                    '--no-first-run',
                    '--no-default-browser-check',
                    '--disable-sync'
                ]
                if is_blank:
                    cmd.append('about:blank')
                else:
                    cmd.extend(STATIC_10_SITES)
                res = measure_run(cmd, prof=prof, settle_sec=settle, window_sec=10)
            elif stype == "brave_trimmed":
                prof = tempfile.mkdtemp(prefix='brave-idle-trim-')
                cmd = [
                    '/usr/bin/brave',
                    f'--user-data-dir={prof}',
                    '--no-first-run',
                    '--no-default-browser-check',
                    '--disable-sync',
                    '--disable-features=BraveRewards,BraveNews,BraveWallet,BraveLeo'
                ]
                if is_blank:
                    cmd.append('about:blank')
                else:
                    cmd.extend(STATIC_10_SITES)
                res = measure_run(cmd, prof=prof, settle_sec=settle, window_sec=10)

            res['run_index'] = run_idx
            raw_data[key].append(res)
            checkpoint()
            print(f" CPU: {res['total_cpu_pct']}% (procs: {res['process_count']})", flush=True)

    print(f"\nCompleted all 56 runs! Output written to {RAW_OUT}")

if __name__ == '__main__':
    main()
