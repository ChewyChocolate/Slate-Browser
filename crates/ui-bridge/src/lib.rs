#![forbid(unsafe_code)]
//! `slate-ui-bridge`: Typed IPC protocol and strict Content-Security-Policy enforcement
//! for Slate's internal HTML/CSS/JS shell pages (`slate://ui`, `slate://newtab`).

use serde::{Deserialize, Serialize};
use slate_core::{ScrollPosition, TabId, TabLifecycleState, TabState};
use thiserror::Error;

/// Strict Content-Security-Policy applied to all internal browser UI pages.
/// Blocks all remote scripts, remote stylesheets, remote frames, and `eval()`.
pub const INTERNAL_UI_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// Allowed internal origin prefixes permitted to send privileged UI IPC commands.
pub const ALLOWED_UI_ORIGINS: &[&str] = &["slate://ui"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IpcError {
    #[error("Untrusted IPC origin rejected: {0}")]
    UntrustedOrigin(String),
    #[error("Malformed IPC JSON payload: {0}")]
    InvalidPayload(String),
}

/// Commands sent from the HTML/CSS/JS shell UI to the Rust core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum UiCommand {
    OpenTab {
        url: String,
        activate: bool,
    },
    FocusTab {
        id: TabId,
    },
    SuspendTab {
        id: TabId,
        scroll: ScrollPosition,
    },
    NavigateActive {
        url: String,
    },
    CloseTab {
        id: TabId,
    },
    GoBack,
    GoForward,
    Reload,
    /// Sent once by the shell page after it has loaded; core replies with a full state push.
    ShellReady,
    ToggleCommandPalette,
    OpenDevTools {
        id: TabId,
    },
}

/// What the shell UI is allowed to know about a tab. `blocked` is `None` when the core has no
/// real count for the tab (the UI must show "-", never a guess).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabView {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub lifecycle: TabLifecycleState,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub blocked: Option<u64>,
}

/// Events broadcast from the Rust core to the HTML/CSS/JS shell UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CoreEvent {
    /// Full shell state (sent after every change, throttled for blocked-count updates).
    State {
        tabs: Vec<TabView>,
        active_tab: Option<TabId>,
    },
    TabsUpdated {
        tabs: Vec<TabState>,
        active_tab: Option<TabId>,
    },
    RequestBlocked {
        tab_id: Option<TabId>,
        url: String,
    },
}

/// Validates that an IPC message comes from a privileged internal UI origin and parses it.
pub fn parse_ui_command(origin: &str, raw_json: &str) -> Result<UiCommand, IpcError> {
    let is_trusted = ALLOWED_UI_ORIGINS
        .iter()
        .any(|allowed| origin == *allowed || origin.starts_with(&format!("{allowed}/")));
    if !is_trusted {
        return Err(IpcError::UntrustedOrigin(origin.to_string()));
    }
    serde_json::from_str(raw_json).map_err(|e| IpcError::InvalidPayload(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_untrusted_web_origins_and_accepts_internal_ui_origin() {
        let payload = r#"{"cmd":"open_tab","url":"https://example.com","activate":true}"#;

        // Web pages must never be able to invoke shell IPC:
        assert_eq!(
            parse_ui_command("https://evil.example.com", payload),
            Err(IpcError::UntrustedOrigin(
                "https://evil.example.com".to_string()
            ))
        );

        // Internal UI origin succeeds:
        let cmd = parse_ui_command("slate://ui/index.html", payload).unwrap();
        assert_eq!(
            cmd,
            UiCommand::OpenTab {
                url: "https://example.com".to_string(),
                activate: true
            }
        );
    }

    #[test]
    fn internal_csp_forbids_remote_code_and_unsafe_eval() {
        assert!(INTERNAL_UI_CSP.contains("default-src 'none'"));
        assert!(INTERNAL_UI_CSP.contains("script-src 'self'"));
        assert!(!INTERNAL_UI_CSP.contains("unsafe-eval"));
        assert!(!INTERNAL_UI_CSP.contains("unsafe-inline"));
        assert!(!INTERNAL_UI_CSP.contains("http:"));
        assert!(!INTERNAL_UI_CSP.contains("https:"));
    }
}
