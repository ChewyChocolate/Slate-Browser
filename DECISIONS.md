# Architectural & Engineering Decisions

This log records major technical decisions, options considered, tradeoffs, and verification results so future sessions have complete context.

---

## ADR-001: Workspace Crate Decomposition
- **Date**: 2026-11-09
- **Status**: Accepted
- **Context**: The browser must isolate engine-specific bindings from core tab/session logic, ad-blocking, and UI IPC, and prohibit `unsafe` Rust everywhere except where FFI bindings strictly require it.
- **Decision**: Split the Cargo workspace into four library crates (`slate-core`, `slate-blocking`, `slate-ui-bridge`, `slate-engine-adapter`) with `#![forbid(unsafe_code)]`, plus engine binary crates.

---

## ADR-002: Ad-Blocking Engine (`adblock-rust` Multi-Threaded Configuration)
- **Date**: 2026-11-09
- **Status**: Accepted
- **Context**: `adblock = "0.13.5"` enables the `single-thread` feature by default, which uses `RefCell<RegexManager>` and makes `adblock::Engine` `!Sync` and `!Send`. Browser request interception in CEF runs on the CEF IO thread while background filter updates run on worker threads.
- **Decision**: Depend on `adblock = { version = "0.13.5", default-features = false, features = ["embedded-domain-resolver", "full-regex-handling", "css-validation"] }`. Wrap `adblock::Engine` in `Arc<RwLock<Engine>>` inside `slate-blocking::BlockingPipeline` so background filter list compilation builds a new `Engine` off-thread and swaps the pointer atomically in `< 1 ms`.

---

## ADR-003: Phase 0 Engine Selection (CEF vs. WebKitGTK)
- **Date**: 2026-11-10
- **Status**: Accepted (Option A — CEF)
- **Context**: Compare Option A (Rust shell + CEF 156 via `cef` crate) and Option B (Rust shell + WebKitGTK 2.54 via `webkit2gtk` crate) against Brave `1.99.55` on Arch Linux (`x86_64`), evaluating disk footprint, cold start, 1-tab and 10-tab RAM, site compatibility (YouTube, Google Docs, banking login, DRM video, WebGL/WebRTC), and ad-blocking interception capabilities.
- **Options**:
  1. **Option A — CEF (`cef = "156.4.0+156.0.26"`, Chromium 156)**
     - *Pros*:
       - **Zero broken major sites**: Identical Blink/V8 engine to Brave/Chrome; verified `webgl_supported: true`, `webrtc_supported: true` (`RTCPeerConnection`), and `eme_drm_api_present: true` (`navigator.requestMediaKeySystemAccess`) across YouTube, Google Docs, Chase banking login, and Bitmovin DRM.
       - **Lower RAM than BOTH Brave and WebKitGTK at 10 tabs**: Because Chromium's Zygote shares read-only mappings across site-isolated renderers, Spike A uses **335.4 MiB PSS** (`343,436 KiB`, 9 processes) at 1 tab and **461.5 MiB PSS** (`472,573 KiB`, 18 processes) at 10 tabs — beating Brave (`382.9 MiB` PSS at 1 tab, `470.1 MiB` PSS at 10 tabs) and beating WebKitGTK at 10 tabs (`622.8 MiB` PSS, 62 processes).
       - **Smaller install size than Brave**: Stripped CEF runtime bundle is **298.1 MiB** (`312.6 MB`) vs. Brave's **481.3 MiB** (`504.7 MB`) — 38% smaller on disk.
       - **Full in-process `adblock-rust` interception**: `ResourceRequestHandler::on_before_resource_load` intercepts and blocks all 10/10 subresource ad scripts in the 10-tab benchmark, rewrites URLs synchronously (`req.set_url()`), and filters third-party cookies per request (`CookieAccessFilter`).
     - *Cons*:
       - Requires bundling `libcef.so` (~265.5 MiB stripped; ~1.45 GB unstripped with SDK debug symbols); 1-tab idle PSS (`335.4 MiB`) is higher than 1-tab WebKitGTK (`221.5 MiB`).
  2. **Option B — WebKitGTK (`webkit2gtk-4.1 2.54.2`, `webkit2gtk = "2.0.2"`)**
     - *Pros*:
       - Tiny standalone binary (**5.10 MiB** when dynamically linked against system `libwebkit2gtk-4.1.so` + `libjavascriptcoregtk-4.1.so`, which total **137.8 MiB** on disk), zero `unsafe` macro expansions (`#![forbid(unsafe_code)]`), lowest 1-tab idle RAM (**221.5 MiB PSS** / `226,805 KiB`, 8 processes).
     - *Cons*:
       - **Broken WebRTC & DRM out of the box on Arch Linux**: Live `--compat-probe` testing on Arch Linux (`webkit2gtk-4.1 2.54.2-1`) proved that `typeof RTCPeerConnection === 'function'` is **`false`** and `typeof navigator.requestMediaKeySystemAccess === 'function'` is **`false`** even with `set_enable_media_stream(true)` and `set_enable_encrypted_media(true)`. Video calls (Meet/Zoom/Discord) and Widevine DRM streaming fail out of the box, and `chase.com` requires WebKitGTK to spoof a macOS Safari User-Agent.
       - **Higher 10-tab RAM & process explosion**: At 10 tabs across 10 origins, WebKitGTK spawns **62 processes** (`WebKitWebProcess` + `bwrap` sandbox helpers per tab) consuming **622.8 MiB PSS** (`637,707 KiB`), which is **35% more PSS RAM than CEF (`461.5 MiB`) and 32% more than Brave (`470.1 MiB`)**.
       - **Subresource ad-blocking gap**: UI-process `connect_decide_policy` blocked **0 of 10** `<script>` ad subresources in the 10-tab benchmark because it only fires for frame navigations; subresource blocking requires either Apple Content Blocking JSON (150k rule ceiling, missing uBO `$removeparam`/`$csp` modifiers, and `add_filter` is commented out in `webkit2gtk-rs`) or a separate C/Rust WebProcess `.so` extension (which still cannot intercept WebSockets or Service Worker fetches).
