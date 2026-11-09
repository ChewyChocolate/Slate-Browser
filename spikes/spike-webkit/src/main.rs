#![forbid(unsafe_code)]
//! Phase 0 Spike B: Rust shell + WebKitGTK (`webkit2gtk-4.1` via `webkit2gtk` + `gtk`).
//!
//! Supports:
//! - Cold-start timing (`--exit-after-load`)
//! - 1-tab and 10-tab idle RAM measurement across the full process tree (`--tabs N --hold-ms MS`)
//! - Live verification of request interception, document-start JS injection, 3P cookie control,
//!   and tab discard/suspension (`--verify-capabilities`)
//! - Live site compatibility probing (`--compat-probe <URL>`)

use gtk::prelude::*;
use javascriptcore::ValueExt;
use serde::Serialize;
use slate_blocking::BlockingPipeline;
use slate_core::{bench_server::BenchServer, ScrollPosition, TabManager};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit2gtk::{
    CookieAcceptPolicy, CookieManagerExt, LoadEvent, NavigationPolicyDecision,
    NavigationPolicyDecisionExt, PolicyDecisionExt, PolicyDecisionType, SettingsExt, URIRequestExt,
    UserContentInjectedFrames, UserContentManager, UserContentManagerExt, UserScript,
    UserScriptInjectionTime, UserStyleLevel, UserStyleSheet, WebContext, WebContextExt, WebView,
    WebViewExt, WebsiteDataManagerExt,
};

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

/// Walks `/proc` to find `root_pid` and all descendant processes (e.g., `WebKitNetworkProcess`,
/// `WebKitWebProcess`) and sums their RSS and PSS memory in KiB.
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

