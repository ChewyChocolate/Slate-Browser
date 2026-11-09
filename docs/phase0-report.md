# Phase 0 Report: Engine Prototype & Ad-Blocking Pipeline

**Date**: 2026-11-10  
**Target Platform**: Arch Linux (`x86_64`, kernel `7.2.14-arch1-1`, `rustc 1.99.1`)  
**Status**: Completed & Approved (Option A — CEF selected)

---

## 1. What Was Built

- **Cargo Workspace** (`Cargo.toml`):
  - **`crates/core` (`slate-core`, `#![forbid(unsafe_code)]`)**: `TabManager`, `TabState`, `TabLifecycleState`, `ScrollPosition`, tab sleeping/suspension policy (excludes focused tabs and tabs playing audio/video, preserves URL/title/scroll position, restores on focus), and `BenchServer` (a local loopback HTTP server serving distinct IPv4 origins `http://127.0.0.1:{port}` .. `http://127.0.0.10:{port}` for deterministic multi-origin benchmarks).
  - **`crates/blocking` (`slate-blocking`, `#![forbid(unsafe_code)]`)**: Wraps `adblock = "0.13.5"` (configured multi-threaded `Send + Sync` without `single-thread`) in `Arc<RwLock<Engine>>` via `BlockingPipeline`. Implements all 7 required ad-blocking capabilities, FlatBuffer `.dat` binary cache serialization (`serialize_cache` / `deserialize_cache`), and non-blocking background filter updates (`update_lists_in_background`).
  - **`crates/ui-bridge` (`slate-ui-bridge`, `#![forbid(unsafe_code)]`)**: Typed JSON IPC (`UiCommand`, `CoreEvent`), strict origin validation (`ALLOWED_UI_ORIGINS`), and `INTERNAL_UI_CSP` forbidding remote code loading and `eval()`.
  - **`crates/engine-adapter` (`slate-engine-adapter`)**: `BrowserEngineAdapter` trait and engine capability matrices (`cef_capability_report`, `webkitgtk_capability_report`).
  - **`spikes/spike-cef` (`slate-spike-cef`)**: Spike A — Rust shell + Chromium Embedded Framework (`cef = "156.4.0+156.0.26"`, Chromium 156).
  - **`spikes/spike-webkit` (`slate-spike-webkit`, `#![forbid(unsafe_code)]`)**: Spike B — Rust shell + WebKitGTK (`webkit2gtk = "2.0.2"` / system `webkit2gtk-4.1 2.54.2`).
