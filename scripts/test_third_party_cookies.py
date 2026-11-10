#!/usr/bin/env python3
"""Live test: a third-party iframe's cookie must be judged against the TAB's top-level site.

Runs `slate-spike-cef --test-third-party-cookies`, which serves a top page on host `localhost`
embedding an iframe from host `127.0.0.1` (a different site). Asserts, from the raw output:
  1. the filter saw the third-party Set-Cookie with top_url_used on host localhost (not the iframe's own URL),
  2. that third-party cookie save was refused (allowed == false),
  3. the first-party cookie control was allowed,
  4. the server never received the third-party cookie back (independent evidence from the server log).
"""
import json, os, subprocess, sys
from urllib.parse import urlparse

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
env = dict(os.environ, LD_LIBRARY_PATH=f"{root}/target/release:" + os.environ.get("LD_LIBRARY_PATH", ""))
res = subprocess.run([f"{root}/target/release/slate-spike-cef", "--test-third-party-cookies"],
                     capture_output=True, text=True, env=env, timeout=90)
if res.returncode != 0:
    print(f"FAIL: runner exit code {res.returncode}\n{res.stdout}\n{res.stderr}")
    sys.exit(1)
line = [l for l in res.stdout.splitlines() if l.startswith("{")][-1]
print("raw:", line)
d = json.loads(line)
dec = d["cookie_decisions"]
host = lambda u: urlparse(u).hostname
errors = []
tp_save = [x for x in dec if x["kind"] == "save" and host(x["request_url"]) == "127.0.0.1"]
fp_save = [x for x in dec if x["kind"] == "save" and host(x["request_url"]) == "localhost"]
if not tp_save:
    errors.append("no 'save' decision recorded for the third-party (127.0.0.1) cookie")
for x in tp_save:
    if host(x["top_url_used"]) != "localhost":
        errors.append(f"top-level site wrong: filter used {x['top_url_used']!r}, expected host localhost")
    if x["allowed"]:
        errors.append("third-party cookie save was ALLOWED")
if not fp_save:
    errors.append("no 'save' decision recorded for the first-party (localhost) cookie control")
for x in fp_save:
    if not x["allowed"]:
        errors.append("first-party cookie was refused (control failed)")
pix = [r for r in d["server_requests"] if r["path"].startswith("/pixel")]
if not pix:
    errors.append("server never saw /pixel (iframe did not load/run), test inconclusive")
for r in pix:
    if "tp=1" in r["cookie_header"]:
        errors.append("server received the third-party cookie back on /pixel")
if errors:
    print("FAIL:\n  - " + "\n  - ".join(errors))
    sys.exit(1)
print("PASS: third-party cookie judged against the tab's top-level site and refused; first-party allowed")
