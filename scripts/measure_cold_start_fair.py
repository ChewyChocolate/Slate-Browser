#!/usr/bin/env python3
"""
Fair Cold-Start Session Measurement (Slate vs Brave):
- Run in the same session under identical conditions.
- Target URL: https://news.ycombinator.com
- Fresh temporary user profile directory for every single run.
- Event measured: Process launch (t0) to DOM readiness / load complete (document.readyState == 'complete').
- Compares:
  1. Slate (Blocking ON, Level 3)
  2. Slate (Blocking OFF, Level 3, --no-blocking)
  3. Brave (Default fresh profile, remote debugging port for readyState polling)
- 7 runs each, Run 1 discarded as warmup, Runs 2-7 analyzed (Median, Q1, Q3, IQR, min, max).
- Raw data written directly to disk incrementally.
"""

import sys, os, time, subprocess, tempfile, shutil, statistics, json, urllib.request

ROOT = '/home/earl/Desktop/Slate'
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from verify_readiness_gates import ws_connect, cdp_eval

TARGET_URL = 'https://news.ycombinator.com'
BRAVE_BIN = '/usr/bin/brave'
CEF_BIN = os.path.join(ROOT, 'target/release/slate-spike-cef')
OUT_FILE = os.path.join(ROOT, 'benchmarks/results/realworld/cold_start_session_comparison.json')

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

    t0 = time.perf_counter()
    res = subprocess.run(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    t1 = time.perf_counter()

    shutil.rmtree(prof, ignore_errors=True)
    load_ms = round((t1 - t0) * 1000.0, 1)
    return load_ms

def measure_brave_run(port=9230):
    prof = tempfile.mkdtemp(prefix='brave-cold-prof-')
    cmd = [
        BRAVE_BIN,
        f'--user-data-dir={prof}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        f'--remote-debugging-port={port}',
        TARGET_URL
    ]

    t0 = time.perf_counter()
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    t_ready = None

    while time.perf_counter() - t0 < 10:
        try:
            req = urllib.request.urlopen(f'http://127.0.0.1:{port}/json', timeout=0.1)
            tabs = json.loads(req.read().decode('utf-8'))
            page_tabs = [t for t in tabs if t.get('type') == 'page' and 'news.ycombinator.com' in t.get('url', '')]
            if page_tabs:
                ws_url = page_tabs[0]['webSocketDebuggerUrl']
                sock = ws_connect(ws_url)
                rs = cdp_eval(sock, 'document.readyState', 1)
                sock.close()
                if rs == 'complete':
                    t_ready = time.perf_counter()
                    break
        except Exception:
            time.sleep(0.015)

    proc.terminate()
    try: proc.wait(timeout=2)
    except: proc.kill()

    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    shutil.rmtree(prof, ignore_errors=True)

    if t_ready is None:
        raise RuntimeError("Brave failed to reach readyState == 'complete' within 10s")

    return round((t_ready - t0) * 1000.0, 1)

def compute_stats(runs):
    if not runs:
        return {}
    s = sorted(runs)
    n = len(s)
    med = round(statistics.median(s), 1)
    if n == 1:
        q1 = med
        q3 = med
    elif n < 4:
        q1 = s[0]
        q3 = s[-1]
    else:
        q1 = round(statistics.median(s[:n//2]), 1)
        q3 = round(statistics.median(s[(n+1)//2:]) if n%2 != 0 else statistics.median(s[n//2:]), 1)
    return {
        'median_ms': med,
        'q1_ms': q1,
        'q3_ms': q3,
        'iqr_ms': round(q3 - q1, 1),
        'min_ms': s[0],
        'max_ms': s[-1],
        'raw_analyzed': s
    }

def main():
    print("================================================================================")
    print(" 7-RUN FAIR COLD START COMPARISON (HACKER NEWS, FRESH PROFILES)")
    print("================================================================================")
    results = {
        'target_url': TARGET_URL,
        'timestamp': time.time(),
        'slate_blocking_on': [],
        'slate_blocking_off': [],
        'brave': []
    }

    def checkpoint():
        summary = {
            'target_url': TARGET_URL,
            'timestamp': results['timestamp'],
            'slate_blocking_on': {
                'stats': compute_stats(results['slate_blocking_on'][1:]) if len(results['slate_blocking_on']) > 1 else {},
                'all_runs': results['slate_blocking_on']
            },
            'slate_blocking_off': {
                'stats': compute_stats(results['slate_blocking_off'][1:]) if len(results['slate_blocking_off']) > 1 else {},
                'all_runs': results['slate_blocking_off']
            },
            'brave': {
                'stats': compute_stats(results['brave'][1:]) if len(results['brave']) > 1 else {},
                'all_runs': results['brave']
            }
        }
        with open(OUT_FILE, 'w') as f:
            json.dump(summary, f, indent=2)

    print("Measuring Slate (Blocking ON, Level 3)...")
    for r in range(1, 8):
        val = measure_slate_run(no_blocking=False)
        print(f"  [Slate ON] Run {r}/7: {val} ms")
        results['slate_blocking_on'].append(val)
        checkpoint()
        time.sleep(1)

    print("\nMeasuring Slate (Blocking OFF, Level 3, --no-blocking)...")
    for r in range(1, 8):
        val = measure_slate_run(no_blocking=True)
        print(f"  [Slate OFF] Run {r}/7: {val} ms")
        results['slate_blocking_off'].append(val)
        checkpoint()
        time.sleep(1)

    print("\nMeasuring Brave (Fresh Profile, readyState complete)...")
    for r in range(1, 8):
        val = measure_brave_run(port=9230 + r)
        print(f"  [Brave] Run {r}/7: {val} ms")
        results['brave'].append(val)
        checkpoint()
        time.sleep(1)

    print(f"\nSaved cold start session comparison to: {OUT_FILE}")

if __name__ == '__main__':
    main()
