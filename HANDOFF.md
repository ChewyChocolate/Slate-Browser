# Slate Browser — Session Handoff (`HANDOFF.md`)

**Last Updated**: 2026-11-10
**Target Platform**: Arch Linux (`x86_64`, Wayland/X11) first; Windows and Android later
**Selected Engine**: **Option A — CEF (`cef = "156.4.0+156.0.26"`, Chromium `156.0.8087.17`)**

This document contains everything a new session needs to continue building **Slate** without the prior conversation history.

---

## 1. Current Status

- **Phase 0 (Engine Prototype & Comparison)**: **Complete & Approved**
  - Full report: [`docs/phase0-report.md`](file:///home/earl/Desktop/Slate/docs/phase0-report.md)
  - Built and benchmarked both `slate-spike-cef` (CEF 156) and `slate-spike-webkit` (WebKitGTK 2.54.2) against Brave `1.99.55` (`Chromium 156.0.8087.17`).
  - User approved **Option A (CEF)**.
- **Phase 1 Early Milestones (1–6)**: **Complete & Verified**
  - Full report: [`docs/phase1-early-report.md`](file:///home/earl/Desktop/Slate/docs/phase1-early-report.md)
  - Real-world 10-site benchmark + Tab Sleeping vs. Brave completed (`benchmarks/results/realworld/`).
  - Deep compatibility probes (`codecs-drm` with and without `--widevine-cdm-path`, `google-login`, `gdocs-edit`, `banking-login`) completed (`benchmarks/results/deep_compat/`).
  - Dual `Client::request_handler` + `RequestContextHandler` wired in `crates/engine-adapter/src/cef_backend.rs` and proven against a live Service Worker (`benchmarks/results/service_worker_test.json`).
  - Progressive Chromium/CEF flag & feature bloat audit (`--opt-level 0..3`) + subprocess fast-path completed (`benchmarks/results/cef_opt_level_*_10tabs.json`).
  - Upstream Chromium/CEF version check script (`scripts/check_cef_version.sh`) and update runbook added.
  - All `cef` `wrap_*!` FFI macros confined strictly to `crates/engine-adapter/src/cef_backend.rs`; `#![forbid(unsafe_code)]` enforced on all other crates and binaries.
- **Phase 1 Full Browser Shell & v1 Features**: **Paused / Ready to Start** (awaiting user review of Phase 1 early milestones).

---

## 2. Verified vs. Unverified Matrix

Per project rules, we never claim something works without executing it and capturing real output.

### Verified (Executed with Live Output & Automated Tests)

| Area | What Was Executed | Key Measured Result | Evidence / Artifact |
| :--- | :--- | :--- | :--- |
| **Workspace CI Checks** | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (12 unit tests), `cargo audit` | All pass with 0 warnings and 0 vulnerabilities | [`scripts/check.sh`](file:///home/earl/Desktop/Slate/scripts/check.sh) |
| **`unsafe` Isolation** | `#![forbid(unsafe_code)]` enforced on `slate-core`, `slate-blocking`, `slate-ui-bridge`, `slate-spike-cef`, `slate-spike-webkit`; CEF macros confined to `crates/engine-adapter/src/cef_backend.rs` | Enforced at compile time + verified by unit test `unsafe_code_is_forbidden_in_all_crates_outside_engine_adapter` | [`crates/engine-adapter/src/lib.rs`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/lib.rs#L232-L253) |
| **Phase 0 Benchmark** | CEF 156 vs. WebKitGTK 2.54.2 vs. Brave 1.99.55 (install size, cold start, 1-tab & 10-tab PSS/RSS) | Stripped CEF bundle: **298.1 MiB** (vs. Brave **481.3 MiB**); 10-tab PSS: CEF **461.5 MiB** vs. Brave **470.1 MiB** vs. WebKitGTK **622.8 MiB** | [`docs/phase0-report.md`](file:///home/earl/Desktop/Slate/docs/phase0-report.md), [`benchmarks/results/phase0_summary.md`](file:///home/earl/Desktop/Slate/benchmarks/results/phase0_summary.md) |
| **Milestone 1: Real-World 10-Site Benchmark + Tab Sleeping** | Live navigation across 10 real sites (`YouTube`, `Google Docs`, `Hacker News`, `Wikipedia`, `Reddit`, `BBC News`, `Proton Mail`, `GitHub`, `Chase`, `NYTimes`) at 1, 5, 10 tabs, and 10 tabs after Tab Sleeping | 10-tab PSS: Slate **888.7 MiB** (19 procs, 232 ads/trackers blocked) vs. Brave **992.6 MiB** (20 procs). After Tab Sleeping (9 tabs suspended): **450.1 MiB PSS** (10 procs, **-54.7% vs. Brave**) | [`scripts/bench_realworld.sh`](file:///home/earl/Desktop/Slate/scripts/bench_realworld.sh), [`benchmarks/results/realworld/realworld_summary.md`](file:///home/earl/Desktop/Slate/benchmarks/results/realworld/realworld_summary.md) |
| **Milestone 2: Codec & Widevine EME Probes** | Live `--deep-compat codecs-drm` probe with stock `libcef.so` and with `--widevine-cdm-path "$HOME/.config/BraveSoftware/Brave-Browser/WidevineCdm"` | Stock `libcef.so`: `VP9`, `AV1`, `Opus` = `true`; `H.264`, `AAC` = `false`. With `--widevine-cdm-path`: Widevine `com.widevine.alpha` `SW_SECURE_CRYPTO` = `true`, `mediaKeysCreated: true` | [`benchmarks/results/deep_compat/codecs-drm.json`](file:///home/earl/Desktop/Slate/benchmarks/results/deep_compat/codecs-drm.json), [`benchmarks/results/deep_compat/codecs-drm-widevine.json`](file:///home/earl/Desktop/Slate/benchmarks/results/deep_compat/codecs-drm-widevine.json) |
| **Milestone 2: Google Login, Google Docs, Chase Login Form Probes** | Live `--deep-compat google-login`, `gdocs-edit`, `banking-login` DOM/input/WebCrypto/Canvas2D probes | Google identifier input typed & matched (`input_matches: true`); Chase username/password typed inside iframe (`username_typed_ok: true`, `password_typed_ok: true`) | [`benchmarks/results/deep_compat/`](file:///home/earl/Desktop/Slate/benchmarks/results/deep_compat) |
| **Milestone 3: Service Worker Ad/Tracker Blocking** | Live `--test-service-worker` registering `/sw.js` and issuing `fetch()` from inside the Service Worker worker thread + unit test | `sw_registered: true`, `sw_allowed_fetch_ok: true`, `sw_ad_fetch_blocked: true`, `sw_tracker_fetch_blocked: true`, `request_context_intercepts: 3`, `service_worker_blocked_requests: 2` | [`benchmarks/results/service_worker_test.json`](file:///home/earl/Desktop/Slate/benchmarks/results/service_worker_test.json), [`crates/engine-adapter/src/cef_backend.rs`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/cef_backend.rs#L605-L639) |
| **Milestone 4: Chromium/CEF Flag & Feature Bloat Audit** | Measured `--opt-level 0`, `1`, `2`, `3` across 10 tabs + subprocess fast-path before `BlockingPipeline` init | Reduced 10-tab PSS from **457.7 MiB** (18 procs) to **343.3 MiB** (11 procs, **-25.0%**) and cold start from **589 ms** to **566 ms** | [`benchmarks/results/cef_opt_level_3_10tabs.json`](file:///home/earl/Desktop/Slate/benchmarks/results/cef_opt_level_3_10tabs.json) |
| **Milestone 5: Chromium Security Release Tracker** | Executed `./scripts/check_cef_version.sh` querying `crates.io` and Google Chromium Dash API | Pinned `156.4.0+156.0.26` (`156.0.8087.17`) vs. upstream Linux stable `157.0.8103.38` | [`scripts/check_cef_version.sh`](file:///home/earl/Desktop/Slate/scripts/check_cef_version.sh) |

### Unverified (Not Yet Tested / Requires Credentials or Custom Build)

| Item | Why It Was Not Tested | How to Verify Next |
| :--- | :--- | :--- |
| **Authenticated Google Account Login (Password + 2FA/Passkey)** | Automated test environment has no user Google credentials; only step 1 (`identifierId` typing + submission trigger) was tested automatically. | Manual interactive login by the user once the Phase 1 interactive window + persistent profile UI are wired. |
| **Authenticated Google Docs Document Editing** | Accessing the live `/document/d/.../edit` canvas requires an authenticated Google session. | Manual test after signing into Google in Slate's persistent profile (`~/.local/share/slate/profile`). |
| **Authenticated Banking Post-Login Dashboard** | Requires real banking credentials + 2FA; only the live `chase.com` login iframe (`#userId-input-field-input`, `#password-input-field-input`) was tested. | Manual test by user with a real banking session. |
| **Live Commercial Widevine Stream (Netflix / Spotify / Disney+)** | While `requestMediaKeySystemAccess('com.widevine.alpha')` and `createMediaKeys()` succeeded (`mediaKeysCreated: true`), commercial streaming services also require VMP (Verified Media Path) signature verification and an active subscription. | Test against a commercial DRM test stream or user account; note VMP limitations in [`docs/phase1-early-report.md`](file:///home/earl/Desktop/Slate/docs/phase1-early-report.md). |
| **Custom-Compiled `libcef.so` with `proprietary_codecs=true` (H.264/AAC)** | Stock Spotify prebuilt `libcef.so` is compiled with `proprietary_codecs=false` (`VP9`/`AV1`/`Opus` enabled, `H.264`/`AAC` disabled). We have not compiled Chromium/CEF from source (a multi-hour build). | Decide whether to provide an AUR/source build script or system FFmpeg patch for H.264/AAC. |

---

## 3. Summary of All Decisions Made (`DECISIONS.md`)

Full details and tradeoffs are in [`DECISIONS.md`](file:///home/earl/Desktop/Slate/DECISIONS.md):

1. **[ADR-001: Workspace Crate Decomposition](file:///home/earl/Desktop/Slate/DECISIONS.md#L7-L12)** — Four library crates (`slate-core`, `slate-blocking`, `slate-ui-bridge`, `slate-engine-adapter`) + binary crates, isolating engine-specific code exclusively inside `crates/engine-adapter`.
2. **[ADR-002: `adblock-rust` Multi-Threaded Configuration](file:///home/earl/Desktop/Slate/DECISIONS.md#L15-L20)** — Disable `adblock`'s default `single-thread` feature (`default-features = false, features = ["embedded-domain-resolver", "full-regex-handling", "css-validation"]`) and wrap `adblock::Engine` in `Arc<RwLock<Engine>>` so CEF IO threads and background filter update threads share it safely.
3. **[ADR-003: Phase 0 Engine Selection — Option A (CEF)](file:///home/earl/Desktop/Slate/DECISIONS.md#L23-L44)** — Selected CEF 156 over WebKitGTK 2.54 because CEF had zero broken major sites, working WebRTC/WebGL/EME, full subresource `adblock-rust` interception, and lower 10-tab PSS RAM (`461.5 MiB` vs. WebKitGTK's `622.8 MiB` across 62 processes).
4. **[ADR-004: Chromium / CEF Bloat Reduction & Subprocess Fast-Path](file:///home/earl/Desktop/Slate/DECISIONS.md#L47-L55)** — Short-circuit `--type=*` child processes via `cef::execute_process` before constructing `BlockingPipeline`, and apply `CefOptimizationLevel::Level3MemoryAndV8Tuning` (`--renderer-process-limit=6`, `--disk-cache-size=33554432`, `--media-cache-size=16777216`, `--js-flags=--optimize-for-size`, and appended `--disable-features`).
5. **[ADR-005: Proprietary Codecs (H.264/AAC) & Widevine CDM Strategy](file:///home/earl/Desktop/Slate/DECISIONS.md#L58-L66)** — Keep Widevine CDM loading opt-in via `--widevine-cdm-path` (pointing to an existing system/Brave/Chrome `WidevineCdm` directory) so YouTube does not spawn the Widevine utility process unnecessarily, and avoid redistributing proprietary binaries.
6. **[ADR-006: Service Worker Request Interception via Dual `Client::request_handler` + `RequestContextHandler`](file:///home/earl/Desktop/Slate/DECISIONS.md#L69-L77)** — Attach `SlateResourceRequestHandler` to both `Client::request_handler` and `RequestContextHandler` (created via `request_context_cef_create_context_shared`) so out-of-tab Service Worker `fetch()` calls cannot bypass `adblock-rust`.
7. **[ADR-007: Confining CEF `unsafe` FFI Macros to `crates/engine-adapter`](file:///home/earl/Desktop/Slate/DECISIONS.md#L80-L87)** — Keep all `cef` `wrap_*!` macro invocations inside `crates/engine-adapter/src/cef_backend.rs` and enforce `#![forbid(unsafe_code)]` everywhere else.

---

## 4. Known Issues, Gotchas & Open Questions

### Important Technical Gotchas Discovered
1. **Never overwrite `--disable-features` or disable `MediaRouter` in CEF 156**:
   - CEF's Chrome runtime sets internal disabled features before `on_before_command_line_processing` and dereferences `MediaRouter` during `ChromeBrowserView` creation. Overwriting `--disable-features` or adding `MediaRouter` / `GlobalMediaControls` to `--disable-features` causes an immediate startup `SIGSEGV`. Always use `append_csv_switch(cmd, "disable-features", ...)` in [`crates/engine-adapter/src/cef_backend.rs`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/cef_backend.rs#L653-L669).
2. **Use `request_context_cef_create_context_shared` (not `request_context_create_context` with empty settings)**:
   - In CEF 156 Chrome runtime, `request_context_create_context(&RequestContextSettings::default(), ...)` returns `None` when not initialized with a full profile path, whereas `request_context_cef_create_context_shared(request_context_get_global_context().as_mut(), Some(&mut rc_handler))` reliably attaches `RequestContextHandler` to the initialized global profile context.
3. **Eager Widevine CDM loading increases RAM on YouTube by ~132 MiB PSS**:
   - YouTube probes `navigator.requestMediaKeySystemAccess('com.widevine.alpha', ...)` on homepage load even for normal non-DRM videos. If `WidevineCdm` is loaded unconditionally, Chromium spawns the Widevine utility process on YouTube load. Keep `--widevine-cdm-path` opt-in or gated behind a per-site "Enable DRM" toggle in Phase 1 UI.
4. **Environment Variables Required When Building/Running**:
   - The prebuilt CEF 156 SDK is cached at `/home/earl/.cache/slate-cef`.
   - Always pass `CEF_PATH="$HOME/.cache/slate-cef"` when building (`cargo build`, `cargo clippy`, `cargo test`) with `--features cef-engine`.
   - Always pass `LD_LIBRARY_PATH="$PWD/target/release:${LD_LIBRARY_PATH:-}"` when executing the release binary so the dynamic linker finds the stripped `target/release/libcef.so`.

### Open Questions for the User Before Full Phase 1 Build
1. **H.264 / AAC Codec Strategy**:
   - Stock prebuilt `libcef.so` plays `VP9`, `AV1`, and `Opus` (YouTube works out of the box) but has `H.264` and `AAC` disabled (`proprietary_codecs=false`). Should we continue using the prebuilt `libcef.so` during Phase 1 feature development and add an Arch Linux build script / `PKGBUILD` flag for `proprietary_codecs=true` at packaging time, or address H.264/AAC earlier?
2. **Per-Site DRM Toggle**:
   - Should Widevine CDM loading (`--widevine-cdm-path`) be exposed as a per-site shield/DRM toggle in the address bar (similar to Brave's Widevine prompt) so it never loads on YouTube unless needed for Movies/TV?

---

## 5. Exact Next Steps (Phase 1 Full Browser & v1 Scope)

Once the user approves the Phase 1 Early Milestone results:

1. **Step 1 — Single-Window Multi-Tab Shell & `slate` Binary (`crates/engine-adapter` + `crates/app`)**:
   - Create the main `crates/app` binary (`slate`) using `slate-engine-adapter`, `slate-core`, `slate-blocking`, and `slate-ui-bridge`.
   - Host the internal HTML/CSS/JS shell UI (`slate://ui`) and content tabs inside a single top-level window (using CEF Views / layout management so switching tabs hides/shows or attaches the active `BrowserView` while keeping suspended tabs at zero renderer processes).
2. **Step 2 — Internal HTML/CSS/JS Browser UI (`slate://ui`) with Strict CSP**:
   - Enforce `slate_ui_bridge::INTERNAL_UI_CSP` (`default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'`).
   - Build the vertical tab sidebar (showing active, audio-playing, and sleeping/suspended indicators + blocked request badge), address bar / omnibox, keyboard-driven command palette (`Ctrl+K`), and custom New Tab Page.
   - Connect UI events (`UiCommand`) and state updates (`CoreEvent`) over `slate-ui-bridge`.
3. **Step 3 — Full Filter List Persistence & Background Updater (`crates/blocking`)**:
   - Download EasyList, EasyPrivacy, and uBlock Origin scriptlet resources in a background thread after startup, serialize to `~/.local/share/slate/filters.dat` (`Engine::serialize()`), and load the `.dat` snapshot at startup before swapping in fresh rules via `BlockingPipeline::swap_engine_from_rules`.
   - Wire cosmetic filtering (`url_cosmetic_resources`), scriptlet injection, and custom user filter lists (by URL, local file, or pasted rules) at `document-start`.
4. **Step 4 — Remaining v1 Features**:
   - Automatic tab sleeping timer (`TabManager::suspend_inactive_tabs`) excluding tabs playing audio/video and restoring URL/title/scroll position on focus.
   - Fingerprinting protection injection at `document-start` (canvas/audio noise, navigator/screen normalization, with clear documentation of what JS injection cannot prevent).
   - Userscripts engine (`@match`, `@run-at document-start|document-end|document-idle`).
   - Bookmarks, browsing history, downloads manager, DevTools toggle (`F12` / `Inspect Element`), and PDF viewing.

---

## 6. How to Run Checks, Tests, and Benchmarks

Run all commands from `/home/earl/Desktop/Slate`:

```bash
# 1. Run all CI checks (cargo fmt, clippy -D warnings, 12 unit tests, cargo audit)
./scripts/check.sh

# 2. Check if our pinned cef crate lags upstream Chromium Stable on Linux
./scripts/check_cef_version.sh

# 3. Run the live Service Worker ad/tracker interception test
CEF_PATH="$HOME/.cache/slate-cef" \
LD_LIBRARY_PATH="$PWD/target/release:${LD_LIBRARY_PATH:-}" \
./target/release/slate-spike-cef \
  --test-service-worker \
  --output benchmarks/results/service_worker_test.json

# 4. Run deep compatibility probes (codecs/DRM, Google login, Google Docs, Chase login)
for p in codecs-drm google-login gdocs-edit banking-login; do
  CEF_PATH="$HOME/.cache/slate-cef" \
  LD_LIBRARY_PATH="$PWD/target/release:${LD_LIBRARY_PATH:-}" \
  ./target/release/slate-spike-cef \
    --deep-compat "$p" \
    --output "benchmarks/results/deep_compat/${p}.json"
done

# 5. Run codecs/DRM probe with external Widevine CDM enabled
CEF_PATH="$HOME/.cache/slate-cef" \
LD_LIBRARY_PATH="$PWD/target/release:${LD_LIBRARY_PATH:-}" \
./target/release/slate-spike-cef \
  --deep-compat codecs-drm \
  --widevine-cdm-path "$HOME/.config/BraveSoftware/Brave-Browser/WidevineCdm" \
  --output benchmarks/results/deep_compat/codecs-drm-widevine.json

# 6. Re-run the Phase 0 benchmark (CEF vs. WebKitGTK vs. Brave on local deterministic pages)
./scripts/bench_phase0.sh

# 7. Re-run the Phase 1 Real-World 10-Site Benchmark (1, 5, 10 tabs + Tab Sleeping vs. Brave)
./scripts/bench_realworld.sh
```
