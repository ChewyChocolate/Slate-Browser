# Slate Shell Architecture — design only (nothing here is built yet)

Status: **DESIGN PROPOSAL, awaiting owner's choice.** No code was written for this document.

Legend used throughout:
- **VERIFIED (code)**: read in this repo's source this session (file named).
- **VERIFIED (crate)**: the symbol exists in the `cef` crate source, `cef-156.4.0+156.0.26`,
  file `src/bindings/x86_64_unknown_linux_gnu.rs` (found by grep; this proves the API *exists*, not that it
  *behaves* as hoped).
- **UNVERIFIED**: not checked, or only behaviour is unknown. Must be proven by a running test in step 3.
- **ESTIMATE**: my judgement, no measurement behind it.

---

## 1. Problem and ground truth

### What exists today
- `crates/app/src/main.rs` launches **one** CEF `BrowserView` that loads `crates/app/resources/ui/index.html`.
  That page shows tabs as an `<iframe id="active-web-frame">`. Sites that forbid framing
  (`X-Frame-Options` / CSP `frame-ancestors`, e.g. youtube.com per the owner's test) show Chromium's error page.
  This cannot be fixed inside an iframe, so each tab must be its own top-level browser. (VERIFIED (code):
  `crates/app/resources/ui/shell.js`, `index.html`.)
- The spike (`crates/engine-adapter/src/cef_backend.rs`, `run_with_options`, ~line 1740–1785) already calls
  `browser_view_create(...)` + `window_create_top_level(...)` once per tab, so **a real top-level browser per
  tab already works in the spike** — but each tab gets its own OS window and there is no chrome. (VERIFIED (code).)
- UI → core "IPC" today is scraping `console.log` text with prefixes like `SLATE_PROBE:`
  in `SlateDisplayHandler::on_console_message` (VERIFIED (code), cef_backend.rs ~line 921). It is one-way and
  any web page sharing that client could print the same prefix, so it is **not acceptable** as a command channel.
- `crates/ui-bridge` already defines `UiCommand`, `CoreEvent`, `parse_ui_command(origin, json)` (rejects
  non-`slate://` origins) and an internal CSP. Nothing transports these yet. (VERIFIED (code).)
- `crates/core` has `TabManager`/`TabState` with `Active`/`Suspended` lifecycle and saved scroll. It is pure
  logic; it is not connected to real browsers. (VERIFIED (code).)

### A bug found while reading the filters (affects every option)
`SlateCookieFilter::can_send_cookie/can_save_cookie` (cef_backend.rs ~lines 686–735) passes
`frame.url()` as the "top-level URL" to `should_allow_cookie`. `frame` is the frame that issued the request.
For a third-party iframe that is the **iframe's own URL**, not the page's top-level site, so the cookie would
look first-party. Likewise `on_before_resource_load` uses `frame.url()` as `source_url`, and
`on_before_browse` uses the frame's URL. This is from reading the code; I have **not run a test** to see the
effect. The design below fixes it by resolving the top-level URL from the tab, not the frame.

---

## 2. Facts about the `cef` crate that the options rely on

All VERIFIED (crate) = the symbol exists; behaviour is UNVERIFIED unless noted.

| Need | API found |
|---|---|
| One browser per tab as a view | `browser_view_create(client, url, settings, extra_info, request_context, delegate)` |
| Top-level window | `window_create_top_level(delegate)`; `Window::add_child_view` (already used in repo) |
| Layout | `Panel`/`panel_create`, `View::set_to_fill_layout`, `View::set_to_box_layout(BoxLayoutSettings)`, `BoxLayout::set_flex_for_view`, `View::set_visible`, `View::set_bounds`, `View::request_focus`, `View::add_child_view`/`remove_child_view` |
| Browser from view | `BrowserView::browser()`, `browser_view_get_for_browser(browser)` |
| Browser identity | `Browser::identifier() -> i32`, `Browser::main_frame()` |
| Close / hide | `BrowserHost::close_browser(force)`, `try_close_browser`, `BrowserHost::was_hidden(bool)` |
| Lifecycle callbacks | `LifeSpanHandler::{on_after_created, do_close, on_before_close, on_before_popup}`, `BrowserViewDelegate::{on_browser_created, on_browser_destroyed, on_popup_browser_view_created}` |
| Page events for the UI | `DisplayHandler::{on_address_change, on_title_change, on_favicon_urlchange, on_loading_progress_change, on_fullscreen_mode_change}`, `LoadHandler::on_loading_state_change` |
| Per-request hooks | `RequestHandler::{on_before_browse, resource_request_handler}`, `ResourceRequestHandler::{on_before_resource_load, cookie_access_filter}`, `CookieAccessFilter::{can_send_cookie, can_save_cookie}` — each receives `Option<&mut Browser>` |
| Cookie-site hint | `Request::first_party_for_cookies()` exists; whether Chromium fills it correctly: UNVERIFIED |
| Custom scheme as IPC | `App::on_register_custom_schemes`, `SchemeRegistrar::add_custom_scheme`, `register_scheme_handler_factory`, `SchemeHandlerFactory::create(browser, frame, scheme, request) -> ResourceHandler`, `ResourceHandler` |
| Renderer-side JS bridge (alternative IPC) | `RenderProcessHandler`, `V8Handler`, `v8_value_create_function`, `Frame::send_process_message`, `Client::on_process_message_received` |
| Push events to UI | `Frame::execute_java_script(code, url, line)` |
| Windowed embedding (option 2) | `WindowInfo { parent_window: c_ulong, window: c_ulong, bounds, runtime_style, .. }`, `browser_host_create_browser` / `_sync` |
| Message router (`cefQuery`) | **Not found** in the crate (grep for `message_router` returned nothing). Not available. |

Not verified at all (must be settled by prototype): which **runtime style** (Alloy vs Chrome) our
`browser_view_create` produces in this build (`BrowserViewDelegate::browser_runtime_style`,
`WindowInfo.runtime_style` exist); whether Chrome-style adds its own toolbar to each tab view
(`BrowserView::chrome_toolbar` exists, which suggests it can).

---

## 3. Cross-cutting design (shared by all options)

### 3.1 Tab registry
A single `TabRegistry` in the adapter (behind `Arc<Mutex<…>>`), keyed by `Browser::identifier()`:

```
TabEntry { tab_id: TabId, top_level_url: String, blocked: AtomicU64,
           is_shell: bool, lifecycle: Active|Suspended }
```
`browser_id → TabEntry` lookup is what every filter does first. The shell's own browser is registered with
`is_shell = true` and is never blocked/filtered as a web page.

### 3.2 How filters learn the top-level site (fix for the bug in §1)
- On `RequestHandler::on_before_browse` for the **main frame** and in `DisplayHandler::on_address_change`
  (main frame), write the new URL into `TabEntry.top_level_url`.
- `on_before_resource_load`, `can_send_cookie`, `can_save_cookie` take their `browser` argument,
  call `registry.top_level_url(browser.identifier())`, and pass **that** as the first-party / top-level
  URL to `evaluate_network_request` and `should_allow_cookie`. The frame URL stays available only as the
  *initiator* (useful for adblock-rust's source-URL logic).
- Fallback chain when `browser` is `None` (service-worker / profile-level requests): keep the existing
  ADR-006 behaviour (`request_initiator` → referrer), unchanged.
- Known gaps to test (all UNVERIFIED): (a) during a cross-site navigation, does `on_address_change` fire
  before the first subresource? A stale top-level URL could mis-classify the first requests;
  (b) are `Browser` objects handed to the filters for requests from out-of-process iframes the *tab's*
  browser (expected yes);
  (c) `Request::first_party_for_cookies()` as a cross-check.
- Required test: a top-level page on host A embeds an iframe from host B that sets a cookie;
  assert the filter classes it as third-party. (To be added in step 3.)

### 3.3 UI ↔ Rust core IPC (three candidates, all options use the same one)
| IPC | Direction | Security | Effort | Notes |
|---|---|---|---|---|
| **A. Custom scheme `slate-ipc://…` handled in the browser process** (recommended) | UI→core via `fetch()` POST; core→UI via `execute_java_script` | Handler only answers requests whose browser is the registered shell browser *and* whose origin is `slate://`; payload still goes through `ui-bridge::parse_ui_command` | Low–medium | No renderer-process code. UNVERIFIED: that `fetch()` to a custom scheme works from the shell origin (needs scheme registered with CORS/fetch enabled flags) |
| B. Renderer `V8Handler` + process messages | both | Strong (messages carry the browser id) | Medium–high | Needs `RenderProcessHandler` running in the subprocess; more FFI surface in `engine-adapter` |
| C. Console-message scraping (today) | UI→core only | **Weak** — spoofable by web content | Zero | Keep only for the existing probes; never for commands |

Recommendation: **A**, with per-message validation through the existing `ui-bridge` crate.
The shell is served from a `slate://` scheme (not `file://`) so origin checks mean something.
Events (`TabsUpdated`, per-tab blocked count, title/URL/loading) are pushed with `execute_java_script`
to the shell frame only.

### 3.4 Per-tab blocked counter fed from the real pipeline
Today's counters are global atomics in `SharedSpikeState` (VERIFIED (code)) — no per-tab data exists.
Design:
1. In `SlateResourceRequestHandler::on_before_resource_load`, when `decision.should_block` is true
   (the real `adblock-rust` decision), look up `TabEntry` by `browser.identifier()` and
   `blocked.fetch_add(1)`. If `browser` is `None` (service worker), only the global counter increments.
2. Reset `blocked` to 0 when the **main frame** commits a new document (`on_before_browse`, main frame,
   not same-document). UNVERIFIED: whether to reset at start vs. commit; requests that race the navigation
   may be attributed to the wrong page. The prototype must test this.
3. A UI throttle (≤ ~4 events/s ESTIMATE) pushes `{event:"tab_blocked", tab_id, count}` to the shell.
4. The UI shows `-` until a first event arrives for that tab, and shows the number verbatim afterwards.
   The shell never computes or estimates a count. `scripts/check.sh` stage 9 greps `shell.js` for hardcoded
   counts.
5. What the number means (must be labelled in UI/docs): **network requests cancelled by the filter engine**.
   It does not include cosmetic-filter hides, scriptlet injections, or cookies refused by `can_save_cookie`
   (those could get separate counters later).
6. Tests: a local test page that loads N known-blocked URLs; assert the tab's count equals the number of
   cancelled requests observed independently in the adapter log. A second tab must show its own count.

### 3.5 Tab sleeping = destroy and recreate the browser, never the profile
- All tabs share one `RequestContext` (existing code: `request_context_cef_create_context_shared`,
  VERIFIED (code), ADR-006), so cookies/storage survive destroying a tab's browser.
- **Sleep(tab)**: only if `TabManager::can_suspend` allows (`is_playing_audio`/`is_playing_video`
  false — UNVERIFIED who sets these flags today; no audio/video observer is wired up); read scroll
  (`execute_java_script` result back through IPC), `TabManager::suspend_tab`, remove the view from the layout
  (`remove_child_view`), `BrowserHost::close_browser(true)`, wait for `on_before_close`, drop the
  registry's browser handle, keep `TabEntry` (url, title, scroll, last blocked count).
- **Wake(tab)**: `browser_view_create(url = saved_url, shared context)`, add to layout, re-register
  the new `Browser::identifier()` under the **same** `TabId`, restore scroll after `on_loading_state_change`
  (not loading). Form contents, JS state, in-page video position are lost by design.
- Cheaper first step available (VERIFIED (crate) it exists): `BrowserHost::was_hidden(true)` — does not free
  memory; not a substitute.
- Whether destroying a tab actually frees its renderer under site-per-process is UNVERIFIED; measure with
  `measure_process_tree_detailed` (exists in the adapter) before claiming savings.
  **No new memory benchmarks are being run now** (owner instruction); any sleep test only checks
  functional correctness (tab comes back at the same URL).

### 3.6 Popups, focus, fullscreen
`LifeSpanHandler::on_before_popup` → cancel and open the URL as a new tab (`window.open` for OAuth popups
needs real testing: Google sign-in and bank logins use popups/`postMessage` — **UNVERIFIED**, a hard
requirement, test it). Fullscreen video: `on_fullscreen_mode_change` → hide the shell view and let the tab view
fill the window (UNVERIFIED).

### 3.7 Security constraints kept in every option
Sandbox on, `--site-per-process` preserved (no `--renderer-process-limit`), no `unsafe` outside
`crates/engine-adapter`, shell page has `INTERNAL_UI_CSP`, web tabs never receive the IPC scheme.
The shell browser and tab browsers use **different `Client` objects** so shell-only handlers are never
attached to web content.

---

## 4. Options

### Option 1 — CEF Views: one shell view + one browser view per tab (full design)
```
Window (window_create_top_level)
 └─ root Panel, BoxLayout horizontal
     ├─ shell BrowserView  (slate://shell, fixed width, sidebar + toolbar)   flex 0
     └─ content Panel, FillLayout                                            flex 1
         ├─ tab BrowserView #1  (visible)
         ├─ tab BrowserView #2  (set_visible(false))
         └─ …
```
- **Layout caveat (decision for owner):** an L-shaped chrome (sidebar full-height + toolbar above content only)
  cannot be a single rectangular view. Two clean ways: (a) move back/forward/address bar *into* the sidebar
  column (vertical-tabs style, one shell view — matches "one view for sidebar/toolbar"); or
  (b) use two shell views (sidebar and toolbar), which likely costs an extra renderer process
  (ESTIMATE; the spike smoke output shows a renderer at 60,569 kB PSS in the task output of this session,
  one run only, not a benchmark). Recommendation: (a).
- **Effort:** medium (ESTIMATE: the largest items are the tab registry, filter changes, IPC scheme, layout).
- **Risk:** medium. Risks: runtime-style behaviour (see §2), Wayland focus/keyboard routing between
  views (UNVERIFIED), OAuth popups, fullscreen.
- **Carries over:** `crates/blocking` untouched; request/resource/cookie handlers (edited to use the
  registry); shared request context + SW handler (ADR-006); `window_create_top_level` + `add_child_view`
  usage; `crates/core::TabManager`; `crates/ui-bridge` types + origin validation; shell HTML/CSS and tab-list
  rendering in `shell.js`; `check.sh` stages incl. site isolation. **Discarded:** the iframe viewport,
  console-scraping for commands, global-only counters.
- **Filters / top-level site:** §3.2 (registry keyed by browser id, written on main-frame navigation).
- **Sleeping:** §3.5, `remove_child_view` + `close_browser` + recreate.
- **Counter:** §3.4.

### Option 2 — Windowed/native embedding: tab browsers as child windows, HTML chrome in a separate browser
```
Native top-level window (created by us)
 ├─ child window ← chrome browser (HTML shell)         via WindowInfo.parent_window
 └─ child window ← tab browser(s)                       via WindowInfo.parent_window, one shown at a time
```
- `WindowInfo.parent_window` is an `unsigned long` (VERIFIED (crate)) = an **X11 window id**. On this
  machine the session is Wayland (`WAYLAND_DISPLAY=wayland-1`, `DISPLAY=:1` present, observed this session via
  `echo`). Whether windowed CEF parents correctly under Wayland, or only via XWayland
  (`--ozone-platform=x11`): UNVERIFIED and likely a real blocker for Arch/Wayland.
- We would need our own native window and manual resize/z-order handling → a windowing crate (winit/GTK/x11rb).
  Any of these links C or does raw handle work: ADR-008 rule applies (smoke test before and after), and raw
  window-handle calls are `unsafe` and must live in `engine-adapter`.
- **Effort:** high. **Risk:** high (Wayland, input focus, DPI, resize flicker, drag regions).
- **Carries over:** same filters/registry/IPC/counters design (§3 is engine-view-agnostic); `crates/blocking`;
  `core`, `ui-bridge`. **Discarded:** the Views code in `cef_backend.rs`.
- **Filters / top-level site:** identical to §3.2 — still keyed by `Browser::identifier()`.
- **Sleeping:** `close_browser` on the child window's browser, destroy/hide the X11 child, recreate with
  `browser_host_create_browser` into a new child window.
- **Counter:** identical (§3.4).
- Why consider it: avoids Views-layout limits (L-shaped chrome, overlays). Why not: the Wayland question and
  a new native-window stack for a lightweight browser.

### Option 3 — Minimum build effort: Option 1 cut down to the thinnest slice
This is deliberately a **subset of Option 1**, not a different architecture, so no work is thrown away.
- Single window; shell view = sidebar column containing tab list + address bar + back/forward/reload
  (layout (a) above); tab content = one `BrowserView` per tab in a fill-layout panel, switching via
  `set_visible`.
- IPC: **console-scrape for *shell → core* is explicitly not used**; instead the minimal piece of IPC A
  (scheme handler with 4 commands: `open_tab`, `focus_tab`, `close_tab`, `navigate`).
- Counters: per-tab counter via registry (§3.4) — required, because the fake counter removal needs real data.
- **Cut for v1:** tab sleeping, popups beyond "open in new tab", fullscreen handling, favicons, history.
- **Effort:** lowest (ESTIMATE); **Risk:** low–medium — the cut features carry the OAuth-popup risk, which is
  a hard requirement, so popups-as-tabs is **kept**, not cut. Sleep can wait: it only needs `TabManager` +
  §3.5 and nothing else depends on it.
- **Carries over:** everything Option 1 carries over.

---

## 5. Comparison

| | 1. Views (full) | 2. Windowed + HTML chrome | 3. Views, thinnest slice |
|---|---|---|---|
| Effort | medium (ESTIMATE) | high (ESTIMATE) | lowest (ESTIMATE) |
| Biggest risk | Wayland focus, runtime style | Wayland/X11 parenting, new native stack | OAuth popups, same as 1 |
| New C-linking deps (ADR-008) | none | likely (windowing crate) | none |
| Reuses current Views code | yes | no | yes |
| Fixes iframe/`frame-ancestors` problem | yes | yes | yes |
| Upgrade path to full | n/a | rewrite | extend to Option 1 |

## 6. Recommendation
**Option 3 as the first build, designed to grow into Option 1.** Same architecture, smallest first deliverable,
no new C dependency, reuses code that already runs on this Wayland session (the spike creates Views windows and
the shell screenshot in `screenshots/step1-no-fake-counters.png` is a Views window). Reject Option 2 unless
Option 1 fails a test for a Wayland-specific reason.

Decisions I need from you:
1. Option 1, 2, or 3 (recommended: 3 → 1).
2. Toolbar placement: inside the sidebar column (recommended) vs. a separate toolbar view.
3. IPC: custom scheme (A, recommended) vs. renderer V8 bridge (B).

## 7. What the prototype (step 3) must prove, mapped to UNVERIFIED items
1. youtube.com top-level, video actually playing (read `currentTime` advancing; OS screenshot).
2. Filter sees top-level site: iframe-cookie test; per-tab blocked counts differ between two tabs.
3. Frame-restricted page loads in a tab and is not a Chromium error page (check.sh test).
4. Third-party-cookie login flow on a test page.
5. OAuth-style `window.open` popup opens as a tab and `postMessage` back works.
6. Runtime style: no unexpected Chrome toolbar inside tab views.
7. IPC: `fetch()` to the custom scheme works from the shell origin and is rejected from a web tab.