- **Recommendation**: **Option A (CEF)** is strongly recommended because "Zero tolerance for broken major sites: banking, YouTube, Google Docs must work", video-call/DRM compatibility, 10-tab memory scaling, and Brave-grade subresource ad-blocking are hard requirements that CEF satisfies natively and WebKitGTK fails in live testing.

---

## ADR-004: Chromium / CEF Bloat Reduction & Subprocess Fast-Path
- **Date**: 2026-11-10
- **Status**: Accepted
- **Context**: Stock CEF initializes background telemetry, spare renderer processes, and large default caches, and if `BlockingPipeline` (`adblock-rust`) is initialized before `cef::execute_process`, every child subprocess (`--type=renderer`, `--type=gpu-process`, `--type=utility`, `--type=zygote`) allocates its own unused copy of the adblock engine in private RAM.
- **Decision**:
  1. Execute a fast-path `execute_process` check for `--type=` subprocesses before allocating `BlockingPipeline` so `adblock-rust` exists only in the browser process.
  2. Apply progressive command-line optimizations in `on_before_command_line_processing` (`CefOptimizationLevel::Level3MemoryAndV8Tuning`), appending to CEF's existing `--disable-features` (never overwriting it) to disable `Translate`, `OptimizationHints`, `AutofillServerCommunication`, `CertificateTransparencyComponentUpdater`, `InterestFeedContentSuggestions`, `BrowsingTopics`, `Fledge`, `PrivacySandboxSettings4`, `LiveCaption`, `AudioServiceOutOfProcess`, and `SpareRendererForSitePerProcess`, plus setting `--renderer-process-limit=6`, `--disk-cache-size=33554432`, `--media-cache-size=16777216`, `--enable-features=BackForwardCacheMemoryControls`, and `--js-flags=--optimize-for-size`.
  3. Keep `MediaRouter` / `GlobalMediaControls` enabled because CEF 156 Chrome runtime dereferences `MediaRouter` during `ChromeBrowserView` startup.

---

## ADR-005: Proprietary Codecs (H.264/AAC) & Widevine CDM Strategy
- **Date**: 2026-11-10
- **Status**: Accepted
- **Context**:
  - Spotify's prebuilt CEF binary (`cef_binary_156.0.26..._linux64_minimal`) compiles Chromium with `proprietary_codecs=false ffmpeg_branding="Chromium"`, supporting royalty-free `VP9`, `AV1`, and `Opus` (which covers YouTube), while returning `""` / `false` for patent-encumbered `H.264` (`avc1`) and `AAC` (`mp4a.40.2`).
  - `com.widevine.alpha` (Widevine EME DRM) works in CEF 156 (`mediaKeysCreated: true` for `SW_SECURE_CRYPTO`) when pointed at an existing system/browser `WidevineCdm` directory (`--widevine-cdm-path`), but cannot be bundled in uncertified redistributions without a Google Widevine CWIP license.
- **Decision**:
  - Support on-demand external Widevine CDM loading (`--widevine-cdm-path` or user-configured system CDM path from `/opt/google/chrome/WidevineCdm` / `~/.config/BraveSoftware/Brave-Browser/WidevineCdm`) without redistributing Google's proprietary `libwidevinecdm.so`. Keep it opt-in / on-demand so non-DRM browsing does not spawn the Widevine utility process on sites like YouTube that probe EME on load.
  - For Arch Linux packaging (AUR `PKGBUILD`), provide a source build option for `libcef.so` with `proprietary_codecs=true ffmpeg_branding="Chrome"` (or system `ffmpeg`) so end users compiling locally get full H.264/AAC support without binary patent redistribution liability.

---