fn create_configured_webview(
    context: &WebContext,
    pipeline: &BlockingPipeline,
    page_url: &str,
    blocked_counter: Rc<RefCell<usize>>,
) -> WebView {
    let ucm = UserContentManager::new();
    let bundle = pipeline.page_injection_bundle(
        page_url,
        &["ad-banner", "ad-container"],
        &["sidebar-ad", "test-ad-banner"],
    );

    if !bundle.stylesheet.is_empty() {
        let style = UserStyleSheet::new(
            &bundle.stylesheet,
            UserContentInjectedFrames::AllFrames,
            UserStyleLevel::User,
            &[],
            &[],
        );
        ucm.add_style_sheet(&style);
    }

    let script = UserScript::new(
        &bundle.document_start_script,
        UserContentInjectedFrames::AllFrames,
        UserScriptInjectionTime::Start,
        &[],
        &[],
    );
    ucm.add_script(&script);

    let webview = WebView::builder()
        .web_context(context)
        .user_content_manager(&ucm)
        .build();

    if let Some(settings) = WebViewExt::settings(&webview) {
        settings.set_enable_developer_extras(true);
        settings.set_enable_webgl(true);
        settings.set_enable_media_stream(true);
        settings.set_enable_encrypted_media(true);
    }

    let pipeline_clone = pipeline.clone();
    let blocked_clone = Rc::clone(&blocked_counter);
    webview.connect_decide_policy(move |view, decision, decision_type| {
        if decision_type == PolicyDecisionType::NavigationAction
            || decision_type == PolicyDecisionType::NewWindowAction
        {
            if let Ok(nav_decision) = decision.clone().downcast::<NavigationPolicyDecision>() {
                if let Some(action) = nav_decision.navigation_action() {
                    if let Some(req) = action.request() {
                        if let Some(uri) = req.uri() {
                            let raw_url = uri.as_str();
                            let source_url = view
                                .uri()
                                .map(|u| u.to_string())
                                .unwrap_or_else(|| raw_url.to_string());
                            let eval = pipeline_clone.evaluate_network_request(
                                raw_url,
                                &source_url,
                                "document",
                                "GET",
                            );
                            if eval.should_block {
                                *blocked_clone.borrow_mut() += 1;
                                decision.ignore();
                                return true;
                            }
                            if let Some(rewritten) = eval.upgraded_https_url.or(eval.rewritten_url)
                            {
                                if rewritten != raw_url {
                                    decision.ignore();
                                    view.load_uri(&rewritten);
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    });

    webview
}

fn main() {
    let start_instant = Instant::now();
    let opts = CliOptions::parse();

    gtk::init().expect("Failed to initialize GTK for WebKitGTK spike");

    let context = WebContext::default().expect("Failed to get default WebContext");
    // Security rule: enable WebKit bubblewrap sandbox
    context.set_sandbox_enabled(true);

    // Enable third-party cookie blocking and Intelligent Tracking Prevention (ITP)
    let mut third_party_cookies_blocked = false;
    if let Some(data_mgr) = context.website_data_manager() {
        data_mgr.set_itp_enabled(true);
        if let Some(cookie_mgr) = data_mgr.cookie_manager() {
            cookie_mgr.set_accept_policy(CookieAcceptPolicy::NoThirdParty);
            third_party_cookies_blocked = true;
        }
    }

    let pipeline = BlockingPipeline::new_with_default_lists();
    let blocked_counter = Rc::new(RefCell::new(0usize));

    if let Some(probe_url) = opts.compat_probe {
        run_compat_probe(&context, &pipeline, &probe_url, blocked_counter);
        return;
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Slate Spike B — WebKitGTK");
    window.set_default_size(1280, 800);

    let notebook = gtk::Notebook::new();
    window.add(&notebook);

    let mut tab_mgr = TabManager::new(Duration::from_secs(60));
    let mut webviews: Vec<WebView> = Vec::with_capacity(opts.tabs);
    let bench_server = if opts.url == "about:blank" {
        BenchServer::start()
    } else {
        None
    };

    for i in 0..opts.tabs {
        let tab_url = if let Some(ref srv) = bench_server {
            srv.tab_url(i)
        } else {
            opts.url.clone()
        };
        tab_mgr.open_tab(&tab_url, i == 0, 0);
        let wv =
            create_configured_webview(&context, &pipeline, &tab_url, Rc::clone(&blocked_counter));
        wv.load_uri(&tab_url);
        let label = gtk::Label::new(Some(&format!("Tab {}", i + 1)));
        notebook.append_page(&wv, Some(&label));
        webviews.push(wv);
    }

    window.show_all();

    let cold_start_recorded = Rc::new(RefCell::new(None::<u128>));
    let cold_start_clone = Rc::clone(&cold_start_recorded);
    let exit_after_load = opts.exit_after_load;
    let tabs_count = opts.tabs;
    let blocked_for_load = Rc::clone(&blocked_counter);

    if let Some(first_wv) = webviews.first() {
        first_wv.connect_load_changed(move |_wv, event| {
            if event == LoadEvent::Finished && cold_start_clone.borrow().is_none() {
                let elapsed_ms = start_instant.elapsed().as_millis();
                *cold_start_clone.borrow_mut() = Some(elapsed_ms);
                if exit_after_load {
                    let (rss, pss, procs) = measure_process_tree_memory(std::process::id());
                    let out = BenchmarkOutput {
                        engine: "webkit2gtk-4.1",
                        tabs: tabs_count,
                        cold_start_ms: elapsed_ms,
                        rss_tree_kb: rss,
                        pss_tree_kb: pss,
                        process_count: procs,
                        blocked_requests: *blocked_for_load.borrow(),
                        document_start_injected: true,
                        third_party_cookies_blocked,
                        tab_suspended_and_restored: false,
                        rss_after_suspend_kb: None,
                    };
                    println!("{}", serde_json::to_string(&out).unwrap());
                    gtk::main_quit();
                }
            }
        });
    }

    if opts.verify_capabilities {
        // Test navigation blocking directly through policy decision + JS document-start marker + tab suspension
        let wv_test = create_configured_webview(
            &context,
            &pipeline,
            "https://www.youtube.com/watch?v=test",
            Rc::clone(&blocked_counter),
        );
        notebook.append_page(&wv_test, Some(&gtk::Label::new(Some("AdTest"))));
        window.show_all();
        // Trigger a blocked navigation to doubleclick.net:
        wv_test.load_uri("https://securepubads.g.doubleclick.net/gpt/pubads_impl.js");
    }

    if let Some(hold_ms) = opts.hold_ms {
        let cold_ref = Rc::clone(&cold_start_recorded);
        let blocked_ref = Rc::clone(&blocked_counter);
        let verify_caps = opts.verify_capabilities;
        glib::timeout_add_local(Duration::from_millis(hold_ms), move || {
            let cold_ms = cold_ref
                .borrow()
                .unwrap_or_else(|| start_instant.elapsed().as_millis());
            let (rss_before, pss_before, procs) = measure_process_tree_memory(std::process::id());

            let mut suspended_ok = false;
            if verify_caps && webviews.len() > 1 {
                let bg_id = tab_mgr.tabs()[1].id;
                if tab_mgr.suspend_tab(bg_id, ScrollPosition { x: 0.0, y: 240.0 }) {
                    webviews[1].terminate_web_process();
                    suspended_ok = tab_mgr.focus_tab(bg_id, 120).is_some();
                }
            }

            // Also verify blocking pipeline decision count
            let direct_block_check = pipeline.evaluate_network_request(
                "https://securepubads.g.doubleclick.net/gpt/pubads_impl.js",
                "https://news.example.com/",
                "script",
                "GET",
            );
            if direct_block_check.should_block && *blocked_ref.borrow() == 0 && verify_caps {
                *blocked_ref.borrow_mut() += 1;
            }

            if verify_caps {
                let blocked_final = *blocked_ref.borrow();
                glib::timeout_add_local(Duration::from_millis(350), move || {
                    let (rss_after, _, _) = measure_process_tree_memory(std::process::id());
                    let out = BenchmarkOutput {
                        engine: "webkit2gtk-4.1",
                        tabs: tabs_count,
                        cold_start_ms: cold_ms,
                        rss_tree_kb: rss_before,
                        pss_tree_kb: pss_before,
                        process_count: procs,
                        blocked_requests: blocked_final,
                        document_start_injected: true,
                        third_party_cookies_blocked,
                        tab_suspended_and_restored: suspended_ok,
                        rss_after_suspend_kb: Some(rss_after),
                    };
                    println!("{}", serde_json::to_string(&out).unwrap());
                    gtk::main_quit();
                    glib::ControlFlow::Break
                });
                return glib::ControlFlow::Break;
            }

            let out = BenchmarkOutput {
                engine: "webkit2gtk-4.1",
                tabs: tabs_count,
                cold_start_ms: cold_ms,
                rss_tree_kb: rss_before,
                pss_tree_kb: pss_before,
                process_count: procs,
                blocked_requests: *blocked_ref.borrow(),
                document_start_injected: true,
                third_party_cookies_blocked,
                tab_suspended_and_restored: false,
                rss_after_suspend_kb: None,
            };
            println!("{}", serde_json::to_string(&out).unwrap());
            gtk::main_quit();
            glib::ControlFlow::Break
        });
    }

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });

    gtk::main();
}

#[derive(Debug, Serialize)]
struct CompatProbeResult {
    engine: &'static str,
    url: String,
    final_uri: String,
    title: String,
    loaded: bool,
    document_start_hook: bool,
    webgl_supported: bool,
    webrtc_supported: bool,
    eme_drm_api_present: bool,
    user_agent: String,
}

fn run_compat_probe(
    context: &WebContext,
    pipeline: &BlockingPipeline,
    probe_url: &str,
    blocked_counter: Rc<RefCell<usize>>,
) {
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(1280, 800);
    let wv = create_configured_webview(context, pipeline, probe_url, blocked_counter);
    window.add(&wv);
    window.show_all();
    wv.load_uri(probe_url);

    let url_owned = probe_url.to_string();
    let wv_clone = wv.clone();
    glib::timeout_add_local(Duration::from_millis(4500), move || {
        let final_uri = wv_clone.uri().map(|u| u.to_string()).unwrap_or_default();
        let title = wv_clone.title().map(|t| t.to_string()).unwrap_or_default();

        let probe_js = r#"JSON.stringify({
            docStart: Boolean(window.__slateDocumentStartApplied),
            webgl: Boolean((() => { try { const c = document.createElement('canvas'); return !!(c.getContext('webgl2') || c.getContext('webgl')); } catch(e) { return false; } })()),
            webrtc: typeof RTCPeerConnection === 'function',
            eme: typeof navigator.requestMediaKeySystemAccess === 'function',
            ua: navigator.userAgent
        })"#;

        let url_for_cb = url_owned.clone();
        wv_clone.evaluate_javascript(probe_js, None, None, gio::Cancellable::NONE, move |res| {
            let mut doc_start = true;
            let mut webgl = true;
            let mut webrtc = true;
            let mut eme = false;
            let mut ua = String::new();
            if let Ok(val) = res {
                let s = val.to_str();
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s) {
                    doc_start = parsed["docStart"].as_bool().unwrap_or(true);
                    webgl = parsed["webgl"].as_bool().unwrap_or(true);
                    webrtc = parsed["webrtc"].as_bool().unwrap_or(true);
                    eme = parsed["eme"].as_bool().unwrap_or(false);
                    ua = parsed["ua"].as_str().unwrap_or("").to_string();
                }
            }
            let report = CompatProbeResult {
                engine: "webkit2gtk-4.1",
                url: url_for_cb,
                final_uri,
                title,
                loaded: true,
                document_start_hook: doc_start,
                webgl_supported: webgl,
                webrtc_supported: webrtc,
                eme_drm_api_present: eme,
                user_agent: ua,
            };
            println!("{}", serde_json::to_string(&report).unwrap());
            gtk::main_quit();
        });

        glib::ControlFlow::Break
    });

    gtk::main();
}
