//! The host API the agent's commands talk to (plan §2 "Host API auth"): HTTP on
//! 127.0.0.1 at a random port, written with the person's token to a 0600 file.
//! Every request carries `Authorization: Bearer <token>`. A request with an
//! `Origin` header (a web page) or a `Host` other than the loopback address
//! (DNS rebinding) is refused. Each sidecar start gets its own agent token,
//! revoked when it exits. `X-Buddy-Turn` names the turn a command ran in; a
//! command from a turn that already ended still runs, flagged `afterTurn` (`buddy mascot
//! do|stop|emote` too).
//! The sink answers each command (design §4.6): its JSON is merged into
//! `{ "ok": true, … }`, a `Refusal` becomes the error reply.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

/// The largest request body the API reads.
const MAX_BODY: u64 = 64 * 1024;
/// The longest `say` text.
pub const MAX_SAY: usize = 2000;
/// The emotions `say` takes. (Emotes and activities are the pack's: `buddy mascot list`.)
pub const EMOTIONS: [&str; 6] = ["neutral", "happy", "sad", "surprised", "thinking", "sleepy"];
/// `buddy mascot do --for` bounds, ms (10 s to 30 min; the pack's `max_for_s` may cap lower).
pub const MIN_FOR_MS: u64 = 10_000;
pub const MAX_FOR_MS: u64 = 1_800_000;
/// The usage error for a `--for` out of bounds.
pub const FOR_RANGE: &str = "--for must be between 10s and 30m";
/// The longest reminder text.
pub const MAX_REMIND_TEXT: usize = 500;
/// How far ahead a reminder may be: 7 days.
pub const MAX_REMIND_MS: u64 = 7 * 24 * 3600 * 1000;
/// How soon a reminder may be, unless `Host::set_min_remind_ms` says otherwise: 10 s.
pub const MIN_REMIND_MS: u64 = 10_000;
/// The refusal when no agent runs to hold a reminder.
pub const NO_AGENT: &str = "desk-buddy’s agent isn’t running";

/// What the sink answers a command with.
pub type Reply = Result<Value, Refusal>;

/// A command the app refused, as the HTTP status and error the caller sees.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl Refusal {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Refusal {
        Refusal { status, code, message: message.into() }
    }

    /// 503 `no_agent`: nothing runs to take the command.
    pub fn no_agent() -> Refusal {
        Refusal::new(503, "no_agent", NO_AGENT)
    }
}

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
    /// `say`, a mascot op, a reminder op or a widget op.
    pub action: Action,
    /// The `X-Buddy-Turn` the command sent.
    pub turn: Option<String>,
    /// The turn it names isn't the agent's current one.
    pub after_turn: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Say { text: String, emotion: Option<String> },
    /// `buddy mascot …` (body-tools): the app answers with the brain's `BodyReply`.
    Mascot(MascotOp),
    /// `buddy remind …`: the app's agent keeps reminders (durable, in its sidecar).
    Remind(RemindOp),
    /// `buddy widget …` and `buddy <widget> <action> …` (design §4.9).
    Widget(WidgetOp),
    /// `buddy screen look` (and the agent's `screen_look` tool): the app reads the screen text.
    Screen(ScreenOp),
}

