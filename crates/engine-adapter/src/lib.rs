#![deny(unsafe_code)]
//! `slate-engine-adapter`: The sole abstraction boundary between `slate-core` / `slate-blocking`
//! and the underlying web rendering engine (CEF or WebKitGTK).
//!
//! Security note: All safe adapter traits live in `lib.rs` under `#![deny(unsafe_code)]`.
//! The `cef` crate's `wrap_*!` FFI reference-counting macros are strictly confined to
//! [`cef_backend`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/cef_backend.rs).

#[allow(unsafe_code)]
pub mod cef_backend;
pub mod tabs;
pub mod test_servers;

use serde::{Deserialize, Serialize};
use slate_core::{ScrollPosition, TabId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Tab view not found: {0:?}")]
    TabNotFound(TabId),
    #[error("Engine operation failed: {0}")]
    OperationFailed(String),
}

/// Level of support an engine provides for a required privacy/ad-blocking capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportLevel {
    /// Supported directly via first-class engine APIs in the browser process.
    Native(String),
    /// Supported via a workaround (e.g. WebProcess extension `.so`, bytecode compilation, or JS).
    Workaround(String),
    /// Partial or missing support with known gaps.
    Gap(String),
}

/// Detailed capability report for an engine backend evaluated in Phase 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCapabilityReport {
    pub engine_name: String,
    pub engine_version: String,
    pub network_request_blocking: SupportLevel,
    pub cosmetic_filtering: SupportLevel,
    pub scriptlet_injection: SupportLevel,
    pub third_party_cookie_blocking: SupportLevel,
    pub https_upgrades: SupportLevel,
    pub tracking_param_stripping: SupportLevel,
    pub custom_filter_lists: SupportLevel,
    pub document_start_js_injection: SupportLevel,
    pub tab_discard_suspend: SupportLevel,
    pub interception_gaps: Vec<String>,
}

/// Trait implemented by concrete rendering engine backends (Spike A: CEF, Spike B: WebKitGTK).
pub trait BrowserEngineAdapter {
    /// Returns the static capability report for this engine.
    fn capability_report(&self) -> EngineCapabilityReport;

    /// Creates a new live engine view for `tab_id` and navigates to `url`.
    fn create_tab_view(&mut self, tab_id: TabId, url: &str) -> Result<(), EngineError>;

    /// Navigates an existing tab view to `url`.
    fn navigate(&mut self, tab_id: TabId, url: &str) -> Result<(), EngineError>;

    /// Registers a script to run at `document-start` before any page scripts execute.
    fn inject_document_start_script(
        &mut self,
        tab_id: TabId,
        script: &str,
    ) -> Result<(), EngineError>;

    /// Enables or disables third-party cookie blocking at the engine level.
    fn set_block_third_party_cookies(&mut self, block: bool) -> Result<(), EngineError>;

    /// Discards/destroys the underlying web view & renderer process for `tab_id` to free RAM
    /// while returning the last known scroll position.
    fn discard_tab_view(&mut self, tab_id: TabId) -> Result<ScrollPosition, EngineError>;

    /// Restores a previously discarded tab view to `url` and scrolls to `scroll`.
    fn restore_tab_view(
        &mut self,
        tab_id: TabId,
        url: &str,
        scroll: ScrollPosition,
    ) -> Result<(), EngineError>;
}

