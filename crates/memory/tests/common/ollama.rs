//! A stand-in Ollama on loopback for the tests: `GET /api/tags` and `POST /api/embed` with [`super::concept_vec`].
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Stub {
    pub url: String,
    server: Arc<tiny_http::Server>,
    pub calls: Arc<AtomicUsize>,
    /// Every input the embed endpoint was sent, in order.
    pub inputs: Arc<Mutex<Vec<String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone, Default)]
pub struct Cfg {
    /// The models `/api/tags` lists (default `nomic-embed-text:latest`).
    pub models: Option<Vec<String>>,
    /// Sleep before each embed answer.
    pub delay: Duration,
    /// Answer embeddings of this length instead of `DIMS`.
    pub dims: Option<usize>,
}

impl Stub {
    pub fn start(cfg: Cfg) -> Stub {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let (calls, inputs) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
        let (srv, c, i) = (server.clone(), calls.clone(), inputs.clone());
        let thread = std::thread::spawn(move || {
            for mut rq in srv.incoming_requests() {
                c.fetch_add(1, Ordering::SeqCst);
                let mut body = String::new();
                let _ = rq.as_reader().read_to_string(&mut body);
                let reply = match (rq.method().as_str(), rq.url()) {
                    ("GET", "/api/tags") => {
                        let names = cfg.models.clone().unwrap_or_else(|| vec!["nomic-embed-text:latest".into()]);
                        json!({"models": names.iter().map(|n| json!({"name": n, "model": n})).collect::<Vec<_>>()})
                    }
                    ("POST", "/api/embed") => {
                        std::thread::sleep(cfg.delay);
                        let v: Value = serde_json::from_str(&body).unwrap_or_default();
                        let texts: Vec<String> = v["input"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
                        i.lock().unwrap().extend(texts.iter().cloned());
                        let rows: Vec<Vec<f32>> = texts
                            .iter()
                            .map(|t| {
                                let mut e = super::concept_vec(t);
                                e.resize(cfg.dims.unwrap_or(memory::DIMS), 0.0);
                                e
                            })
                            .collect();
                        json!({"model": v["model"], "embeddings": rows})
                    }
                    _ => {
                        let _ = rq.respond(tiny_http::Response::empty(404));
                        continue;
                    }
                };
                let h = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
                let _ = rq.respond(tiny_http::Response::from_string(reply.to_string()).with_header(h));
            }
        });
        Stub { url, server, calls, inputs, thread: Some(thread) }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A loopback URL where nothing listens (a port that was just free).
pub fn closed_url() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}")
}