## ADR-006: Service Worker Request Interception via Dual `Client::request_handler` + `RequestContextHandler`
- **Date**: 2026-11-10
- **Status**: Accepted
- **Context**: In CEF / Chromium, Service Worker script loads (`RT_SERVICE_WORKER`) and background `fetch()` calls initiated inside an active Service Worker thread do not belong to a single `CefBrowser` frame and can bypass `Client::request_handler` (`BrowserClient::request_handler`).
- **Decision**:
  - Wire both `Client::request_handler` (`SlateRequestHandler`) and `RequestContextHandler::resource_request_handler` (`SlateRequestContextHandler`) to the shared `Arc<BlockingPipeline>`.
  - Create browser instances using `request_context_cef_create_context_shared(request_context_get_global_context(), Some(&mut rc_handler))` so every profile-level request (including Service Worker `fetch()` calls without a `Browser` handle) flows through `SlateResourceRequestHandler`.
  - When `browser` or `frame` is `None` (out-of-tab Service Worker context), fall back to `request.referrer_url()` or `request.url()` as `source_url` so `adblock-rust` evaluates third-partyness accurately.

---

## ADR-007: Confining CEF `unsafe` FFI Macros to `crates/engine-adapter`
- **Date**: 2026-11-10
- **Status**: Accepted
- **Context**: The `cef` crate (`156.4.0+156.0.26`) generates `unsafe extern "C"` reference-counted C-ABI wrapper structs inside its `wrap_app!`, `wrap_client!`, `wrap_request_handler!`, `wrap_request_context_handler!`, `wrap_resource_request_handler!`, `wrap_cookie_access_filter!`, `wrap_life_span_handler!`, and `wrap_load_handler!` proc macros.
- **Decision**:
  - Place all CEF `wrap_*!` macro invocations inside `crates/engine-adapter/src/cef_backend.rs` behind the `cef-engine` Cargo feature.
  - Enforce `#![forbid(unsafe_code)]` on `crates/core`, `crates/blocking`, `crates/ui-bridge`, `spikes/spike-cef`, `spikes/spike-webkit`, and the non-`cef-engine` surface of `crates/engine-adapter`, verified by the automated test `unsafe_code_is_forbidden_in_all_crates_outside_engine_adapter`.

---

## ADR-008: C/ASM Native Dependency Conflict (`ureq` / `ring`) & Filter List Fetcher Strategy
- **Date**: 2026-11-10
- **Status**: Accepted
- **Context**:
  - When adding remote EasyList/EasyPrivacy downloading to `slate-blocking`, adding `ureq = "3.5"` (which pulls in `rustls` and `ring 0.17.14`) caused `slate-spike-cef` to abort immediately at `cef::initialize` with `close symbol missing` (`SIGTRAP`, exit code `133`).
  - **How we bisected it**:
    1. Created a clean git worktree at the last known-good commit (`42764dd`), built `slate-spike-cef`, and verified it exited `0` with valid JSON output.
    2. Applied only the `slate-blocking` + `Cargo.toml` edit (`ureq = "3.5"`) on top of `42764dd`: the crash (`close symbol missing`, exit code `133`) immediately reproduced.
    3. Removed `ureq` from `slate-blocking` and applied all `crates/engine-adapter` changes: `slate-spike-cef` built and exited `0` cleanly.
  - **Root-cause hypothesis (unverified internally)**: Linking `ring`'s static C/assembly archive into the main executable appears to alter dynamic linker symbol resolution/visibility in the host binary, interfering with Chromium/CEF's startup `close` syscall interposition check (`close symbol missing`).
- **Decision**:
  1. **Mandatory Live CEF Launch Smoke Test in `scripts/check.sh`**: Added step `[5/5]` to `./scripts/check.sh` that runs `slate-spike-cef --tabs 1 --exit-after-load` and asserts exit code `0` and valid JSON output on every check run.
  2. **Rule for New Native/C/ASM Dependencies**: Before adding any dependency that compiles or links C/assembly code (`ring`, `aws-lc-rs`, OpenSSL, compression, or TLS crates) into the browser binary, run `./scripts/check.sh` (including the live CEF launch smoke test) immediately.
  3. **Known Tech Debt (`curl` CLI for background filter downloads)**:
     - Currently, `slate-blocking::fetch_https_filter_list` invokes `/usr/bin/curl` with `--fail --proto =https --tlsv1.2` (full TLS certificate verification) and validates payloads via `validate_filter_list_payload` so failed, empty, or HTML error responses never overwrite a good `filters.dat`.
     - If `curl` is missing or offline, `BlockingPipeline::new_with_cache_and_background_refresh` still starts and runs from the cached `~/.local/share/slate/filters.dat` (or `DEFAULT_FILTER_LIST` on first run), logs a clear message (`[slate-blocking] Background filter update skipped (...)`), and never crashes.
     - **Proposed Long-Term Replacement (not yet built)**: Either fetch filter lists through CEF's own network stack (`CefURLRequest` / `BrowserHost` inside `slate-engine-adapter`, which uses Chromium's BoringSSL without linking a second TLS stack into the main binary) or use a tiny standalone updater helper binary (`slate-filter-updater`) that writes `filters.dat` out-of-process.
