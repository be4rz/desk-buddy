//! A minimal HTTP/1.1 client for the loopback API: the `buddy` CLI and tests.
//! Plain TCP, `Connection: close`, no TLS: the API only listens on 127.0.0.1.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;

/// One request's outcome: the status and the JSON body (`Null` when it isn't JSON).
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

/// Extra request headers, as `(name, value)`.
pub type Headers<'a> = &'a [(&'a str, &'a str)];

/// Send `method path` to `127.0.0.1:port` with `headers` and an optional JSON body.
pub fn request(port: u16, method: &str, path: &str, headers: Headers, body: Option<&Value>) -> io::Result<Reply> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    // a widget action waits for its pages: `music play` worst case is the search, the media page
    // (8 s ready + 15 s reply) and, on embed error 150, the browser page (8 s + 15 s load)
    let wait = if path.starts_with("/v1/widget") { 75 } else { 10 };
    s.set_read_timeout(Some(Duration::from_secs(wait)))?;
    s.set_write_timeout(Some(Duration::from_secs(10)))?;
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let mut head = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    // Callers may name their own Host (tests do); otherwise the loopback address.
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        head.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        if k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a header can’t hold a line break"));
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if body.is_some() {
        head.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", payload.len()));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(payload.as_bytes())?;
    s.flush()?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    parse(&raw)
}

fn parse(raw: &[u8]) -> io::Result<Reply> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "not an HTTP reply");
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(bad)?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| bad())?;
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(bad)?;
    let mut body = raw[split + 4..].to_vec();
    if head.lines().any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.to_ascii_lowercase().contains("chunked")) {
        body = dechunk(&body).ok_or_else(bad)?;
    }
    Ok(Reply { status, body: serde_json::from_slice(&body).unwrap_or(Value::Null) })
}

fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = b.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(std::str::from_utf8(&b[..nl]).ok()?.split(';').next()?.trim(), 16).ok()?;
        b = &b[nl + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(b.get(..size)?);
        b = b.get(size + 2..)?;
    }
}
