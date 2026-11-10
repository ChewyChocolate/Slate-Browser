#!/usr/bin/env python3
"""
Compares exact blocked URLs per site: Slate vs Brave (seeded Shields profile).
"""

import os, sys, time, json, subprocess, tempfile, shutil, urllib.request, socket

ROOT_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
CEF_BIN = os.path.join(ROOT_DIR, 'target/release/slate-spike-cef')

SITES = [
    ("01_youtube", "https://www.youtube.com/watch?v=jNQXAC9IVRw"),
    ("02_google_docs", "https://docs.google.com/document/d/1hXalsO6oixyfHXQIo9C6iozYBHcXmDL_/preview"),
    ("03_hacker_news", "https://news.ycombinator.com"),
    ("04_wikipedia", "https://en.wikipedia.org/wiki/Rust_(programming_language)"),
    ("05_slashdot", "https://slashdot.org"),
    ("06_bbc_news", "https://www.bbc.com/news"),
    ("07_openstreetmap", "https://www.openstreetmap.org"),
    ("08_github", "https://github.com/rust-lang/rust"),
    ("09_chase", "https://www.chase.com"),
    ("10_theverge", "https://www.theverge.com")
]

def get_brave_blocked_for_site(url):
    prof = tempfile.mkdtemp(prefix='brave-block-')
    os.makedirs(os.path.join(prof, 'Default'), exist_ok=True)
    cache_src = '/home/earl/.config/BraveSoftware/Brave-Browser/Default/adblock_cache'
    if os.path.exists(cache_src):
        shutil.copytree(cache_src, os.path.join(prof, 'Default/adblock_cache'))

    cmd = [
        '/usr/bin/brave',
        f'--user-data-dir={prof}',
        '--remote-debugging-port=9222',
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-sync',
        'about:blank'
    ]
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2)

    tabs = json.loads(urllib.request.urlopen('http://127.0.0.1:9222/json').read().decode())
    ws_url = [t['webSocketDebuggerUrl'] for t in tabs if t.get('type') == 'page'][0]

    host_port = ws_url.split('/')[2]
    host, port = host_port.split(':')
    path = '/' + '/'.join(ws_url.split('/')[3:])
    s = socket.create_connection((host, int(port)), timeout=5)
    s.sendall(f'GET {path} HTTP/1.1\r\nHost: {host_port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n'.encode())
    s.recv(4096)

    def ws_send(text):
        payload = text.encode('utf-8')
        h = bytearray([0x81, 0x80 | (len(payload) if len(payload) <= 125 else 126)])
        if len(payload) > 125:
            import struct
            h.extend(struct.pack('!H', len(payload)))
        h.extend(b'\x12\x34\x56\x78')
        m = bytearray(payload)
        for i in range(len(m)): m[i] ^= b'\x12\x34\x56\x78'[i%4]
        s.sendall(h + m)

    def ws_recv():
        s.settimeout(2.0)
        try:
            h = s.recv(2)
            if not h or len(h) < 2: return None
            length = h[1] & 0x7f
            if length == 126:
                import struct
                length = struct.unpack('!H', s.recv(2))[0]
            m = s.recv(4) if (h[1] & 0x80) else None
            buf = bytearray()
            while len(buf) < length:
                chunk = s.recv(length - len(buf))
                if not chunk: break
                buf.extend(chunk)
            if m:
                for i in range(len(buf)): buf[i] ^= m[i%4]
            return buf.decode('utf-8', errors='ignore')
        except Exception:
            return None

    ws_send(json.dumps({'id': 1, 'method': 'Network.enable', 'params': {}}))
    time.sleep(0.3)
    ws_send(json.dumps({'id': 2, 'method': 'Page.navigate', 'params': {'url': url}}))

    reqs = {}
    blocked = []
    t0 = time.time()
    while time.time() - t0 < 8:
        msg = ws_recv()
        if not msg: continue
        try:
            d = json.loads(msg)
            m = d.get('method')
            if m == 'Network.requestWillBeSent':
                rid = d.get('params', {}).get('requestId')
                req_url = d.get('params', {}).get('request', {}).get('url')
                if rid and req_url: reqs[rid] = req_url
            elif m == 'Network.loadingFailed':
                rid = d.get('params', {}).get('requestId')
                err = d.get('params', {}).get('errorText', '')
                reason = d.get('params', {}).get('blockedReason', '')
                if 'BLOCKED_BY_CLIENT' in err or reason:
                    if rid in reqs: blocked.append(reqs[rid])
        except Exception:
            pass

    p.terminate()
    p.wait()

    # Read Brave's stats from Preferences if available
    pref_blocked = None
    pref_file = os.path.join(prof, 'Default/Preferences')
    if os.path.exists(pref_file):
        try:
            with open(pref_file) as f:
                pref_data = json.load(f)
            pref_blocked = int(pref_data.get('brave', {}).get('stats', {}).get('ads_blocked', 0))
        except Exception: pass

    subprocess.run(f"pkill -9 -f '{prof}' >/dev/null 2>&1 || true", shell=True)
    subprocess.run(['rm', '-rf', prof], check=False)

    return {
        'total_requests': len(reqs),
        'blocked_count': len(blocked),
        'blocked_sample': blocked[:5],
        'brave_pref_ads_blocked': pref_blocked
    }

def get_slate_blocked_for_site(url):
    env = os.environ.copy()
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = os.path.join(ROOT_DIR, 'target/release')
    env['WAYLAND_DISPLAY'] = 'wayland-1'

    cmd = [
        CEF_BIN,
        '--url', url,
        '--hold-ms', '8000',
        '--allow-autoplay',
        '--opt-level', '3'
    ]
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    out, _ = p.communicate()
    for line in out.splitlines():
        if line.startswith('{') and 'evaluated_requests' in line:
            try:
                d = json.loads(line)
                return {
                    'evaluated_requests': d.get('evaluated_requests', 0),
                    'blocked_requests': d.get('blocked_requests', 0),
                }
            except Exception: pass
    return {'evaluated_requests': 0, 'blocked_requests': 0}

def main():
    print("================================================================================")
    print(" PER-SITE BLOCKED REQUEST AUDIT: SLATE vs BRAVE (SEEDED)")
    print("================================================================================")
    results = {}
    for slug, url in SITES:
        print(f"\nSite: {slug} ({url})")
        s_data = get_slate_blocked_for_site(url)
        print(f"  Slate -> Evaluated: {s_data['evaluated_requests']}, Blocked: {s_data['blocked_requests']}")
        b_data = get_brave_blocked_for_site(url)
        print(f"  Brave -> Evaluated: {b_data['total_requests']}, Blocked: {b_data['blocked_count']} (Pref counter: {b_data['brave_pref_ads_blocked']})")
        if b_data['blocked_sample']:
            print(f"  Brave sample blocked: {b_data['blocked_sample'][:2]}")
        results[slug] = {
            'url': url,
            'slate': s_data,
            'brave': b_data
        }

    out_file = os.path.join(ROOT_DIR, 'benchmarks/results/realworld/per_site_blocking_comparison.json')
    with open(out_file, 'w') as f:
        json.dump(results, f, indent=2)
    print(f"\nDetailed per-site comparison written to: {out_file}")

if __name__ == '__main__':
    main()
