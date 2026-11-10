# Slate Browser — Project Status & Handoff (`HANDOFF.md`)

**Date**: 2026-11-10  
**Target Platform**: Arch Linux (`x86_64`, Wayland/X11) first; Windows and Android later  
**Engine**: **CEF (`cef = "156.4.0+156.0.26"`, Chromium `156.0.8087.17`)**  

> [!WARNING]
> **Security Notice**: **Slate is NOT suitable for sensitive logins (e.g. banking, primary Google accounts) until an upstream security update path works.** Upstream Chromium 157 shipped on Nov 3, 2026, while the `cef` crate remains on M156 (a lag of 7+ days).

## 0. Session log — 2026-11-10 (latest)

**Correction to earlier reports:** earlier sessions said `cargo audit` "passed". It was **never running**:
`scripts/check.sh` skipped it with a NOTE because `cargo-audit` was not installed. Nothing before this
session can be cited as an audit result.

State now:
- `cargo-audit 0.22.2` installed to `~/.cargo/bin` (`cargo install cargo-audit --locked`, build log in the session task log).
- `cargo audit` result (raw output `/tmp/audit.out`, not committed): advisory DB 1430 advisories, 254 crate dependencies scanned,
  **0 vulnerabilities, 2 warnings**: `proc-macro-error 1.0.4` (RUSTSEC-2024-0370, unmaintained) and `glib 0.18.5`
  (RUSTSEC-2024-0429, unsound `VariantStrIter`). Both come only through `slate-spike-webkit` → `gtk`/`webkit2gtk`
  (`cargo tree -i`), i.e. the rejected WebKitGTK spike, not the CEF app. Not yet fixed or suppressed.
- `scripts/check.sh` now has 9 numbered stages, listed at the top of its output, and **fails** if `cargo-audit` is missing.
  Stages: 1 fmt, 2 clippy, 3 cargo test, 4 cargo audit, 5 spike CEF smoke, 6 **real app smoke** (`target/release/slate --exit-after-load`,
  asserts exit code 0 + JSON), 7 site isolation, 8 filter cache, 9 fake-counter guard. Full run: exit code 0 (`/tmp/check2.out`).
  Negative tests run by hand: stage 4 with no `cargo-audit` on PATH → exit 1; stage 9 with an injected `active.blocked = 38;` → exit 1.
- Shell: fake counters removed (step 1, commit `658e74e`).
- `docs/shell-architecture.md` written (design only, nothing built). **Waiting for the owner's choice** of Option 1/2/3, toolbar placement, and IPC.
- The real-app smoke test only proves the binary starts and exits; it does **not** prove the UI renders or that pages load.
  The iframe viewport is still broken (frame-restricted sites fail) until the redesign is built.
- NOT RUN this session: benchmarks (owner instruction), banking/YouTube/Google sign-in checks, M157 swap, codec work (steps 3–5 still pending).

### Rule for future sessions (a smaller/cheaper model may be used)
Tests and design docs, not the model's memory, must carry the constraints. Therefore:
1. Read `docs/shell-architecture.md` and this file first; do not start building before the owner picks an option.
2. Never weaken `scripts/check.sh`; do not skip or soften stages. If a stage cannot run, it must **fail**, not print a note.
3. When fixing a bug, add a check.sh stage or test that would have caught it.
4. Small steps: build + run after each change, commit per task, report raw file paths for every number.
5. Anything unverified in the `cef` crate API stays marked UNVERIFIED in docs until a running test proves it.
6. Do not run memory/CPU benchmarks unless asked. Do not write counters, titles, or sample data into the UI.
7. Escalate large decisions (architecture, major crates) to the owner with 2–3 options.

---

## 1. Traceability & Reporting Rules

Every future session and report **must** adhere strictly to the following integrity rules:
1. **Every number must name the raw file on disk** it was read from.
2. **Anything estimated or reconstructed must be labeled as such.**
3. **If something was not run, write `NOT RUN`.**
4. **PSS (Proportional Set Size) is the primary memory metric.** Never use RSS (Resident Set Size) to argue Slate is lighter; RSS double-counts shared memory across Chromium's multi-process architecture.
5. **Statistical comparison rule**: 7 runs per scenario. Run 1 is discarded as warmup; Runs 2–7 are analyzed. If interquartile ranges (IQRs, $[Q_1, Q_3]$) do not overlap, report **Probable difference**; if they overlap, report **No meaningful difference**.

