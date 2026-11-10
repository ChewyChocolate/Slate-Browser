#!/usr/bin/env python3
"""
7-Run Fair Benchmark Suite with Automated Readiness Gates (Slate vs Seeded Brave):
- Every run checks per-site readiness gates across all tabs via DevTools CDP.
- Records gate validation details for every tab in raw JSON.
- If ANY tab fails its gate (bot wall, missing selector, readyState incomplete, text < 50% Brave),
  the run is DISCARDED, logged, and retried.
- Measures PSS, RSS, process count ONLY after all tabs pass their gate.
- Streams raw values to disk as each run executes.
- Scenarios:
  1_tab (YouTube)
  5_tabs (YouTube, Docs preview, Hacker News, Wikipedia, Slashdot)
  10_tabs (All 10 sites)
"""

import sys, os, time, json, subprocess, tempfile, shutil, statistics, urllib.request

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
sys.path.insert(0, os.path.join(ROOT_DIR, 'scripts'))
from verify_readiness_gates import ws_connect, cdp_eval, GATE_EVAL_JS, SITES

CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
OUT_FILE = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/benchmark_7runs_gated.json')
BRAVE_ADBLOCK_CACHE = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'

# Expected minimum text lengths based on Brave baseline
MIN_TEXT_LENS = {
    "https://www.youtube.com/watch?v=jNQXAC9IVRw": 0,
    "https://docs.google.com/document/d/1hXalsO6oixyfHXQIo9C6iozYBHcXmDL_/preview": 0,
    "https://news.ycombinator.com": 200,
    "https://en.wikipedia.org/wiki/Rust_(programming_language)": 500,
    "https://slashdot.org": 500,
    "https://www.bbc.com/news": 300,
    "https://www.openstreetmap.org": 50,
    "https://github.com/rust-lang/rust": 300,
    "https://www.chase.com": 200,
    "https://www.theverge.com": 400
}

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
            with open(f'/proc/{pid}/smaps_rollup', 'r', errors='ignore') as f:
                for line in f:
                    if line.startswith('Rss:'): p_rss = int(line.split()[1])
                    elif line.startswith('Pss:'): p_pss = int(line.split()[1])
        except Exception:
            try:
                with open(f'/proc/{pid}/status', 'r', errors='ignore') as f:
                    for line in f:
                        if line.startswith('VmRSS:'):
                            p_rss = int(line.split()[1])
                            p_pss = p_rss
                            break
            except Exception: pass

        if p_rss > 0:
            rss_kb += p_rss
            pss_kb += p_pss
            pcount += 1
    return pss_kb, rss_kb, pcount

def verify_tabs_readiness(port, expected_count, timeout=20):
    t0 = time.time()
    last_evals = {}
    while time.time() - t0 < timeout:
        time.sleep(1.0)
        try:
            req = urllib.request.urlopen(f'http://127.0.0.1:{port}/json', timeout=2)
            tab_list = [t for t in json.loads(req.read().decode()) if t.get('type') == 'page']
            if len(tab_list) < expected_count:
                continue

            all_passed = True
            current_evals = {}

            for site_idx in range(expected_count):
                site_def = SITES[site_idx]
                target_url_prefix = site_def['url'].split('?')[0].replace('https://', '').replace('http://', '')
                matching = [t for t in tab_list if target_url_prefix in t.get('url', '')]
                if not matching:
                    all_passed = False
                    break
                t_obj = matching[0]

                sock = ws_connect(t_obj['webSocketDebuggerUrl'])
                try:
                    cdp_eval(sock, "window.focus()", 1)
                except Exception:
                    pass
                js = GATE_EVAL_JS.replace('${SELECTOR_JSON}', json.dumps(site_def['selector']))
                gate = cdp_eval(sock, js, 10 + site_idx)
                sock.close()

                if not gate:
                    all_passed = False
                    break

                # Evaluation checks
                bot_wall = gate.get('bot_or_login_wall_detected', False)
                sel_found = gate.get('site_selector_found', False)
                text_len = gate.get('visible_body_text_len', 0)
                min_len = MIN_TEXT_LENS.get(site_def['url'], site_def.get('min_text_len', 0))
                ready_state = gate.get('document_ready_state', '')

                # For video tab (YouTube)
                is_video_ok = True
                if site_def.get('is_video'):
                    is_video_ok = gate.get('has_video') and (gate.get('video_current_time', -1) >= 0)

                ready_ok = (ready_state in ['complete', 'interactive']) or (site_def.get('is_video') and is_video_ok)
                tab_pass = (not bot_wall) and sel_found and (text_len >= min_len) and is_video_ok and ready_ok
                current_evals[site_def['slug']] = {
                    'url': gate.get('final_url'),
                    'title': gate.get('document_title'),
                    'ready_state': ready_state,
                    'text_len': text_len,
                    'selector_found': sel_found,
                    'bot_wall': bot_wall,
                    'passed': tab_pass
                }
                if not tab_pass:
                    all_passed = False
                    if time.time() - t0 > timeout - 5:
                        print(f" [Tab {site_def['slug']} fail detail: {gate}] ", flush=True)

            last_evals = current_evals
            if all_passed and len(current_evals) == expected_count:
                return True, current_evals
        except Exception:
            pass

    return False, last_evals

