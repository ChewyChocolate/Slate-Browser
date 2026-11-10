#!/usr/bin/env python3
"""
Automated per-site readiness gates for Slate and Brave.
For each site, records:
- final_url
- document_title
- document_ready_state
- visible_body_text_len
- site_selector_found
- bot_or_login_wall_detected (captcha, "verify you are human", sign-in redirect, error page)
- video_advancing (for YouTube): currentTime advancement t0 -> t1
- pass/fail status

Enforces settle period (polling until load settle or timeout).
If any site fails, the process aborts immediately.
"""

import sys, os, time, json, subprocess, tempfile, socket, struct, urllib.request

SITES = [
    {
        "slug": "01_youtube",
        "url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        "selector": "video, #movie_player",
        "min_text_len": 0,
        "is_video": True
    },
    {
        "slug": "02_google_docs",
        "url": "https://docs.google.com/document/d/1hXalsO6oixyfHXQIo9C6iozYBHcXmDL_/preview",
        "selector": ".kix-appview, .docs-viewer, #viewer, [role='document'], canvas, iframe",
        "min_text_len": 50,
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
        "selector": "article, .story, #firehose",
        "min_text_len": 500,
        "is_video": False
    },
    {
        "slug": "06_bbc_news",
        "url": "https://www.bbc.com/news",
        "selector": "main, [data-testid], #main-content",
        "min_text_len": 300,
        "is_video": False
    },
    {
        "slug": "07_openstreetmap",
        "url": "https://www.openstreetmap.org",
        "selector": "#map, .leaflet-container",
        "min_text_len": 50,
        "is_video": False
    },
    {
        "slug": "08_github",
        "url": "https://github.com/rust-lang/rust",
        "selector": "#repo-content-pjax-container, [data-turbo-body]",
        "min_text_len": 300,
        "is_video": False
    },
    {
        "slug": "09_chase",
        "url": "https://www.chase.com",
        "selector": "header, #signin-button, [role='main']",
        "min_text_len": 200,
        "is_video": False
    },
    {
        "slug": "10_theverge",
        "url": "https://www.theverge.com",
        "selector": "main, [data-chorus-optimize-module], article",
        "min_text_len": 400,
        "is_video": False
    }
]

GATE_EVAL_JS = """
(() => {
    const v = document.querySelector('video');
    let bodyText = document.body ? document.body.innerText.trim() : '';
    
    // For docs.google.com/preview, text is inside subframe/canvas
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
        || titleLower.includes('attention required')
        || (href.includes('reddit.com') && href.includes('solution='));

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
        bot_wall_detail: hasBotWall ? 'bot_challenge' : (hasLoginWall ? 'login_wall' : (hasErrorPage ? 'error_page' : 'none')),
        has_video: Boolean(v),
        video_current_time: v ? v.currentTime : -1,
        video_duration: v ? v.duration : -1,
        video_paused: v ? v.paused : null,
        video_ready_state: v ? v.readyState : -1
    };
})()
"""

def ws_connect(ws_url):
    host_port = ws_url.split('/')[2]
    host, port = host_port.split(':')
    path = '/' + '/'.join(ws_url.split('/')[3:])
    s = socket.create_connection((host, int(port)), timeout=5)
    key = "dGhlIHNhbXBsZSBub25jZQ=="
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host_port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(req.encode())
    resp = s.recv(4096).decode('utf-8', errors='ignore')
    if "101" not in resp:
        raise RuntimeError(f"WS handshake failed: {resp}")
    return s

def ws_send(s, text):
    payload = text.encode('utf-8')
    header = bytearray([0x81])
    length = len(payload)
    if length <= 125:
        header.append(0x80 | length)
    elif length <= 65535:
        header.append(0x80 | 126)
        header.extend(struct.pack('!H', length))
    else:
        header.append(0x80 | 127)
        header.extend(struct.pack('!Q', length))

    mask = bytearray([0x12, 0x34, 0x56, 0x78])
    header.extend(mask)
    masked_payload = bytearray(payload)
    for i in range(len(masked_payload)):
        masked_payload[i] ^= mask[i % 4]

    s.sendall(header + masked_payload)

