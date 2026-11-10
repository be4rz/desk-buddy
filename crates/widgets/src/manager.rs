//! The widget manager: which widgets are open, routing `buddy <widget> <action>` to the widget's
//! page and waiting for its reply, the media widget's queue and fallbacks, the browser widget's
//! drive / take-over / taint state. It never touches a window itself: the app's `Shell` does, so
//! this is tested headless with a fake one.
//!
//! Threads: `open`/`close`/`action` block (they wait for the page) and run on host request
//! threads, never the app's main thread. The page callbacks (`ready`, `reply`, `state`,
//! `visibility`, `control`, `closed`) only update state and wake waiters, so they're safe to call
//! from the main thread; `control("next")` hands its work to a thread of its own.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::manifest::Manifest;
use crate::media::{self, Video};

/// A refused widget command, as the host API's status, code and message.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl WidgetError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> WidgetError {
        WidgetError { status, code, message: message.into() }
    }
}

type R<T> = Result<T, WidgetError>;

/// An action's reply, and whether it put untrusted page text in front of the agent.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub value: Value,
    /// The agent read this URL's page: the caller taints the agent's conversation.
    pub taint: Option<String>,
}

/// What the app does for the manager (on its main thread; each call returns once done).
pub trait Shell: Send + Sync {
    /// Create widget `m`'s window, loading `url`. The page calls `ready` once it's up.
    fn open(&self, m: &Manifest, url: &str) -> Result<(), String>;
    fn close(&self, widget: &str);
    /// Show, or hide (ordered out, or parked off-screen when `m.window.park_offscreen`).
    fn set_visible(&self, m: &Manifest, visible: bool);
    /// Deliver `msg` to the widget page's `window.__buddyWidget.dispatch`.
    fn send(&self, widget: &str, msg: &Value) -> Result<(), String>;
    /// Load `url` in the browser widget's page webview; returns once it loaded (or gave up).
    fn browser_goto(&self, url: &str) -> Result<(), String>;
    /// Evaluate `script` in the browser widget's page webview; its string result.
    fn browser_eval(&self, script: &str) -> Result<String, String>;
    /// `pause` / `next` / `status` for Music or Spotify, only when one is already running
    /// (`None`: neither runs).
    fn os_media(&self, op: &str) -> Option<Result<Value, String>>;
}

pub type Search = Box<dyn Fn(&str) -> Result<Vec<Video>, String> + Send + Sync>;

const MAX_STATE: usize = 8 * 1024;

struct Inst {
    id: String,
    widget: String,
    visible: bool,
    ready: bool,
    state: Value,
}

#[derive(Default)]
struct Browser {
    url: Option<String>,
    tainted: bool,
    taken_over: bool,
}

#[derive(Default)]
struct Inner {
    open: Vec<Inst>,
    next_id: u64,
    next_req: u64,
    pending: HashSet<String>,
    replies: HashMap<String, Value>,
    queue: Vec<Video>,
    pos: usize,
    browser: Browser,
}

pub struct Manager {
    shell: Arc<dyn Shell>,
    registry: Vec<Manifest>,
    origin: String,
    fake_media: bool,
    refused_ports: Mutex<Vec<u16>>,
    search: Search,
    inner: Mutex<Inner>,
    cv: Condvar,
    open_lock: Mutex<()>,
    ready_timeout: Duration,
    reply_timeout: Duration,
}

/// Why `url` can't be opened in the browser widget: only http(s), never `refused` ports
/// (the widget server's and the host API's, on any host spelling).
pub fn check_url(url: &str, refused: &[u16]) -> Result<(), String> {
    let lower = url.trim().to_ascii_lowercase();
    let (scheme, rest) = lower.split_once("://").ok_or("the browser widget opens http(s) URLs only")?;
    let default = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return Err("the browser widget opens http(s) URLs only".into()),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or("");
    if hostport.is_empty() {
        return Err("that URL has no host".into());
    }
    let port = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(':') && !p.contains(']') => p.parse::<u16>().map_err(|_| "that URL's port isn't a number")?,
        _ => default,
    };
    if refused.contains(&port) {
        return Err("the browser widget can't open desk-buddy's own pages".into());
    }
    Ok(())
}