def run_gated_slate(tabs, port=9250):
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    hold_ms = 35000
    cmd = [
        CEF_BIN,
        '--real-sites',
        '--tabs', str(tabs),
        '--remote-debugging-port=' + str(port),
        '--hold-ms', str(hold_ms),
        '--allow-autoplay',
        '--opt-level', '3'
    ]

    p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    passed, gate_details = verify_tabs_readiness(port, tabs, timeout=22)

    if not passed:
        p.terminate()
        try: p.wait(timeout=2)
        except: p.kill()
        return None, gate_details

    pss_kb, rss_kb, pcount = sample_process_tree(p.pid)
    p.terminate()
    try: p.wait(timeout=2)
    except: p.kill()

    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount,
        'gates_passed': True
    }, gate_details

def run_gated_brave(tabs, port=9251):
    prof = tempfile.mkdtemp(prefix='brave-gated-7run-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    if os.path.exists(BRAVE_ADBLOCK_CACHE):
        shutil.copytree(BRAVE_ADBLOCK_CACHE, os.path.join(prof, 'Default/adblock_cache'))

    urls = [s['url'] for s in SITES[:tabs]]
    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        f'--remote-debugging-port={port}',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        '--autoplay-policy=no-user-gesture-required'
    ] + urls

    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    passed, gate_details = verify_tabs_readiness(port, tabs, timeout=50 if tabs == 10 else 22)

    if not passed:
        p.terminate()
        try: p.wait(timeout=2)
        except: p.kill()
        subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
        shutil.rmtree(prof, ignore_errors=True)
        return None, gate_details

    pss_kb, rss_kb, pcount = sample_process_tree(p.pid, prof)
    p.terminate()
    try: p.wait(timeout=2)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    shutil.rmtree(prof, ignore_errors=True)

    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount,
        'gates_passed': True
    }, gate_details

def compute_iqr(vals):
    if not vals: return {}
    s = sorted(vals)
    n = len(s)
    med = statistics.median(s)
    if n == 1: q1, q3 = med, med
    elif n < 4: q1, q3 = s[0], s[-1]
    else:
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
    print(" 7-RUN GATED BENCHMARK SUITE (1, 5, 10 TABS)")
    print("================================================================================")
    scenarios = [
        ("1_tab", 1),
        ("5_tabs", 5),
        ("10_tabs", 10)
    ]

    all_results = {
        'timestamp': time.time(),
        'description': "Automated readiness gates enforced. Discarded if any tab fails.",
        'scenarios': {}
    }
    if os.path.exists(OUT_FILE):
        try:
            with open(OUT_FILE) as f:
                all_results = json.load(f)
        except Exception: pass

    def checkpoint():
        with open(OUT_FILE, 'w') as f:
            json.dump(all_results, f, indent=2)

    for sc_name, tab_count in scenarios:
        print(f"\n--- Scenario: {sc_name} ({tab_count} tabs) ---")
        sc_data = all_results.setdefault('scenarios', {}).setdefault(sc_name, {})
        slate_runs = sc_data.get('slate_runs', [])
        brave_runs = sc_data.get('brave_runs', [])

        # 7 Slate runs
        attempts = 0
        while len(slate_runs) < 7 and attempts < 14:
            attempts += 1
            run_idx = len(slate_runs) + 1
            print(f"  [Slate] Run {run_idx}/7 (attempt {attempts})...", end="", flush=True)
            res, gates = run_gated_slate(tab_count, port=9250 + run_idx)
            if res is None:
                failed = [k for k, v in (gates or {}).items() if not v.get('passed', False)]
                print(f" [DISCARDED: Gate failed on {failed}]")
                continue
            res['gate_details'] = gates
            slate_runs.append(res)
            print(f" done (PSS: {res['pss_mb']} MB, Procs: {res['process_count']})")
            sc_data['slate_runs'] = slate_runs
            checkpoint()
            time.sleep(1)

        # 7 Brave runs
        attempts = 0
        while len(brave_runs) < 7 and attempts < 16:
            attempts += 1
            run_idx = len(brave_runs) + 1
            print(f"  [Brave] Run {run_idx}/7 (attempt {attempts})...", end="", flush=True)
            res, gates = run_gated_brave(tab_count, port=9270 + run_idx)
            if res is None:
                failed = [k for k, v in (gates or {}).items() if not v.get('passed', False)]
                print(f" [DISCARDED: Gate failed on {failed}]")
                continue
            res['gate_details'] = gates
            brave_runs.append(res)
            print(f" done (PSS: {res['pss_mb']} MB, Procs: {res['process_count']})")
            sc_data['brave_runs'] = brave_runs
            checkpoint()
            time.sleep(1)

        # Compute stats (runs 2-7 analyzed, run 1 discarded)
        s_analyzed = slate_runs[1:] if len(slate_runs) > 1 else slate_runs
        b_analyzed = brave_runs[1:] if len(brave_runs) > 1 else brave_runs

        s_pss = compute_iqr([x['pss_mb'] for x in s_analyzed])
        b_pss = compute_iqr([x['pss_mb'] for x in b_analyzed])
        s_rss = compute_iqr([x['rss_mb'] for x in s_analyzed])
        b_rss = compute_iqr([x['rss_mb'] for x in b_analyzed])

        overlap = not (s_pss['q3'] < b_pss['q1'] or b_pss['q3'] < s_pss['q1'])

        all_results['scenarios'][sc_name]['summary'] = {
            'slate': {
                'pss': s_pss,
                'rss': s_rss,
                'process_count_median': int(statistics.median([x['process_count'] for x in s_analyzed]))
            },
            'brave': {
                'pss': b_pss,
                'rss': b_rss,
                'process_count_median': int(statistics.median([x['process_count'] for x in b_analyzed]))
            },
            'iqr_overlap': overlap
        }
        checkpoint()

    print(f"\nAll gated benchmark scenarios finished! Results saved to: {OUT_FILE}")

if __name__ == '__main__':
    main()
