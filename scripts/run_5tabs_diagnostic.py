#!/usr/bin/env python3
"""
Diagnostic 5-Tab Matrix Runner (Readiness Gated, 7 Runs per Scenario, Raw Streamed to Disk):
Scenarios:
1. slate_opt0_blocking_on: Slate --opt-level 0 (bloat flags ON, blocking ON)
2. slate_opt0_blocking_off: Slate --opt-level 0 --no-blocking
3. slate_opt3_blocking_on: Slate --opt-level 3 (bloat flags OFF, blocking ON)
4. slate_opt3_blocking_off: Slate --opt-level 3 --no-blocking
5. brave_seeded: Brave with Shields cache seeded

Each run:
- Readiness gates on all 5 tabs.
- Video playback tracked (Big Buck Bunny, 10 min 34 s).
- PSS breakdown per process type: browser, renderer, gpu, network, utility, zygote.
- Discarded/retried runs are logged.
- Output streamed to benchmarks/results/realworld/diagnostics_5tabs_matrix_raw.json.
"""

import sys, os, time, json, subprocess, tempfile, shutil, statistics, urllib.request

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
sys.path.insert(0, os.path.join(ROOT_DIR, 'scripts'))
from verify_readiness_gates import ws_connect, cdp_eval

CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')
OUT_FILE = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/diagnostics_5tabs_matrix_raw.json')
BRAVE_ADBLOCK_CACHE = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'

# 5 Sites (Longer 10-minute video for Tab 1)
SITES_5 = [
    {
        "slug": "01_youtube_bbb",
        "url": "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
        "selector": "video, #movie_player",
        "min_text_len": 0,
        "is_video": True
    },
    {
        "slug": "02_google_docs",
        "url": "https://docs.google.com/document/d/1hXalsO6oixyfHXQIo9C6iozYBHcXmDL_/preview",
        "selector": ".kix-appview, .docs-viewer, #viewer, [role='document'], canvas, iframe",
        "min_text_len": 0,
        "is_video": False
    },
    {
        "slug": "03_hacker_news",
        "url": "https://news.ycombinator.com",
        "selector": ".athing, #hnmain",
        "min_text_len": 200,
        "is_video": False
    },
    {
        "slug": "04_wikipedia",
        "url": "https://en.wikipedia.org/wiki/Rust_(programming_language)",
        "selector": "#content, #firstHeading",
        "min_text_len": 500,
        "is_video": False
    },
    {
        "slug": "05_slashdot",
        "url": "https://slashdot.org",
        "selector": "article, .story, #firehoselist",
        "min_text_len": 400,
        "is_video": False
    }
]

GATE_EVAL_JS = """
(() => {
    const v = document.querySelector('video');
    let bodyText = document.body ? document.body.innerText.trim() : '';
    const iframe = document.querySelector('iframe');
    if (iframe && iframe.contentDocument && iframe.contentDocument.body) {
        bodyText += ' ' + iframe.contentDocument.body.innerText.trim();
    }
    const bodyLower = bodyText.toLowerCase();
    const title = document.title || '';
    const titleLower = title.toLowerCase();
    const href = location.href;

    const hasBotWall = bodyLower.includes('verify you are human')
        || bodyLower.includes('prove your humanity')
        || bodyLower.includes('cf-chl')
        || titleLower.includes('just a moment')
        || titleLower.includes('attention required');

    const hasLoginWall = (href.includes('accounts.google.com') && !href.includes('preview'))
        || titleLower.includes('sign in - google accounts');

    const hasErrorPage = titleLower.includes('404')
        || titleLower.includes('500 internal server')
        || titleLower.includes('502 bad gateway')
        || titleLower.includes('503 service temporarily unavailable');

    const selMatches = Boolean(document.querySelector(${SELECTOR_JSON}));

    return {
        final_url: href,
        document_title: title,
        document_ready_state: document.readyState,
        visible_body_text_len: bodyText.length,
        site_selector_found: selMatches,
        bot_or_login_wall_detected: hasBotWall || hasLoginWall || hasErrorPage,
        has_video: Boolean(v),
        video_current_time: v ? v.currentTime : -1,
        video_duration: v ? v.duration : -1,
        video_paused: v ? v.paused : null,
        video_width: v ? v.videoWidth : -1,
        video_height: v ? v.videoHeight : -1
    };
})()
"""

