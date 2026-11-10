#![forbid(unsafe_code)]
//! `slate`: Production browser application binary (`#![forbid(unsafe_code)]`).
//!
//! Provides the Slate browser shell with vertical tabs, address bar, and
//! Brave-grade ad/tracker blocking powered by `adblock-rust` on CEF 156.
//!
//! All CEF FFI and unsafe operations are strictly confined inside
//! [`slate_engine_adapter::cef_backend`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/cef_backend.rs).

use std::env;
use std::path::PathBuf;

fn find_shell_ui_path() -> PathBuf {
    // 1. Check relative to current working directory:
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let in_cwd = cwd.join("crates/app/resources/ui/index.html");
    if in_cwd.exists() {
        return in_cwd;
    }

    // 2. Check relative to executable:
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let in_exe = parent.join("../../../crates/app/resources/ui/index.html");
            if in_exe.exists() {
                return in_exe;
            }
        }
    }

    // Fallback:
    PathBuf::from("/home/earl/Desktop/Slate/crates/app/resources/ui/index.html")
}

fn main() {
    // If not a subprocess and no --url was provided, launch into the Slate Shell UI:
    let is_subprocess = env::args().any(|a| a.starts_with("--type="));
    let has_url = env::args().any(|a| a == "--url" || a == "--compat-probe" || a == "--real-sites");

    if !is_subprocess && !has_url {
        let ui_path = find_shell_ui_path();
        let ui_url = format!(
            "file://{}",
            ui_path.canonicalize().unwrap_or(ui_path).display()
        );

        // Append --url <ui_url> to environment arguments for CEF runner:
        let mut custom_args: Vec<String> = env::args().collect();
        custom_args.push("--url".to_string());
        custom_args.push(ui_url);

        // Run CEF engine adapter:
        slate_engine_adapter::cef_backend::run_with_args(custom_args);
        return;
    }

    slate_engine_adapter::cef_backend::run_from_cli();
}