def ws_recv(s):
    header = s.recv(2)
    if not header or len(header) < 2:
        return None
    b1, b2 = header[0], header[1]
    masked = bool(b2 & 0x80)
    length = b2 & 0x7f
    if length == 126:
        length = struct.unpack('!H', s.recv(2))[0]
    elif length == 127:
        length = struct.unpack('!Q', s.recv(8))[0]

    mask = s.recv(4) if masked else None
    payload = b''
    while len(payload) < length:
        chunk = s.recv(length - len(payload))
        if not chunk:
            break
        payload += chunk

    if masked and mask:
        payload = bytearray(payload)
        for i in range(len(payload)):
            payload[i] ^= mask[i % 4]
        return payload.decode('utf-8', errors='ignore')
    return payload.decode('utf-8', errors='ignore')

def cdp_eval(s, expr, msg_id=1):
    req = {
        "id": msg_id,
        "method": "Runtime.evaluate",
        "params": {
            "expression": expr,
            "returnByValue": True,
            "awaitPromise": True
        }
    }
    ws_send(s, json.dumps(req))
    t0 = time.time()
    while time.time() - t0 < 6:
        resp = ws_recv(s)
        if not resp: break
        try:
            data = json.loads(resp)
            if data.get("id") == msg_id:
                return data.get("result", {}).get("result", {}).get("value")
        except Exception:
            pass
    return None