fn refuse_taken() -> WidgetError {
    WidgetError::new(409, "taken_over", "the person took over the browser widget (they hand it back with “Hand back”)")
}

fn err_of_page(code: &str, message: &str) -> WidgetError {
    let message = if message.is_empty() { "the widget refused".to_string() } else { message.to_string() };
    match code {
        "usage" => WidgetError::new(400, "usage", message),
        "nothing_playing" => WidgetError::new(409, "nothing_playing", message),
        "no_element" => WidgetError::new(404, "no_element", message),
        "media_offline" => WidgetError::new(502, "media_offline", message),
        _ => WidgetError::new(409, "widget_refused", message),
    }
}

impl Manager {
    /// `registry`: the installed widgets; `origin`: the widget server's `http://127.0.0.1:<port>`.
    pub fn new(shell: Arc<dyn Shell>, registry: Vec<Manifest>, origin: &str, fake_media: bool) -> Manager {
        let port = origin.rsplit(':').next().and_then(|p| p.parse().ok());
        Manager {
            shell,
            registry,
            origin: origin.trim_end_matches('/').to_string(),
            fake_media,
            refused_ports: Mutex::new(port.into_iter().collect()),
            search: Box::new(media::search),
            inner: Mutex::new(Inner::default()),
            cv: Condvar::new(),
            open_lock: Mutex::new(()),
            ready_timeout: Duration::from_secs(8),
            reply_timeout: Duration::from_secs(15),
        }
    }

    pub fn with_search(mut self, search: Search) -> Manager {
        self.search = search;
        self
    }

    pub fn with_timeouts(mut self, ready: Duration, reply: Duration) -> Manager {
        self.ready_timeout = ready;
        self.reply_timeout = reply;
        self
    }

    /// Another port the browser widget must never open (the host API's).
    pub fn refuse_port(&self, port: u16) {
        self.refused_ports.lock().unwrap_or_else(|e| e.into_inner()).push(port);
    }