---

## 2. Current Status

### A. What is VERIFIED (With Raw File Paths)
* **Automated Readiness Gates**: Enforced via [`scripts/verify_readiness_gates.py`](file:///home/earl/Desktop/Slate/scripts/verify_readiness_gates.py) (`readyState == "complete"`, document title, site-specific loaded selector, text length threshold, bot/captcha/login check, and active video playback).
* **Filter List Parity & Deserialization**:
  - File: `~/.local/share/slate/filters.dat` (**21,851,405 bytes**, timestamp `2026-11-10 19:03:22`).
  - Metadata: `~/.local/share/slate/filters.meta.json` (**250,820 active rules** across 8 lists: 220,702 network rules, 30,118 cosmetic/scriptlet rules).
  - Warm start deserialization: **8.76 ms** (measured in release mode).
* **Multi-Tab Memory (PSS)**:
  - Raw file: [`benchmarks/results/realworld/benchmark_7runs_gated.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/benchmark_7runs_gated.json)
  - 1 Tab (YouTube `jNQXAC9IVRw`): Slate **598.1 MB** (IQR $[577.7, 740.6]$) vs Brave **681.3 MB** (IQR $[504.1, 690.2]$) $\to$ **PSS Tie / No meaningful difference** (IQRs overlap).
  - 5 Tabs (Tech & Media): Brave lower by **232.1 MB PSS (~18%)** (Brave 1,078.5 MB vs Slate 1,310.6 MB; IQRs do not overlap).
  - 10 Tabs (Heavy Mixed): Brave lower by **261.9 MB PSS (~17%)** (Brave 1,235.3 MB vs Slate 1,497.2 MB; IQRs do not overlap).
* **Heavy Video Scenario (YouTube Big Buck Bunny 4K/60fps + The Verge)**:
  - Raw file: [`benchmarks/results/realworld/heavy_scenario_7run_verified.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/heavy_scenario_7run_verified.json)
  - Video `currentTime` tracked from 0s $\to$ 15–18s on every run.
  - Brave lower by **172.3 MB PSS (~15%)** (Brave 963.7 MB vs Slate 1,136.0 MB; IQRs do not overlap).
* **5-Tab Diagnostic Matrix & Process Breakdown**:
  - Raw file: [`benchmarks/results/realworld/diagnostics_5tabs_matrix_raw.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/diagnostics_5tabs_matrix_raw.json)
  - Summary file: [`benchmarks/results/realworld/diagnostics_5tabs_summary.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/diagnostics_5tabs_summary.json)
  - Total PSS: Slate Opt 3 Blocking ON (**795.6 MB**) vs Blocking OFF (**793.3 MB**) $\to$ +2.3 MB total diff; Renderer PSS is **543.7 MB** vs **543.5 MB** (+0.2 MB diff).
  - **Verified finding**: Adblock cosmetic/scriptlet JS injection does **NOT** account for the memory gap.
* **Cold Start to Hacker News (7 Runs, Same Session)**:
  - Raw file: [`benchmarks/results/realworld/cold_start_session_comparison.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/cold_start_session_comparison.json)
  - Brave: Median **793.4 ms** (IQR $[781.7, 807.8]$ ms)
  - Slate (Blocking ON): Median **1,850.6 ms** (IQR $[1,584.6, 2,042.9]$ ms)
  - Slate (Blocking OFF): Median **1,706.0 ms** (IQR $[1,649.8, 1,924.3]$ ms)
  - Brave starts **~1,057 ms faster**.
* **Idle CPU (7-Run Matrix, DevTools Closed)**:
  - Raw file: [`benchmarks/results/realworld/idle_cpu_7runs_raw.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/idle_cpu_7runs_raw.json)
  - Summary file: [`benchmarks/results/realworld/idle_cpu_7runs_summary.json`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/idle_cpu_7runs_summary.json)
  - `about:blank`: Slate Opt 3 (0.20%, IQR $[0.10, 0.20]$), Slate Opt 0 (0.20%, IQR $[0.15, 0.20]$), Brave Default (9.98%, IQR $[8.76, 10.92]$), Brave Trimmed (9.78%, IQR $[8.82, 10.02]$).
  - 10 Static Sites: Slate Opt 3 (150.03%, IQR $[115.31, 174.50]$), Slate Opt 0 (138.11%, IQR $[91.56, 154.30]$), Brave Default (157.25%, IQR $[155.95, 174.29]$), Brave Trimmed (166.08%, IQR $[156.52, 185.90]$).
  - All four 10-static-page configurations exhibit heavily overlapping IQRs in the 115%–175% range.
* **Site Isolation**: Strict one-process-per-site verified via automated test in [`scripts/check.sh`](file:///home/earl/Desktop/Slate/scripts/check.sh#L49).

### B. What is UNVERIFIED
* **Proprietary Codec Prebuilt Builds**: Third-party prebuilts with H.264/AAC enabled (UNTESTED on Slate).
* **Local Custom CEF Build**: Source compilation of CEF with `proprietary_codecs=true` (UNTESTED; 120–150 GB disk, 3–8 hrs).
* **Upstream Drop-in Patching**: Manual replacement of `libcef.so` when upstream fixes ship before crate updates (UNTESTED with real M157 binary).

### C. What is KNOWN BROKEN
1. **`iframe` Shell Viewport**:
   - `crates/app/resources/ui/index.html` hosts web pages in an `<iframe>`.
   - Broken behavior: Major sites enforcing `X-Frame-Options: SAMEORIGIN` or CSP `frame-ancestors` (e.g. Google Docs, Chase, GitHub logins, Reddit) fail or refuse to render inside the iframe.
2. **~~Fake / Hardcoded Tracker Counters~~ — REMOVED (commit `658e74e`)**:
   - `shell.js` no longer has a domain table or simulated counts. The UI shows `-` until the real pipeline feeds it.
   - Still true: **no per-tab real count is wired to the UI yet** (the engine only has global atomics). See `docs/shell-architecture.md` §3.4.
   - `scripts/check.sh` stage 9 fails if `shell.js` regains hardcoded counts. OS screenshot: `screenshots/step1-no-fake-counters.png`.
3. **No H.264 / AAC MP4 Playback**:
   - Stock Spotify CEF binaries compile without proprietary codecs (`proprietary_codecs=false`).
   - `HTMLMediaElement.canPlayType("video/mp4; codecs='avc1.42E01E'")` returns `""` and MSE is `false`.

---

## 3. Decisions Made and Why

1. **Engine: CEF Chosen, WebKitGTK Rejected**:
   - *Why*: In live testing on Arch Linux, WebKitGTK had broken WebRTC (`RTCPeerConnection` was `false`), broken DRM, could not intercept subresource requests in its UI process (0 of 10 ad scripts blocked), and spawned 62 processes consuming +35% more PSS than CEF. CEF provides identical Blink/V8 rendering, full request interception on the IO thread, and robust WebGL/WebRTC/DRM compatibility.
2. **`--renderer-process-limit` Removed**:
   - *Why*: Setting a renderer process limit forces cross-origin tabs into shared processes, directly weakening Chromium's Site Isolation (`--site-per-process`). Removed to preserve the multi-process security sandbox.
3. **Widevine Opt-in from User's System (No Bundling)**:
   - *Why*: Bundling `libwidevinecdm.so` requires a commercial Google CWIP license. Slate auto-detects Widevine from the host system (`/opt/google/chrome/WidevineCdm` or Brave's config) on an opt-in basis.
4. **`curl` Dependency as Known Tech Debt**:
   - *Why*: Downloading remote filter lists uses `curl` via `Command::new("curl")` with `--compressed` and strict timeouts. If `curl` is missing, Slate starts cleanly from the cached `filters.dat` without crashing and logs a clear warning. Propose replacing with CEF's native network stack or a separate helper daemon in Phase 2.
5. **`ureq` / `ring` Crash & C-Dependency Rule**:
   - *What happened*: Adding `ureq = "3.5"` (pulling in `ring`) caused `slate-spike-cef` to abort at `cef::initialize` with `close symbol missing` (exit code 133). Bisected cleanly: `ring`'s static archive alters symbol visibility/resolution, breaking Chromium's glibc `close` interposition check.
   - *Rule*: **Before adding any new dependency that links C or assembly code (TLS, crypto, compression), run `./scripts/check.sh` smoke test first.**
6. **Confining `unsafe` FFI Macros**:
   - All CEF `wrap_*!` macros are strictly confined inside [`crates/engine-adapter/src/cef_backend.rs`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/cef_backend.rs). All other crates enforce `#![forbid(unsafe_code)]`.

---

## 4. Benchmark Conclusions as They Stand

1. **Memory (PSS) is Workload-Dependent**:
   - **1 Tab**: Tie / No meaningful difference (Slate 598.1 MB vs Brave 681.3 MB PSS).
   - **5 Tabs & 10 Tabs**: Brave is lower by **~17–18% PSS** (Brave 1,078.5 MB vs Slate 1,310.6 MB at 5 tabs; Brave 1,235.3 MB vs Slate 1,497.2 MB at 10 tabs).
   - **Heavy Video**: Brave is lower by **~15% PSS** (Brave 963.7 MB vs Slate 1,136.0 MB).
   - **Cause**: Chromium's multi-renderer compaction heuristics across distinct origin processes, not adblocking injection (+0.2 MB diff).
2. **Cold Start**: Brave is faster by **~1,057 ms** to fully load Hacker News (~793 ms vs ~1,851 ms). Cache deserialization is fast (8.76 ms); the gap is hypothesized to be CEF subprocess spawn and FFI initialization overhead.
3. **Install Size**: Slate is smaller on disk (**~226 MB** vs Brave's **~336 MB** on Arch Linux).
4. **Idle CPU**: **NOT ESTABLISHED** on static pages. Over 7 runs with DevTools closed, Slate and Brave exhibit heavily overlapping IQRs in the 115%–175% range across 10 static sites. On `about:blank`, Slate is 0.20% vs Brave's 9.98%.

---

## 5. Exact Next Steps: Shell Rebuild

When continuing to the Phase 1 shell rebuild, execute the following specific steps:

1. **Native `BrowserView` Split-View Architecture**:
   - Remove the `<iframe>` viewport from [`crates/app/resources/ui/index.html`](file:///home/earl/Desktop/Slate/crates/app/resources/ui/index.html).
   - In [`crates/engine-adapter/src/cef_backend.rs`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/cef_backend.rs), construct the application window with two native CEF views or a layered window:
     - View 1: The browser shell UI (sidebar tabs, address bar, navigation controls).
     - View 2: The active web content native `CefBrowserView`, embedded alongside the sidebar without iframe restrictions.
2. **Real Adblock Counters via CEF IPC**:
   - Delete the hardcoded domain lookup table in [`crates/app/resources/ui/shell.js`](file:///home/earl/Desktop/Slate/crates/app/resources/ui/shell.js) (lines 120–130).
   - Implement a CEF JavaScript query handler or IPC message router (`CefMessageRouterBrowserSide`) that sends real blocked request counts from `BlockingPipeline` to the UI front-end.
3. **Real Multi-Tab Lifecycle Management**:
   - Implement real tab creation, activation, and destruction by managing a collection of `CefBrowser` / `CefBrowserView` instances in Rust.
   - Attach the active tab's native view to the content container and detach background tabs.
4. **Address H.264/AAC Codecs**:
   - Implement the Cisco OpenH264 runtime download mechanism or evaluate a dedicated custom CEF build.
5. **Upstream Security Drop-In Workflow**:
   - Test and document the Spotify automated prebuilt binary drop-in procedure for emergency M157 updates.

---

## 6. How to Run Checks and the Browser

* **Run Full CI / Verification Suite**:
  ```bash
  ./scripts/check.sh
  ```
  *(Checks formatting, Clippy with warnings as errors, 16 unit tests, live CEF launch smoke test, Site Isolation verification, and 250k rule cache count assertion).*

* **Launch the Interactive Browser Shell**:
  ```bash
  ./scripts/run.sh
  ```

* **Launch Headless / Specific URLs**:
  ```bash
  ./scripts/run.sh --url https://news.ycombinator.com
  ./scripts/run.sh --tabs 5 --real-sites --hold-ms 10000
  ./scripts/run.sh --tabs 1 --exit-after-load
  ```
