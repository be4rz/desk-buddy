//! The host API the agent's commands talk to (plan §2 "Host API auth"): HTTP on
//! 127.0.0.1 at a random port, written with the person's token to a 0600 file.
//! Every request carries `Authorization: Bearer <token>`. A request with an
//! `Origin` header (a web page) or a `Host` other than the loopback address
//! (DNS rebinding) is refused. Each sidecar start gets its own agent token,
//! revoked when it exits. `X-Buddy-Turn` names the turn a command ran in; a
//! command from a turn that already ended still runs, flagged `afterTurn`.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

/// The largest request body the API reads.
const MAX_BODY: u64 = 64 * 1024;
/// The longest `say` text.
pub const MAX_SAY: usize = 2000;
/// The emotions `say` takes, and the emotes `mascot emote` plays.
pub const EMOTIONS: [&str; 6] = ["neutral", "happy", "sad", "surprised", "thinking", "sleepy"];
pub const EMOTES: [&str; 5] = ["wave", "nod", "shake", "jump", "spin"];

/// `n` random bytes as hex.
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("the OS random source failed");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Who a token belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// The person, from a terminal (`buddy` reading the port file).
    User,
    /// One sidecar start of an agent.
    Agent { agent_id: String, gen: u64 },
}

/// What a command asked the mascot to do.
#[derive(Clone, Debug, PartialEq)]
pub struct HostEvent {
    pub caller: Caller,
    /// `say` or `emote`.
    pub action: Action,
    /// The `X-Buddy-Turn` the command sent.
    pub turn: Option<String>,
    /// The turn it names isn't the agent's current one.
    pub after_turn: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Say { text: String, emotion: Option<String> },
    Emote { name: String },
}

type Sink = Box<dyn Fn(HostEvent) + Send + Sync>;

#[derive(Default)]
struct State {
    tokens: HashMap<String, Caller>,
    /// The agent's running turn (its trigger's request id), if any.
    turns: HashMap<String, String>,
}

struct Shared {
    state: Mutex<State>,
    sink: Sink,
    port: u16,
}

pub struct Host {
    shared: Arc<Shared>,
    server: Arc<Server>,
    thread: Option<JoinHandle<()>>,
    file: PathBuf,
    user_token: String,
}

/// What the port file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct PortFile {
    pub port: u16,
    pub token: String,
}

impl PortFile {
    pub fn read(path: &Path) -> io::Result<PortFile> {
        let v: Value = serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)?;
        let port = v["port"].as_u64().and_then(|p| u16::try_from(p).ok()).ok_or_else(|| io::Error::other("no port"))?;
        let token = v["token"].as_str().ok_or_else(|| io::Error::other("no token"))?.to_string();
        Ok(PortFile { port, token })
    }
}

/// Write `bytes` to `path` readable by its owner only.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        use std::io::Write;
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

impl Host {
    /// Listen on 127.0.0.1 at a free port and write `file` (0600) with the port and the person's token.
    pub fn start(file: &Path, sink: impl Fn(HostEvent) + Send + Sync + 'static) -> io::Result<Host> {
        let server = Server::http("127.0.0.1:0").map_err(io::Error::other)?;
        let port = server.server_addr().to_ip().map(|a| a.port()).ok_or_else(|| io::Error::other("no port"))?;
        let user_token = format!("bu_{}", random_hex(24));
        write_private(file, &serde_json::to_vec(&json!({ "port": port, "token": user_token, "pid": std::process::id() }))?)?;
        let mut state = State::default();
        state.tokens.insert(user_token.clone(), Caller::User);
        let shared = Arc::new(Shared { state: Mutex::new(state), sink: Box::new(sink), port });
        let server = Arc::new(server);
        let (s, sh) = (server.clone(), shared.clone());
        let thread = thread::Builder::new().name("buddy-host".into()).spawn(move || {
            for req in s.incoming_requests() {
                let sh = sh.clone();
                // Requests are tiny; one thread each keeps a slow client from blocking the rest.
                thread::spawn(move || serve(&sh, req));
            }
        })?;
        Ok(Host { shared, server, thread: Some(thread), file: file.to_path_buf(), user_token })
    }

