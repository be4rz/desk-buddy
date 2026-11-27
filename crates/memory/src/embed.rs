//! Local embeddings (p3): an Ollama on this Mac's loopback, and nothing else (A17).
//!
//! The URL must be `http://` on `127.0.0.1`, `localhost` or `[::1]` ([`Embedder::new`] refuses anything else),
//! the client has no TLS and no proxy, follows no redirect, and its resolver only hands back loopback addresses,
//! so a request can't leave the machine whatever the URL or `/etc/hosts` say. Memory never calls a cloud API:
//! with no local model answering, recall is keyword only.
//!
//! - `GET {url}/api/tags` lists the models ([`Embedder::probe`]).
//! - `POST {url}/api/embed {model, input: [...], keep_alive}` → `{embeddings: [[f32; 768], ...]}` ([`Embedder::embed`]);
//!   `keep_alive` asks Ollama to keep the model loaded for [`KEEP_ALIVE`] (its default is 5 min, and a cold load
//!   takes longer than a message's 500 ms budget).
//!
//! This module never logs; the caller logs `[buddy] memory vectors on|off`.

use crate::schema::DIMS;
use serde_json::{json, Value};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

/// The default local model and where Ollama listens.
pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "nomic-embed-text";
/// How long Ollama keeps the model loaded after a call.
pub const KEEP_ALIVE: &str = "30m";

/// An embed URL that is not `http://` on a loopback host (the app logs `memory embed url ignored (not local)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotLocal;

/// A loopback Ollama client for one model.
pub struct Embedder {
    base: String,
    model: String,
    agent: ureq::Agent,
}

/// `http://<loopback host>[:port][/]` → the base URL without a trailing slash; anything else is `NotLocal`.
pub fn local_base(url: &str) -> Result<String, NotLocal> {
    let rest = url.trim().strip_prefix("http://").ok_or(NotLocal)?;
    let netloc = rest.strip_suffix('/').unwrap_or(rest);
    if netloc.is_empty() || netloc.contains(['/', '@', '?', '#', ' ']) {
        return Err(NotLocal);
    }
    let (host, port) = if let Some(v6) = netloc.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or(NotLocal)?;
        (format!("[{h}]"), after.strip_prefix(':').map(str::to_string).or_else(|| after.is_empty().then(String::new)).ok_or(NotLocal)?)
    } else {
        match netloc.split_once(':') {
            Some((h, p)) => (h.to_string(), p.to_string()),
            None => (netloc.to_string(), String::new()),
        }
    };
    if !["127.0.0.1", "localhost", "[::1]"].contains(&host.to_ascii_lowercase().as_str()) {
        return Err(NotLocal);
    }
    if !port.is_empty() && port.parse::<u16>().is_err() {
        return Err(NotLocal);
    }
    Ok(if port.is_empty() { format!("http://{}", host.to_ascii_lowercase()) } else { format!("http://{}:{port}", host.to_ascii_lowercase()) })
}

/// Resolves a host only to loopback addresses (an empty answer is an error), so nothing is ever sent elsewhere.
struct LoopbackOnly;

impl ureq::Resolver for LoopbackOnly {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let found: Vec<SocketAddr> = netloc.to_socket_addrs()?.filter(|a| a.ip().is_loopback()).collect();
        if found.is_empty() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "not a loopback address"));
        }
        Ok(found)
    }
}

/// `v` scaled to length 1 (a zero vector stays zero).
pub fn normalise(v: &mut [f32]) {
    let n = v.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x = (f64::from(*x) / n) as f32;
        }
    }
}

