#![forbid(unsafe_code)]
//! Slate CEF runner binary (`#![forbid(unsafe_code)]`).
//!
//! All CEF FFI and `wrap_*!` macro expansions are strictly confined inside
//! [`slate_engine_adapter::cef_backend`](file:///home/earl/Desktop/Slate/crates/engine-adapter/src/cef_backend.rs).

fn main() {
    slate_engine_adapter::cef_backend::run_from_cli();
}