    pub fn port(&self) -> u16 {
        self.shared.port
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.shared.port)
    }

    pub fn user_token(&self) -> &str {
        &self.user_token
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A token for one sidecar start; only ever placed in its commands' env.
    pub fn issue(&self, agent_id: &str, gen: u64) -> String {
        let token = format!("ba_{}", random_hex(24));
        self.state().tokens.insert(token.clone(), Caller::Agent { agent_id: agent_id.into(), gen });
        token
    }

    /// The port file, which holds the person's token.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Revoke every token of `agent_id`'s start `gen`.
    pub fn revoke_gen(&self, agent_id: &str, gen: u64) {
        self.state().tokens.retain(|_, c| !matches!(c, Caller::Agent { agent_id: a, gen: g } if a == agent_id && *g == gen));
    }

    /// The agent's running turn: its trigger's request id, `None` when idle.
    pub fn set_turn(&self, agent_id: &str, turn: Option<&str>) {
        let mut s = self.state();
        match turn {
            Some(t) => s.turns.insert(agent_id.into(), t.into()),
            None => s.turns.remove(agent_id),
        };
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = fs::remove_file(&self.file);
    }
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn reply(req: Request, status: u16, body: Value) {
    let ct = Header::from_bytes("Content-Type", "application/json").unwrap();
    let _ = req.respond(Response::from_string(body.to_string()).with_status_code(status).with_header(ct));
}

fn refuse(req: Request, status: u16, code: &str, message: &str) {
    reply(req, status, json!({ "ok": false, "error": { "code": code, "message": message } }));
}

fn serve(sh: &Shared, mut req: Request) {
    // A web page's fetch always carries an Origin; the CLI never does.
    if header(&req, "Origin").is_some() {
        return refuse(req, 403, "forbidden", "Browsers can’t use this API.");
    }
    // DNS rebinding: a page on evil.example resolving to 127.0.0.1 still sends its own Host.
    let host = header(&req, "Host").unwrap_or("");
    if host != format!("127.0.0.1:{}", sh.port) && host != format!("localhost:{}", sh.port) {
        return refuse(req, 403, "forbidden", "Wrong Host header.");
    }
    if !req.remote_addr().is_some_and(|a: &SocketAddr| a.ip().is_loopback()) {
        return refuse(req, 403, "forbidden", "Loopback only.");
    }
    let token = header(&req, "Authorization").and_then(|v| v.strip_prefix("Bearer ")).map(str::trim).unwrap_or("");
    let caller = sh.state.lock().unwrap_or_else(|e| e.into_inner()).tokens.get(token).cloned();
    let Some(caller) = caller.filter(|_| !token.is_empty()) else {
        return refuse(req, 401, "unauthorized", "Missing or revoked token.");
    };
    let turn = header(&req, "X-Buddy-Turn").map(str::to_string).filter(|t| !t.is_empty());
    let after_turn = match (&caller, &turn) {
        (Caller::Agent { agent_id, .. }, Some(t)) => sh.state.lock().unwrap_or_else(|e| e.into_inner()).turns.get(agent_id) != Some(t),
        _ => false,
    };
    let method = req.method().clone();
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let body = match (method == Method::Post).then(|| read_body(&mut req)) {
        Some(Err(e)) => return refuse(req, 400, "bad_request", &e),
        Some(Ok(v)) => v,
        None => Value::Null,
    };
    match (method, path.as_str()) {
        (Method::Get, "/v1/status") => {
            let who = match &caller {
                Caller::User => json!({ "kind": "user" }),
                Caller::Agent { agent_id, .. } => json!({ "kind": "agent", "agentId": agent_id }),
            };
            reply(req, 200, json!({ "ok": true, "caller": who, "turn": turn, "afterTurn": after_turn }))
        }
        (Method::Post, "/v1/say") => {
            let text = body["text"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return refuse(req, 400, "usage", "Say something: buddy say <text>.");
            }
            if text.chars().count() > MAX_SAY {
                return refuse(req, 400, "usage", &format!("Keep it under {MAX_SAY} characters."));
            }
            let emotion = body["emotion"].as_str().map(str::to_string);
            if let Some(e) = &emotion {
                if !EMOTIONS.contains(&e.as_str()) {
                    return refuse(req, 400, "usage", &format!("Emotion is one of {}.", EMOTIONS.join(", ")));
                }
            }
            (sh.sink)(HostEvent { caller, action: Action::Say { text, emotion }, turn, after_turn });
            reply(req, 200, json!({ "ok": true, "afterTurn": after_turn }))
        }
        (Method::Post, "/v1/mascot/emote") => {
            let name = body["name"].as_str().unwrap_or("").to_string();
            if !EMOTES.contains(&name.as_str()) {
                return refuse(req, 400, "usage", &format!("Emote is one of {}.", EMOTES.join(", ")));
            }
            (sh.sink)(HostEvent { caller, action: Action::Emote { name }, turn, after_turn });
            reply(req, 200, json!({ "ok": true, "afterTurn": after_turn }))
        }
        (_, "/v1/status" | "/v1/say" | "/v1/mascot/emote") => refuse(req, 405, "method", "Wrong method."),
        _ => refuse(req, 404, "not_found", "No such endpoint."),
    }
}

fn read_body(req: &mut Request) -> Result<Value, String> {
    if req.body_length().is_some_and(|n| n as u64 > MAX_BODY) {
        return Err("The body is too large.".into());
    }
    let mut buf = Vec::new();
    req.as_reader().take(MAX_BODY + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_BODY {
        return Err("The body is too large.".into());
    }
    if buf.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_slice::<Value>(&buf) {
        Ok(v @ Value::Object(_)) => Ok(v),
        _ => Err("The body must be a JSON object.".into()),
    }
}

pub mod client;

#[cfg(test)]
mod tests;
