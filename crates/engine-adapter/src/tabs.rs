//! Pure (no CEF types) tab registry and IPC gate. Safe Rust, unit-tested.
//!
//! The registry is keyed by CEF browser id so request filters (which only receive a `Browser`)
//! can find the tab and its top-level site. Blocked counts live here, per tab, and are only ever
//! incremented by the real blocking pipeline.

use slate_core::{ScrollPosition, TabId, TabLifecycleState};
use slate_ui_bridge::TabView;

/// Origin of the privileged shell page. Only this origin may talk to the core.
pub const SHELL_ORIGIN: &str = "slate://ui";

pub fn is_shell_origin(url: &str) -> bool {
    url == SHELL_ORIGIN || url.starts_with("slate://ui/")
}

/// Decides whether a process message may be treated as a core command.
/// Rule: it must come from the single shell browser, from its main frame, whose URL is the shell origin.
#[derive(Debug, Clone, Copy, Default)]
pub struct IpcGate {
    pub shell_browser_id: Option<i32>,
}

impl IpcGate {
    pub fn authorize(&self, browser_id: i32, frame_is_main: bool, frame_url: &str) -> bool {
        self.shell_browser_id == Some(browser_id) && frame_is_main && is_shell_origin(frame_url)
    }
}

#[derive(Debug, Clone)]
pub struct TabEntry {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub browser_id: Option<i32>,
    pub lifecycle: TabLifecycleState,
    pub scroll: ScrollPosition,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// `None` until the tab has navigated to a web page and counting has started.
    pub blocked: Option<u64>,
}

#[derive(Debug, Default)]
pub struct TabRegistry {
    tabs: Vec<TabEntry>,
    active: Option<TabId>,
    next_id: u64,
}

impl TabRegistry {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            next_id: 1,
        }
    }

    pub fn open_tab(&mut self, url: &str, activate: bool) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        self.tabs.push(TabEntry {
            id,
            url: url.to_string(),
            title: String::new(),
            browser_id: None,
            lifecycle: TabLifecycleState::Active,
            scroll: ScrollPosition::default(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            blocked: None,
        });
        if activate || self.active.is_none() {
            self.active = Some(id);
        }
        id
    }

    pub fn get(&self, id: TabId) -> Option<&TabEntry> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut TabEntry> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    pub fn ids(&self) -> Vec<TabId> {
        self.tabs.iter().map(|t| t.id).collect()
    }

    pub fn focus(&mut self, id: TabId) -> bool {
        if self.get(id).is_some() {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    /// Removes the tab. Returns the tab that became active (if the closed one was active).
    pub fn close(&mut self, id: TabId) -> Option<TabId> {
        let idx = self.tabs.iter().position(|t| t.id == id)?;
        self.tabs.remove(idx);
        if self.active == Some(id) {
            self.active = if self.tabs.is_empty() {
                None
            } else {
                Some(self.tabs[idx.saturating_sub(1).min(self.tabs.len() - 1)].id)
            };
            return self.active;
        }
        None
    }

    pub fn attach_browser(&mut self, id: TabId, browser_id: i32) {
        if let Some(t) = self.get_mut(id) {
            t.browser_id = Some(browser_id);
        }
    }

    pub fn detach_browser(&mut self, id: TabId) {
        if let Some(t) = self.get_mut(id) {
            t.browser_id = None;
            t.loading = false;
            t.can_go_back = false;
            t.can_go_forward = false;
        }
    }

    pub fn tab_for_browser(&self, browser_id: i32) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|t| t.browser_id == Some(browser_id))
            .map(|t| t.id)
    }

    /// Top-level URL of the tab owning `browser_id` (committed main-frame URL).
    pub fn top_level_url(&self, browser_id: i32) -> Option<String> {
        self.tabs
            .iter()
            .find(|t| t.browser_id == Some(browser_id))
            .map(|t| t.url.clone())
    }

    /// Called by the real blocking pipeline when it cancels a request. Returns the new count.
    /// Requests from browsers that are not tabs (shell, service workers) are not counted.
    pub fn record_blocked(&mut self, browser_id: i32) -> Option<u64> {
        let t = self
            .tabs
            .iter_mut()
            .find(|t| t.browser_id == Some(browser_id))?;
        let n = t.blocked.unwrap_or(0) + 1;
        t.blocked = Some(n);
        Some(n)
    }

    /// A new document is being loaded in the main frame: counting restarts at a real 0.
    pub fn reset_blocked(&mut self, id: TabId) {
        if let Some(t) = self.get_mut(id) {
            t.blocked = Some(0);
        }
    }

    /// Marks the tab suspended: browser detached, metadata (url, title, scroll) retained.
    pub fn suspend(&mut self, id: TabId, scroll: ScrollPosition) -> bool {
        match self.get_mut(id) {
            Some(t) if t.lifecycle == TabLifecycleState::Active => {
                t.lifecycle = TabLifecycleState::Suspended;
                t.scroll = scroll;
                t.browser_id = None;
                t.loading = false;
                t.can_go_back = false;
                t.can_go_forward = false;
                true
            }
            _ => false,
        }
    }

    pub fn mark_active(&mut self, id: TabId) {
        if let Some(t) = self.get_mut(id) {
            t.lifecycle = TabLifecycleState::Active;
        }
    }

    pub fn views(&self) -> Vec<TabView> {
        self.tabs
            .iter()
            .map(|t| TabView {
                id: t.id,
                url: t.url.clone(),
                title: if t.title.is_empty() {
                    t.url.clone()
                } else {
                    t.title.clone()
                },
                lifecycle: t.lifecycle,
                loading: t.loading,
                can_go_back: t.can_go_back,
                can_go_forward: t.can_go_forward,
                blocked: t.blocked,
            })
            .collect()
    }
}

