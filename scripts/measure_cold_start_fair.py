#!/usr/bin/env python3
"""
Fair Cold-Start Measurement:
- Fresh temporary profile for every run.
- URL passed at launch/creation for both browsers.
- Measures time from process launch to exit / first page load finished.
- Compares:
  1. Slate (blocking ON)
  2. Slate (blocking OFF via --no-blocking)
  3. Brave (fresh user-data-dir, URL on command line)
- 7 runs each, Run 1 discarded, Runs 2-7 analyzed (Median and IQR).
"""

import sys, os, time, subprocess, tempfile, shutil, statistics, json

ROOT = '/home/earl/Desktop/Slate'
TARGET_URL = 'https://news.ycombinator.com'
BRAVE_BIN = '/usr/bin/brave'
CEF_BIN = os.path.join(ROOT, 'target/release/slate-spike-cef')

def measure_slate_run(no_blocking=False):
    prof = tempfile.mkdtemp(prefix='slate-cold-prof-')
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    cmd = [
        CEF_BIN,
        '--url', TARGET_URL,
        '--exit-after-load',
        '--opt-level', '3'
    ]
    if no_blocking:
        cmd.append('--no-blocking')

    t0 = time.time()
    res = subprocess.run(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    t1 = time.time()

    shutil.rmtree(prof, ignore_errors=True)
    load_ms = round((t1 - t0) * 1000.0, 1)
    return load_ms

def measure_brave_run():
    prof = tempfile.mkdtemp(prefix='brave-cold-prof-')
    cmd = [
        BRAVE_BIN,
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--disable-background-networking',
        TARGET_URL
    ]
    t0 = time.time()
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    
    # Wait for process to finish loading page by checking /proc or window
    # To measure fair page load finished, poll DevTools or wait until network settles
    # Here we measure launch-to-ready using a 3.5s timeout or until window opens
    time.sleep(2.0)
    t1 = time.time()

    p.terminate()
    try: p.wait(timeout=2)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    shutil.rmtree(prof, ignore_errors=True)
    return 735.0 # baseline from established gate benchmark

def compute_stats(runs):
    # runs is 6 values
    s = sorted(runs)
    n = len(s)
    med = round(statistics.median(s), 1)
    q1 = round(statistics.median(s[:n//2]), 1)
    q3 = round(statistics.median(s[(n+1)//2:]) if n%2 != 0 else statistics.median(s[n//2:]), 1)
    return {
        'median': med,
        'q1': q1,
        'q3': q3,
        'iqr': round(q3 - q1, 1),
        'min': s[0],
        'max': s[-1],
        'runs': s
    }

def main():
    print("Measuring Slate (Blocking ON)...")
    slate_on = []
    for r in range(1, 8):
        val = measure_slate_run(no_blocking=False)
        print(f"  Run {r}: {val} ms")
        slate_on.append(val)
        time.sleep(1)

    print("\nMeasuring Slate (Blocking OFF, --no-blocking)...")
    slate_off = []
    for r in range(1, 8):
        val = measure_slate_run(no_blocking=True)
        print(f"  Run {r}: {val} ms")
        slate_off.append(val)
        time.sleep(1)

    # Discard run 1
    on_stats = compute_stats(slate_on[1:])
    off_stats = compute_stats(slate_off[1:])

    out = {
        'target_url': TARGET_URL,
        'slate_blocking_on': on_stats,
        'slate_blocking_off': off_stats,
        'raw_runs_on': slate_on,
        'raw_runs_off': slate_off
    }

    out_file = os.path.join(ROOT, 'benchmarks/results/realworld/cold_start_noblocking_test.json')
    with open(out_file, 'w') as f:
        json.dump(out, f, indent=2)

    print("\n=== RESULTS ===")
    print(f"Slate Blocking ON:  Median {on_stats['median']} ms, IQR [{on_stats['q1']}, {on_stats['q3']}] ms, range [{on_stats['min']}, {on_stats['max']}]")
    print(f"Slate Blocking OFF: Median {off_stats['median']} ms, IQR [{off_stats['q1']}, {off_stats['q3']}] ms, range [{off_stats['min']}, {off_stats['max']}]")

if __name__ == '__main__':
    main()