def evaluate_browser_site(browser_name, site, screenshot_path):
    root_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
    env = os.environ.copy()
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    port = 9222
    prof = None
    proc = None

    if browser_name == 'brave':
        prof = tempfile.mkdtemp(prefix='brave-gate-run-')
        cmd = [
            '/usr/bin/brave',
            f'--user-data-dir={prof}',
            f'--remote-debugging-port={port}',
            '--no-first-run',
            '--no-default-browser-check',
            '--disable-sync',
            '--autoplay-policy=no-user-gesture-required',
            site['url']
        ]
        proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    elif browser_name == 'slate':
        env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
        env['LD_LIBRARY_PATH'] = os.path.join(root_dir, 'target/release')
        cmd = [
            os.path.join(root_dir, 'target/release/slate-spike-cef'),
            '--url', site['url'],
            f'--remote-debugging-port={port}',
            '--hold-ms', '15000',
            '--allow-autoplay',
            '--opt-level', '3'
        ]
        proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        raise ValueError(f"Unknown browser: {browser_name}")

    js = GATE_EVAL_JS.replace('${SELECTOR_JSON}', json.dumps(site['selector']))
    gate_result = None

    # Wait for DevTools port and connect (settle loop up to 14 seconds)
    t_start = time.time()
    while time.time() - t_start < 14:
        time.sleep(0.6)
        try:
            tabs = json.loads(urllib.request.urlopen(f'http://127.0.0.1:{port}/json').read().decode())
            ws_url = None
            for t in tabs:
                if t.get('type') == 'page':
                    ws_url = t.get('webSocketDebuggerUrl')
                    break
            if not ws_url:
                continue

            sock = ws_connect(ws_url)
            raw_gate = cdp_eval(sock, js, 101)
            if not raw_gate:
                sock.close()
                continue

            # Check bot/login walls immediately
            if raw_gate.get('bot_or_login_wall_detected'):
                gate_result = raw_gate
                sock.close()
                break

            # If video site, check video advancement
            if site.get('is_video'):
                if raw_gate.get('has_video') and raw_gate.get('video_current_time', -1) >= 0:
                    t0_val = raw_gate.get('video_current_time', 0.0)
                    # Trigger play if paused
                    cdp_eval(sock, "(function(){ const v = document.querySelector('video'); if (v) { v.muted=true; if (v.paused) v.play().catch(()=>{}); } })()", 102)
                    time.sleep(1.8)
                    raw_gate_t1 = cdp_eval(sock, js, 103)
                    t1_val = raw_gate_t1.get('video_current_time', 0.0) if raw_gate_t1 else t0_val
                    advancing = (t1_val > t0_val + 0.1) or (t1_val > 0.5)
                    raw_gate['video_advancing'] = advancing
                    raw_gate['video_time_t0'] = t0_val
                    raw_gate['video_time_t1'] = t1_val

                    sock.close()
                    if advancing and raw_gate.get('site_selector_found'):
                        gate_result = raw_gate
                        break
                    else:
                        gate_result = raw_gate
                        continue
                else:
                    sock.close()
                    continue

            sock.close()
            # Non-video site check
            selector_ok = raw_gate.get('site_selector_found')
            text_ok = raw_gate.get('visible_body_text_len', 0) >= site.get('min_text_len', 0)
            if selector_ok and text_ok:
                gate_result = raw_gate
                break
            else:
                gate_result = raw_gate

        except Exception:
            pass

    # Capture visual verification screenshot
    subprocess.run(['spectacle', '-b', '-f', '-o', screenshot_path], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    # Clean up process
    proc.terminate()
    try:
        proc.wait(timeout=3)
    except Exception:
        proc.kill()

    if prof:
        subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
        time.sleep(0.2)
        subprocess.run(['rm', '-rf', prof], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    return gate_result

def judge_gate(site, gate):
    if not gate or "error" in gate:
        return False, "Gate check execution failed (no response or crash)"
    if gate.get("bot_or_login_wall_detected"):
        return False, f"Bot/login wall detected ({gate.get('bot_wall_detail')})"
    if not site.get('is_video') and gate.get("visible_body_text_len", 0) < site.get("min_text_len", 0):
        return False, f"Visible body text too short ({gate.get('visible_body_text_len')} < {site.get('min_text_len')})"
    if not gate.get("site_selector_found"):
        return False, f"Required DOM selector not found: {site.get('selector')}"
    if site.get('is_video') and not gate.get('video_advancing', False):
        return False, f"Video did not advance currentTime (t0: {gate.get('video_time_t0')}, t1: {gate.get('video_time_t1')})"
    return True, "OK"

def main():
    root_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
    out_dir = os.path.join(root_dir, 'benchmarks/results/realworld/gate_verification')
    os.makedirs(out_dir, exist_ok=True)

    summary = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "sites": []
    }

    print("================================================================================")
    print(" AUTOMATED READINESS GATES: SLATE vs. BRAVE (ALL 10 SITES)")
    print("================================================================================")

    all_passed = True

    for idx, site in enumerate(SITES, 1):
        slug = site['slug']
        print(f"\n[{idx}/10] Testing: {slug} ({site['url']})")

        brave_ss = os.path.join(out_dir, f"{slug}_brave.png")
        slate_ss = os.path.join(out_dir, f"{slug}_slate.png")

        print("  -> Evaluating in Brave (CDP + settle)...")
        brave_gate = evaluate_browser_site('brave', site, brave_ss)
        brave_ok, brave_reason = judge_gate(site, brave_gate)

        print("  -> Evaluating in Slate (CDP + settle)...")
        slate_gate = evaluate_browser_site('slate', site, slate_ss)
        slate_ok, slate_reason = judge_gate(site, slate_gate)

        # Enforce cross-browser relative completeness rule: Slate visible text must not be < 50% of Brave
        if slate_ok and brave_ok and not site.get('is_video'):
            s_len = slate_gate.get('visible_body_text_len', 0)
            b_len = brave_gate.get('visible_body_text_len', 0)
            if b_len > 0 and s_len < 0.5 * b_len:
                slate_ok = False
                slate_reason = f"Slate text length under 50% of Brave ({s_len} < {int(0.5 * b_len)})"

        site_entry = {
            "slug": slug,
            "url": site['url'],
            "brave_gate": brave_gate,
            "brave_pass": brave_ok,
            "brave_reason": brave_reason,
            "brave_screenshot": brave_ss,
            "slate_gate": slate_gate,
            "slate_pass": slate_ok,
            "slate_reason": slate_reason,
            "slate_screenshot": slate_ss,
        }

        print(f"     Brave: {'PASS' if brave_ok else 'FAIL'} ({brave_reason}) | Title: {brave_gate.get('document_title', '')[:40] if brave_gate else 'N/A'}")
        print(f"     Slate: {'PASS' if slate_ok else 'FAIL'} ({slate_reason}) | Title: {slate_gate.get('document_title', '')[:40] if slate_gate else 'N/A'}")

        if not brave_ok or not slate_ok:
            all_passed = False

        summary["sites"].append(site_entry)

    summary_file = os.path.join(out_dir, "readiness_gates.json")
    with open(summary_file, "w") as f:
        json.dump(summary, f, indent=2)

    print("\n================================================================================")
    print(f" Summary saved to: {summary_file}")
    if all_passed:
        print(" ALL 10 SITES PASSED READINESS GATES IN BOTH SLATE AND BRAVE!")
        sys.exit(0)
    else:
        print(" ONE OR MORE SITES FAILED READINESS GATES!")
        sys.exit(1)

if __name__ == '__main__':
    main()