/// Normalizes text typed in the address bar into a navigable URL.
/// Only http/https are navigable from the address bar; `slate:`, `file:`, `javascript:`, `data:` are rejected.
pub fn normalize_address(input: &str) -> Option<String> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(t.to_string());
    }
    if let Some((scheme, _)) = t.split_once(':') {
        let is_scheme = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
        // "localhost:3000" and "host:port" look like schemes; treat as host:port when the rest is digits.
        let rest = &t[scheme.len() + 1..];
        let looks_like_port = rest
            .split('/')
            .next()
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        if is_scheme && !looks_like_port {
            return None;
        }
    }
    if t.contains(' ') {
        return None;
    }
    Some(format!("https://{t}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_accepts_only_the_shell_browser_main_frame_on_shell_origin() {
        let gate = IpcGate {
            shell_browser_id: Some(1),
        };
        assert!(gate.authorize(1, true, "slate://ui/index.html"));
        assert!(
            !gate.authorize(2, true, "slate://ui/index.html"),
            "other browser"
        );
        assert!(
            !gate.authorize(1, false, "slate://ui/index.html"),
            "subframe"
        );
        assert!(
            !gate.authorize(1, true, "https://evil.example/"),
            "web origin"
        );
        assert!(
            !gate.authorize(1, true, "slate://ui.evil.example/"),
            "lookalike origin"
        );
        assert!(
            !IpcGate::default().authorize(1, true, "slate://ui/index.html"),
            "no shell yet"
        );
    }

    #[test]
    fn blocked_counts_are_per_tab_and_only_from_recorded_blocks() {
        let mut r = TabRegistry::new();
        let a = r.open_tab("https://a.example/", true);
        let b = r.open_tab("https://b.example/", false);
        r.attach_browser(a, 10);
        r.attach_browser(b, 11);
        assert_eq!(
            r.get(a).unwrap().blocked,
            None,
            "no data before counting starts"
        );
        r.reset_blocked(a);
        r.reset_blocked(b);
        assert_eq!(r.record_blocked(10), Some(1));
        assert_eq!(r.record_blocked(10), Some(2));
        assert_eq!(r.record_blocked(11), Some(1));
        assert_eq!(r.record_blocked(99), None, "unknown browser is not counted");
        assert_eq!(r.get(a).unwrap().blocked, Some(2));
        assert_eq!(r.get(b).unwrap().blocked, Some(1));
        r.reset_blocked(a);
        assert_eq!(r.get(a).unwrap().blocked, Some(0));
    }

    #[test]
    fn top_level_url_comes_from_the_tab() {
        let mut r = TabRegistry::new();
        let a = r.open_tab("https://a.example/", true);
        r.attach_browser(a, 5);
        assert_eq!(r.top_level_url(5).as_deref(), Some("https://a.example/"));
        assert_eq!(r.top_level_url(6), None);
    }

    #[test]
    fn close_and_focus_pick_a_neighbour() {
        let mut r = TabRegistry::new();
        let a = r.open_tab("https://a/", true);
        let b = r.open_tab("https://b/", true);
        let c = r.open_tab("https://c/", true);
        assert_eq!(r.active(), Some(c));
        assert_eq!(r.close(c), Some(b));
        assert!(r.focus(a));
        assert_eq!(
            r.close(b),
            None,
            "closing a background tab keeps the active tab"
        );
        assert_eq!(r.active(), Some(a));
        assert_eq!(r.close(a), None);
        assert_eq!(r.active(), None);
    }

    #[test]
    fn suspend_keeps_metadata_and_detaches_browser() {
        let mut r = TabRegistry::new();
        let a = r.open_tab("https://a/", true);
        r.attach_browser(a, 7);
        r.get_mut(a).unwrap().title = "A".into();
        assert!(r.suspend(a, ScrollPosition { x: 0.0, y: 321.0 }));
        let t = r.get(a).unwrap();
        assert_eq!(t.lifecycle, TabLifecycleState::Suspended);
        assert_eq!(t.browser_id, None);
        assert_eq!(t.scroll.y, 321.0);
        assert_eq!(t.title, "A");
        assert!(
            !r.suspend(a, ScrollPosition::default()),
            "already suspended"
        );
        assert_eq!(r.tab_for_browser(7), None);
    }

    #[test]
    fn address_bar_only_navigates_to_web_urls() {
        assert_eq!(
            normalize_address("example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_address(" http://a.b/c ").as_deref(),
            Some("http://a.b/c")
        );
        assert_eq!(
            normalize_address("localhost:3000").as_deref(),
            Some("https://localhost:3000")
        );
        assert_eq!(normalize_address("javascript:alert(1)"), None);
        assert_eq!(normalize_address("slate://ui/index.html"), None);
        assert_eq!(normalize_address("file:///etc/passwd"), None);
        assert_eq!(normalize_address("data:text/html,hi"), None);
        assert_eq!(normalize_address("two words"), None);
        assert_eq!(normalize_address(""), None);
    }
}
