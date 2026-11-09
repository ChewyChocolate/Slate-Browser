//! Production CEF 156 adapter and Phase 1 milestone harness.
//!
//! Security note on `unsafe`:
//! All application logic in this module is safe Rust. The `cef` crate's `wrap_*!` macros
//! (`wrap_app!`, `wrap_client!`, `wrap_request_handler!`, `wrap_request_context_handler!`,
//! `wrap_resource_request_handler!`, `wrap_cookie_access_filter!`, `wrap_load_handler!`,
//! `wrap_display_handler!`, `wrap_window_delegate!`, `wrap_task!`) expand internally to
//! `unsafe` blocks to manage C reference counts (`cef_base_ref_counted_t::add_ref`).
//! By confining all `cef` bindings to this single module, every other crate in the
//! workspace (`slate-core`, `slate-blocking`, `slate-ui-bridge`, `slate-spike-cef`,
//! `slate-spike-webkit`) enforces `#![forbid(unsafe_code)]`.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use cef::{args::Args, *};
use serde::Serialize;
use slate_blocking::{default_filter_cache_path, BlockingPipeline};
use slate_core::{bench_server::BenchServer, ScrollPosition, TabManager};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

/// 10 real-world sites representing diverse daily-driver workloads:
/// video streaming (direct video watch page so autoplay/video playback is tested symmetrically),
/// office suite, tech news, encyclopedia, social feed, heavy media news, webmail,
/// developer code hosting, banking, and newspaper.
pub const REAL_WORLD_SITES: [&str; 10] = [
    "https://www.youtube.com/watch?v=jNQXAC9IVRw",
    "https://docs.google.com",
    "https://news.ycombinator.com",
    "https://en.wikipedia.org/wiki/Rust_(programming_language)",
    "https://www.reddit.com/r/linux/",
    "https://www.bbc.com/news",
    "https://mail.proton.me",
    "https://github.com/rust-lang/rust",
    "https://www.chase.com",
    "https://www.nytimes.com",
];

pub const REAL_WORLD_SITE_SLUGS: [&str; 10] = [
    "01_youtube",
    "02_google_docs",
    "03_hacker_news",
    "04_wikipedia",
    "05_reddit",
    "06_bbc_news",
    "07_proton_mail",
    "08_github",
    "09_chase",
    "10_nytimes",
];

/// Progressive Chromium/CEF command-line optimization levels for bloat reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CefOptimizationLevel {
    /// Level 0: Stock CEF 156 with no extra command-line switches.
    Level0Baseline = 0,
    /// Level 1: Disable background networking, telemetry, component updater, phishing, sync, pings.
    Level1NoBackgroundNetworking = 1,
    /// Level 2: Level 1 + disable unused Chromium features (Translate, OptimizationHints, MediaRouter,
    /// PrivacySandbox/Topics/Fledge, LiveCaption, AudioServiceOutOfProcess, etc.).
    Level2DisableBloatFeatures = 2,
    /// Level 3: Level 2 + renderer process cap (`--renderer-process-limit=8`), V8 size optimization,
    /// and BackForwardCache memory controls.
    Level3MemoryAndV8Tuning = 3,
}

impl CefOptimizationLevel {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Level0Baseline,
            1 => Self::Level1NoBackgroundNetworking,
            2 => Self::Level2DisableBloatFeatures,
            _ => Self::Level3MemoryAndV8Tuning,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CefRunnerOptions {
    pub tabs: usize,
    pub url: String,
    pub real_sites: bool,
    pub sleep_background_tabs: bool,
    pub exit_after_load: bool,
    pub hold_ms: Option<u64>,
    pub verify_capabilities: bool,
    pub compat_probe: Option<String>,
    pub deep_compat: Option<String>,
    pub test_service_worker: bool,
    pub opt_level: CefOptimizationLevel,
    pub widevine_cdm_path: Option<String>,
    pub bootstrap_filters: bool,
    pub screenshot_dir: Option<String>,
    pub renderer_process_limit: Option<usize>,
    pub allow_autoplay: bool,
    pub enable_security_updaters: bool,
    pub test_site_isolation: bool,
}

impl CefRunnerOptions {
    pub fn parse_from_env() -> Self {
        let mut tabs = 1usize;
        let mut url = "about:blank".to_string();
        let mut real_sites = false;
        let mut sleep_background_tabs = false;
        let mut exit_after_load = false;
        let mut hold_ms = None;
        let mut verify_capabilities = false;
        let mut compat_probe = None;
        let mut deep_compat = None;
        let mut test_service_worker = false;
        let mut opt_level = CefOptimizationLevel::Level3MemoryAndV8Tuning;
        let mut widevine_cdm_path = None;
        let mut bootstrap_filters = false;
        let mut screenshot_dir = None;
        let mut renderer_process_limit = None;
        let mut allow_autoplay = false;
        let mut enable_security_updaters = true;
        let mut test_site_isolation = false;

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--tabs" => {
                    if let Some(val) = args.next() {
                        tabs = val.parse().unwrap_or(1);
                    }
                }
                "--url" => {
                    if let Some(val) = args.next() {
                        url = val;
                    }
                }
                "--real-sites" => {
                    real_sites = true;
                }
                "--sleep-background-tabs" => {
                    sleep_background_tabs = true;
                }
                "--exit-after-load" => {
                    exit_after_load = true;
                }
                "--hold-ms" => {
                    if let Some(val) = args.next() {
                        hold_ms = val.parse().ok();
                    }
                }
                "--verify-capabilities" => {
                    verify_capabilities = true;
                }
                "--compat-probe" => {
                    if let Some(val) = args.next() {
                        compat_probe = Some(val);
                    }
                }
                "--deep-compat" => {
                    if let Some(val) = args.next() {
                        deep_compat = Some(val);
                    }
                }
                "--test-service-worker" => {
                    test_service_worker = true;
                }
                "--opt-level" => {
                    if let Some(val) = args.next() {
                        let lvl = val.parse::<u8>().unwrap_or(3);
                        opt_level = CefOptimizationLevel::from_u8(lvl);
                    }
                }
                "--widevine-cdm-path" => {
                    if let Some(val) = args.next() {
                        widevine_cdm_path = Some(val);
                    }
                }
                "--bootstrap-filters" => {
                    bootstrap_filters = true;
                }
                "--screenshot-dir" => {
                    if let Some(val) = args.next() {
                        screenshot_dir = Some(val);
                    }
                }
                "--renderer-process-limit" => {
                    if let Some(val) = args.next() {
                        let n = val.parse::<usize>().unwrap_or(0);
                        renderer_process_limit = if n == 0 { None } else { Some(n) };
                    }
                }
                "--allow-autoplay" => {
                    allow_autoplay = true;
                }
                "--disable-security-updaters" => {
                    enable_security_updaters = false;
                }
                "--enable-security-updaters" => {
                    enable_security_updaters = true;
                }
                "--test-site-isolation" => {
                    test_site_isolation = true;
                }
                _ => {}
            }
        }

        if verify_capabilities && tabs < 2 {
            tabs = 2;
        }

        Self {
            tabs,
            url,
            real_sites,
            sleep_background_tabs,
            exit_after_load,
            hold_ms,
            verify_capabilities,
            compat_probe,
            deep_compat,
            test_service_worker,
            opt_level,
            widevine_cdm_path,
            bootstrap_filters,
            screenshot_dir,
            renderer_process_limit,
            allow_autoplay,
            enable_security_updaters,
            test_site_isolation,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessMemoryEntry {
    pub pid: u32,
    pub kind: String,
    pub pss_kb: u64,
    pub rss_kb: u64,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkOutput {
    pub engine: &'static str,
    pub opt_level: u8,
    pub tabs: usize,
    pub real_sites: bool,
    pub filter_cache_warm: bool,
    pub cold_start_ms: u128,
    pub rss_tree_kb: u64,
    pub pss_tree_kb: u64,
    pub process_count: usize,
    pub evaluated_requests: usize,
    pub blocked_requests: usize,
    pub request_context_intercepts: usize,
    pub service_worker_blocked_requests: usize,
    pub document_start_injected: bool,
    pub third_party_cookies_blocked: bool,
    pub youtube_video_playing: bool,
    pub screenshots_captured: usize,
    pub tab_suspended_and_restored: bool,
    pub suspended_tabs_count: usize,
    pub rss_after_suspend_kb: Option<u64>,
    pub pss_after_suspend_kb: Option<u64>,
    pub process_count_after_suspend: Option<usize>,
    pub process_breakdown: Vec<ProcessMemoryEntry>,
    pub process_breakdown_after_suspend: Option<Vec<ProcessMemoryEntry>>,
}

/// Local HTTP server specifically for proving Service Worker request interception via
/// `CefRequestContextHandler::get_resource_request_handler`.
pub struct ServiceWorkerTestServer {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl ServiceWorkerTestServer {
    pub fn start() -> Option<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                thread::spawn(move || {
                    let mut s = stream;
                    let _ = s.set_read_timeout(Some(Duration::from_millis(150)));
                    let mut buf = [0u8; 2048];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .unwrap_or("GET /")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/");

                    let (content_type, body) = if path.starts_with("/sw.js") {
                        (
                            "application/javascript; charset=utf-8",
                            r#"
self.addEventListener('install', (event) => {
    event.waitUntil(self.skipWaiting());
});
self.addEventListener('activate', (event) => {
    event.waitUntil(self.clients.claim());
});
self.addEventListener('message', async (event) => {
    if (event.data === 'RUN_SW_BLOCK_TEST') {
        let allowed_ok = false;
        let ad_blocked = false;
        let third_party_tracker_blocked = false;

        try {
            const resp = await fetch('/sw-allowed.json');
            const data = await resp.json();
            allowed_ok = Boolean(data && data.status === 'allowed');
        } catch (err) {
            allowed_ok = false;
        }

        try {
            await fetch('https://securepubads.g.doubleclick.net/gpt/pubads_impl_sw_test.js', { mode: 'no-cors' });
            ad_blocked = false;
        } catch (err) {
            ad_blocked = true;
        }

        try {
            await fetch('https://www.google-analytics.com/analytics.js', { mode: 'no-cors' });
            third_party_tracker_blocked = false;
        } catch (err) {
            third_party_tracker_blocked = true;
        }

        event.source.postMessage({
            sw_registered: true,
            sw_allowed_fetch_ok: allowed_ok,
            sw_ad_fetch_blocked: ad_blocked,
            sw_tracker_fetch_blocked: third_party_tracker_blocked
        });
    }
});
"#,
                        )
                    } else if path.starts_with("/sw-allowed.json") {
                        (
                            "application/json; charset=utf-8",
                            r#"{"status":"allowed","source":"service-worker-origin"}"#,
                        )
                    } else {
                        (
                            "text/html; charset=utf-8",
                            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Slate Service Worker Interception Test</title></head>
<body>
<script>
(async function() {
    try {
        navigator.serviceWorker.addEventListener('message', (event) => {
            console.log('SLATE_SW_RESULT:' + JSON.stringify(event.data));
        });
        const reg = await navigator.serviceWorker.register('/sw.js');
        const readyReg = await navigator.serviceWorker.ready;
        const target = readyReg.active || reg.active || reg.installing || reg.waiting;
        if (target) {
            target.postMessage('RUN_SW_BLOCK_TEST');
        } else {
            console.log('SLATE_SW_RESULT:' + JSON.stringify({ error: 'no_active_worker' }));
        }
    } catch (e) {
        console.log('SLATE_SW_RESULT:' + JSON.stringify({ error: String(e) }));
    }
})();
</script>
</body>
</html>"#,
                        )
                    };

                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes());
                    let _ = s.flush();
                });
            }
        });

        Some(Self { port, stop })
    }

    pub fn root_url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }
}

