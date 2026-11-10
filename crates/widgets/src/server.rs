//! The widget server (design §4.9): static files only, 127.0.0.1 at a random port, GET/HEAD,
//! `Host` must be this exact loopback address (DNS rebinding), no directory listing. Each page
//! gets a CSP: the media widget may load the YouTube IFrame API and frames; everything else is
//! same-origin only. IPC goes over Tauri's `ipc:` scheme, which the CSP allows to connect.

use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use tiny_http::{Header, Method, Request, Response, Server as Http};

use crate::asset;

pub struct Server {
    http: Arc<Http>,
    port: u16,
    thread: Option<JoinHandle<()>>,
}

const IPC: &str = "ipc: http://ipc.localhost";

fn csp(path: &str) -> String {
    if path.starts_with("/media/") {
        format!(
            "default-src 'self'; script-src 'self' https://www.youtube.com https://s.ytimg.com; \
             frame-src https://www.youtube.com https://www.youtube-nocookie.com; img-src 'self' data: https://i.ytimg.com; \
             media-src 'self' blob: data:; style-src 'self' 'unsafe-inline'; connect-src 'self' {IPC}; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
        )
    } else {
        format!("default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' {IPC}; frame-ancestors 'none'; base-uri 'none'; form-action 'none'")
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn header(req: &Request, name: &str) -> Option<String> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("ascii header")
}

fn plain(req: Request, status: u16, text: &str) {
    let _ = req.respond(Response::from_string(text).with_status_code(status).with_header(h("Content-Type", "text/plain; charset=utf-8")).with_header(h("X-Content-Type-Options", "nosniff")));
}

fn serve(port: u16, req: Request) {
    if !req.remote_addr().is_some_and(|a| a.ip().is_loopback()) {
        return plain(req, 403, "loopback only");
    }
    if header(&req, "Host").as_deref() != Some(&format!("127.0.0.1:{port}")) {
        return plain(req, 403, "wrong Host");
    }
    let head = *req.method() == Method::Head;
    if *req.method() != Method::Get && !head {
        return plain(req, 405, "GET only");
    }
    let path = req.url().split(['?', '#']).next().unwrap_or("").to_string();
    let Some(a) = asset(&path) else { return plain(req, 404, "not found") };
    let body = if head { Vec::new() } else { a.bytes.to_vec() };
    let resp = Response::from_data(body)
        .with_header(h("Content-Type", mime(&path)))
        .with_header(h("X-Content-Type-Options", "nosniff"))
        .with_header(h("Cache-Control", "no-store"))
        .with_header(h("Cross-Origin-Resource-Policy", "same-origin"))
        .with_header(h("Content-Security-Policy", &csp(&path)));
    let _ = req.respond(resp);
}

impl Server {
    /// Listen on 127.0.0.1 at a free port.
    pub fn start() -> io::Result<Server> {
        let http = Arc::new(Http::http("127.0.0.1:0").map_err(io::Error::other)?);
        let port = http.server_addr().to_ip().map(|a| a.port()).ok_or_else(|| io::Error::other("no port"))?;
        let s = http.clone();
        let thread = thread::Builder::new().name("buddy-widgets".into()).spawn(move || {
            for req in s.incoming_requests() {
                serve(port, req);
            }
        })?;
        Ok(Server { http, port, thread: Some(thread) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:<port>`
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.http.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn get(port: u16, path: &str, host: &str, method: &str) -> (u16, String, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, head.to_ascii_lowercase(), body.to_string())
    }

    #[test]
    fn serves_widget_files_with_a_csp() {
        let srv = Server::start().unwrap();
        let host = format!("127.0.0.1:{}", srv.port());
        let (st, head, body) = get(srv.port(), "/media/index.html?mock=1", &host, "GET");
        assert_eq!(st, 200);
        assert!(body.contains("/sdk/buddy-widget.js"));
        assert!(head.contains("content-security-policy: default-src 'self'; script-src 'self' https://www.youtube.com"));
        assert!(head.contains("x-content-type-options: nosniff"));
        let (st, head, _) = get(srv.port(), "/browser/index.html", &host, "GET");
        assert_eq!(st, 200);
        assert!(!head.contains("youtube"), "only the media widget may load YouTube");
        assert_eq!(get(srv.port(), "/sdk/buddy-widget.js", &host, "GET").0, 200);
    }

    #[test]
    fn refuses_other_hosts_methods_and_paths() {
        let srv = Server::start().unwrap();
        let host = format!("127.0.0.1:{}", srv.port());
        assert_eq!(get(srv.port(), "/media/index.html", "evil.example", "GET").0, 403);
        assert_eq!(get(srv.port(), "/media/index.html", &format!("localhost:{}", srv.port()), "GET").0, 403);
        assert_eq!(get(srv.port(), "/media/index.html", &host, "POST").0, 405);
        for p in ["/", "/media/", "/media/../media/index.html", "/browser/inject.js", "/media/widget.json.bak", "/etc/passwd"] {
            assert_eq!(get(srv.port(), p, &host, "GET").0, 404, "{p}");
        }
    }
}
