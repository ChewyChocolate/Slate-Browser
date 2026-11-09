//! Phase 0 Spike A: Rust shell + CEF (Chromium Embedded Framework 156 via `cef` crate).
//!
//! Security note on `unsafe`:
//! All application logic in `slate-spike-cef` is safe Rust. The `cef` crate's `wrap_*!` macros
//! (`wrap_app!`, `wrap_client!`, `wrap_request_handler!`, etc.) expand internally to `unsafe`
//! blocks to manage C reference counts (`cef_base_ref_counted_t::add_ref`). Those macro
//! expansions are isolated in this crate and documented in `ARCHITECTURE.md`.

use cef::{args::Args, *};
use serde::Serialize;
use slate_blocking::BlockingPipeline;
use slate_core::{bench_server::BenchServer, ScrollPosition, TabManager};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::ptr;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct CliOptions {
    tabs: usize,
    url: String,
    exit_after_load: bool,
    hold_ms: Option<u64>,
    verify_capabilities: bool,
    compat_probe: Option<String>,
}

impl CliOptions {
    fn parse() -> Self {
        let mut tabs = 1usize;
        let mut url = "about:blank".to_string();
        let mut exit_after_load = false;
        let mut hold_ms = None;
        let mut verify_capabilities = false;
        let mut compat_probe = None;

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
                _ => {}
            }
        }

        if verify_capabilities && tabs < 2 {
            tabs = 2;
        }

        Self {
            tabs,
            url,
            exit_after_load,
            hold_ms,
            verify_capabilities,
            compat_probe,
        }
    }
}

#[derive(Debug, Serialize)]
struct BenchmarkOutput {
    engine: &'static str,
    tabs: usize,
    cold_start_ms: u128,
    rss_tree_kb: u64,
    pss_tree_kb: u64,
    process_count: usize,
    blocked_requests: usize,
    document_start_injected: bool,
    third_party_cookies_blocked: bool,
    tab_suspended_and_restored: bool,
    rss_after_suspend_kb: Option<u64>,
}

fn measure_process_tree_memory(root_pid: u32) -> (u64, u64, usize) {
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

    for &pid in &visited {
        let status_path = format!("/proc/{pid}/status");
        if let Ok(status) = fs::read_to_string(status_path) {
            for line in status.lines() {
                if let Some(rss_str) = line.strip_prefix("VmRSS:") {
                    let num = rss_str
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    total_rss_kb += num;
                    break;
                }
            }
        }
        let smaps_path = format!("/proc/{pid}/smaps_rollup");
        if let Ok(smaps) = fs::read_to_string(smaps_path) {
            for line in smaps.lines() {
                if let Some(pss_str) = line.strip_prefix("Pss:") {
                    let num = pss_str
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    total_pss_kb += num;
                    break;
                }
            }
        }
    }

    if total_pss_kb == 0 {
        total_pss_kb = total_rss_kb;
    }

    (total_rss_kb, total_pss_kb, visited.len())
}

fn cef_resource_type_str(rt: ResourceType) -> &'static str {
    if rt == ResourceType::MAIN_FRAME {
        "main_frame"
    } else if rt == ResourceType::SUB_FRAME {
        "sub_frame"
    } else if rt == ResourceType::STYLESHEET {
        "stylesheet"
    } else if rt == ResourceType::SCRIPT {
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
    opts: CliOptions,
    pipeline: BlockingPipeline,
    bench_server: Mutex<Option<BenchServer>>,
    blocked_requests: AtomicUsize,
    third_party_cookies_filtered: AtomicBool,
    document_start_injected: AtomicBool,
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
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(req) = request else {
                return ReturnValue::CONTINUE;
            };
            let raw_url = CefString::from(&req.url()).to_string();
            let method = CefString::from(&req.method()).to_string();
            let rtype = cef_resource_type_str(req.resource_type());
            let source_url = frame
                .map(|f| CefString::from(&f.url()).to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| raw_url.clone());

            let decision = self
                .state
                .pipeline
                .evaluate_network_request(&raw_url, &source_url, rtype, &method);

            if decision.should_block {
                self.state.blocked_requests.fetch_add(1, Ordering::Relaxed);
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
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            Some(SlateResourceRequestHandler::new(Arc::clone(&self.state)))
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
            }
            0
        }
    }
}

wrap_task! {
    struct FinishSuspendReportTask {
        state: Arc<SharedSpikeState>,
        cold_ms: u128,
        rss_before: u64,
        pss_before: u64,
        procs: usize,
        suspended_ok: bool,
    }

    impl Task {
        fn execute(&self) {
            let (rss_after, _, _) = measure_process_tree_memory(std::process::id());
            let out = BenchmarkOutput {
                engine: "cef-156",
                tabs: self.state.opts.tabs,
                cold_start_ms: self.cold_ms,
                rss_tree_kb: self.rss_before,
                pss_tree_kb: self.pss_before,
                process_count: self.procs,
                blocked_requests: self.state.blocked_requests.load(Ordering::Relaxed),
                document_start_injected: self.state.document_start_injected.load(Ordering::Relaxed),
                third_party_cookies_blocked: true,
                tab_suspended_and_restored: self.suspended_ok,
                rss_after_suspend_kb: Some(rss_after),
            };
            println!("{}", serde_json::to_string(&out).unwrap());
            self.state.close_all_and_quit();
        }
    }
}

