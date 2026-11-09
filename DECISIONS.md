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
- **Date**: 2026-11-09
- **Status**: Pending User Approval (Phase 0 Gate)
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