    pub fn refused_ports(&self) -> Vec<u16> {
        self.refused_ports.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn registry(&self) -> &[Manifest] {
        &self.registry
    }

    /// The widget server's origin.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The manifest named by its id or its CLI word.
    pub fn manifest(&self, name: &str) -> Option<&Manifest> {
        self.registry.iter().find(|m| m.id == name || m.cli == name)
    }

    /// The page a widget's window loads.
    pub fn page_url(&self, m: &Manifest) -> String {
        let fake = if m.id == "media" && self.fake_media { "?fake=1" } else { "" };
        format!("{}/{}/{}{fake}", self.origin, m.id, m.entry)
    }

    /// Whether `url` is `widget`'s own page on the widget server (the IPC check).
    pub fn is_widget_page(&self, widget: &str, url: &str) -> bool {
        url.strip_prefix(&self.origin).and_then(|p| p.strip_prefix('/')).and_then(|p| p.strip_prefix(widget)).is_some_and(|p| p.starts_with('/'))
    }

    fn is_open(&self, widget: &str) -> bool {
        self.inner().open.iter().any(|i| i.widget == widget)
    }

    /// Open (or show) the window of `m`, waiting for its page; its instance id.
    fn ensure_open(&self, m: &Manifest) -> R<String> {
        let _one = self.open_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut g = self.inner();
        if !g.open.iter().any(|i| i.widget == m.id) {
            g.next_id += 1;
            let id = format!("w_{}", g.next_id);
            g.open.push(Inst { id, widget: m.id.clone(), visible: true, ready: false, state: json!({}) });
            drop(g);
            if let Err(e) = self.shell.open(m, &self.page_url(m)) {
                self.forget(&m.id);
                return Err(WidgetError::new(503, "widget_unavailable", format!("couldn't open the {} widget: {e}", m.name)));
            }
            g = self.inner();
        }
        let deadline = Instant::now() + self.ready_timeout;
        loop {
            match g.open.iter().find(|i| i.widget == m.id) {
                None => return Err(WidgetError::new(410, "widget_closed", format!("the {} widget closed", m.name))),
                Some(i) if i.ready => return Ok(i.id.clone()),
                Some(_) => {}
            }
            let now = Instant::now();
            if now >= deadline {
                drop(g);
                self.shell.close(&m.id);
                self.forget(&m.id);
                return Err(WidgetError::new(504, "widget_timeout", format!("the {} widget didn't load", m.name)));
            }
            g = self.cv.wait_timeout(g, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    fn forget(&self, widget: &str) {
        let mut g = self.inner();
        g.open.retain(|i| i.widget != widget);
        if widget == "browser" {
            g.browser = Browser::default();
        }
        if widget == "media" {
            g.queue.clear();
            g.pos = 0;
        }
        drop(g);
        self.cv.notify_all();
    }

    /// Send action `name` to `widget`'s page and wait for its reply.
    fn call(&self, widget: &str, name: &str, args: Value) -> R<Value> {
        let req = {
            let mut g = self.inner();
            g.next_req += 1;
            let r = format!("r{}", g.next_req);
            g.pending.insert(r.clone());
            r
        };
        if let Err(e) = self.shell.send(widget, &json!({ "kind": "action", "req": req, "name": name, "args": args })) {
            self.inner().pending.remove(&req);
            return Err(WidgetError::new(503, "widget_unavailable", format!("couldn't reach the {widget} widget: {e}")));
        }
        let deadline = Instant::now() + self.reply_timeout;
        let mut g = self.inner();
        let reply = loop {
            if let Some(r) = g.replies.remove(&req) {
                break r;
            }
            let gone = !g.open.iter().any(|i| i.widget == widget);
            let now = Instant::now();
            if gone || now >= deadline {
                g.pending.remove(&req);
                return Err(if gone {
                    WidgetError::new(410, "widget_closed", format!("the {widget} widget closed"))
                } else {
                    WidgetError::new(504, "widget_timeout", format!("the {widget} widget didn't answer in {} s", self.reply_timeout.as_secs()))
                });
            }
            g = self.cv.wait_timeout(g, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        };
        drop(g);
        if reply["ok"] == true {
            Ok(reply.get("value").cloned().unwrap_or_else(|| json!({})))
        } else {
            Err(err_of_page(reply["code"].as_str().unwrap_or(""), reply["message"].as_str().unwrap_or("")))
        }
    }

    // ---- the host API ----

    /// `POST /v1/widget/open {kind, url?}` → `{id}`.
    pub fn open(&self, kind: &str, url: Option<&str>) -> R<Value> {
        let m = self.manifest(kind).ok_or_else(|| WidgetError::new(404, "no_widget", format!("no widget “{kind}” ({})", self.names())))?.clone();
        if url.is_some() && m.id != "browser" {
            return Err(WidgetError::new(400, "usage", format!("only the browser widget opens a URL (buddy {} …)", m.cli)));
        }
        if let Some(u) = url {
            check_url(u, &self.refused_ports()).map_err(|e| WidgetError::new(403, "forbidden_url", e))?;
            if self.inner().browser.taken_over {
                return Err(refuse_taken());
            }
        }
        let id = self.ensure_open(&m)?;
        self.set_visible(&m.id, true);
        if let Some(u) = url {
            self.goto(u)?;
        }
        Ok(json!({ "id": id }))
    }

    fn names(&self) -> String {
        self.registry.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
    }

    fn browser_state(b: &Browser) -> Value {
        json!({ "url": b.url, "tainted": b.tainted, "takenOver": b.taken_over })
    }

    /// `GET /v1/widget/list` → `{widgets: [{id, kind, visible, state}]}`.
    pub fn list(&self) -> Value {
        let g = self.inner();
        let widgets: Vec<Value> = g
            .open
            .iter()
            .map(|i| {
                let state = if i.widget == "browser" { Self::browser_state(&g.browser) } else { i.state.clone() };
                json!({ "id": i.id, "kind": i.widget, "visible": i.visible, "state": state })
            })
            .collect();
        json!({ "widgets": widgets })
    }

    /// `POST /v1/widget/close {id}` (an instance id or a widget's name).
    /// `agent`: the agent asked. It can't close the browser widget while the person has taken it
    /// over (a reopened widget would start handed back).
    pub fn close(&self, agent: bool, id: &str) -> R<Value> {
        let widget = {
            let g = self.inner();
            g.open.iter().find(|i| i.id == id || i.widget == id || self.manifest(id).is_some_and(|m| m.id == i.widget)).map(|i| i.widget.clone())
        };
        let Some(w) = widget else { return Err(WidgetError::new(404, "no_widget", format!("no open widget “{id}” (buddy widget list)"))) };
        if agent && w == "browser" && self.inner().browser.taken_over {
            return Err(refuse_taken());
        }
        self.shell.close(&w);
        self.forget(&w);
        Ok(json!({}))
    }

    /// `GET /v1/widget/commands` → every installed widget action, for `buddy --help`.
    pub fn commands(&self) -> Value {
        let mut out = vec![];
        for m in &self.registry {
            for a in &m.actions {
                out.push(json!({ "widget": m.id, "cli": m.cli, "name": a.name, "usage": a.usage(&m.cli), "about": a.about, "readOnly": a.read_only, "args": a.args }));
            }
        }
        json!({ "commands": out })
    }

    /// `POST /v1/widget/action {widget, action, args}`; `agent`: the agent asked (not the person).
    pub fn action(&self, agent: bool, widget: &str, action: &str, args: &Value) -> R<Done> {
        let m = self.manifest(widget).ok_or_else(|| WidgetError::new(404, "no_widget", format!("no widget “{widget}” ({})", self.names())))?.clone();
        if m.action(action).is_none() {
            let list = m.actions.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
            return Err(WidgetError::new(400, "usage", format!("buddy {} has no “{action}” ({list})", m.cli)));
        }
        let args = if args.is_object() { args.clone() } else { json!({}) };
        match m.id.as_str() {
            "media" => self.media(agent, action, &args),
            "browser" => self.browser(agent, action, &args),
            _ => {
                self.ensure_open(&m)?;
                Ok(Done { value: self.call(&m.id, action, args)?, taint: None })
            }
        }
    }

    // ---- media ----

    fn media_current(&self) -> bool {
        self.inner().open.iter().any(|i| i.widget == "media" && i.ready && i.state["videoId"].is_string())
    }

    fn media(&self, agent: bool, action: &str, args: &Value) -> R<Done> {
        match action {
            "play" => {
                let q = args["query"].as_str().unwrap_or("");
                let found = media::resolve(q, self.fake_media, &*self.search).map_err(|e| {
                    if q.trim().is_empty() {
                        WidgetError::new(400, "usage", e)
                    } else {
                        WidgetError::new(404, "not_found", e)
                    }
                })?;
                let first = found[0].clone();
                {
                    let mut g = self.inner();
                    g.queue = found;
                    g.pos = 0;
                }
                self.play_video(agent, &first)
            }
            "next" if self.media_current() => {
                let v = {
                    let mut g = self.inner();
                    if g.pos + 1 >= g.queue.len() {
                        return Err(WidgetError::new(409, "nothing_next", "nothing is queued after this one (buddy music play <query>)"));
                    }
                    g.pos += 1;
                    g.queue[g.pos].clone()
                };
                self.play_video(agent, &v)
            }
            "pause" | "status" if self.media_current() => Ok(Done { value: self.call("media", action, json!({}))?, taint: None }),
            _ => match self.shell.os_media(action) {
                Some(Ok(v)) => Ok(Done { value: v, taint: None }),
                Some(Err(e)) => Err(WidgetError::new(502, "os_media", e)),
                None if action == "status" => Ok(Done { value: json!({ "state": { "playing": false } }), taint: None }),
                None => Err(WidgetError::new(409, "nothing_playing", "nothing is playing (buddy music play <query>)")),
            },
        }
    }

    fn play_video(&self, agent: bool, v: &Video) -> R<Done> {
        let m = self.manifest("media").expect("media widget").clone();
        self.ensure_open(&m)?;
        let r = self.call("media", "play", json!({ "videoId": v.id, "title": v.title, "by": if agent { "agent" } else { "user" } }))?;
        if let Some(code) = r["embedError"].as_u64() {
            let url = media::watch_url(&v.id);
            if agent && self.inner().browser.taken_over {
                return Ok(Done {
                    value: json!({ "state": { "playing": false }, "url": url, "message": format!("This video can't play here (error {code}), and the person is using the browser widget: {url}") }),
                    taint: None,
                });
            }
            let bm = self.manifest("browser").expect("browser widget").clone();
            self.ensure_open(&bm)?;
            self.set_visible("browser", true);
            self.goto(&url)?;
            return Ok(Done {
                value: json!({ "state": { "playing": false }, "openedIn": "browser", "url": url, "message": format!("This video can't play here (error {code}); opened it in the browser widget.") }),
                taint: None,
            });
        }
        Ok(Done { value: r, taint: None })
    }

    // ---- browser ----

    fn push_browser(&self) {
        let s = Self::browser_state(&self.inner().browser);
        let _ = self.shell.send("browser", &json!({ "kind": "state", "data": s }));
    }

    /// The page reported where it is (a click may have navigated); `read` marks the strip.
    fn page_at(&self, url: &str, read: bool) {
        let changed = {
            let mut g = self.inner();
            let b = &mut g.browser;
            let moved = !url.is_empty() && b.url.as_deref() != Some(url);
            if moved {
                b.url = Some(url.to_string());
            }
            let marked = read && !b.tainted;
            b.tainted |= read;
            moved || marked
        };
        if changed {
            self.push_browser();
        }
    }

    fn goto(&self, url: &str) -> R<()> {
        check_url(url, &self.refused_ports()).map_err(|e| WidgetError::new(403, "forbidden_url", e))?;
        self.shell.browser_goto(url).map_err(|e| WidgetError::new(502, "page_error", e))?;
        {
            let mut g = self.inner();
            // the banner stays: the conversation stays marked after a page read
            g.browser.url = Some(url.to_string());
        }
        self.push_browser();
        Ok(())
    }

    fn eval(&self, op: Value) -> R<Value> {
        let raw = self.shell.browser_eval(&crate::inject_script(&op)).map_err(|e| WidgetError::new(502, "page_error", format!("the page didn't answer: {e}")))?;
        let v: Value = serde_json::from_str(&raw).map_err(|_| WidgetError::new(502, "page_error", "the page's answer wasn't readable"))?;
        if v["ok"] == true {
            Ok(v)
        } else {
            Err(err_of_page(v["code"].as_str().unwrap_or(""), v["message"].as_str().unwrap_or("")))
        }
    }

    fn browser(&self, agent: bool, action: &str, args: &Value) -> R<Done> {
        let taken = |s: &Self| agent && s.inner().browser.taken_over;
        if action == "goto" {
            let url = args["url"].as_str().unwrap_or("").trim().to_string();
            if url.is_empty() {
                return Err(WidgetError::new(400, "usage", "go where? buddy browser goto <url>"));
            }
            check_url(&url, &self.refused_ports()).map_err(|e| WidgetError::new(403, "forbidden_url", e))?;
            if taken(self) {
                return Err(refuse_taken());
            }
            let m = self.manifest("browser").expect("browser widget").clone();
            self.ensure_open(&m)?;
            self.goto(&url)?;
            return Ok(Done { value: json!({ "url": url }), taint: None });
        }
        if !self.is_open("browser") {
            return Err(WidgetError::new(404, "no_widget", "no browser widget is open (buddy widget open browser)"));
        }
        match action {
            "snapshot" => {
                let v = self.eval(json!({ "op": "snapshot" }))?;
                let url = v["url"].as_str().unwrap_or("").to_string();
                self.page_at(&url, agent);
                Ok(Done { value: json!({ "url": url, "title": v["title"], "text": v["text"], "refs": v["refs"] }), taint: agent.then_some(url) })
            }
            "click" | "type" => {
                if taken(self) {
                    return Err(refuse_taken());
                }
                let r = args["ref"].as_str().unwrap_or("").trim().to_string();
                if r.is_empty() {
                    return Err(WidgetError::new(400, "usage", format!("{action} which element? buddy browser {action} <ref> (refs come from buddy browser snapshot)")));
                }
                let mut op = json!({ "op": action, "ref": r });
                if action == "type" {
                    op["text"] = args["text"].clone();
                }
                let v = self.eval(op)?;
                self.page_at(v["url"].as_str().unwrap_or(""), false);
                Ok(Done { value: json!({ "url": v["url"] }), taint: None })
            }
            _ => Err(WidgetError::new(400, "usage", format!("buddy browser has no “{action}”"))),
        }
    }

    // ---- page callbacks (the app checked the caller is `widget-<widget>` on its own page) ----

    /// The page loaded and registered its handlers.
    pub fn ready(&self, widget: &str) {
        let mut g = self.inner();
        let found = g.open.iter_mut().find(|i| i.widget == widget).map(|i| i.ready = true).is_some();
        drop(g);
        if found {
            self.cv.notify_all();
            if widget == "browser" {
                self.push_browser();
            }
        }
    }

    /// The page's answer to action `req`; ignored unless someone waits for it.
    pub fn reply(&self, widget: &str, req: &str, reply: Value) {
        let mut g = self.inner();
        if !g.open.iter().any(|i| i.widget == widget) || !g.pending.remove(req) {
            return;
        }
        g.replies.insert(req.to_string(), reply);
        drop(g);
        self.cv.notify_all();
    }

    /// `w.setState(obj)`; at most 8 KB.
    pub fn state(&self, widget: &str, state: Value) {
        if !state.is_object() || state.to_string().len() > MAX_STATE {
            return;
        }
        if let Some(i) = self.inner().open.iter_mut().find(|i| i.widget == widget) {
            i.state = state;
        }
    }

    fn set_visible(&self, widget: &str, visible: bool) {
        let Some(m) = self.manifest(widget).cloned() else { return };
        if !visible && !m.window.hideable {
            return;
        }
        let changed = {
            let mut g = self.inner();
            match g.open.iter_mut().find(|i| i.widget == widget) {
                Some(i) if i.visible != visible => {
                    i.visible = visible;
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.shell.set_visible(&m, visible);
        }
    }

    /// `w.requestHide()` / `requestShow()` / `requestClose()`, and the tray's show / hide.
    pub fn visibility(&self, widget: &str, op: &str) {
        match op {
            "hide" => self.set_visible(widget, false),
            "show" => self.set_visible(widget, true),
            "close" => {
                let _ = self.close(false, widget);
            }
            _ => {}
        }
    }

    /// The page's own buttons: the browser's Take over / Hand back, the media widget's Next.
    pub fn control(self: &Arc<Self>, widget: &str, op: &str) -> Result<(), String> {
        match (widget, op) {
            ("browser", "take_over" | "hand_back") => {
                self.inner().browser.taken_over = op == "take_over";
                self.push_browser();
                Ok(())
            }
            ("media", "next") => {
                let me = self.clone();
                std::thread::spawn(move || {
                    if let Err(e) = me.action(false, "media", "next", &json!({})) {
                        eprintln!("[buddy] music next: {}", e.message);
                    }
                });
                Ok(())
            }
            _ => Err(format!("no control “{op}”")),
        }
    }

    /// The window went away (closed by the person or the system).
    pub fn closed(&self, widget: &str) {
        self.forget(widget);
    }

    /// The media widget's state while it has a video, for `Senses.now_playing`.
    pub fn now_playing(&self) -> Option<Value> {
        let g = self.inner();
        let i = g.open.iter().find(|i| i.widget == "media" && i.state["videoId"].is_string())?;
        Some(i.state.clone())
    }

}

#[cfg(test)]
mod tests;