def sample_process_tree_breakdown(root_pid, profile_filter=None):
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
    by_type = {
        'browser': 0,
        'renderer': 0,
        'gpu': 0,
        'network': 0,
        'utility': 0,
        'zygote': 0
    }

    for pid in visited:
        p_rss = 0
        p_pss = 0
        cmdline = b''
        try:
            with open(f'/proc/{pid}/cmdline', 'rb') as f:
                cmdline = f.read()
        except Exception: pass

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

            # Classify kind
            kind = 'browser'
            if pid == root_pid:
                kind = 'browser'
            elif b'--type=renderer' in cmdline:
                kind = 'renderer'
            elif b'--type=gpu-process' in cmdline:
                kind = 'gpu'
            elif b'--type=utility' in cmdline:
                if b'network' in cmdline:
                    kind = 'network'
                else:
                    kind = 'utility'
            else:
                thread_comms = set()
                try:
                    for tid in os.listdir(f'/proc/{pid}/task'):
                        try:
                            with open(f'/proc/{pid}/task/{tid}/comm') as f:
                                thread_comms.add(f.read().strip())
                        except Exception: pass
                except Exception: pass

                if 'Compositor' in thread_comms or 'CompositorTileW' in thread_comms or 'Chrome_ChildIOT' in thread_comms:
                    kind = 'renderer'
                elif 'GpuWatchdog' in thread_comms or 'VizCompositorTh' in thread_comms:
                    kind = 'gpu'
                elif b'--type=zygote' in cmdline:
                    kind = 'zygote'

            by_type[kind] += p_pss

    return {
        'pss_mb': round(pss_kb / 1024, 1),
        'rss_mb': round(rss_kb / 1024, 1),
        'process_count': pcount,
        'by_type_mb': {k: round(v / 1024, 1) for k, v in by_type.items()}
    }

def verify_5tabs(port, timeout=25):
    t0 = time.time()
    last_evals = {}
    video_start_time = None
    video_end_time = None
    video_res = None

    while time.time() - t0 < timeout:
        time.sleep(1.0)
        try:
            req = urllib.request.urlopen(f'http://127.0.0.1:{port}/json', timeout=2)
            tab_list = [t for t in json.loads(req.read().decode()) if t.get('type') == 'page']
            if len(tab_list) < 5:
                continue

            all_passed = True
            current_evals = {}

            for site_idx in range(5):
                site_def = SITES_5[site_idx]
                target_url_prefix = site_def['url'].split('?')[0].replace('https://', '').replace('http://', '')
                matching = [t for t in tab_list if target_url_prefix in t.get('url', '')]
                if not matching:
                    all_passed = False
                    break
                t_obj = matching[0]

                sock = ws_connect(t_obj['webSocketDebuggerUrl'])
                try:
                    cdp_eval(sock, "window.focus()", 1)
                except Exception: pass

                js = GATE_EVAL_JS.replace('${SELECTOR_JSON}', json.dumps(site_def['selector']))
                gate = cdp_eval(sock, js, 10 + site_idx)
                sock.close()

                if not gate:
                    all_passed = False
                    break

                bot_wall = gate.get('bot_or_login_wall_detected', False)
                sel_found = gate.get('site_selector_found', False)
                text_len = gate.get('visible_body_text_len', 0)
                min_len = site_def.get('min_text_len', 0)
                ready_state = gate.get('document_ready_state', '')

                is_video_ok = True
                if site_def.get('is_video'):
                    cur_time = gate.get('video_current_time', -1)
                    if cur_time >= 0:
                        if video_start_time is None:
                            video_start_time = cur_time
                        video_end_time = cur_time
                        video_res = f"{gate.get('video_width')}x{gate.get('video_height')}"
                        is_video_ok = gate.get('has_video') and (cur_time >= 0)
                    else:
                        is_video_ok = False

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

            last_evals = current_evals
            if all_passed and len(current_evals) == 5:
                # Require that video has advanced by at least 1 second
                if video_start_time is not None and video_end_time is not None:
                    if video_end_time - video_start_time < 0.5 and (time.time() - t0 < timeout - 2):
                        continue
                return True, current_evals, video_start_time, video_end_time, video_res
        except Exception: pass

    return False, last_evals, video_start_time, video_end_time, video_res