/// Returns the Phase 0 capability report for Engine A (CEF / Chromium 156).
pub fn cef_capability_report() -> EngineCapabilityReport {
    EngineCapabilityReport {
        engine_name: "CEF (Chromium Embedded Framework)".to_string(),
        engine_version: "156.4.0+156.0.26 (Chromium 156)".to_string(),
        network_request_blocking: SupportLevel::Native(
            "CefRequestHandler::on_before_browse + CefResourceRequestHandler::on_before_resource_load intercept every subresource (script, xhr/fetch, image, stylesheet, media, font, ping, websocket) in-process with full RequestType and initiator URL.".to_string(),
        ),
        cosmetic_filtering: SupportLevel::Native(
            "Per-site and generic CSS selectors from adblock-rust injected via CefRenderProcessHandler::on_context_created / DevTools Page.addScriptToEvaluateOnNewDocument + DOM style injection.".to_string(),
        ),
        scriptlet_injection: SupportLevel::Native(
            "V8 context creation hook (CefRenderProcessHandler::on_context_created) and CDP Page.addScriptToEvaluateOnNewDocument execute uBlock scriptlets synchronously before any page script on YouTube and all frames.".to_string(),
        ),
        third_party_cookie_blocking: SupportLevel::Native(
            "CefCookieAccessFilter (can_send_cookie / can_save_cookie) per request with full request URL + top-level site URL, plus Chromium third-party storage partitioning.".to_string(),
        ),
        https_upgrades: SupportLevel::Native(
            "In-place request.set_url() inside on_before_resource_load and navigation redirect inside on_before_browse, with automatic fallback in on_certificate_error / on_load_error.".to_string(),
        ),
        tracking_param_stripping: SupportLevel::Native(
            "Synchronous URL rewrite in on_before_browse and on_before_resource_load via request.set_url() before network socket connection.".to_string(),
        ),
        custom_filter_lists: SupportLevel::Native(
            "Direct Rust calls to slate-blocking Arc<RwLock<Engine>> from CEF IO thread; zero rule-count limit and zero conversion delay.".to_string(),
        ),
        document_start_js_injection: SupportLevel::Native(
            "CefRenderProcessHandler::on_context_created + DevTools Page.addScriptToEvaluateOnNewDocument.".to_string(),
        ),
        tab_discard_suspend: SupportLevel::Native(
            "CefBrowserHost::close_browser(1) terminates the tab's renderer process and reclaims all V8/Blink/GPU-tile RAM while TabManager retains URL, title, and scroll position.".to_string(),
        ),
        interception_gaps: vec![
            "POST request bodies are readable via request.post_data(), except chunked streaming upload pipes where elements may not be buffered in memory.".to_string(),
            "Service Worker internal background fetches that bypass a Browser/Frame pointer require attaching CefRequestContextHandler::get_resource_request_handler on the shared CefRequestContext (supported by CEF, must be wired on RequestContext in addition to Client).".to_string(),
        ],
    }
}