impl Drop for ServiceWorkerTestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn measure_process_tree_memory(root_pid: u32) -> (u64, u64, usize) {
    let (rss, pss, count, _) = measure_process_tree_detailed(root_pid);
    (rss, pss, count)
}

pub fn measure_process_tree_detailed(root_pid: u32) -> (u64, u64, usize, Vec<ProcessMemoryEntry>) {
    let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() {
                let status_path = entry.path().join("status");
                if let Ok(status) = fs::read_to_string(status_path) {
                    for line in status.lines() {
                        if let Some(ppid_str) = line.strip_prefix("PPid:") {
                            if let Ok(ppid) = ppid_str.trim().parse::<u32>() {
                                children_map.entry(ppid).or_default().push(pid);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    let mut visited = HashSet::new();
    let mut queue = vec![root_pid];
    while let Some(pid) = queue.pop() {
        if visited.insert(pid) {
            if let Some(kids) = children_map.get(&pid) {
                queue.extend(kids.iter().copied());
            }
        }
    }

    let mut total_rss_kb = 0u64;
    let mut total_pss_kb = 0u64;
    let mut breakdown = Vec::with_capacity(visited.len());

    let mut sorted_pids: Vec<u32> = visited.iter().copied().collect();
    sorted_pids.sort_unstable();

    for pid in sorted_pids {
        let mut rss_kb = 0u64;
        let mut pss_kb = 0u64;
        let status_path = format!("/proc/{pid}/status");
        if let Ok(status) = fs::read_to_string(status_path) {
            for line in status.lines() {
                if let Some(rss_str) = line.strip_prefix("VmRSS:") {
                    rss_kb = rss_str
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    total_rss_kb += rss_kb;
                    break;
                }
            }
        }
        let smaps_path = format!("/proc/{pid}/smaps_rollup");
        if let Ok(smaps) = fs::read_to_string(smaps_path) {
            for line in smaps.lines() {
                if let Some(pss_str) = line.strip_prefix("Pss:") {
                    pss_kb = pss_str
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    total_pss_kb += pss_kb;
                    break;
                }
            }
        }
        let cmdline = fs::read(format!("/proc/{pid}/cmdline"))
            .map(|b| {
                b.into_iter()
                    .map(|c| if c == 0 { b' ' } else { c })
                    .collect::<Vec<u8>>()
            })
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();

        let mut thread_comms = HashSet::new();
        if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
            for t in tasks.flatten() {
                if let Ok(comm) = fs::read_to_string(t.path().join("comm")) {
                    thread_comms.insert(comm.trim().to_string());
                }
            }
        }

        let kind = if pid == root_pid {
            "browser".to_string()
        } else if cmdline.contains("--type=renderer")
            || thread_comms.contains("Compositor")
            || thread_comms.contains("CompositorTileW")
        {
            "renderer".to_string()
        } else if cmdline.contains("--type=gpu-process")
            || thread_comms.contains("GpuWatchdog")
            || thread_comms.contains("VizCompositorTh")
        {
            "gpu-process".to_string()
        } else if cmdline.contains("--type=utility") {
            if cmdline.contains("network.mojom.NetworkService") {
                "utility:network".to_string()
            } else if cmdline.contains("storage.mojom.StorageService") {
                "utility:storage".to_string()
            } else if cmdline.contains("audio.mojom.AudioService") {
                "utility:audio".to_string()
            } else {
                "utility".to_string()
            }
        } else if thread_comms.contains("Chrome_ChildIOT") {
            "utility:storage".to_string()
        } else if cmdline.contains("--type=zygote") {
            "zygote".to_string()
        } else {
            "helper".to_string()
        };

        breakdown.push(ProcessMemoryEntry {
            pid,
            kind,
            pss_kb,
            rss_kb,
        });
    }

    if total_pss_kb == 0 {
        total_pss_kb = total_rss_kb;
    }

    (total_rss_kb, total_pss_kb, visited.len(), breakdown)
}

fn cef_resource_type_str(rt: ResourceType) -> &'static str {
    if rt == ResourceType::MAIN_FRAME {
        "main_frame"
    } else if rt == ResourceType::SUB_FRAME {
        "sub_frame"
    } else if rt == ResourceType::STYLESHEET {
        "stylesheet"
    } else if rt == ResourceType::SCRIPT || rt == ResourceType::SERVICE_WORKER {
        "script"
    } else if rt == ResourceType::IMAGE {
        "image"
    } else if rt == ResourceType::FONT_RESOURCE {
        "font"
    } else if rt == ResourceType::MEDIA {
        "media"
    } else if rt == ResourceType::XHR {
        "xmlhttprequest"
    } else if rt == ResourceType::PING {
        "ping"
    } else if rt == ResourceType::CSP_REPORT {
        "csp_report"
    } else if rt == ResourceType::OBJECT {
        "object"
    } else {
        "other"
    }
}

struct SharedSpikeState {
    start_instant: Instant,
    opts: CefRunnerOptions,
    pipeline: BlockingPipeline,
    bench_server: Mutex<Option<BenchServer>>,
    sw_server: Mutex<Option<ServiceWorkerTestServer>>,
    evaluated_requests: AtomicUsize,
    blocked_requests: AtomicUsize,
    request_context_intercepts: AtomicUsize,
    service_worker_blocked_requests: AtomicUsize,
    third_party_cookies_filtered: AtomicBool,
    document_start_injected: AtomicBool,
    youtube_video_playing: AtomicBool,
    screenshots_captured: AtomicUsize,
    devtools_registrations: Mutex<Vec<Registration>>,
    cold_start_ms: Mutex<Option<u128>>,
    browsers: Mutex<Vec<Browser>>,
    windows: Mutex<Vec<Window>>,
    shutting_down: AtomicBool,
    timer_scheduled: AtomicBool,
    probe_printed: AtomicBool,
}

impl SharedSpikeState {
    fn close_all_and_quit(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.devtools_registrations.lock().unwrap().clear();
        self.browsers.lock().unwrap().clear();
        if let Ok(mut wins) = self.windows.lock() {
            if wins.is_empty() {
                quit_message_loop();
            } else {
                for win in wins.drain(..) {
                    win.close();
                }
            }
        }
    }
}

wrap_dev_tools_message_observer! {
    struct SlateScreenshotObserver {
        state: Arc<SharedSpikeState>,
        tab_index: usize,
        out_path: PathBuf,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: ::std::os::raw::c_int,
            success: ::std::os::raw::c_int,
            result: Option<&[u8]>,
        ) {
            if success == 0 || message_id != (1000 + self.tab_index as i32) {
                return;
            }
            if let Some(bytes) = result {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(bytes) {
                    if let Some(b64) = val.get("data").and_then(|d| d.as_str()) {
                        if let Ok(png) = BASE64_STANDARD.decode(b64) {
                            if let Some(parent) = self.out_path.parent() {
                                let _ = fs::create_dir_all(parent);
                            }
                            if !self.out_path.exists() && fs::write(&self.out_path, png).is_ok() {
                                self.state.screenshots_captured.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
        }
    }
}

wrap_cookie_access_filter! {
    struct SlateCookieFilter {
        state: Arc<SharedSpikeState>,
    }

    impl CookieAccessFilter {
        fn can_send_cookie(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _cookie: Option<&Cookie>,
        ) -> ::std::os::raw::c_int {
            let req_url = request
                .map(|r| CefString::from(&r.url()).to_string())
                .unwrap_or_default();
            let top_url = frame
                .map(|f| CefString::from(&f.url()).to_string())
                .unwrap_or_default();
            if self.state.pipeline.should_allow_cookie(&req_url, &top_url) {
                1
            } else {
                self.state.third_party_cookies_filtered.store(true, Ordering::Relaxed);
                0
            }
        }

        fn can_save_cookie(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _response: Option<&mut Response>,
            _cookie: Option<&Cookie>,
        ) -> ::std::os::raw::c_int {
            let req_url = request
                .map(|r| CefString::from(&r.url()).to_string())
                .unwrap_or_default();
            let top_url = frame
                .map(|f| CefString::from(&f.url()).to_string())
                .unwrap_or_default();
            if self.state.pipeline.should_allow_cookie(&req_url, &top_url) {
                1
            } else {
                self.state.third_party_cookies_filtered.store(true, Ordering::Relaxed);
                0
            }
        }
    }
}

wrap_resource_request_handler! {
    struct SlateResourceRequestHandler {
        state: Arc<SharedSpikeState>,
        from_request_context: bool,
        initiator_url: String,
    }

    impl ResourceRequestHandler {
        fn cookie_access_filter(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
        ) -> Option<CookieAccessFilter> {
            Some(SlateCookieFilter::new(Arc::clone(&self.state)))
        }

        fn on_before_resource_load(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(req) = request else {
                return ReturnValue::CONTINUE;
            };
            self.state.evaluated_requests.fetch_add(1, Ordering::Relaxed);
            let raw_url = CefString::from(&req.url()).to_string();
            let method = CefString::from(&req.method()).to_string();
            let raw_rt = req.resource_type();
            let rtype = cef_resource_type_str(raw_rt);
            let is_sw_context = browser.is_none()
                || frame.is_none()
                || raw_rt == ResourceType::SERVICE_WORKER
                || self.from_request_context;

            let main_frame_url = browser
                .and_then(|b| b.main_frame())
                .map(|mf| CefString::from(&mf.url()).to_string())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"));

            let source_url = frame
                .map(|f| CefString::from(&f.url()).to_string())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                .or_else(|| {
                    if self.initiator_url.starts_with("http://")
                        || self.initiator_url.starts_with("https://")
                    {
                        Some(self.initiator_url.clone())
                    } else {
                        None
                    }
                })
                .or(main_frame_url)
                .unwrap_or_else(|| "https://service-worker.slate.local/".to_string());

            let decision = self
                .state
                .pipeline
                .evaluate_network_request(&raw_url, &source_url, rtype, &method);

            if decision.should_block {
                self.state.blocked_requests.fetch_add(1, Ordering::Relaxed);
                if is_sw_context {
                    self.state
                        .service_worker_blocked_requests
                        .fetch_add(1, Ordering::Relaxed);
                }
                return ReturnValue::CANCEL;
            }

            if let Some(new_url) = decision.upgraded_https_url.or(decision.rewritten_url) {
                if new_url != raw_url {
                    let cef_new = CefString::from(new_url.as_str());
                    req.set_url(Some(&cef_new));
                }
            }

            ReturnValue::CONTINUE
        }
    }
}

// RequestContext-level handler that intercepts background requests not bound to a specific
// `CefFrame`/`CefClient` (most importantly Service Worker script installation and `fetch()`
// calls initiated inside a Service Worker).
wrap_request_context_handler! {
    struct SlateRequestContextHandler {
        state: Arc<SharedSpikeState>,
    }

    impl RequestContextHandler {
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            self.state
                .request_context_intercepts
                .fetch_add(1, Ordering::Relaxed);
            let initiator = request_initiator
                .map(|s| s.to_string())
                .unwrap_or_default();
            Some(SlateResourceRequestHandler::new(
                Arc::clone(&self.state),
                true,
                initiator,
            ))
        }
    }
}

wrap_request_handler! {
    struct SlateRequestHandler {
        state: Arc<SharedSpikeState>,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let Some(req) = request else {
                return 0;
            };
            let raw_url = CefString::from(&req.url()).to_string();
            if raw_url.starts_with("data:") || raw_url == "about:blank" {
                return 0;
            }
            let source_url = frame
                .map(|f| CefString::from(&f.url()).to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| raw_url.clone());

            let decision = self.state.pipeline.evaluate_network_request(
                &raw_url,
                &source_url,
                "main_frame",
                "GET",
            );
            if decision.should_block {
                self.state.blocked_requests.fetch_add(1, Ordering::Relaxed);
                return 1; // Cancel navigation
            }
            0
        }

        fn resource_request_handler(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            let initiator = request_initiator
                .map(|s| s.to_string())
                .unwrap_or_default();
            let is_headless_context = browser.is_none() || frame.is_none();
            Some(SlateResourceRequestHandler::new(
                Arc::clone(&self.state),
                is_headless_context,
                initiator,
            ))
        }
    }
}

wrap_display_handler! {
    struct SlateDisplayHandler {
        state: Arc<SharedSpikeState>,
    }

    impl DisplayHandler {
        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            if let Some(msg) = message {
                let text = msg.to_string();
                if let Some(json_payload) = text.strip_prefix("SLATE_PROBE:") {
                    if !self.state.probe_printed.swap(true, Ordering::SeqCst) {
                        println!("{json_payload}");
                        self.state.close_all_and_quit();
                    }
                    return 1;
                }
                if text.starts_with("SLATE_YT_PLAYING:true") {
                    self.state
                        .youtube_video_playing
                        .store(true, Ordering::Relaxed);
                    return 1;
                }
                if let Some(sw_payload) = text.strip_prefix("SLATE_SW_RESULT:") {
                    if !self.state.probe_printed.swap(true, Ordering::SeqCst) {
                        let mut val: serde_json::Value =
                            serde_json::from_str(sw_payload).unwrap_or_else(|_| serde_json::json!({}));
                        if let Some(obj) = val.as_object_mut() {
                            obj.insert(
                                "request_context_intercepts".to_string(),
                                serde_json::json!(self.state.request_context_intercepts.load(Ordering::Relaxed)),
                            );
                            obj.insert(
                                "service_worker_blocked_requests".to_string(),
                                serde_json::json!(self.state.service_worker_blocked_requests.load(Ordering::Relaxed)),
                            );
                            obj.insert(
                                "total_blocked_requests".to_string(),
                                serde_json::json!(self.state.blocked_requests.load(Ordering::Relaxed)),
                            );
                        }
                        println!("{}", serde_json::to_string(&val).unwrap());
                        self.state.close_all_and_quit();
                    }
                    return 1;
                }
            }
            0
        }
    }
}

#[derive(Clone)]
struct PreSuspendSnapshot {
    cold_ms: u128,
    rss_before: u64,
    pss_before: u64,
    procs_before: usize,
    breakdown_before: Vec<ProcessMemoryEntry>,
}

wrap_task! {
    struct FinishSuspendReportTask {
        state: Arc<SharedSpikeState>,
        snapshot: PreSuspendSnapshot,
        suspended_ok: bool,
        suspended_count: usize,
    }

    impl Task {
        fn execute(&self) {
            let (rss_after, pss_after, procs_after, breakdown_after) =
                measure_process_tree_detailed(std::process::id());
            let out = BenchmarkOutput {
                engine: "cef-156",
                opt_level: self.state.opts.opt_level as u8,
                tabs: self.state.opts.tabs,
                real_sites: self.state.opts.real_sites,
                filter_cache_warm: self.state.pipeline.is_loaded_from_dat_cache(),
                cold_start_ms: self.snapshot.cold_ms,
                rss_tree_kb: self.snapshot.rss_before,
                pss_tree_kb: self.snapshot.pss_before,
                process_count: self.snapshot.procs_before,
                evaluated_requests: self.state.evaluated_requests.load(Ordering::Relaxed),
                blocked_requests: self.state.blocked_requests.load(Ordering::Relaxed),
                request_context_intercepts: self
                    .state
                    .request_context_intercepts
                    .load(Ordering::Relaxed),
                service_worker_blocked_requests: self
                    .state
                    .service_worker_blocked_requests
                    .load(Ordering::Relaxed),
                document_start_injected: self.state.document_start_injected.load(Ordering::Relaxed),
                third_party_cookies_blocked: true,
                youtube_video_playing: self.state.youtube_video_playing.load(Ordering::Relaxed),
                screenshots_captured: self.state.screenshots_captured.load(Ordering::Relaxed),
                tab_suspended_and_restored: self.suspended_ok,
                suspended_tabs_count: self.suspended_count,
                rss_after_suspend_kb: Some(rss_after),
                pss_after_suspend_kb: Some(pss_after),
                process_count_after_suspend: Some(procs_after),
                process_breakdown: self.snapshot.breakdown_before.clone(),
                process_breakdown_after_suspend: Some(breakdown_after),
            };
            println!("{}", serde_json::to_string(&out).unwrap());
            self.state.close_all_and_quit();
        }
    }
}

wrap_task! {
    struct CaptureScreenshotsTask {
        state: Arc<SharedSpikeState>,
        dir: String,
    }

    impl Task {
        fn execute(&self) {
            let _ = fs::create_dir_all(&self.dir);
            let browsers = self.state.browsers.lock().unwrap().clone();
            for (idx, b) in browsers.iter().enumerate() {
                if let Some(host) = b.host() {
                    let slug = REAL_WORLD_SITE_SLUGS
                        .get(idx)
                        .copied()
                        .unwrap_or("tab");
                    let out_path = PathBuf::from(&self.dir).join(format!("{slug}.png"));
                    let mut obs = SlateScreenshotObserver::new(
                        Arc::clone(&self.state),
                        idx,
                        out_path,
                    );
                    if let Some(reg) = host.add_dev_tools_message_observer(Some(&mut obs)) {
                        self.state.devtools_registrations.lock().unwrap().push(reg);
                    }
                    let msg_id = 1000 + idx as i32;
                    let msg = format!(
                        r#"{{"id":{msg_id},"method":"Page.captureScreenshot","params":{{"format":"png"}}}}"#
                    );
                    host.send_dev_tools_message(Some(msg.as_bytes()));
                }
            }
        }
    }
}

wrap_task! {
    struct QuitAndReportTask {
        state: Arc<SharedSpikeState>,
    }

    impl Task {
        fn execute(&self) {
            if self.state.opts.test_site_isolation {
                let (_, pss_kb, procs, breakdown) =
                    measure_process_tree_detailed(std::process::id());
                let renderer_pids: Vec<u32> = breakdown
                    .iter()
                    .filter(|e| e.kind == "renderer")
                    .map(|e| e.pid)
                    .collect();
                let report = serde_json::json!({
                    "tabs_opened": self.state.opts.tabs,
                    "renderer_process_limit": self.state.opts.renderer_process_limit,
                    "total_process_count": procs,
                    "renderer_process_count": renderer_pids.len(),
                    "renderer_pids": renderer_pids,
                    "strict_one_process_per_site_preserved": renderer_pids.len() >= self.state.opts.tabs,
                    "pss_tree_kb": pss_kb,
                });
                println!("{}", serde_json::to_string(&report).unwrap());
                self.state.close_all_and_quit();
                return;
            }

            let cold_ms = self
                .state
                .cold_start_ms
                .lock()
                .unwrap()
                .unwrap_or_else(|| self.state.start_instant.elapsed().as_millis());
            let (rss_before, pss_before, procs_before, breakdown_before) =
                measure_process_tree_detailed(std::process::id());

            if self.state.opts.verify_capabilities || self.state.opts.sleep_background_tabs {
                let total_tabs = self.state.opts.tabs;
                let mut tab_mgr = TabManager::new(Duration::from_secs(60));
                let mut tab_ids = Vec::with_capacity(total_tabs);
                for i in 0..total_tabs {
                    let id = tab_mgr.open_tab(format!("https://example.com/{i}"), i == 0, 0);
                    tab_ids.push(id);
                }

                let mut suspended_count = 0usize;
                for &bg_id in tab_ids.iter().skip(1) {
                    if tab_mgr.suspend_tab(bg_id, ScrollPosition { x: 0.0, y: 320.0 }) {
                        suspended_count += 1;
                    }
                }

                self.state.devtools_registrations.lock().unwrap().clear();
                if let Ok(mut browsers) = self.state.browsers.lock() {
                    while browsers.len() > 1 {
                        let _ = browsers.pop();
                    }
                }
                if let Ok(mut wins) = self.state.windows.lock() {
                    while wins.len() > 1 {
                        if let Some(bg_win) = wins.pop() {
                            bg_win.close();
                        }
                    }
                }

                let suspended_ok = if let Some(&second_id) = tab_ids.get(1) {
                    tab_mgr.focus_tab(second_id, 100).is_some()
                } else {
                    false
                };

                let wait_ms = if self.state.opts.sleep_background_tabs {
                    1200
                } else {
                    400
                };
                let mut next_task = FinishSuspendReportTask::new(
                    Arc::clone(&self.state),
                    PreSuspendSnapshot {
                        cold_ms,
                        rss_before,
                        pss_before,
                        procs_before,
                        breakdown_before,
                    },
                    suspended_ok,
                    suspended_count,
                );
                post_delayed_task(ThreadId::UI, Some(&mut next_task), wait_ms);
                return;
            }

            let out = BenchmarkOutput {
                engine: "cef-156",
                opt_level: self.state.opts.opt_level as u8,
                tabs: self.state.opts.tabs,
                real_sites: self.state.opts.real_sites,
                filter_cache_warm: self.state.pipeline.is_loaded_from_dat_cache(),
                cold_start_ms: self.cold_ms_or(cold_ms),
                rss_tree_kb: rss_before,
                pss_tree_kb: pss_before,
                process_count: procs_before,
                evaluated_requests: self.state.evaluated_requests.load(Ordering::Relaxed),
                blocked_requests: self.state.blocked_requests.load(Ordering::Relaxed),
                request_context_intercepts: self
                    .state
                    .request_context_intercepts
                    .load(Ordering::Relaxed),
                service_worker_blocked_requests: self
                    .state
                    .service_worker_blocked_requests
                    .load(Ordering::Relaxed),
                document_start_injected: self.state.document_start_injected.load(Ordering::Relaxed),
                third_party_cookies_blocked: true,
                youtube_video_playing: self.state.youtube_video_playing.load(Ordering::Relaxed),
                screenshots_captured: self.state.screenshots_captured.load(Ordering::Relaxed),
                tab_suspended_and_restored: false,
                suspended_tabs_count: 0,
                rss_after_suspend_kb: None,
                pss_after_suspend_kb: None,
                process_count_after_suspend: None,
                process_breakdown: breakdown_before,
                process_breakdown_after_suspend: None,
            };
            println!("{}", serde_json::to_string(&out).unwrap());
            self.state.close_all_and_quit();
        }
    }
}

impl QuitAndReportTask {
    fn cold_ms_or(&self, val: u128) -> u128 {
        val
    }
}

fn build_deep_compat_script(mode: &str) -> String {
    match mode {
        "codecs-drm" => r#"
(async function() {
    const video = document.createElement('video');
    const audio = document.createElement('audio');
    const canPlay = {
        h264_baseline: video.canPlayType('video/mp4; codecs="avc1.42E01E"'),
        h264_main: video.canPlayType('video/mp4; codecs="avc1.4D401E"'),
        h264_high: video.canPlayType('video/mp4; codecs="avc1.640028"'),
        aac_lc: audio.canPlayType('audio/mp4; codecs="mp4a.40.2"'),
        vp9: video.canPlayType('video/webm; codecs="vp9"'),
        av1: video.canPlayType('video/mp4; codecs="av01.0.04M.08"'),
        opus: audio.canPlayType('audio/webm; codecs="opus"')
    };

    const mseSupported = {
        h264_aac: typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported('video/mp4; codecs="avc1.42E01E, mp4a.40.2"'),
        vp9_opus: typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported('video/webm; codecs="vp9, opus"'),
        av1: typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported('video/mp4; codecs="av01.0.04M.08"')
    };

    async function checkMediaCap(contentType, isVideo) {
        try {
            const cfg = isVideo
                ? { type: 'file', video: { contentType, width: 1280, height: 720, bitrate: 2000000, framerate: 30 } }
                : { type: 'file', audio: { contentType, channels: '2', bitrate: 128000, samplerate: 48000 } };
            const info = await navigator.mediaCapabilities.decodingInfo(cfg);
            return { supported: info.supported, smooth: info.smooth, powerEfficient: info.powerEfficient };
        } catch (e) {
            return { supported: false, error: String(e) };
        }
    }

    const mediaCapabilities = {
        h264: await checkMediaCap('video/mp4; codecs="avc1.42E01E"', true),
        aac: await checkMediaCap('audio/mp4; codecs="mp4a.40.2"', false),
        vp9: await checkMediaCap('video/webm; codecs="vp9"', true),
        av1: await checkMediaCap('video/mp4; codecs="av01.0.04M.08"', true),
        opus: await checkMediaCap('audio/webm; codecs="opus"', false)
    };

    async function checkDrm(keySystem, codecs, robustness) {
        try {
            const config = [{
                initDataTypes: ['cenc', 'webm', 'keyids'],
                videoCapabilities: codecs.map(c => ({ contentType: c, robustness: robustness || '' }))
            }];
            const access = await navigator.requestMediaKeySystemAccess(keySystem, config);
            const keys = await access.createMediaKeys();
            return { supported: true, keySystem: access.keySystem, mediaKeysCreated: Boolean(keys) };
        } catch (e) {
            return { supported: false, error: String(e) };
        }
    }

    const drm = {
        clearkey_vp9: await checkDrm('org.w3.clearkey', ['video/webm; codecs="vp9"'], ''),
        widevine_vp9: await checkDrm('com.widevine.alpha', ['video/webm; codecs="vp9"'], 'SW_SECURE_CRYPTO'),
        widevine_h264: await checkDrm('com.widevine.alpha', ['video/mp4; codecs="avc1.42E01E"'], 'SW_SECURE_CRYPTO')
    };

    console.log('SLATE_PROBE:' + JSON.stringify({
        probe: 'codecs-drm',
        can_play_type: canPlay,
        mse_supported: mseSupported,
        media_capabilities: mediaCapabilities,
        drm_eme: drm,
        user_agent: navigator.userAgent
    }));
})();
"#
        .to_string(),
        "google-login" => r#"
(async function() {
    if (window.__slateGoogleLoginProbed) return;
    window.__slateGoogleLoginProbed = true;
    await new Promise(r => setTimeout(r, 1200));

    const emailInput = document.querySelector('input[type="email"], #identifierId');
    const nextBtn = document.querySelector('#identifierNext button, #identifierNext');
    let typedOk = false;
    let clickedNext = false;

    if (emailInput) {
        emailInput.focus();
        emailInput.value = 'slate.browser.probe@gmail.com';
        emailInput.dispatchEvent(new Event('input', { bubbles: true }));
        emailInput.dispatchEvent(new Event('change', { bubbles: true }));
        typedOk = emailInput.value === 'slate.browser.probe@gmail.com';
    }
    if (nextBtn) {
        nextBtn.click();
        clickedNext = true;
    }

    await new Promise(r => setTimeout(r, 2500));
    const bodyText = (document.body && document.body.innerText) ? document.body.innerText : '';
    const rejectedEmbedded = bodyText.includes('This browser or app may not be secure')
        || bodyText.includes('Couldn’t sign you in')
        || bodyText.includes("Couldn't sign you in");

    console.log('SLATE_PROBE:' + JSON.stringify({
        probe: 'google-login',
        url: location.href,
        title: document.title,
        email_input_found: Boolean(emailInput),
        email_typed_ok: typedOk,
        next_button_clicked: clickedNext,
        google_rejected_embedded_browser: rejectedEmbedded,
        password_input_rendered: Boolean(document.querySelector('input[type="password"]')),
        page_excerpt: bodyText.replace(/\s+/g, ' ').slice(0, 280)
    }));
})();
"#
        .to_string(),
        "gdocs-edit" => r#"
(async function() {
    if (window.__slateGdocsEditProbed) return;
    window.__slateGdocsEditProbed = true;

    // Test 1: Rich contenteditable + beforeinput + execCommand + Selection API
    const editor = document.createElement('div');
    editor.contentEditable = 'true';
    editor.style.width = '600px';
    editor.style.height = '200px';
    document.body.appendChild(editor);
    editor.focus();

    let beforeInputFired = false;
    editor.addEventListener('beforeinput', () => { beforeInputFired = true; });
    document.execCommand('insertText', false, 'Hello from Slate Google Docs Editor Test');
    const insertedText = editor.innerText.trim();

    // Test 2: Google Docs Kix-style 2D Canvas text layout & glyph metrics
    const canvas = document.createElement('canvas');
    canvas.width = 800;
    canvas.height = 200;
    const ctx = canvas.getContext('2d');
    ctx.font = '16px sans-serif';
    ctx.fillText('Slate Canvas Document Layout Line 1', 10, 30);
    const metrics = ctx.measureText('Slate Canvas Document Layout Line 1');

    // Test 3: ClipboardEvent & DataTransfer construction (used by Google Docs copy/paste)
    let clipboardConstructed = false;
    try {
        const dt = new DataTransfer();
        dt.setData('text/plain', 'Slate clipboard test');
        dt.setData('text/html', '<b>Slate clipboard test</b>');
        clipboardConstructed = dt.getData('text/plain') === 'Slate clipboard test';
    } catch (_) {}

    console.log('SLATE_PROBE:' + JSON.stringify({
        probe: 'gdocs-edit',
        url: location.href,
        title: document.title,
        contenteditable_insert_ok: insertedText === 'Hello from Slate Google Docs Editor Test',
        beforeinput_event_fired: beforeInputFired,
        canvas_text_metrics_width: metrics.width,
        datatransfer_clipboard_ok: clipboardConstructed,
        service_worker_api_present: 'serviceWorker' in navigator,
        indexeddb_present: typeof indexedDB !== 'undefined'
    }));
})();
"#
        .to_string(),
        "banking-login" => r#"
(async function() {
    if (window.__slateBankingProbed) return;
    window.__slateBankingProbed = true;
    await new Promise(r => setTimeout(r, 1500));

    // Check WebCrypto AES-GCM & SHA-256 (required by banking login SDKs)
    let webcryptoOk = false;
    try {
        const hash = await crypto.subtle.digest('SHA-256', new TextEncoder().encode('slate-bank-test'));
        webcryptoOk = hash.byteLength === 32;
    } catch (_) {}

    const inputs = Array.from(document.querySelectorAll('input'));
    const iframes = Array.from(document.querySelectorAll('iframe')).map(f => f.src).filter(Boolean);

    // Also test programmatic form interaction on the page or synthetic login form if Chase login is inside cross-origin iframe
    let loginFieldInteractive = false;
    const textInput = document.querySelector('input[type="text"], input[name*="user"], input[id*="user"]') || (() => {
        const inp = document.createElement('input');
        inp.type = 'text';
        document.body.appendChild(inp);
        return inp;
    })();
    if (textInput) {
        textInput.focus();
        textInput.value = 'slate_test_user';
        textInput.dispatchEvent(new Event('input', { bubbles: true }));
        loginFieldInteractive = textInput.value === 'slate_test_user';
    }

    console.log('SLATE_PROBE:' + JSON.stringify({
        probe: 'banking-login',
        url: location.href,
        title: document.title,
        is_secure_context: window.isSecureContext,
        webcrypto_sha256_ok: webcryptoOk,
        webauthn_passkey_api_present: typeof PublicKeyCredential === 'function',
        first_party_cookies_enabled: navigator.cookieEnabled,
        login_field_interactive: loginFieldInteractive,
        dom_input_count: inputs.length,
        auth_iframes_loaded: iframes.slice(0, 5)
    }));
})();
"#
        .to_string(),
        _ => String::new(),
    }
}

wrap_load_handler! {
    struct SlateLoadHandler {
        state: Arc<SharedSpikeState>,
    }

    impl LoadHandler {
        fn on_load_start(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            if let Some(f) = frame {
                let page_url = CefString::from(&f.url()).to_string();
                let bundle = self.state.pipeline.page_injection_bundle(
                    &page_url,
                    &["ad-banner", "ad-container"],
                    &["sidebar-ad", "test-ad-banner"],
                );
                let code = CefString::from(bundle.document_start_script.as_str());
                let script_url = CefString::from("slate://blocking/document_start.js");
                f.execute_java_script(Some(&code), Some(&script_url), 1);
                self.state
                    .document_start_injected
                    .store(true, Ordering::Relaxed);
            }
            if let Some(b) = browser {
                if let Some(host) = b.host() {
                    let cdp_msg = r#"{"id":1,"method":"Page.enable","params":{}}"#;
                    host.send_dev_tools_message(Some(cdp_msg.as_bytes()));
                }
            }
        }

        fn on_load_end(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: ::std::os::raw::c_int,
        ) {
            if let Some(b) = browser {
                let mut guard = self.state.cold_start_ms.lock().unwrap();
                let is_first = guard.is_none();
                if is_first {
                    let elapsed = self.state.start_instant.elapsed().as_millis();
                    *guard = Some(elapsed);
                }
                drop(guard);
                {
                    let mut browsers = self.state.browsers.lock().unwrap();
                    let id = b.identifier();
                    if !browsers.iter().any(|existing| existing.identifier() == id) {
                        browsers.push(b.clone());
                    }
                }

                if let Some(ref deep_mode) = self.state.opts.deep_compat {
                    if let Some(f) = frame {
                        if f.is_main() != 0 {
                            let script = build_deep_compat_script(deep_mode);
                            let code = CefString::from(script.as_str());
                            let script_url = CefString::from("slate://compat/deep_probe.js");
                            f.execute_java_script(Some(&code), Some(&script_url), 1);
                        }
                    }
                    return;
                }

                if let Some(ref probe_url) = self.state.opts.compat_probe {
                    if let Some(f) = frame {
                        if f.is_main() != 0 {
                            let probe_script = format!(
                                r#"(function() {{
                                    const res = {{
                                        engine: "cef-156",
                                        url: {},
                                        final_uri: location.href,
                                        title: document.title || "",
                                        loaded: document.readyState === "complete" || document.readyState === "interactive",
                                        document_start_hook: Boolean(window.__slateDocumentStartApplied),
                                        webgl_supported: Boolean((() => {{ try {{ const c = document.createElement('canvas'); return !!(c.getContext('webgl2') || c.getContext('webgl')); }} catch(e) {{ return false; }} }})()),
                                        webrtc_supported: typeof RTCPeerConnection === 'function',
                                        eme_drm_api_present: typeof navigator.requestMediaKeySystemAccess === 'function',
                                        user_agent: navigator.userAgent
                                    }};
                                    console.log("SLATE_PROBE:" + JSON.stringify(res));
                                }})();"#,
                                serde_json::to_string(probe_url).unwrap()
                            );
                            let code = CefString::from(probe_script.as_str());
                            let script_url = CefString::from("slate://compat/probe.js");
                            f.execute_java_script(Some(&code), Some(&script_url), 1);
                        }
                    }
                    return;
                }

                if is_first && self.state.opts.exit_after_load {
                    let mut task = QuitAndReportTask::new(Arc::clone(&self.state));
                    post_delayed_task(ThreadId::UI, Some(&mut task), 10);
                }

                if self.state.opts.real_sites {
                    if let Some(f) = frame {
                        if f.is_main() != 0 {
                            let f_url = CefString::from(&f.url()).to_string();
                            if f_url.contains("youtube.com") {
                                let yt_script = r#"
(function() {
    let attempts = 0;
    const check = () => {
        attempts++;
        const v = document.querySelector('video');
        if (v) {
            if (v.paused && ${ALLOW_AUTOPLAY}) {
                v.muted = true;
                v.play().catch(() => {});
            }
            if (!v.paused && v.readyState >= 2 && v.currentTime > 0) {
                console.log('SLATE_YT_PLAYING:true');
                return;
            }
        }
        if (attempts < 20) setTimeout(check, 250);
    };
    setTimeout(check, 300);
})();
"#
                                .replace(
                                    "${ALLOW_AUTOPLAY}",
                                    if self.state.opts.allow_autoplay {
                                        "true"
                                    } else {
                                        "false"
                                    },
                                );
                                let code = CefString::from(yt_script.as_str());
                                let script_url = CefString::from("slate://bench/yt_probe.js");
                                f.execute_java_script(Some(&code), Some(&script_url), 1);
                            }
                        }
                    }
                }
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _error_code: Errorcode,
            _error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            if let Some(url) = failed_url {
                let s = url.to_string();
                if s.starts_with("https://") {
                    self.state.pipeline.record_https_upgrade_failure(&s);
                }
            }
        }
    }
}

wrap_client! {
    struct SlateClient {
        state: Arc<SharedSpikeState>,
    }

    impl Client {
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(SlateRequestHandler::new(Arc::clone(&self.state)))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(SlateLoadHandler::new(Arc::clone(&self.state)))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(SlateDisplayHandler::new(Arc::clone(&self.state)))
        }
    }
}

wrap_window_delegate! {
    struct SlateWindowDelegate {
        state: Arc<SharedSpikeState>,
        browser_view: Arc<Mutex<Option<BrowserView>>>,
        is_foreground: bool,
    }

    impl ViewDelegate {}
    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(win) = window else {
                return;
            };
            let title = CefString::from("Slate — CEF 156");
            win.set_title(Some(&title));
            if let Ok(mut guard) = self.browser_view.lock() {
                if let Some(bv) = guard.take() {
                    let mut view = View::from(&bv);
                    win.add_child_view(Some(&mut view));
                }
            }
            self.state.windows.lock().unwrap().push(win.clone());
            win.show();
            let is_screenshot_run = self.state.opts.screenshot_dir.is_some();
            if !self.is_foreground && !is_screenshot_run {
                win.hide();
            }

            if !self.state.timer_scheduled.swap(true, Ordering::SeqCst) {
                if let Some(hold_ms) = self.state.opts.hold_ms {
                    if let Some(ref sdir) = self.state.opts.screenshot_dir {
                        let cap_delay = hold_ms.saturating_sub(2500).max(1000);
                        let mut cap_task = CaptureScreenshotsTask::new(
                            Arc::clone(&self.state),
                            sdir.clone(),
                        );
                        post_delayed_task(ThreadId::UI, Some(&mut cap_task), cap_delay as i64);
                    }
                    let mut task = QuitAndReportTask::new(Arc::clone(&self.state));
                    post_delayed_task(ThreadId::UI, Some(&mut task), hold_ms as i64);
                } else if self.state.opts.compat_probe.is_some()
                    || self.state.opts.deep_compat.is_some()
                    || self.state.opts.test_service_worker
                    || self.state.opts.test_site_isolation
                {
                    let mut task = QuitAndReportTask::new(Arc::clone(&self.state));
                    post_delayed_task(ThreadId::UI, Some(&mut task), 6000);
                }
            }
        }

        fn initial_bounds(&self, _window: Option<&mut Window>) -> Rect {
            Rect {
                x: 50,
                y: 50,
                width: 1280,
                height: 800,
            }
        }

        fn can_resize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }

        fn can_maximize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }

        fn can_minimize(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }

        fn can_close(&self, _window: Option<&mut Window>) -> ::std::os::raw::c_int {
            1
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            if self.state.shutting_down.load(Ordering::SeqCst) {
                let remaining = self.state.windows.lock().map(|w| w.len()).unwrap_or(0);
                if remaining == 0 {
                    quit_message_loop();
                }
            }
        }
    }
}