impl Embedder {
    /// A client for `model` at `url`, which must be local ([`local_base`]).
    pub fn new(url: &str, model: &str) -> Result<Embedder, NotLocal> {
        let base = local_base(url)?;
        let model = model.trim();
        if model.is_empty() {
            return Err(NotLocal);
        }
        let agent = ureq::AgentBuilder::new().redirects(0).resolver(LoopbackOnly).build();
        Ok(Embedder { base, model: model.to_string(), agent })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// `nomic-embed-text` wants a task prefix on each input; other models get the text as it is.
    fn prefixed(&self, task: &str, text: &str) -> String {
        if self.model.starts_with("nomic-embed") {
            format!("{task}: {text}")
        } else {
            text.to_string()
        }
    }

    /// Whether Ollama answers and lists the model (`name` or `name:latest`). Lists only; [`Embedder::warm`] loads it.
    pub fn probe(&self, timeout: Duration) -> bool {
        let Ok(r) = self.agent.get(&format!("{}/api/tags", self.base)).timeout(timeout).call() else { return false };
        let Ok(v) = r.into_json::<Value>() else { return false };
        let want = |name: &str| name == self.model || (!self.model.contains(':') && name == format!("{}:latest", self.model));
        v["models"].as_array().into_iter().flatten().any(|m| m["name"].as_str().is_some_and(want) || m["model"].as_str().is_some_and(want))
    }

    /// Embeddings of `texts` (normalised, [`DIMS`] long), in order. `Err` is a short reason, never the text:
    /// `slow` (the timeout ran out: Ollama is there but busy or loading), `io` / `transport` (not reachable),
    /// `status`, `bad_reply`, `dims`.
    pub fn embed(&self, texts: &[String], timeout: Duration) -> Result<Vec<Vec<f32>>, &'static str> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let r = self
            .agent
            .post(&format!("{}/api/embed", self.base))
            .timeout(timeout)
            .send_json(json!({"model": self.model, "input": texts, "keep_alive": KEEP_ALIVE}))
            .map_err(|e| match e {
                ureq::Error::Status(..) => "status",
                ureq::Error::Transport(t) if t.kind() == ureq::ErrorKind::Io => {
                    let timed_out = std::error::Error::source(&t)
                        .and_then(|s| s.downcast_ref::<io::Error>())
                        .is_some_and(|e| matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock));
                    if timed_out {
                        "slow"
                    } else {
                        "io"
                    }
                }
                ureq::Error::Transport(_) => "transport",
            })?;
        let v: Value = r.into_json().map_err(|_| "bad_reply")?;
        let rows = v["embeddings"].as_array().ok_or("bad_reply")?;
        if rows.len() != texts.len() {
            return Err("bad_reply");
        }
        rows.iter()
            .map(|row| {
                let mut out: Vec<f32> = row.as_array().ok_or("bad_reply")?.iter().map(|x| x.as_f64().map(|f| f as f32)).collect::<Option<_>>().ok_or("bad_reply")?;
                if out.len() != DIMS {
                    return Err("dims");
                }
                normalise(&mut out);
                Ok(out)
            })
            .collect()
    }

    /// Loads the model ahead of a message (one tiny embed); false when that fails or runs out of time.
    pub fn warm(&self, timeout: Duration) -> bool {
        self.embed(&[String::from("warm")], timeout).is_ok()
    }

    /// The message's embedding, for recall.
    pub fn query(&self, text: &str, timeout: Duration) -> Result<Vec<f32>, &'static str> {
        self.embed(&[self.prefixed("search_query", text)], timeout)?.pop().ok_or("bad_reply")
    }

    /// The rows' embeddings, for `facts_vec`.
    pub fn documents(&self, texts: &[String], timeout: Duration) -> Result<Vec<Vec<f32>>, &'static str> {
        let texts: Vec<String> = texts.iter().map(|t| self.prefixed("search_document", t)).collect();
        self.embed(&texts, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_on_a_loopback_host_is_local() {
        for ok in ["http://127.0.0.1:11434", "http://127.0.0.1:11434/", "http://localhost:11434", "http://LOCALHOST", "http://[::1]:11434", "http://[::1]"] {
            assert!(local_base(ok).is_ok(), "{ok}");
        }
        assert_eq!(local_base("http://127.0.0.1:11434/").unwrap(), "http://127.0.0.1:11434");
        for bad in [
            "https://127.0.0.1:11434",
            "http://10.0.0.2:11434",
            "http://ollama.example.com",
            "http://127.0.0.1.example.com:11434",
            "http://user@127.0.0.1:11434",
            "http://127.0.0.1:11434/api",
            "http://127.0.0.1:port",
            "http://[::2]:11434",
            "ftp://127.0.0.1",
            "127.0.0.1:11434",
            "http://",
            "",
        ] {
            assert_eq!(local_base(bad), Err(NotLocal), "{bad}");
        }
        assert!(Embedder::new("http://127.0.0.1:11434", " ").is_err(), "a model is needed");
    }

    #[test]
    fn normalise_makes_unit_length() {
        let mut v = vec![3.0, 4.0];
        normalise(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        let mut z = vec![0.0, 0.0];
        normalise(&mut z);
        assert_eq!(z, vec![0.0, 0.0]);
    }
}