- **Living Docs & Scripts**:
  - `ARCHITECTURE.md` and `DECISIONS.md`
  - `scripts/check.sh` (local CI: `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `cargo audit`)
  - `scripts/bench_phase0.sh` (reproducible Phase 0 benchmark suite)

---

## 2. How to Run It

```bash
# 1. Run all formatting, Clippy (-D warnings), and automated unit tests:
./scripts/check.sh

# 2. Run the full Phase 0 benchmark suite (Spike A vs. Spike B vs. Brave):
./scripts/bench_phase0.sh

# 3. Run individual spikes or live compatibility probes:
export LD_LIBRARY_PATH="$PWD/target/release:${LD_LIBRARY_PATH:-}"
./target/release/slate-spike-cef --tabs 2 --hold-ms 2500 --verify-capabilities
./target/release/slate-spike-webkit --tabs 2 --hold-ms 2500 --verify-capabilities
./target/release/slate-spike-cef --compat-probe https://www.youtube.com
./target/release/slate-spike-webkit --compat-probe https://www.youtube.com
```

---

## 3. Phase 0 Measurements & Comparison vs. Brave

Memory is measured by walking `/proc` across the entire process tree (main process + all renderer, GPU, network, zygote, and `bwrap` sandbox child processes) and summing both **PSS** (`Pss:` from `/proc/<pid>/smaps_rollup`, which accurately divides shared library pages across processes) and **RSS** (`VmRSS:` from `/proc/<pid>/status`). Raw JSON outputs are stored in `benchmarks/results/`.

### A. Size, Startup, RAM, and Core Capabilities

| Metric | Spike A: Rust + CEF 156 (`slate-spike-cef`) | Spike B: Rust + WebKitGTK 2.54 (`slate-spike-webkit`) | Baseline: Brave `1.99.55` (`/usr/bin/brave`) |
| :--- | :--- | :--- | :--- |
| **Shell binary size** | **5.08 MiB** (`5,323,712 B`) | **5.10 MiB** (`5,351,808 B`) | — (monolithic) |
| **Total install size on disk** | **298.1 MiB** stripped bundle (`libcef.so` + `.pak` + ICU + Angle; 1.42 GiB unstripped SDK) | **5.10 MiB** app + **137.8 MiB** system shared `libwebkit2gtk-4.1.so` & `libjavascriptcoregtk-4.1.so` | **481.3 MiB** (`/opt/brave-bin`, `504,674,161 B`) |
| **Cold start time (1 tab to load finished)** | **570 – 594 ms** | **685 – 694 ms** (up to **2,052 ms** when unblocked external ad scripts stall load) | **~520 ms** (`163 ms` to first renderer spawn) |
| **Idle RAM — 1 tab (PSS / RSS / Procs)** | **335.4 MiB PSS** (`343,436 KiB`) / 881.8 MiB RSS (9 procs) | **221.5 MiB PSS** (`226,805 KiB`) / 431.1 MiB RSS (8 procs) | **382.9 MiB PSS** (`392,144 KiB`) / 1,043.6 MiB RSS (9 procs) |
| **Idle RAM — 10 tabs, 10 origins (PSS / RSS / Procs)** | **461.5 MiB PSS** (`472,573 KiB`) / 1,988.9 MiB RSS (**18 procs**) | **622.8 MiB PSS** (`637,707 KiB`) / 1,979.7 MiB RSS (**62 procs**) | **470.1 MiB PSS** (`481,413 KiB`) / 2,107.3 MiB RSS (**18 procs**) |
| **Intercept & block subresource requests?** | **Yes (Native)**: Blocked **10 / 10** `<script>` ad subresources in-process | **No (UI-process gap)**: Blocked **0 / 10** `<script>` subresources (`decide-policy` only sees frame navigations) | Yes |
| **Inject scripts at `document-start`?** | **Yes** (`on_load_start` / `on_context_created` / CDP `Page.addScriptToEvaluateOnNewDocument`) | **Yes** (`UserScriptInjectionTime::Start`) | Yes |
| **Control third-party cookies?** | **Yes (Per-request & per-domain)** via `CookieAccessFilter` (`can_send_cookie` / `can_save_cookie`) | **Yes (Global policy)** via `CookieAcceptPolicy::NoThirdParty` + ITP | Yes |
| **Discard / suspend a background tab?** | **Yes**: Closing background `Browser` freed **114.1 MiB RSS** (`1,045,332 -> 928,444 KiB`) in 350 ms | **Yes**: `terminate_web_process()` freed **161.8 MiB RSS** (`782,792 -> 617,100 KiB`) in 350 ms | Yes |

> **Key Memory Finding at 10 Tabs**: While WebKitGTK is lighter at 1 tab (`221.5 MiB` PSS vs. CEF's `335.4 MiB` PSS), **WebKitGTK uses 35% MORE real memory (PSS) than CEF at 10 tabs (`622.8 MiB` vs. `461.5 MiB`) and spawns 62 processes vs. CEF's 18**. WebKitGTK spawns a new `WebKitWebProcess` plus multiple `bwrap`/`xdg-dbus-proxy` helper processes per site without a Chromium-style Zygote sharing pre-initialized V8/Blink pages.

### B. Live Compatibility Smoke Tests (`--compat-probe`)

Both spikes were executed against live sites and APIs (`benchmarks/results/compat/*.json`):

| Compatibility Test | Spike A: CEF 156 (`slate-spike-cef`) | Spike B: WebKitGTK 2.54 (`slate-spike-webkit`) | Evidence from Live Probe Execution |
| :--- | :--- | :--- | :--- |
| **YouTube (`https://www.youtube.com`)** | **Pass** | **Pass (playback only; subresource ads not blocked by UI process)** | Both loaded `https://www.youtube.com/` with `title: "YouTube"` and `document_start_hook: true`. |
| **Google Docs (`https://docs.google.com`)** | **Pass** | **Partial / Risk** | Both reached Google's sign-in/editor flow, but Google Docs relies on Chromium clipboard/canvas behavior and warns or degrades features on non-Chrome Linux User-Agents. |
| **Banking Login (`https://www.chase.com`)** | **Pass (Native UA)** | **Quirk Workaround** | CEF loaded Chase natively (`Chrome/156.0.0.0`). WebKitGTK had to activate its compiled-in UA quirk to impersonate macOS Safari (`Macintosh; Intel Mac OS X 10_15 ... Safari/605.1.15`) so Chase would not reject the browser. |
| **DRM Video (`navigator.requestMediaKeySystemAccess` / Bitmovin DRM)** | **Pass (`eme_drm_api_present: true`)** | **FAIL (`eme_drm_api_present: false`)** | Even with `settings.set_enable_encrypted_media(true)`, Arch Linux's `webkit2gtk-4.1` binary does **not** expose `navigator.requestMediaKeySystemAccess` (`false`), and Widevine has no supported WebKitGTK Linux plugin. |
| **WebGL2 & Video Calls (`RTCPeerConnection`)** | **Pass (`webgl: true`, `webrtc: true`)** | **FAIL on WebRTC (`webgl: true`, `webrtc: false`)** | Even with `settings.set_enable_media_stream(true)`, `typeof RTCPeerConnection === 'function'` returned **`false`** in Arch Linux's official `webkit2gtk-4.1` package. WebRTC video calls (Google Meet, Zoom Web, Discord) fail out of the box on WebKitGTK. |

---

## 4. Ad-Blocking Pipeline (`adblock-rust`) & Interception Gap Analysis

All 7 required capabilities are implemented in `crates/blocking/src/lib.rs` and verified by 6 automated tests (`cargo test -p slate-blocking`):

| # | Required Capability | Spike A (CEF 156) Support | Spike B (WebKitGTK 2.54) Support |
| :--- | :--- | :--- | :--- |
| **1** | **Network request blocking (EasyList/EasyPrivacy)** | **Native**: `ResourceRequestHandler::on_before_resource_load` evaluates `adblock-rust` synchronously on the IO thread for all resource types (`script`, `xhr`, `image`, `stylesheet`, `sub_frame`, `ping`, etc.). | **Workaround required**: UI-process `decide-policy` only sees main/iframe navigations. Subresources require either WebKit `UserContentFilter` (150k rule limit, loses uBO modifiers, and `add_filter` is missing from `webkit2gtk-rs` safe bindings) or a separate WebProcess `.so` extension. |
| **2** | **Cosmetic filtering (per-site & generic element hiding)** | **Native + Workaround**: `adblock::Engine::url_cosmetic_resources` + `hidden_class_id_selectors` injected as `<style>` at `document-start` and updated dynamically via `MutationObserver`. | **Native**: `UserContentManager::add_style_sheet` (`UserStyleLevel::User`, `AllFrames`) + `adblock-rust` selector generation. |
| **3** | **Scriptlet injection (uBlock `##+js(...)` resources)** | **Native**: Compiled by `adblock-rust` (`set-constant`, `json-prune`, `abort-on-property-read`) and evaluated before page scripts via `on_load_start` / `CefRenderProcessHandler::on_context_created` / CDP `Page.addScriptToEvaluateOnNewDocument`. | **Native**: Injected via `UserContentManager::add_script` with `UserScriptInjectionTime::Start` (`AllFrames`). |
| **4** | **Third-party cookie & storage blocking** | **Native (Per-request)**: `CookieAccessFilter::can_send_cookie` & `can_save_cookie` compare registrable domains (`adblock::url_parser::get_host_domain`) on every request; third-party iframe `localStorage`/`sessionStorage` isolated at `document-start`. | **Native (Coarse global)**: `CookieManager::set_accept_policy(CookieAcceptPolicy::NoThirdParty)` + `WebsiteDataManager::set_itp_enabled(true)` (cannot easily exempt per-site breakage without toggling globally). |
| **5** | **HTTPS upgrades (HTTPS-first with safe fallback)** | **Native**: Synchronous URL rewrite via `req.set_url()` in `on_before_resource_load`; `on_load_error` records TLS/connect failures in `https_fallback_hosts` and retries over HTTP. | **Workaround**: Cancel navigation in `decide-policy` and call `view.load_uri(&https_url)`; subresources are not upgraded by `decide-policy`. |
| **6** | **Tracking-parameter stripping on navigation** | **Native**: Synchronous URL rewrite via `req.set_url()` + `$removeparam` support in `adblock-rust`. | **Workaround**: Cancel navigation in `decide-policy` and call `view.load_uri(&cleaned_url)`. |
| **7** | **Custom user filter lists + background updates** | **Native**: Supports pasted rules, local files, and remote URLs; `update_lists_in_background` compiles a new `adblock::Engine` on a background thread and swaps `Arc<RwLock<Engine>>` atomically; `.dat` FlatBuffer cache (`serialize_cache` / `deserialize_cache`) keeps startup fast. | **Native** in `slate-blocking`, but pushing updated rules into WebKit's `UserContentFilterStore` requires re-translating to Apple JSON and re-compiling on disk. |

---

## 5. Decision Outcome

**Option A — CEF (`cef` 156 / Chromium Embedded Framework)** was recommended and approved by the user based on zero broken sites, full WebRTC & EME API presence, native subresource ad blocking, 38.1% smaller stripped disk footprint than Brave, and lower 10-tab PSS RAM than both Brave and WebKitGTK.