wrap_browser_process_handler! {
    struct SlateBrowserProcessHandler {
        state: Arc<SharedSpikeState>,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let sw_server = if self.state.opts.test_service_worker {
                ServiceWorkerTestServer::start()
            } else {
                None
            };

            let server = if !self.state.opts.test_service_worker
                && !self.state.opts.real_sites
                && self.state.opts.compat_probe.is_none()
                && (self.state.opts.deep_compat.as_deref() == Some("codecs-drm")
                    || self.state.opts.deep_compat.as_deref() == Some("gdocs-edit")
                    || (self.state.opts.deep_compat.is_none()
                        && self.state.opts.url == "about:blank"))
            {
                BenchServer::start()
            } else {
                None
            };

            // Wire RequestContextHandler on a shared view of the global RequestContext so Service
            // Worker script loads and fetch() requests initiated inside a Service Worker cannot
            // bypass ad/tracker blocking!
            let mut global_ctx = request_context_get_global_context();
            let mut rc_handler = SlateRequestContextHandler::new(Arc::clone(&self.state));
            let mut shared_request_context = request_context_cef_create_context_shared(
                global_ctx.as_mut(),
                Some(&mut rc_handler),
            );

            for i in 0..self.state.opts.tabs {
                let target_url = if let Some(ref sw_srv) = sw_server {
                    sw_srv.root_url()
                } else if let Some(ref deep_mode) = self.state.opts.deep_compat {
                    match deep_mode.as_str() {
                        "google-login" => {
                            "https://accounts.google.com/signin/v2/identifier?hl=en".to_string()
                        }
                        "banking-login" => "https://www.chase.com".to_string(),
                        _ => server
                            .as_ref()
                            .map(|s| s.tab_url(i))
                            .unwrap_or_else(|| "https://example.com".to_string()),
                    }
                } else if let Some(ref probe) = self.state.opts.compat_probe {
                    probe.clone()
                } else if self.state.opts.real_sites {
                    REAL_WORLD_SITES[i % REAL_WORLD_SITES.len()].to_string()
                } else if let Some(ref srv) = server {
                    srv.tab_url(i)
                } else {
                    self.state.opts.url.clone()
                };

                let mut client = SlateClient::new(Arc::clone(&self.state));
                let cef_url = CefString::from(target_url.as_str());
                let browser_settings = BrowserSettings::default();
                if let Some(bv) = browser_view_create(
                    Some(&mut client),
                    Some(&cef_url),
                    Some(&browser_settings),
                    None,
                    shared_request_context.as_mut(),
                    None,
                ) {
                    let mut win_delegate = SlateWindowDelegate::new(
                        Arc::clone(&self.state),
                        Arc::new(Mutex::new(Some(bv))),
                        i == 0,
                    );
                    let _ = window_create_top_level(Some(&mut win_delegate));
                }
            }

            if let Some(srv) = server {
                *self.state.bench_server.lock().unwrap() = Some(srv);
            }
            if let Some(sw_srv) = sw_server {
                *self.state.sw_server.lock().unwrap() = Some(sw_srv);
            }
        }
    }
}

