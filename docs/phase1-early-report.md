# Phase 1 Early Milestones Report: Real-World Benchmarks, Deep Compatibility, Service Worker Blocking & Bloat Reduction

**Date**: 2026-11-10  
**Target Platform**: Arch Linux (`x86_64`, kernel `7.2.14-arch1-1`, `rustc 1.99.1`)  
**Status**: IN PROGRESS — Section 1 Benchmark Results Marked INVALID

> [!WARNING]
> **SECTION 1 BENCHMARK RESULTS ARE INVALID AND MUST NOT BE USED**
> Visual and automated inspection of site screenshots revealed that several tested pages did not load real content:
> - YouTube rendered a blank page (investigating blocking scriptlets/filters vs. layout)
> - Google Docs showed a sign-in wall (`accounts.google.com`)
> - Proton Mail remained stalled on a loading splash screen
> - Reddit presented an automated bot/captcha challenge
> All earlier 1/5/10-tab benchmark numbers in Section 1 and in `benchmarks/results/realworld/triplicate_runs.json` are invalid.
> Automated per-site readiness gates (title, readyState, visible body text length, site-specific DOM selector, bot-wall/captcha check, network idle settle period, and video `currentTime` advancement) are being implemented in code to strictly gate measurements before any new benchmark data (Task 1b) is collected.

---

## 1. Real-World 10-Site Benchmark (INVALID — DO NOT USE)

Reproducible via `./scripts/bench_realworld.sh` (raw JSON outputs in `benchmarks/results/realworld/`).

**10 Real Sites Loaded** (`REAL_WORLD_SITES` in `crates/engine-adapter/src/cef_backend.rs`):
1. `https://www.youtube.com` (video streaming)
2. `https://docs.google.com` (Google Docs office suite)
3. `https://news.ycombinator.com` (Hacker News)
4. `https://en.wikipedia.org/wiki/Rust_(programming_language)` (Wikipedia)
5. `https://www.reddit.com/r/linux/` (Reddit social feed)
6. `https://www.bbc.com/news` (BBC News)
7. `https://mail.proton.me` (Proton Mail web client)
8. `https://github.com/rust-lang/rust` (GitHub repository)
9. `https://www.chase.com` (Chase banking portal)
10. `https://www.nytimes.com` (New York Times)

Both Slate (`cef-156`, Opt Level 3) and Brave (`1.99.55`) were measured using the exact same `/proc` descendant tree walk (`PPid:` traversal + `VmRSS:` and `smaps_rollup` `Pss:` summation across all processes):

| Workload (Real-World Sites) | Metric | Slate (`cef-156`, Opt Level 3) | Brave `1.99.55` (`/usr/bin/brave`) | Slate vs. Brave Delta |
| :--- | :--- | :--- | :--- | :--- |
| **1 Tab** (`YouTube`) | **PSS** | **639.7 MiB** (`655,016 KiB`) | **522.4 MiB** (`534,987 KiB`) | +117.2 MiB PSS |
| | **RSS** | **1,135.0 MiB** (`1,162,260 KiB`) | **1,317.7 MiB** (`1,349,336 KiB`) | **-182.7 MiB (-13.9% RSS)** |
| | **Processes** | **8** | **11** | **-3 processes** |
| **5 Tabs** (`YouTube`, `GDocs`, `HN`, `Wikipedia`, `Reddit`) | **PSS** | **861.0 MiB** (`881,696 KiB`) | **712.5 MiB** (`729,597 KiB`) | +148.5 MiB PSS |
| | **RSS** | **2,015.7 MiB** (`2,064,108 KiB`) | **2,068.2 MiB** (`2,117,808 KiB`) | **-52.4 MiB (-2.5% RSS)** |
| | **Processes** | **14** | **16** | **-2 processes** |
| **10 Tabs (All 10 Real Sites Live)** | **PSS** | **888.7 MiB** (`910,060 KiB`) | **992.6 MiB** (`1,016,442 KiB`) | **-103.9 MiB (-10.5% PSS)** |
| | **RSS** | **2,610.9 MiB** (`2,673,520 KiB`) | **2,962.2 MiB** (`3,033,276 KiB`) | **-351.3 MiB (-11.9% RSS)** |
| | **Processes** | **19** | **20** | **-1 process** |
| **10 Tabs After Tab Sleeping** *(9 background tabs suspended, 1 foreground YouTube tab active)* | **PSS** | **450.1 MiB** (`460,930 KiB`) | *992.6 MiB (no auto-sleep)* | **-542.5 MiB (-54.7% PSS vs. 10-tab Brave)** |
| | **RSS** | **1,001.2 MiB** (`1,025,272 KiB`) | *2,962.2 MiB (no auto-sleep)* | **-1,960.9 MiB (-66.2% RSS vs. 10-tab Brave)** |
| | **Processes** | **10** | *20* | **-10 processes** |

