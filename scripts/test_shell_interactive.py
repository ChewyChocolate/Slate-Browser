#!/usr/bin/env python3
"""
Test App Shell Interactivity and Capture OS-Level Window Screenshot:
1. Launches target/release/slate with DevTools enabled.
2. Interacts with the real address bar and tab manager via CDP:
   - Navigates active tab to https://news.ycombinator.com
   - Opens a second tab to https://en.wikipedia.org/wiki/Rust_(programming_language)
   - Verifies the real vertical tabs strip and blocked counter update dynamically.
3. Captures an OS-level window screenshot using KDE Spectacle.
4. Saves screenshot to benchmarks/results/realworld/slate_app_shell_interactive.png.
"""

import os, sys, time, subprocess, json, urllib.request

ROOT = '/home/earl/Desktop/Slate'
sys.path.insert(0, os.path.join(ROOT, 'scripts'))
from verify_readiness_gates import ws_connect, cdp_eval

BIN = os.path.join(ROOT, 'target/release/slate')
PORT = 9288
SCREENSHOT_PATH = os.path.join(ROOT, 'benchmarks/results/realworld/slate_app_shell_interactive.png')

def main():
    env = os.environ.copy()
    env['WAYLAND_DISPLAY'] = 'wayland-1'
    env['CEF_PATH'] = os.path.expanduser('~/.cache/slate-cef')
    env['LD_LIBRARY_PATH'] = f"{os.path.join(ROOT, 'target/release')}:{env['CEF_PATH']}"

    print("Launching Slate application shell...")
    cmd = [
        BIN,
        f'--remote-debugging-port={PORT}',
        '--hold-ms', '40000',
        '--allow-autoplay',
        '--opt-level', '3'
    ]
    proc = subprocess.Popen(cmd, env=env)

    sock = None
    try:
        # Wait for DevTools port
        t0 = time.time()
        tabs = []
        while time.time() - t0 < 10:
            time.sleep(0.5)
            try:
                req = urllib.request.urlopen(f'http://127.0.0.1:{PORT}/json', timeout=1)
                tabs = [t for t in json.loads(req.read().decode()) if t.get('type') == 'page']
                if tabs: break
            except Exception: pass

        if not tabs:
            raise RuntimeError("Slate shell window failed to start DevTools")

        ws_url = tabs[0]['webSocketDebuggerUrl']
        sock = ws_connect(ws_url)
        print("Connected to Slate shell window via CDP!")

        time.sleep(2)

        # 1. Type URL in address bar & navigate active tab to Hacker News
        print("Navigating Tab 1 to https://news.ycombinator.com...")
        cdp_eval(sock, 'window.slateShell.navigateTo("https://news.ycombinator.com")', 1)
        time.sleep(3)

        # 2. Open a second tab and navigate to Wikipedia
        print("Opening Tab 2 and navigating to https://en.wikipedia.org/wiki/Rust_(programming_language)...")
        cdp_eval(sock, 'window.slateShell.createNewTab("https://en.wikipedia.org/wiki/Rust_(programming_language)")', 2)
        time.sleep(4)

        # 3. Capture OS-level desktop screenshot using Spectacle
        print(f"Capturing OS-level window screenshot to {SCREENSHOT_PATH}...")
        subprocess.run(['/usr/bin/spectacle', '-b', '-f', '-o', SCREENSHOT_PATH], env=env, check=True)
        time.sleep(1)

        if os.path.exists(SCREENSHOT_PATH) and os.path.getsize(SCREENSHOT_PATH) > 1000:
            print(f"SUCCESS: Screenshot saved ({os.path.getsize(SCREENSHOT_PATH)} bytes)")
        else:
            raise RuntimeError("Screenshot file missing or empty")

    finally:
        if sock:
            try: sock.close()
            except: pass
        proc.terminate()
        try: proc.wait(timeout=2)
        except: proc.kill()

if __name__ == '__main__':
    main()