/// Returns the Phase 0 capability report for Engine B (WebKitGTK 2.54).
pub fn webkitgtk_capability_report() -> EngineCapabilityReport {
    EngineCapabilityReport {
        engine_name: "WebKitGTK (webkit2gtk-4.1)".to_string(),
        engine_version: "2.54.2 (webkit2gtk-rs 2.0.2)".to_string(),
        network_request_blocking: SupportLevel::Workaround(
            "UI-process WebView::connect_decide_policy only intercepts navigations and downloads, NOT subresources (scripts, XHR, images). Subresource blocking requires either compiling rules to Apple Content Blocking JSON (capped at 150k rules, missing uBO modifiers) or shipping a separate C/Rust WebProcess extension .so using WebPage::send-request.".to_string(),
        ),
        cosmetic_filtering: SupportLevel::Native(
            "UserContentManager::add_style_sheet and UserScript at UserScriptInjectionTime::Start.".to_string(),
        ),
        scriptlet_injection: SupportLevel::Native(
            "UserContentManager::add_script with UserScriptInjectionTime::Start and UserContentInjectedFrames::AllFrames (isolated world or page world).".to_string(),
        ),
        third_party_cookie_blocking: SupportLevel::Native(
            "CookieManager::set_accept_policy(CookieAcceptPolicy::NoThirdParty) plus WebsiteDataManager Intelligent Tracking Prevention (set_itp_enabled(true)). Does not expose per-request Rust callback for custom eTLD+1 exceptions.".to_string(),
        ),
        https_upgrades: SupportLevel::Workaround(
            "Top-level navigations can be cancelled in connect_decide_policy and re-issued via load_uri('https://...'), but subresource HTTP->HTTPS upgrades cannot be mutated from the UI process without a WebExtension .so.".to_string(),
        ),
        tracking_param_stripping: SupportLevel::Native(
            "Top-level navigation URLs can be inspected in connect_decide_policy (NavigationAction), cancelled via decision.ignore(), and reloaded with stripped parameters.".to_string(),
        ),
        custom_filter_lists: SupportLevel::Gap(
            "If using WebKitUserContentFilterStore, every custom filter change must be converted to Apple JSON and recompiled into WebKit bytecode (slow, 150k rule ceiling, drops unsupported uBO rules). If using a WebExtension .so, filter updates must be serialized over IPC to every WebProcess.".to_string(),
        ),
        document_start_js_injection: SupportLevel::Native(
            "UserContentManager::add_script(UserScript::new(..., UserScriptInjectionTime::Start, ...)).".to_string(),
        ),
        tab_discard_suspend: SupportLevel::Native(
            "Destroying the gtk::Widget / WebView (or calling web_view.terminate_web_process()) kills the WebKitWebProcess and frees its memory; recreating the WebView on tab focus restores the URL and scroll position.".to_string(),
        ),
        interception_gaps: vec![
            "UI process cannot intercept or block subresources (XHR, fetch, script, image, stylesheet) directly via decide-policy; WebResource::connect_resource_load_started is read-only notification after request initiation.".to_string(),
            "WebSocket handshakes and Service Worker fetches bypass WebKitWebPage::send-request even when using a WebProcess extension.".to_string(),
            "POST request bodies are not exposed on WebKitURIRequest in either UI process or WebExtension.".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cef_backend::ServiceWorkerTestServer;
    use slate_blocking::BlockingPipeline;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    #[test]
    fn capability_reports_document_cef_and_webkit_tradeoffs() {
        let cef = cef_capability_report();
        let wk = webkitgtk_capability_report();

        assert!(matches!(
            cef.network_request_blocking,
            SupportLevel::Native(_)
        ));
        assert!(matches!(
            wk.network_request_blocking,
            SupportLevel::Workaround(_)
        ));
        assert!(!cef.interception_gaps.is_empty());
        assert!(!wk.interception_gaps.is_empty());
    }

    #[test]
    fn service_worker_requests_are_evaluated_and_blocked_by_request_context_pipeline() {
        let srv = ServiceWorkerTestServer::start().expect("Failed to start SW test server");
        let root = srv.root_url();
        let addr = root.strip_prefix("http://").unwrap().trim_end_matches('/');

        let mut stream = TcpStream::connect(addr).expect("Connect to SW test server");
        stream
            .write_all(b"GET /sw.js HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.contains("RUN_SW_BLOCK_TEST"));

        let pipeline = BlockingPipeline::new_with_default_lists();
        // Simulate RequestContextHandler::resource_request_handler when frame is None (Service Worker fetch)
        let sw_initiator = "https://service-worker.slate.local/";
        let ad_decision = pipeline.evaluate_network_request(
            "https://securepubads.g.doubleclick.net/gpt/pubads_impl_sw_test.js",
            sw_initiator,
            "xmlhttprequest",
            "GET",
        );
        assert!(ad_decision.should_block);

        let tracker_decision = pipeline.evaluate_network_request(
            "https://www.google-analytics.com/analytics.js",
            sw_initiator,
            "script",
            "GET",
        );
        assert!(tracker_decision.should_block);

        let allowed_decision = pipeline.evaluate_network_request(
            &format!("{root}sw-allowed.json"),
            &root,
            "xmlhttprequest",
            "GET",
        );
        assert!(!allowed_decision.should_block);
    }

    #[test]
    fn unsafe_code_is_forbidden_in_all_crates_outside_engine_adapter() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let crates_requiring_forbid = [
            "crates/core/src/lib.rs",
            "crates/blocking/src/lib.rs",
            "crates/ui-bridge/src/lib.rs",
            "spikes/spike-cef/src/main.rs",
            "crates/app/src/main.rs",
        ];
        for rel_path in crates_requiring_forbid {
            let content = std::fs::read_to_string(workspace_root.join(rel_path))
                .unwrap_or_else(|e| panic!("Failed to read {rel_path}: {e}"));
            assert!(
                content.contains("#![forbid(unsafe_code)]"),
                "Crate {rel_path} must contain #![forbid(unsafe_code)]"
            );
        }
    }
}