/// `POST /v1/screen/look {image}`. Reply `{text, truncated, noText, width, height, lines}` and, with
/// `image: true`, `image: {mimeType, base64, width, height}` (a downscaled PNG). A refusal is an
/// ordinary error reply (403 `screen_look_off` / `permission` / `denied_app` / `sense_turn`, 503
/// `no_display` / `busy`, 502 `capture_failed`).
#[derive(Clone, Debug, PartialEq)]
pub enum ScreenOp {
    Look { image: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub enum MascotOp {
    Emote { name: String },
    Do { name: String, for_ms: Option<u64> },
    Stop,
    List,
}

/// An activity or emote name's shape: `[a-z0-9_-]{1,32}`.
pub fn mascot_name_ok(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetOp {
    /// Reply `{id}`.
    Open { kind: String, url: Option<String> },
    /// Reply `{widgets: [{id, kind, visible, state}]}`.
    List,
    /// Reply `{}`.
    Close { id: String },
    /// Reply: the widget's result.
    Action { widget: String, action: String, args: Value },
    /// Reply `{commands: [{widget, cli, name, usage, about, readOnly, args}]}`.
    Commands,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RemindOp {
    /// Reply `{id, atMs}`.
    Add { at_ms: u64, text: String },
    /// Reply `{reminders: [{id, atMs, text}]}`.
    List,
    /// Reply `{}`, or a 404 refusal when there's no such reminder.
    Cancel { id: String },
}

type Sink = Box<dyn Fn(HostEvent) -> Reply + Send + Sync>;

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
    min_remind_ms: AtomicU64,
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
    pub fn start(file: &Path, sink: impl Fn(HostEvent) -> Reply + Send + Sync + 'static) -> io::Result<Host> {
        let server = Server::http("127.0.0.1:0").map_err(io::Error::other)?;
        let port = server.server_addr().to_ip().map(|a| a.port()).ok_or_else(|| io::Error::other("no port"))?;
        let user_token = format!("bu_{}", random_hex(24));
        write_private(file, &serde_json::to_vec(&json!({ "port": port, "token": user_token, "pid": std::process::id() }))?)?;
        let mut state = State::default();
        state.tokens.insert(user_token.clone(), Caller::User);
        let shared = Arc::new(Shared { state: Mutex::new(state), sink: Box::new(sink), port, min_remind_ms: AtomicU64::new(MIN_REMIND_MS) });
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

    /// How soon a `--in` reminder may be (default 10 s; tests and QA go lower).
    pub fn set_min_remind_ms(&self, ms: u64) {
        self.shared.min_remind_ms.store(ms, Ordering::SeqCst);
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

/// Hand `action` to the sink and answer with its reply merged into `{ok: true, …}`.
fn act(sh: &Shared, req: Request, event: HostEvent, extra: Value) {
    match (sh.sink)(event) {
        Ok(v) => {
            let mut body = json!({ "ok": true });
            for src in [extra, v] {
                if let Value::Object(m) = src {
                    // The envelope's `ok` is the host's.
                    body.as_object_mut().unwrap().extend(m.into_iter().filter(|(k, _)| k != "ok"));
                }
            }
            reply(req, 200, body)
        }
        Err(r) => refuse(req, r.status, r.code, &r.message),
    }
}

/// The four mascot routes answer errors in their own shape (design `#json-replies`):
/// `{"ok":false,"error":"<text>"[,"have":[…]]}`. A reply carrying `error` is a 400.
/// Like `say`, a `do`, `stop` or `emote` from a turn that already ended still acts; its reply then
/// carries `"afterTurn": true` (left out when on time, so the frozen shapes stay as they are).
fn act_mascot(sh: &Shared, req: Request, event: HostEvent) {
    let late = event.after_turn && !matches!(event.action, Action::Mascot(MascotOp::List));
    match (sh.sink)(event) {
        Ok(Value::Object(mut m)) if m.get("error").is_some_and(|e| !e.is_null()) => {
            let mut body = json!({ "ok": false, "error": m.remove("error").unwrap() });
            if let Some(have) = m.remove("have") {
                body["have"] = have;
            }
            reply(req, 400, body)
        }
        Ok(v) => {
            let mut body = json!({ "ok": true });
            if let Value::Object(m) = v {
                body.as_object_mut().unwrap().extend(m.into_iter().filter(|(k, _)| k != "ok"));
            }
            if late {
                body["afterTurn"] = json!(true);
            }
            reply(req, 200, body)
        }
        Err(r) => {
            let error = if r.code == "busy" { "busy".to_string() } else { r.message };
            reply(req, r.status, json!({ "ok": false, "error": error }))
        }
    }
}

fn mascot_error(req: Request, error: &str) {
    reply(req, 400, json!({ "ok": false, "error": error }))
}

/// A mascot route's `name`, checked for shape.
fn mascot_name(body: &Value) -> Result<String, String> {
    match body["name"].as_str() {
        Some(n) if mascot_name_ok(n) => Ok(n.to_string()),
        Some(n) => Err(format!("bad name \"{n}\" (a-z, 0-9, _ and -, at most 32)")),
        None => Err("name is missing".into()),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A `--in` duration in ms: `90s`, `20m`, `1h30m`, `2d`, `1d12h` (units in d, h, m, s order, each once).
pub fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    let mut total: u64 = 0;
    let mut num = String::new();
    let mut last_rank = 0;
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let (rank, unit) = match c {
            'd' => (1, 86_400_000u64),
            'h' => (2, 3_600_000),
            'm' => (3, 60_000),
            's' => (4, 1_000),
            _ => return None,
        };
        if num.is_empty() || rank <= last_rank || num.len() > 9 {
            return None;
        }
        total = total.checked_add(num.parse::<u64>().ok()?.checked_mul(unit)?)?;
        num.clear();
        last_rank = rank;
    }
    (num.is_empty() && last_rank > 0).then_some(total)
}

/// `HH:MM` as (hour, minute).
pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 || !h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

#[cfg(unix)]
mod local {
    //! Local time through the C library (`localtime_r`/`mktime`), declared by
    //! hand so the crate needs no libc dependency.
    use std::os::raw::{c_char, c_int, c_long};

    #[repr(C)]
    pub struct Tm {
        pub sec: c_int,
        pub min: c_int,
        pub hour: c_int,
        pub mday: c_int,
        pub mon: c_int,
        pub year: c_int,
        pub wday: c_int,
        pub yday: c_int,
        pub isdst: c_int,
        pub gmtoff: c_long,
        pub zone: *const c_char,
    }

    /// `time_t` is a C `long` on the Unixes the app runs on (macOS, 64-bit Linux).
    type TimeT = c_long;

    extern "C" {
        fn localtime_r(t: *const TimeT, out: *mut Tm) -> *mut Tm;
        fn mktime(tm: *mut Tm) -> TimeT;
    }

    pub fn tm_of(secs: i64) -> Option<Tm> {
        let t = secs as TimeT;
        let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
        // SAFETY: both pointers are valid for the call; localtime_r is the thread-safe form.
        let r = unsafe { localtime_r(&t, &mut tm) };
        (!r.is_null()).then_some(tm)
    }

    pub fn secs_of(tm: &mut Tm) -> Option<i64> {
        // SAFETY: `tm` is a valid, initialized struct tm.
        let t = unsafe { mktime(tm) };
        (t != -1).then_some(t as i64)
    }
}

/// The next local `HH:MM` strictly after `now_ms`, in ms (today's, else tomorrow's).
#[cfg(unix)]
pub fn next_local(hour: u32, minute: u32, now_ms: u64) -> Option<u64> {
    let mut tm = local::tm_of((now_ms / 1000) as i64)?;
    for _ in 0..2 {
        tm.hour = hour as i32;
        tm.min = minute as i32;
        tm.sec = 0;
        tm.isdst = -1;
        let at = local::secs_of(&mut tm)? as u64 * 1000;
        if at > now_ms {
            return Some(at);
        }
        // mktime normalized the fields; one day on (mday 32 rolls over too).
        tm.mday += 1;
    }
    None
}

#[cfg(not(unix))]
pub fn next_local(_hour: u32, _minute: u32, _now_ms: u64) -> Option<u64> {
    None
}

/// `ms` as local `HH:MM` (`None` where local time isn't available).
pub fn local_hhmm(ms: u64) -> Option<String> {
    #[cfg(unix)]
    {
        let tm = local::tm_of((ms / 1000) as i64)?;
        Some(format!("{:02}:{:02}", tm.hour, tm.min))
    }
    #[cfg(not(unix))]
    {
        let _ = ms;
        None
    }
}

/// `POST /v1/remind`'s body as the time to fire and the text.
fn remind_add(sh: &Shared, body: &Value) -> Result<(u64, String), String> {
    let text = body["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return Err("Remind of what? buddy remind --in 20m <text>.".into());
    }
    if text.chars().count() > MAX_REMIND_TEXT {
        return Err(format!("Keep a reminder under {MAX_REMIND_TEXT} characters."));
    }
    let now = now_ms();
    let at_ms = match (body.get("inMs").filter(|v| !v.is_null()), body.get("at").filter(|v| !v.is_null())) {
        (Some(_), Some(_)) | (None, None) => return Err("Give one of inMs or at.".into()),
        (Some(v), None) => {
            let min = sh.min_remind_ms.load(Ordering::SeqCst);
            match v.as_u64() {
                Some(ms) if (min..=MAX_REMIND_MS).contains(&ms) => now + ms,
                _ => return Err(format!("A reminder is {} s to 7 days away.", min / 1000)),
            }
        }
        (None, Some(v)) => {
            let (h, m) = v.as_str().and_then(parse_hhmm).ok_or("at is a local time, HH:MM.")?;
            next_local(h, m, now).ok_or("--at isn’t supported here.")?
        }
    };
    Ok((at_ms, text))
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
            act(sh, req, HostEvent { caller, action: Action::Say { text, emotion }, turn, after_turn }, json!({ "afterTurn": after_turn }))
        }
        (Method::Post, "/v1/mascot/emote") => match mascot_name(&body) {
            Err(e) => mascot_error(req, &e),
            Ok(name) => act_mascot(sh, req, HostEvent { caller, action: Action::Mascot(MascotOp::Emote { name }), turn, after_turn }),
        },
        (Method::Post, "/v1/mascot/do") => {
            let name = match mascot_name(&body) {
                Ok(n) => n,
                Err(e) => return mascot_error(req, &e),
            };
            let for_ms = match body.get("forMs").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => match v.as_u64() {
                    Some(ms) if (MIN_FOR_MS..=MAX_FOR_MS).contains(&ms) => Some(ms),
                    _ => return mascot_error(req, FOR_RANGE),
                },
            };
            act_mascot(sh, req, HostEvent { caller, action: Action::Mascot(MascotOp::Do { name, for_ms }), turn, after_turn })
        }
        (Method::Post, "/v1/mascot/stop") => act_mascot(sh, req, HostEvent { caller, action: Action::Mascot(MascotOp::Stop), turn, after_turn }),
        (Method::Get, "/v1/mascot/list") => act_mascot(sh, req, HostEvent { caller, action: Action::Mascot(MascotOp::List), turn, after_turn }),
        (Method::Post, "/v1/remind") => match remind_add(sh, &body) {
            Err(e) => refuse(req, 400, "usage", &e),
            Ok((at_ms, text)) => act(sh, req, HostEvent { caller, action: Action::Remind(RemindOp::Add { at_ms, text }), turn, after_turn }, Value::Null),
        },
        (Method::Get, "/v1/reminders") => act(sh, req, HostEvent { caller, action: Action::Remind(RemindOp::List), turn, after_turn }, Value::Null),
        (Method::Post, "/v1/remind/cancel") => {
            let id = body["id"].as_str().unwrap_or("").trim().to_string();
            if id.is_empty() {
                return refuse(req, 400, "usage", "Which reminder? buddy remind cancel <id>.");
            }
            act(sh, req, HostEvent { caller, action: Action::Remind(RemindOp::Cancel { id }), turn, after_turn }, Value::Null)
        }
        (Method::Post, "/v1/widget/open") => {
            let kind = body["kind"].as_str().unwrap_or("").trim().to_string();
            if kind.is_empty() {
                return refuse(req, 400, "usage", "Open which widget? buddy widget open <kind> [url].");
            }
            let url = body["url"].as_str().map(str::trim).filter(|u| !u.is_empty()).map(str::to_string);
            act(sh, req, HostEvent { caller, action: Action::Widget(WidgetOp::Open { kind, url }), turn, after_turn }, Value::Null)
        }
        (Method::Get, "/v1/widget/list") => act(sh, req, HostEvent { caller, action: Action::Widget(WidgetOp::List), turn, after_turn }, Value::Null),
        (Method::Get, "/v1/widget/commands") => act(sh, req, HostEvent { caller, action: Action::Widget(WidgetOp::Commands), turn, after_turn }, Value::Null),
        (Method::Post, "/v1/widget/close") => {
            let id = body["id"].as_str().unwrap_or("").trim().to_string();
            if id.is_empty() {
                return refuse(req, 400, "usage", "Close which widget? buddy widget close <id>.");
            }
            act(sh, req, HostEvent { caller, action: Action::Widget(WidgetOp::Close { id }), turn, after_turn }, Value::Null)
        }
        (Method::Post, "/v1/widget/action") => {
            let widget = body["widget"].as_str().unwrap_or("").trim().to_string();
            let action = body["action"].as_str().unwrap_or("").trim().to_string();
            if widget.is_empty() || action.is_empty() {
                return refuse(req, 400, "usage", "Give a widget and an action.");
            }
            let args = match body.get("args") {
                None | Some(Value::Null) => json!({}),
                Some(v @ Value::Object(_)) => v.clone(),
                Some(_) => return refuse(req, 400, "usage", "args must be an object."),
            };
            act(sh, req, HostEvent { caller, action: Action::Widget(WidgetOp::Action { widget, action, args }), turn, after_turn }, Value::Null)
        }
        (Method::Post, "/v1/screen/look") => {
            let image = match body.get("image").filter(|v| !v.is_null()) {
                None => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return refuse(req, 400, "usage", "image is true or false."),
            };
            // A look goes by the host's own record of the agent's turn, not only the header: a command that
            // dropped `BUDDY_TURN` is judged by the turn that is running, and is late when none is.
            let (turn, after_turn) = match (&caller, turn) {
                (Caller::Agent { agent_id, .. }, None) => {
                    let current = sh.state.lock().unwrap_or_else(|e| e.into_inner()).turns.get(agent_id).cloned();
                    let late = current.is_none();
                    (current, late)
                }
                (_, t) => (t, after_turn),
            };
            act(sh, req, HostEvent { caller, action: Action::Screen(ScreenOp::Look { image }), turn, after_turn }, Value::Null)
        }
        (_, "/v1/status" | "/v1/say" | "/v1/screen/look" | "/v1/mascot/emote" | "/v1/mascot/do" | "/v1/mascot/stop" | "/v1/mascot/list" | "/v1/remind" | "/v1/reminders" | "/v1/remind/cancel" | "/v1/widget/open" | "/v1/widget/list" | "/v1/widget/commands" | "/v1/widget/close" | "/v1/widget/action") => refuse(req, 405, "method", "Wrong method."),
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