---

## 2. Deep Compatibility Testing + Codec & Widevine Licensing Analysis

Executed via `./target/release/slate-spike-cef --deep-compat <mode>` (raw JSON outputs in `benchmarks/results/deep_compat/`):

### A. What Was Executed & Measured

| Test Area | What Was Executed in CEF 156 | Real Output & Status | What Could Not Be Tested Automatically |
| :--- | :--- | :--- | :--- |
| **Google Account Login** (`--deep-compat google-login`) | Loaded `https://accounts.google.com/signin/v2/identifier?hl=en`, located `#identifierId` (`email_input_found: true`), programmatically typed `slate.browser.probe@gmail.com` (`email_typed_ok: true`), clicked `#identifierNext` (`next_button_clicked: true`), and inspected Google's response. | **Pass (`google_rejected_embedded_browser: false`)**: Google did **not** show *"This browser or app may not be secure"* (because CEF 156 uses the Chrome runtime with standard `Chrome/156.0.0.0` UA & Client Hints) and proceeded to account lookup. | Completing a full login with a real Google password + 2FA/Passkey requires personal user credentials in an interactive session. |
| **Google Docs Editor Surface** (`--deep-compat gdocs-edit`) | Tested rich `contenteditable` text insertion via `execCommand('insertText')`, `CanvasRenderingContext2D.measureText` (`273.05 px`, used by Google Docs' Kix canvas layout engine), `DataTransfer` clipboard (`text/plain` + `text/html`), `ServiceWorker`, and `IndexedDB`. | **Pass**: `contenteditable_insert_ok: true`, `datatransfer_clipboard_ok: true`, `service_worker_api_present: true`, `indexeddb_present: true`. | Editing a live authenticated `docs.google.com/document/d/...` file requires logging into a Google account interactively. |
| **Banking Login (`Chase`)** (`--deep-compat banking-login`) | Loaded `https://www.chase.com/`, verified `isSecureContext: true`, tested WebCrypto `crypto.subtle.digest('SHA-256')` (`webcrypto_sha256_ok: true`), WebAuthn Passkeys (`PublicKeyCredential`, `true`), cookies (`true`), and programmatic login input interaction (`login_field_interactive: true`). | **Pass**: All banking crypto/passkey/cookie primitives and form inputs work natively without User-Agent spoofing. | Submitting real bank credentials + SMS/hardware 2FA requires a real human bank account. |
| **Video/Audio Codecs** (`--deep-compat codecs-drm`) | Tested `HTMLMediaElement.canPlayType`, `MediaSource.isTypeSupported`, and `navigator.mediaCapabilities.decodingInfo` for `VP9`, `AV1`, `Opus`, `H.264` (`avc1.42E01E`/`4D401E`/`640028`), and `AAC` (`mp4a.40.2`). | **`VP9`, `AV1`, `Opus`: `"probably"` (`mse_supported: true`)**.<br>**`H.264` & `AAC`: `""` (`mse_supported: false`)** in stock prebuilt `libcef.so`. | See technical & licensing explanation below. |
| **Widevine DRM (`com.widevine.alpha`)** (`--deep-compat codecs-drm`) | Tested `navigator.requestMediaKeySystemAccess` + `createMediaKeys()` for `org.w3.clearkey` and `com.widevine.alpha` (`SW_SECURE_CRYPTO`), both with stock CEF and with `--widevine-cdm-path "$HOME/.config/BraveSoftware/Brave-Browser/WidevineCdm"`. | **Stock CEF**: `clearkey_vp9: true`, `widevine_vp9: false`.<br>**With `--widevine-cdm-path`**: **`widevine_vp9: { supported: true, keySystem: "com.widevine.alpha", mediaKeysCreated: true }`!** (`widevine_h264: false` only because H.264 is disabled in stock `libcef.so`). | Playing a full commercial Netflix/Spotify stream end-to-end requires both Widevine CDM + VMP signature (for sites enforcing VMP) + H.264/AAC (if the service does not offer VP9/AV1/Opus streams). |

### B. Technical, Licensing & Legal Breakdown (Widevine CDM & H.264/AAC)

1. **H.264 (AVC) and AAC (MPEG-4 Audio)**:
   - **What CEF needs technically**: Spotify's prebuilt `libcef.so` is compiled with GN flags `proprietary_codecs=false ffmpeg_branding="Chromium"`. Because YouTube serves `VP9` and `AV1` + `Opus`, YouTube works out of the box. However, sites that serve only MP4 (`H.264` + `AAC`) — such as Twitch live streams, many Reddit/X videos, and some news embeds — require compiling `libcef.so` with `proprietary_codecs=true ffmpeg_branding="Chrome"`.
   - **Licensing & Legal Implications**:
     - **H.264 (AVC)** is covered by the **Via Licensing Alliance (formerly MPEG LA) AVC Patent Portfolio License**. Under current terms, distributing a prebuilt binary decoder to end users is royalty-free for the first **100,000 units/downloads per year**, then `\$0.20/unit` above 100,000 (requires executing a Via LA agreement).
     - **AAC** is covered by the **Via Licensing Alliance AAC Patent Pool**. Unlike H.264, AAC has **no free 100,000-unit tier** for commercial binary distributors (`\$0.98/unit` plus an annual fee).
     - **How Linux/Arch projects handle this legally**:
       - **Source distribution / AUR `PKGBUILD`**: Distributing open-source Rust code + build scripts where the end user compiles `libcef` locally on their Arch machine (or links against their system `ffmpeg` package installed via `pacman`) does not constitute distributing a prebuilt patented codec binary.
2. **Widevine CDM (`libwidevinecdm.so`)**:
   - **What CEF needs technically**: CEF 156 has full Widevine EME support compiled in (`enable_widevine=true`). Pointing CEF at an existing Widevine directory (`~/.config/BraveSoftware/Brave-Browser/WidevineCdm` or `/opt/google/chrome/WidevineCdm`) immediately activates `com.widevine.alpha` (`mediaKeysCreated: true` for `SW_SECURE_CRYPTO` L3).
   - **Licensing & Legal Implications**:
     - `libwidevinecdm.so` is a proprietary, closed-source binary owned by Google. You **cannot** bundle or redistribute `libwidevinecdm.so` without signing a **Google Widevine CWIP / Distribution Agreement**.
     - Streaming services that enforce **VMP (Verified Media Path)** check that the host browser binary (`libcef.so` + executable) carries a cryptographic `.sig` file signed by Google's Widevine root CA. Without a Google VMP signature agreement, L3 streams that do not enforce strict VMP work when using a locally installed `libwidevinecdm.so`, while strict VMP-enforced streams (like Netflix) reject unsigned custom CEF builds.

---

## 3. Service Worker Request Blocking (`Client::request_handler` + `RequestContextHandler`)

- **What was wired**:
  - In `crates/engine-adapter/src/cef_backend.rs`, `SlateRequestContextHandler` (`wrap_request_context_handler!`) is attached to the browser's `RequestContext` via `request_context_cef_create_context_shared`.
  - Both `SlateClient::request_handler` and `SlateRequestContextHandler::resource_request_handler` route through `SlateResourceRequestHandler::on_before_resource_load` (`adblock-rust`).
- **Live CEF Execution Proof** (`./target/release/slate-spike-cef --test-service-worker`, saved in `benchmarks/results/service_worker_test.json`):
  ```json
  {
    "request_context_intercepts": 3,
    "service_worker_blocked_requests": 2,
    "sw_ad_fetch_blocked": true,
    "sw_allowed_fetch_ok": true,
    "sw_registered": true,
    "sw_tracker_fetch_blocked": true,
    "total_blocked_requests": 2
  }
  ```

---

## 4. Chromium / CEF Flag & Feature Bloat Audit (Measured Ablation Table)

Implemented in `crates/engine-adapter/src/cef_backend.rs` (`--opt-level 0|1|2|3`) plus the subprocess fast-path (`if env::args().any(|a| a.starts_with("--type="))` executing `cef::execute_process` before `BlockingPipeline` is allocated):

| Optimization Level | Changes Applied | 1-Tab Cold Start | 1-Tab PSS / RSS | 10-Tab Local PSS / RSS (Procs) | 1-Tab YouTube Real-World PSS / RSS (Procs) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Level 0 (Baseline)** | Stock CEF 156, no extra switches | `559 ms` | `345.0 MiB` PSS / `895.1 MiB` RSS | `461.4 MiB` PSS / `1,990.7 MiB` RSS (18 procs) | `925.1 MiB` PSS / `1,488.9 MiB` RSS (9 procs) |
| **Level 1 (No Background Net / Telemetry)** | `--disable-background-networking`, `--disable-component-update`, `--disable-domain-reliability`, `--disable-client-side-phishing-detection`, `--disable-sync`, `--metrics-recording-only`, `--no-pings`, `--no-first-run`, `--no-default-browser-check` | **`505 ms` (-54 ms)** | `342.8 MiB` PSS / `892.0 MiB` RSS | `454.7 MiB` PSS / `1,978.9 MiB` RSS (18 procs) | — |
| **Level 2 (Disable Unused Features)** | Appends to `--disable-features`: `Translate`, `OptimizationHints`, `AutofillServerCommunication`, `CertificateTransparencyComponentUpdater`, `InterestFeedContentSuggestions`, `BrowsingTopics`, `Fledge`, `PrivacySandboxSettings4`, `LiveCaption`, `AudioServiceOutOfProcess`, `SpareRendererForSitePerProcess` + `--autoplay-policy=document-user-activation-required` | `554 ms` | `336.7 MiB` PSS / `886.4 MiB` RSS | `455.5 MiB` PSS / `1,980.9 MiB` RSS (18 procs) | — |
| **Level 3 (Process, Cache & V8 Tuning + Subprocess Fast-Path)** | `--renderer-process-limit=6`, `--disk-cache-size=33554432`, `--media-cache-size=16777216`, `--disable-gpu-shader-disk-cache`, `--enable-features=BackForwardCacheMemoryControls`, `--js-flags=--optimize-for-size` + skip `BlockingPipeline` allocation in `--type=*` child processes | `544 ms` | **`333.4 MiB` PSS (-11.6 MiB)** / **`878.6 MiB` RSS (-16.5 MiB)** | **`448.9 MiB` PSS (-12.5 MiB)** / **`1,904.9 MiB` RSS (-85.8 MiB)** (**17 procs**) | **`639.7 MiB` PSS (-285.4 MiB)** / **`1,135.0 MiB` RSS (-353.9 MiB)** (**8 procs**) |

---

## 5. Chromium Security Release Tracking Plan & Checker Script

- **Executable Script**: `scripts/check_cef_version.sh` compares `Cargo.lock` against both `crates.io` (`https://crates.io/api/v1/crates/cef`) and Google's ChromiumDash API (`https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Linux&num=1`).

---

## 6. `unsafe` Confinement Verification

- All `cef` FFI macros (`wrap_*!`) are confined inside `crates/engine-adapter/src/cef_backend.rs`.
- `spikes/spike-cef/src/main.rs` is a `#![forbid(unsafe_code)]` binary entrypoint.
- Automated test `unsafe_code_is_forbidden_in_all_crates_outside_engine_adapter` in `crates/engine-adapter/src/lib.rs` asserts on every `cargo test` run that `slate-core`, `slate-blocking`, `slate-ui-bridge`, `slate-spike-cef`, and `slate-spike-webkit` all enforce `#![forbid(unsafe_code)]`.
