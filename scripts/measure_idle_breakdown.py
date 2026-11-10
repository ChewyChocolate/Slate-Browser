#!/usr/bin/env python3
"""
True Idle CPU Breakdown (Harness Excluded):
- Completely closes/disables DevTools (no remote debugging port).
- Measures 60-second window after a 15-second settle period.
- Tests:
  1. about:blank
  2. 1 static page (Hacker News)
  3. 10 static pages
- Breaks down CPU ticks and % of 1 CPU core by process kind:
  browser, renderer, gpu-process, utility:network, utility:storage, zygote.
"""

import os, sys, time, subprocess, tempfile, json

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
CLK_TCK = os.sysconf(os.sysconf_names['SC_CLK_TCK'])

STATIC_SITES = [
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

def measure_scenario(browser, target_type, window_sec=60):
    env = os.environ.copy()
    env['WAYLAND_DISPLAY'] = 'wayland-1'
    prof = None

    if target_type == 'blank':
        urls = ['about:blank']
    elif target_type == 'static_1':
        urls = [STATIC_SITES[0]]
    else: # static_10
        urls = STATIC_SITES

    if browser == 'slate':
        env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
        env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
        cmd = [
            CEF_BIN,
            '--hold-ms', str((window_sec + 20) * 1000),
            '--opt-level', '3'
        ]
        if target_type == 'static_10':
            cmd.extend(['--real-sites', '--tabs', '10'])
        else:
            cmd.extend(['--url', urls[0]])
        proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        prof = tempfile.mkdtemp(prefix='brave-idle-breakdown-')
        cmd = [
            '/usr/bin/brave',
            f'--user-data-dir={prof}',
            '--no-first-run',
            '--no-default-browser-check',
            '--disable-sync'
        ] + urls
        proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    time.sleep(15) # Settle period

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
    print("================================================================================")
    print(" IDLE CPU BREAKDOWN BY PROCESS TYPE (NO DEVTOOLS / HARNESS EXCLUDED)")
    print("================================================================================")
    scenarios = [
        ("about_blank", "blank"),
        ("1_static_hn", "static_1"),
        ("10_static_pages", "static_10")
    ]
    results = {}

    for name, target_type in scenarios:
        print(f"\n--- Scenario: {name} (60s settle + measure) ---")
        print("  -> Slate...", end="", flush=True)
        s_res = measure_scenario('slate', target_type, 60)
        print(f" done (Total CPU: {s_res['total_cpu_pct']}%, Breakdown: {s_res['by_process_kind']})")

        print("  -> Brave...", end="", flush=True)
        b_res = measure_scenario('brave', target_type, 60)
        print(f" done (Total CPU: {b_res['total_cpu_pct']}%, Breakdown: {b_res['by_process_kind']})")

        results[name] = {'slate': s_res, 'brave': b_res}

    out_file = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/idle_cpu_breakdown.json')
    with open(out_file, 'w') as f:
        json.dump(results, f, indent=2)
    print(f"\nSaved breakdown to: {out_file}")

if __name__ == '__main__':
    main()