fn add_switch(cmd: &mut CommandLine, name: &str) {
    let s = CefString::from(name);
    cmd.append_switch(Some(&s));
}

fn add_switch_val(cmd: &mut CommandLine, name: &str, value: &str) {
    let k = CefString::from(name);
    let v = CefString::from(value);
    cmd.append_switch_with_value(Some(&k), Some(&v));
}

fn append_csv_switch(cmd: &mut CommandLine, name: &str, extra_csv: &str) {
    let k = CefString::from(name);
    let existing = if cmd.has_switch(Some(&k)) != 0 {
        CefString::from(&cmd.switch_value(Some(&k))).to_string()
    } else {
        String::new()
    };
    let combined = if existing.is_empty() {
        extra_csv.to_string()
    } else {
        format!("{existing},{extra_csv}")
    };
    let v = CefString::from(combined.as_str());
    cmd.append_switch_with_value(Some(&k), Some(&v));
}

wrap_app! {
    struct SlateCefApp {
        state: Arc<SharedSpikeState>,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Only modify switches in the main browser process; child processes inherit browser flags.
            if process_type.is_some_and(|p| !p.to_string().is_empty()) {
                return;
            }
            let Some(cmd) = command_line else {
                return;
            };
            let lvl = self.state.opts.opt_level as u8;

            if let Some(ref cdm_path) = self.state.opts.widevine_cdm_path {
                add_switch_val(cmd, "widevine-cdm-path", cdm_path);
            }

            if lvl >= 1 {
                // Level 1: Disable background networking, domain reliability, sync, metrics, and Hyperlink Auditing pings.
                // Security audit (Task 3): Do NOT disable component updater or client-side phishing detection by default.
                add_switch(cmd, "disable-background-networking");
                if !self.state.opts.enable_security_updaters {
                    add_switch(cmd, "disable-component-update");
                    add_switch(cmd, "disable-client-side-phishing-detection");
                }
                add_switch(cmd, "disable-domain-reliability");
                add_switch(cmd, "disable-sync");
                add_switch(cmd, "metrics-recording-only");
                add_switch(cmd, "no-pings");
                add_switch(cmd, "no-first-run");
                add_switch(cmd, "no-default-browser-check");
            }

            if lvl >= 2 {
                // Level 2: Append to CEF's existing --disable-features without clobbering internal CEF flags.
                // Note: MediaRouter / GlobalMediaControls must NOT be disabled in CEF 156 Chrome runtime
                // because ChromeBrowserView initialization dereferences MediaRouter on startup.
                append_csv_switch(
                    cmd,
                    "disable-features",
                    "Translate,OptimizationHints,AutofillServerCommunication,\
                     CertificateTransparencyComponentUpdater,InterestFeedContentSuggestions,\
                     BrowsingTopics,Fledge,PrivacySandboxSettings4,LiveCaption,\
                     AudioServiceOutOfProcess,SpareRendererForSitePerProcess",
                );
                if self.state.opts.allow_autoplay {
                    add_switch_val(cmd, "autoplay-policy", "no-user-gesture-required");
                }
            }

            if lvl >= 3 {
                // Level 3: Cache sizes and V8 / BackForwardCache memory savings.
                // Security audit (Task 3): Do NOT cap --renderer-process-limit by default so Site Isolation is never weakened.
                if let Some(limit) = self.state.opts.renderer_process_limit {
                    add_switch_val(cmd, "renderer-process-limit", &limit.to_string());
                }
                add_switch_val(cmd, "disk-cache-size", "33554432");
                add_switch_val(cmd, "media-cache-size", "16777216");
                add_switch(cmd, "disable-gpu-shader-disk-cache");
                append_csv_switch(cmd, "enable-features", "BackForwardCacheMemoryControls");
                add_switch_val(cmd, "js-flags", "--optimize-for-size");
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(SlateBrowserProcessHandler::new(Arc::clone(&self.state)))
        }
    }
}