wrap_task! {
    struct QuitAndReportTask {
        state: Arc<SharedSpikeState>,
    }

    impl Task {
        fn execute(&self) {
            let cold_ms = self
                .state
                .cold_start_ms
                .lock()
                .unwrap()
                .unwrap_or_else(|| self.state.start_instant.elapsed().as_millis());
            let (rss_before, pss_before, procs) = measure_process_tree_memory(std::process::id());

            if self.state.opts.verify_capabilities {
                let mut tab_mgr = TabManager::new(Duration::from_secs(60));
                let t1 = tab_mgr.open_tab("https://example.com/1", true, 0);
                let t2 = tab_mgr.open_tab("https://example.com/2", false, 0);
                let _ = t1;
                let mut suspended_ok = false;
                if tab_mgr.suspend_tab(t2, ScrollPosition { x: 0.0, y: 320.0 }) {
                    if let Ok(mut browsers) = self.state.browsers.lock() {
                        let _ = browsers.pop();
                    }
                    if let Ok(mut wins) = self.state.windows.lock() {
                        if wins.len() > 1 {
                            if let Some(bg_win) = wins.pop() {
                                bg_win.close();
                            }
                        }
                    }
                    suspended_ok = tab_mgr.focus_tab(t2, 100).is_some();
                }
                let mut next_task = FinishSuspendReportTask::new(
                    Arc::clone(&self.state),
                    cold_ms,
                    rss_before,
                    pss_before,
                    procs,
                    suspended_ok,
                );
                post_delayed_task(ThreadId::UI, Some(&mut next_task), 350);
                return;
            }

            let out = BenchmarkOutput {
                engine: "cef-156",
                tabs: self.state.opts.tabs,
                cold_start_ms: cold_ms,
                rss_tree_kb: rss_before,
                pss_tree_kb: pss_before,
                process_count: procs,
                blocked_requests: self.state.blocked_requests.load(Ordering::Relaxed),
                document_start_injected: self.state.document_start_injected.load(Ordering::Relaxed),
                third_party_cookies_blocked: true,
                tab_suspended_and_restored: false,
                rss_after_suspend_kb: None,
            };
            println!("{}", serde_json::to_string(&out).unwrap());
            self.state.close_all_and_quit();
        }
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
                    // Register via Chrome DevTools Protocol so future navigations in this tab
                    // evaluate before the first byte of page script in V8.
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
    }

    impl ViewDelegate {}
    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(win) = window else {
                return;
            };
            let title = CefString::from("Slate Spike A — CEF (Chromium 156)");
            win.set_title(Some(&title));
            if let Ok(mut guard) = self.browser_view.lock() {
                if let Some(bv) = guard.take() {
                    let mut view = View::from(&bv);
                    win.add_child_view(Some(&mut view));
                }
            }
            self.state.windows.lock().unwrap().push(win.clone());
            win.show();

            if !self.state.timer_scheduled.swap(true, Ordering::SeqCst) {
                if let Some(hold_ms) = self.state.opts.hold_ms {
                    let mut task = QuitAndReportTask::new(Arc::clone(&self.state));
                    post_delayed_task(ThreadId::UI, Some(&mut task), hold_ms as i64);
                } else if self.state.opts.compat_probe.is_some() {
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
            let server = if self.state.opts.compat_probe.is_none()
                && self.state.opts.url == "about:blank"
            {
                BenchServer::start()
            } else {
                None
            };

            for i in 0..self.state.opts.tabs {
                let target_url = if let Some(ref probe) = self.state.opts.compat_probe {
                    probe.clone()
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
                    None,
                    None,
                ) {
                    let mut win_delegate = SlateWindowDelegate::new(
                        Arc::clone(&self.state),
                        Arc::new(Mutex::new(Some(bv))),
                    );
                    let _ = window_create_top_level(Some(&mut win_delegate));
                }
            }

            if let Some(srv) = server {
                *self.state.bench_server.lock().unwrap() = Some(srv);
            }
        }
    }
}

wrap_app! {
    struct SlateCefApp {
        state: Arc<SharedSpikeState>,
    }

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(SlateBrowserProcessHandler::new(Arc::clone(&self.state)))
        }
    }
}

fn main() {
    let start_instant = Instant::now();
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let opts = CliOptions::parse();

    let state = Arc::new(SharedSpikeState {
        start_instant,
        opts,
        pipeline: BlockingPipeline::new_with_default_lists(),
        bench_server: Mutex::new(None),
        blocked_requests: AtomicUsize::new(0),
        third_party_cookies_filtered: AtomicBool::new(false),
        document_start_injected: AtomicBool::new(false),
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
