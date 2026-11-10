#!/usr/bin/env python3
"""
Measures idle CPU usage over a 60-second window for 1 tab and 10 tabs:
Slate vs Brave.
"""

import os, sys, time, subprocess, tempfile, json, shutil

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
CLK_TCK = os.sysconf(os.sysconf_names['SC_CLK_TCK'])

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

def sample_tree_ticks(root_pid, profile_dir=None):
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
            if profile_dir:
                with open(f'/proc/{pid}/cmdline', 'rb') as f:
                    if profile_dir.encode() in f.read():
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

    total_ticks = 0
    active_procs = 0
    for pid in visited:
        try:
            with open(f'/proc/{pid}/stat', 'r') as f:
                content = f.read()
                fields = content.split(')')[-1].split()
                utime = int(fields[11])
                stime = int(fields[12])
                total_ticks += (utime + stime)
                active_procs += 1
        except Exception:
            pass
    return total_ticks, active_procs

def measure_slate_cpu(tabs, window_sec=60):
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    cmd = [
        CEF_BIN,
        '--real-sites',
        '--tabs', str(tabs),
        '--hold-ms', str((window_sec + 20) * 1000),
        '--allow-autoplay',
        '--opt-level', '3'
    ]
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(15) # Settle period

    t0 = time.time()
    ticks0, pcount0 = sample_tree_ticks(p.pid)
    time.sleep(window_sec)
    t1 = time.time()
    ticks1, pcount1 = sample_tree_ticks(p.pid)

    p.terminate()
    try: p.wait(timeout=3)
    except: p.kill()

    dt = t1 - t0
    dticks = ticks1 - ticks0
    avg_cpu_pct = (dticks / CLK_TCK) / dt * 100.0
    return {
        'avg_cpu_pct': round(avg_cpu_pct, 2),
        'dt_sec': round(dt, 2),
        'dticks': dticks,
        'process_count': pcount1
    }

def measure_brave_cpu(tabs, window_sec=60):
    prof = tempfile.mkdtemp(prefix='brave-cpu-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    cache_src = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'
    if os.path.exists(cache_src):
        shutil.copytree(cache_src, os.path.join(prof, 'Default/adblock_cache'))

    urls = SITES[:tabs]
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--autoplay-policy=no-user-gesture-required'
    ] + urls

    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(15) # Settle period

    t0 = time.time()
    ticks0, pcount0 = sample_tree_ticks(p.pid, prof)
    time.sleep(window_sec)
    t1 = time.time()
    ticks1, pcount1 = sample_tree_ticks(p.pid, prof)

    p.terminate()
    try: p.wait(timeout=3)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    subprocess.run(['rm', '-rf', prof], check=False)

    dt = t1 - t0
    dticks = ticks1 - ticks0
    avg_cpu_pct = (dticks / CLK_TCK) / dt * 100.0
    return {
        'avg_cpu_pct': round(avg_cpu_pct, 2),
        'dt_sec': round(dt, 2),
        'dticks': dticks,
        'process_count': pcount1
    }

def main():
    print("================================================================================")
    print(" IDLE CPU OVER 60 SECONDS: SLATE vs BRAVE")
    print("================================================================================")
    results = {}

    for tabs in [1, 10]:
        print(f"\n--- Measuring {tabs} Tab(s) over 60s ---")
        print("  -> Slate...", end="", flush=True)
        s_cpu = measure_slate_cpu(tabs, 60)
        print(f" done (CPU: {s_cpu['avg_cpu_pct']}%, Procs: {s_cpu['process_count']})")

        print("  -> Brave...", end="", flush=True)
        b_cpu = measure_brave_cpu(tabs, 60)
        print(f" done (CPU: {b_cpu['avg_cpu_pct']}%, Procs: {b_cpu['process_count']})")

        results[f"{tabs}_tab"] = {
            'slate': s_cpu,
            'brave': b_cpu
        }

    out_file = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/idle_cpu_60s.json')
    with open(out_file, 'w') as f:
        json.dump(results, f, indent=2)
    print(f"\nResults saved to: {out_file}")

if __name__ == '__main__':
    main()