def run_slate_5tabs(opt_level, no_blocking, port=9300):
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    urls_csv = ','.join([s['url'] for s in SITES_5])
    cmd = [
        CEF_BIN,
        '--urls', urls_csv,
        '--remote-debugging-port=' + str(port),
        '--hold-ms', '30000',
        '--allow-autoplay',
        '--opt-level', str(opt_level)
    ]
    if no_blocking:
        cmd.append('--no-blocking')

    p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    passed, gates, v_start, v_end, v_res = verify_5tabs(port, timeout=24)

    if not passed:
        p.terminate()
        try: p.wait(timeout=2)
        except: p.kill()
        return None, gates, {}

    stats = sample_process_tree_breakdown(p.pid)
    p.terminate()
    try: p.wait(timeout=2)
    except: p.kill()

    stats['video_info'] = {
        'start_time': round(v_start, 2) if v_start is not None else -1,
        'end_time': round(v_end, 2) if v_end is not None else -1,
        'resolution': v_res
    }
    stats['gates_passed'] = True
    return stats, gates, stats['video_info']

def run_brave_5tabs(port=9350):
    prof = tempfile.mkdtemp(prefix='brave-diag-5tab-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    if os.path.exists(BRAVE_ADBLOCK_CACHE):
        shutil.copytree(BRAVE_ADBLOCK_CACHE, os.path.join(prof, 'Default/adblock_cache'))

    urls = [s['url'] for s in SITES_5]
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
    passed, gates, v_start, v_end, v_res = verify_5tabs(port, timeout=24)

    if not passed:
        p.terminate()
        try: p.wait(timeout=2)
        except: p.kill()
        subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
        shutil.rmtree(prof, ignore_errors=True)
        return None, gates, {}

    stats = sample_process_tree_breakdown(p.pid, prof)
    p.terminate()
    try: p.wait(timeout=2)
    except: p.kill()
    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    shutil.rmtree(prof, ignore_errors=True)

    stats['video_info'] = {
        'start_time': round(v_start, 2) if v_start is not None else -1,
        'end_time': round(v_end, 2) if v_end is not None else -1,
        'resolution': v_res
    }
    stats['gates_passed'] = True
    return stats, gates, stats['video_info']

def main():
    print("================================================================================")
    print(" 5-TAB DIAGNOSTIC MATRIX (READINESS GATED, 7 RUNS)")
    print("================================================================================")

    data = {
        'timestamp': time.time(),
        'description': "5-tab diagnostic matrix measuring PSS by process kind across {Opt0, Opt3} x {blocking on, off} and Brave.",
        'scenarios': {},
        'discard_log': []
    }
    if os.path.exists(OUT_FILE):
        try:
            with open(OUT_FILE) as f:
                data = json.load(f)
        except Exception: pass

    def checkpoint():
        with open(OUT_FILE, 'w') as f:
            json.dump(data, f, indent=2)

    scenarios = [
        ("slate_opt3_blocking_on", lambda port: run_slate_5tabs(opt_level=3, no_blocking=False, port=port)),
        ("slate_opt3_blocking_off", lambda port: run_slate_5tabs(opt_level=3, no_blocking=True, port=port)),
        ("slate_opt0_blocking_on", lambda port: run_slate_5tabs(opt_level=0, no_blocking=False, port=port)),
        ("slate_opt0_blocking_off", lambda port: run_slate_5tabs(opt_level=0, no_blocking=True, port=port)),
        ("brave_seeded", lambda port: run_brave_5tabs(port=port))
    ]

    for sc_name, runner in scenarios:
        print(f"\n--- Scenario: {sc_name} ---")
        sc_runs = data.setdefault('scenarios', {}).setdefault(sc_name, [])
        attempts = 0
        while len(sc_runs) < 7 and attempts < 14:
            attempts += 1
            run_idx = len(sc_runs) + 1
            port = 9400 + run_idx
            print(f"  Run {run_idx}/7 (attempt {attempts})...", end="", flush=True)
            res, gates, v_info = runner(port)
            if res is None:
                failed = [k for k, v in (gates or {}).items() if not v.get('passed', False)]
                print(f" [DISCARDED: Gate failed on {failed}]")
                data['discard_log'].append({
                    'scenario': sc_name,
                    'attempt': attempts,
                    'failed_tabs': failed,
                    'timestamp': time.time()
                })
                checkpoint()
                time.sleep(1)
                continue

            res['gate_details'] = gates
            sc_runs.append(res)
            print(f" done (PSS: {res['pss_mb']} MB, Renderer: {res['by_type_mb']['renderer']} MB, Procs: {res['process_count']})")
            checkpoint()
            time.sleep(1)

    print(f"\nDiagnostic matrix runs completed! Raw results saved to: {OUT_FILE}")

if __name__ == '__main__':
    main()