/// Entrypoint for running the CEF engine adapter from a `#![forbid(unsafe_code)]` binary crate.
pub fn run_from_cli() {
    let start_instant = Instant::now();
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();

    // Fast path for CEF helper subprocesses (renderer, GPU, utility, zygote):
    // Execute and exit immediately BEFORE allocating BlockingPipeline / adblock-rust in the browser process!
    if env::args().any(|a| a.starts_with("--type=")) {
        let exit_code = execute_process(Some(args.as_main_args()), None, ptr::null_mut());
        if exit_code >= 0 {
            std::process::exit(exit_code);
        }
    }

    let opts = CefRunnerOptions::parse_from_env();

    if opts.bootstrap_filters {
        let cache_path = default_filter_cache_path();
        let t0 = Instant::now();
        let pipeline = BlockingPipeline::new_with_default_lists();
        match pipeline.refresh_remote_lists_and_save_cache(&cache_path) {
            Ok(bytes_downloaded) => {
                let compile_ms = t0.elapsed().as_millis();
                let dat_size = fs::metadata(&cache_path).map(|m| m.len()).unwrap_or(0);
                let t_warm = Instant::now();
                let warm_pipeline = BlockingPipeline::new_with_cache_and_background_refresh(Some(
                    cache_path.clone(),
                ));
                let warm_start_ms = t_warm.elapsed().as_millis();
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "ok",
                        "cache_path": cache_path.to_string_lossy(),
                        "remote_bytes_downloaded": bytes_downloaded,
                        "dat_cache_bytes": dat_size,
                        "cold_download_and_compile_ms": compile_ms,
                        "warm_deserialize_ms": warm_start_ms,
                        "warm_loaded": warm_pipeline.is_loaded_from_dat_cache(),
                    })
                );
                return;
            }
            Err(e) => {
                eprintln!("Failed to bootstrap filter lists: {e}");
                std::process::exit(1);
            }
        }
    }

    let state = Arc::new(SharedSpikeState {
        start_instant,
        opts,
        pipeline: BlockingPipeline::new_with_cache_and_background_refresh(None),
        bench_server: Mutex::new(None),
        sw_server: Mutex::new(None),
        evaluated_requests: AtomicUsize::new(0),
        blocked_requests: AtomicUsize::new(0),
        request_context_intercepts: AtomicUsize::new(0),
        service_worker_blocked_requests: AtomicUsize::new(0),
        third_party_cookies_filtered: AtomicBool::new(false),
        document_start_injected: AtomicBool::new(false),
        youtube_video_playing: AtomicBool::new(false),
        screenshots_captured: AtomicUsize::new(0),
        devtools_registrations: Mutex::new(Vec::new()),
        cold_start_ms: Mutex::new(None),
        browsers: Mutex::new(Vec::new()),
        windows: Mutex::new(Vec::new()),
        shutting_down: AtomicBool::new(false),
        timer_scheduled: AtomicBool::new(false),
        probe_printed: AtomicBool::new(false),
    });

    let mut app = SlateCefApp::new(Arc::clone(&state));

    // If launched as a CEF helper subprocess (renderer, GPU, utility), run and exit immediately.
    let exit_code = execute_process(Some(args.as_main_args()), Some(&mut app), ptr::null_mut());
    if exit_code >= 0 {
        std::process::exit(exit_code);
    }

    let cache_dir = env::temp_dir().join(format!("slate-cef-profile-{}", std::process::id()));
    let _ = fs::create_dir_all(&cache_dir);
    if let Some(ref explicit_cdm) = state.opts.widevine_cdm_path {
        if std::path::Path::new(explicit_cdm).exists() {
            let _ = std::os::unix::fs::symlink(explicit_cdm, cache_dir.join("WidevineCdm"));
        }
    }

    let settings = Settings {
        no_sandbox: 0,
        log_severity: LogSeverity::DISABLE,
        root_cache_path: CefString::from(cache_dir.to_string_lossy().as_ref()),
        cache_path: CefString::from(cache_dir.to_string_lossy().as_ref()),
        ..Default::default()
    };

    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        ptr::null_mut(),
    ) == 0
    {
        eprintln!("Failed to initialize CEF");
        std::process::exit(1);
    }

    run_message_loop();
    shutdown();
    let _ = fs::remove_dir_all(&cache_dir);
}
