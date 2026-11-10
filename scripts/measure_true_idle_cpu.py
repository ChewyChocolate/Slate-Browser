#!/usr/bin/env python3
"""
Measures TRUE IDLE CPU over 60 seconds on static pages (zero video/audio/streaming):
- 1 Tab: Hacker News
- 10 Tabs: Static / content pages without video
"""

import os, sys, time, subprocess, tempfile, json, shutil

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
        except Exception: pass

    visited = set()
    queue = list(seed_pids)
    while queue:
        cur = queue.pop()
        if cur in visited: continue
        visited.add(cur)
        queue.extend(children.get(cur, []))

    total_ticks = 0
    pcount = 0
    for pid in visited:
        try:
            with open(f'/proc/{pid}/stat', 'r') as f:
                content = f.read()
                fields = content.split(')')[-1].split()
                utime = int(fields[11])
                stime = int(fields[12])
                total_ticks += (utime + stime)
                pcount += 1
        except Exception: pass
    return total_ticks, pcount

def measure_true_idle(browser, tabs, window_sec=60):
    urls = STATIC_SITES[:tabs]
    env = os.environ.copy()
    env['WAYLAND_DISPLAY'] = 'wayland-1'
    prof = None

    if browser == 'slate':
        env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
        env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
        # pass url or load tabs
        cmd = [
            CEF_BIN,
            '--url', urls[0],
            '--hold-ms', str((window_sec + 20) * 1000),
            '--opt-level', '3'
        ]
        if tabs > 1:
            cmd = [
                CEF_BIN,
                '--real-sites',
                '--tabs', str(tabs),
                '--hold-ms', str((window_sec + 20) * 1000),
                '--opt-level', '3'
            ]
        proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        prof = tempfile.mkdtemp(prefix='brave-idle-')
        os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
        cache_src = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'
        if os.path.exists(cache_src):
            shutil.copytree(cache_src, os.path.join(prof, 'Default/adblock_cache'))
        cmd = [
            '/usr/bin/brave',
            f'--user-data-dir={prof}',
            '--no-first-run',
            '--no-default-browser-check',
            '--disable-sync'
        ] + urls
        proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    time.sleep(15) # Wait 15 seconds for initial load and rendering to completely settle

    t0 = time.time()
    ticks0, p0 = sample_tree_ticks(proc.pid, prof)
    time.sleep(window_sec)
    t1 = time.time()
    ticks1, p1 = sample_tree_ticks(proc.pid, prof)

    proc.terminate()
    try: proc.wait(timeout=3)
    except: proc.kill()

    if prof:
        subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
        subprocess.run(['rm', '-rf', prof], check=False)

    dt = t1 - t0
    dticks = ticks1 - ticks0
    cpu_pct = (dticks / CLK_TCK) / dt * 100.0

    return {
        'cpu_pct': round(cpu_pct, 2),
        'dt_sec': round(dt, 2),
        'dticks': dticks,
        'process_count': p1
    }

def main():
    print("================================================================================")
    print(" TRUE IDLE CPU OVER 60 SECONDS (STATIC PAGES ONLY, ZERO VIDEO)")
    print("================================================================================")
    res = {}
    for tabs in [1, 10]:
        print(f"\n--- Measuring True Idle: {tabs} Tab(s) ---")
        print("  -> Slate...", end="", flush=True)
        s = measure_true_idle('slate', tabs, 60)
        print(f" done (CPU: {s['cpu_pct']}%, Procs: {s['process_count']})")

        print("  -> Brave...", end="", flush=True)
        b = measure_true_idle('brave', tabs, 60)
        print(f" done (CPU: {b['cpu_pct']}%, Procs: {b['process_count']})")

        res[f"{tabs}_tab"] = {'slate': s, 'brave': b}

    out_file = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/true_idle_cpu_60s.json')
    with open(out_file, 'w') as f:
        json.dump(res, f, indent=2)
    print(f"\nSaved true idle results to: {out_file}")

if __name__ == '__main__':
    main()
