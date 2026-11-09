#![forbid(unsafe_code)]
//! `slate-core`: Engine-agnostic browser state, tab lifecycle, and tab suspension policy.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Unique identifier for a browser tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabId(pub u64);

/// Scroll position saved before a tab is discarded/suspended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollPosition {
    pub x: f64,
    pub y: f64,
}

/// Lifecycle state of a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabLifecycleState {
    /// Tab is live in the rendering engine.
    Active,
    /// Tab has been discarded from the rendering engine to free RAM; metadata is retained.
    Suspended,
}

/// Metadata and state for a single browser tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub lifecycle: TabLifecycleState,
    pub scroll: ScrollPosition,
    pub is_playing_audio: bool,
    pub is_playing_video: bool,
    pub is_loading: bool,
    /// Monotonic timestamp (in seconds since browser start) of last user interaction/focus.
    pub last_active_secs: u64,
}

impl TabState {
    pub fn new(id: TabId, url: impl Into<String>, now_secs: u64) -> Self {
        let url_str = url.into();
        Self {
            id,
            title: url_str.clone(),
            url: url_str,
            lifecycle: TabLifecycleState::Active,
            scroll: ScrollPosition::default(),
            is_playing_audio: false,
            is_playing_video: false,
            is_loading: false,
            last_active_secs: now_secs,
        }
    }

    /// Returns true if this tab is eligible to be suspended after `inactivity_timeout` at `now_secs`.
    pub fn can_suspend(
        &self,
        is_focused: bool,
        now_secs: u64,
        inactivity_timeout: Duration,
    ) -> bool {
        if is_focused {
            return false;
        }
        if self.lifecycle == TabLifecycleState::Suspended {
            return false;
        }
        if self.is_playing_audio || self.is_playing_video {
            return false;
        }
        let elapsed = now_secs.saturating_sub(self.last_active_secs);
        elapsed >= inactivity_timeout.as_secs()
    }
}

/// Manages the ordered list of tabs, active selection, and tab suspension rules.
#[derive(Debug, Clone)]
pub struct TabManager {
    tabs: Vec<TabState>,
    active_tab: Option<TabId>,
    next_id: u64,
    suspend_timeout: Duration,
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

impl TabManager {
    pub fn new(suspend_timeout: Duration) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            next_id: 1,
            suspend_timeout,
        }
    }

    pub fn open_tab(&mut self, url: impl Into<String>, activate: bool, now_secs: u64) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        let tab = TabState::new(id, url, now_secs);
        self.tabs.push(tab);
        if activate || self.active_tab.is_none() {
            self.active_tab = Some(id);
        }
        id
    }

    pub fn focus_tab(&mut self, id: TabId, now_secs: u64) -> Option<&mut TabState> {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active_tab = Some(id);
            let tab = self.tabs.iter_mut().find(|t| t.id == id)?;
            tab.last_active_secs = now_secs;
            if tab.lifecycle == TabLifecycleState::Suspended {
                tab.lifecycle = TabLifecycleState::Active;
            }
            Some(tab)
        } else {
            None
        }
    }

    pub fn suspend_tab(&mut self, id: TabId, scroll: ScrollPosition) -> bool {
        if self.active_tab == Some(id) {
            return false;
        }
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            if tab.is_playing_audio || tab.is_playing_video {
                return false;
            }
            tab.scroll = scroll;
            tab.lifecycle = TabLifecycleState::Suspended;
            true
        } else {
            false
        }
    }

    pub fn candidates_for_suspension(&self, now_secs: u64) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|t| {
                t.can_suspend(
                    self.active_tab == Some(t.id),
                    now_secs,
                    self.suspend_timeout,
                )
            })
            .map(|t| t.id)
            .collect()
    }

    pub fn tabs(&self) -> &[TabState] {
        &self.tabs
    }

    pub fn get_tab(&self, id: TabId) -> Option<&TabState> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_tab_mut(&mut self, id: TabId) -> Option<&mut TabState> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn active_tab_id(&self) -> Option<TabId> {
        self.active_tab
    }
}

/// Lightweight local HTTP server for reproducible multi-origin 1-tab and 10-tab benchmarks.
pub mod bench_server {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;

    pub struct BenchServer {
        port: u16,
        stop: Arc<AtomicBool>,
    }

    impl BenchServer {
        pub fn start() -> Option<Self> {
            let listener = TcpListener::bind("0.0.0.0:0").ok()?;
            let port = listener.local_addr().ok()?.port();
            let _ = listener.set_nonblocking(false);
            let stop = Arc::new(AtomicBool::new(false));
            let stop_clone = Arc::clone(&stop);

            thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::spawn(move || {
                        let mut s = stream;
                        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(100)));
                        let mut buf = [0u8; 1024];
                        let n = s.read(&mut buf).unwrap_or(0);
                        let req_line = String::from_utf8_lossy(&buf[..n]);
                        let tab_tag = req_line
                            .lines()
                            .next()
                            .unwrap_or("GET /")
                            .split_whitespace()
                            .nth(1)
                            .unwrap_or("/");
                        let body = format!(
                            r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Slate Phase 0 Benchmark Tab</title>
<script src="https://securepubads.g.doubleclick.net/gpt/pubads_impl_2026.js{tab_tag}"></script>
</head>
<body>
  <h1 id="headline">Slate Phase 0 Benchmark Page</h1>
  <div class="ad-banner" id="test-ad-banner">Sponsored Ad Banner</div>
  <div id="content">Normal page content</div>
</body>
</html>"#
                        );
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = s.write_all(resp.as_bytes());
                        let _ = s.flush();
                    });
                }
            });

            Some(Self { port, stop })
        }

        pub fn tab_url(&self, index: usize) -> String {
            let octet = (index % 250) + 1;
            format!("http://127.0.0.{octet}:{}/?tab={index}", self.port)
        }
    }

    impl Drop for BenchServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_suspension_preserves_state_and_excludes_media_or_active_tabs() {
        let mut mgr = TabManager::new(Duration::from_secs(60));
        let _t1 = mgr.open_tab("https://example.com/1", true, 10);
        let t2 = mgr.open_tab("https://example.com/2", false, 10);
        let t3 = mgr.open_tab("https://youtube.com/watch?v=123", false, 10);

        mgr.get_tab_mut(t2).unwrap().title = "Article Two".to_string();
        mgr.get_tab_mut(t3).unwrap().is_playing_audio = true;

        // At t=80s (70s elapsed >= 60s timeout):
        // - t1 is focused -> not eligible
        // - t2 is inactive & silent -> eligible
        // - t3 is playing audio -> not eligible
        let candidates = mgr.candidates_for_suspension(80);
        assert_eq!(candidates, vec![t2]);

        assert!(mgr.suspend_tab(t2, ScrollPosition { x: 0.0, y: 420.5 }));
        let suspended = mgr.get_tab(t2).unwrap();
        assert_eq!(suspended.lifecycle, TabLifecycleState::Suspended);
        assert_eq!(suspended.url, "https://example.com/2");
        assert_eq!(suspended.title, "Article Two");
        assert_eq!(suspended.scroll, ScrollPosition { x: 0.0, y: 420.5 });

        // Focusing t2 restores it to Active while keeping URL, title, and scroll position.
        let restored = mgr.focus_tab(t2, 90).unwrap();
        assert_eq!(restored.lifecycle, TabLifecycleState::Active);
        assert_eq!(restored.url, "https://example.com/2");
        assert_eq!(restored.title, "Article Two");
        assert_eq!(restored.scroll, ScrollPosition { x: 0.0, y: 420.5 });
    }
}
