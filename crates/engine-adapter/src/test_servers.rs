//! Tiny loopback HTTP servers used only by live tests (`--test-*` modes of the CEF runner).
//! Safe Rust only; no browser/CEF types here so it can be unit-tested directly.

use serde::Serialize;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// One request observed by a test server (what the *server* really received).
#[derive(Debug, Clone, Serialize)]
pub struct ServerRequestLog {
    pub path: String,
    pub host: String,
    pub cookie_header: String,
}

/// Serves a top-level page on host `localhost` that embeds an iframe from host `127.0.0.1`
/// (same loopback server, different registrable host => third party). The iframe response sets a
/// cookie and then requests `/pixel` from the same third-party host, so the server log shows
/// whether the browser really stored and re-sent the third-party cookie.
/// The top page also sets a first-party cookie (control).
pub struct CookieTestServer {
    port: u16,
    stop: Arc<AtomicBool>,
    log: Arc<Mutex<Vec<ServerRequestLog>>>,
}

impl CookieTestServer {
    pub fn start() -> Option<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop_c = Arc::clone(&stop);
        let log_c = Arc::clone(&log);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if stop_c.load(Ordering::Relaxed) {
                    break;
                }
                let log_t = Arc::clone(&log_c);
                thread::spawn(move || handle(stream, port, log_t));
            }
        });
        Some(Self { port, stop, log })
    }

    /// Top-level page URL (first-party host `localhost`).
    pub fn top_url(&self) -> String {
        format!("http://localhost:{}/top", self.port)
    }

    pub fn requests(&self) -> Vec<ServerRequestLog> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Drop for CookieTestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn header_value(req: &str, name: &str) -> String {
    req.lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
        .unwrap_or_default()
}

fn handle(mut s: std::net::TcpStream, port: u16, log: Arc<Mutex<Vec<ServerRequestLog>>>) {
    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
    let mut buf = [0u8; 4096];
    let n = s.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]).to_string();
    let path = req
        .lines()
        .next()
        .unwrap_or("GET /")
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let host = header_value(&req, "host");
    let cookie_header = header_value(&req, "cookie");
    if let Ok(mut l) = log.lock() {
        l.push(ServerRequestLog {
            path: path.clone(),
            host,
            cookie_header,
        });
    }
    let (ctype, set_cookie, body) = if path.starts_with("/top") {
        (
            "text/html; charset=utf-8",
            "Set-Cookie: fp=1; Path=/\r\n".to_string(),
            format!(
                "<!DOCTYPE html><html><body><h1>top</h1><iframe src=\"http://127.0.0.1:{port}/frame\"></iframe></body></html>"
            ),
        )
    } else if path.starts_with("/frame") {
        (
            "text/html; charset=utf-8",
            "Set-Cookie: tp=1; Path=/; SameSite=None; Secure\r\n".to_string(),
            "<!DOCTYPE html><html><body>frame<script>setTimeout(function(){var i=new Image();i.src='/pixel?'+Date.now();},300);</script></body></html>".to_string(),
        )
    } else {
        ("image/gif", String::new(), "GIF89a".to_string())
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\n{set_cookie}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(resp.as_bytes());
    let _ = s.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_server_serves_top_page_with_third_party_iframe() {
        let srv = CookieTestServer::start().expect("bind");
        let mut c = std::net::TcpStream::connect(("127.0.0.1", srv.port)).unwrap();
        c.write_all(b"GET /top HTTP/1.1\r\nHost: localhost\r\nCookie: a=b\r\n\r\n")
            .unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(out.contains("Set-Cookie: fp=1"));
        assert!(out.contains("http://127.0.0.1:"));
        let log = srv.requests();
        assert_eq!(log[0].host, "localhost");
        assert_eq!(log[0].cookie_header, "a=b");
    }
}
