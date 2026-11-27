//! Memory (D18, design memory-core): the app owns the store at `BUDDY_HOME/memory` (`crates/memory`).
//!
//! - **Open.** At launch when memory is on. A failed open never stops the app: the window's state
//!   carries `error {code: "newer"|"store", message}`, the `buddy memory …` routes answer 503, and the
//!   next call that needs the store tries again, at most once every 5 s ([`RETRY`]).
//! - **Off** (`settings.memory.enabled` false, or `BUDDY_MEMORY=0`): no pending turns are recorded and
//!   the waiting ones are cleared, `list|search|remember` answer 409 `memory_off`, the agent's section
//!   is empty and the jobs thread does nothing. `forget`, `open` and the window still work.
//! - **Feed.** `harness.rs` hands every status record to [`Mem::on_status`]: a chat turn (`u:`) that
//!   ends without a failure joins the pending queue; [`jobs`] extracts from it once the chat is idle.
//! - **After every change** (window, host, jobs): `MEMORY.md` is re-rendered, the agent gets the new
//!   section when it changed ([`Mem::sync_agent`]), and the window gets `memory://state`.
//!
//! Logs carry ids and counts only, never a fact's text, a quote or what the person said.

pub mod jobs;
pub mod window;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use host::{MemoryOp, Refusal};
use memory::{Error, Now, RememberIn, Store};
use serde_json::{json, Value};

/// The shortest wait between two tries to open a store that failed.
pub const RETRY: Duration = Duration::from_secs(5);

/// The store's folder under `BUDDY_HOME` (also the agent's private dir: its tools never read it).
pub fn dir(home: &Path) -> PathBuf {
    home.join("memory")
}

/// The local day and the RFC 3339 time with its offset.
pub fn now() -> Now {
    let z = jiff::Zoned::now();
    Now::new(z.date(), z.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string())
}

/// What the app needs of the agent (the bridge's `Agent`; tests use a fake).
pub trait Sidecar: Send + Sync {
    /// The prompt's "What I know about you" section (None empties it).
    fn set_memory(&self, section: Option<String>);
    /// One memory.extract batch; `Err` is the error code only.
    fn extract(&self, args: Value) -> Result<Value, String>;
    /// The sidecar is up (an extraction could run).
    fn running(&self) -> bool;
}

impl Sidecar for bridge::Agent {
    fn set_memory(&self, section: Option<String>) {
        if let Err(e) = bridge::Agent::set_memory(self, section) {
            eprintln!("[buddy] memory section not sent: {}", e.code);
        }
    }
    fn extract(&self, args: Value) -> Result<Value, String> {
        self.memory_extract(args).map_err(|e| e.code.to_string())
    }
    fn running(&self) -> bool {
        self.is_running()
    }
}

/// The pending feed's view of one sidecar run.
#[derive(Default, Debug)]
struct Feed {
    gen: u64,
    busy: bool,
    /// The request id the busy run answers (`status.turn` is only there while busy).
    turn: Option<String>,
}

impl Feed {
    /// One status record: the turn that just ended well, if any (busy true→false, no failure).
    fn step(&mut self, gen: u64, status: &Value) -> Option<String> {
        if gen != self.gen {
            *self = Feed { gen, ..Feed::default() };
        }
        let busy = status["busy"] == true;
        if busy {
            if let Some(t) = status["turn"].as_str() {
                self.turn = Some(t.to_string());
            }
            self.busy = true;
            return None;
        }
        let was = std::mem::replace(&mut self.busy, false);
        let turn = self.turn.take();
        let failed = status.get("failure").is_some_and(|f| !f.is_null());
        turn.filter(|t| was && !failed && t.starts_with("u:"))
    }
}

struct Inner {
    store: Option<Arc<Store>>,
    /// Counts opens, so a reply for a store that was erased since is thrown away.
    gen: u64,
    error: Option<(&'static str, String)>,
    tried: Option<Instant>,
    /// What the agent last got: (store gen, core_rev), or None for "no section".
    sent: Option<Option<(u64, u64)>>,
}

/// The app's memory: the store (or why it isn't open), the switch, and the agent seam.
pub struct Mem {
    dir: PathBuf,
    inner: Mutex<Inner>,
    enabled: AtomicBool,
    feed: Mutex<Feed>,
    last_chat: Mutex<Instant>,
    sidecar: Mutex<Option<Arc<dyn Sidecar>>>,
    /// Where `memory://state` goes after a change (the window; tests count calls).
    notify: Box<dyn Fn(&Mem) + Send + Sync>,
}

static MEM: OnceLock<Arc<Mem>> = OnceLock::new();

/// The app's memory, once `start` ran.
pub fn get() -> Option<Arc<Mem>> {
    MEM.get().cloned()
}

/// At launch, before the harness: open the store (when on) and keep it for the app.
pub fn start(home: &Path, enabled: bool) -> Arc<Mem> {
    let m = Arc::new(Mem::new(dir(home), enabled, Box::new(|m: &Mem| window::emit_from(m))));
    eprintln!("[buddy] memory {}", if enabled { "on" } else { "off" });
    let _ = MEM.set(m.clone());
    m
}

/// A store error as the window's fixed `Err` string.
pub fn werr(e: &Error) -> &'static str {
    match e {
        Error::Secret => "secret",
        Error::Empty => "empty",
        Error::TooLong => "too_long",
        Error::NotFound => "not_found",
        Error::Usage(_) | Error::Newer | Error::Store(_) => "store",
    }
}

/// A store error as the host's refusal.
fn refuse(e: Error) -> Refusal {
    match e {
        Error::Secret => Refusal::secret(),
        Error::NotFound => Refusal::new(404, "not_found", "Nothing like that to forget."),
        Error::Usage(m) => Refusal::new(400, "usage", m),
        Error::Empty | Error::TooLong => Refusal::new(400, "usage", "Keep it between 1 and 200 characters."),
        Error::Newer | Error::Store(_) => Refusal::no_memory(),
    }
}

impl Mem {
    pub fn new(dir: PathBuf, enabled: bool, notify: Box<dyn Fn(&Mem) + Send + Sync>) -> Mem {
        let m = Mem {
            dir,
            inner: Mutex::new(Inner { store: None, gen: 0, error: None, tried: None, sent: None }),
            enabled: AtomicBool::new(enabled),
            feed: Mutex::default(),
            last_chat: Mutex::new(Instant::now()),
            sidecar: Mutex::new(None),
            notify,
        };
        if enabled {
            let mut i = m.lock();
            m.open(&mut i);
        }
        m
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// One try to open the store; `error` says why it failed. A newer file stays closed (it can't be
    /// written; Forget everything recovers it).
    fn open(&self, i: &mut Inner) {
        i.tried = Some(Instant::now());
        match Store::open(&self.dir) {
            Ok(s) if s.is_newer() => {
                eprintln!("[buddy] memory unavailable: newer");
                i.error = Some(("newer", "My memory was saved by a newer version of desk-buddy.".into()));
            }
            Ok(s) => {
                i.gen += 1;
                i.error = None;
                if !self.enabled() {
                    let _ = s.clear_pending();
                }
                let _ = s.write_files();
                i.store = Some(Arc::new(s));
                eprintln!("[buddy] memory open");
            }
            Err(e) => {
                eprintln!("[buddy] memory unavailable: {}", werr(&e));
                i.error = Some(("store", e.to_string()));
            }
        }
    }

    /// The open store (and its gen), trying to open a failed one again once `RETRY` passed. None while
    /// it can't be opened, or before the first open of a store that started off.
    fn store_gen(&self) -> Option<(Arc<Store>, u64)> {
        let reopened;
        {
            let mut i = self.lock();
            if let Some(s) = &i.store {
                return Some((s.clone(), i.gen));
            }
            let due = i.tried.is_none_or(|t| t.elapsed() >= RETRY);
            if !due {
                return None;
            }
            self.open(&mut i);
            reopened = i.store.clone().map(|s| (s, i.gen));
        }
        if reopened.is_some() {
            self.sync_agent();
        }
        reopened
    }

    pub fn store(&self) -> Option<Arc<Store>> {
        self.store_gen().map(|(s, _)| s)
    }

    /// The error the window shows, if the store isn't open.
    fn error(&self) -> Option<(&'static str, String)> {
        let i = self.lock();
        if i.store.is_some() {
            None
        } else {
            i.error.clone()
        }
    }

    /// The agent to keep in sync (the harness attaches it once built).
    pub fn attach(&self, s: Arc<dyn Sidecar>) {
        *self.sidecar.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    }

    fn sidecar(&self) -> Option<Arc<dyn Sidecar>> {
        self.sidecar.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The section for the agent's spec at launch (None when off, in error or empty), remembered as sent.
    pub fn initial_section(&self) -> Option<String> {
        let mut i = self.lock();
        let (text, key) = self.wanted(&i);
        i.sent = Some(key);
        text
    }

    /// What the agent should carry now, and its key, read under the caller's lock so the key always
    /// names the text it goes with (the rev is read first: a write in between only re-sends later).
    fn wanted(&self, i: &Inner) -> (Option<String>, Option<(u64, u64)>) {
        if !self.enabled() {
            return (None, None);
        }
        match &i.store {
            Some(s) => {
                let key = Some((i.gen, s.core_rev()));
                let text = s.section();
                (Some(text).filter(|t| !t.is_empty()), key)
            }
            None => (None, None),
        }
    }

    /// Hand the agent the section when it moved since the last one it got (core_rev, a new store,
    /// the switch). The bridge sends it before the next turn. Compare, mark and hand over under one
    /// lock, so a slower caller can never overwrite a newer section with its older one.
    pub fn sync_agent(&self) {
        let Some(side) = self.sidecar() else { return };
        let mut i = self.lock();
        let (text, key) = self.wanted(&i);
        if i.sent == Some(key) {
            return;
        }
        i.sent = Some(key);
        side.set_memory(text);
    }

    /// After a change: re-render MEMORY.md, the agent's section, the window's state.
    pub fn changed(&self) {
        if let Some(s) = self.lock().store.clone() {
            if let Err(e) = s.write_files() {
                eprintln!("[buddy] memory file not written: {}", werr(&e));
            }
        }
        self.sync_agent();
        (self.notify)(self);
    }

    /// The window's `memory_state` (I6).
    pub fn state(&self) -> Value {
        let today = now().today;
        let store = self.store();
        let open = store.is_some();
        // the Arc goes with the match, before `error()` locks: Forget everything waits for every holder
        let mut v = match store {
            Some(s) => s.view(today),
            None => json!({
                "v": 1, "today": today.to_string(), "phrase": PHRASE,
                "groups": {"about": [], "help": [], "promises": []}, "disputes": [],
                "history": null, "historyMore": false, "counts": {"live": 0, "disputes": 0, "hidden": 0},
            }),
        };
        v["enabled"] = json!(self.enabled());
        v["error"] = match (open, self.error()) {
            (false, Some((code, message))) => json!({"code": code, "message": message, "file": tilde(&self.dir.join("memory.sqlite"))}),
            _ => Value::Null,
        };
        v
    }

    // ---- the pending feed ----

    /// A status record of sidecar run `gen` (any thread).
    pub fn on_status(&self, gen: u64, status: &Value) {
        let done = self.feed.lock().unwrap_or_else(|e| e.into_inner()).step(gen, status);
        let Some(turn) = done else { return };
        if !self.enabled() {
            return;
        }
        if let Some(s) = self.store() {
            if let Err(e) = s.add_pending(&turn, &now().at) {
                eprintln!("[buddy] memory pending not kept: {}", werr(&e));
            }
        }
    }

    /// A turn is running now (the jobs gate waits).
    pub fn busy(&self) -> bool {
        self.feed.lock().unwrap_or_else(|e| e.into_inner()).busy
    }

    /// The person sent a chat message.
    pub fn chat_sent(&self) {
        *self.last_chat.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    pub fn since_chat(&self) -> Duration {
        self.last_chat.lock().unwrap_or_else(|e| e.into_inner()).elapsed()
    }

    // ---- the host's `buddy memory …` ----

    pub fn on_op(&self, op: MemoryOp) -> host::Reply {
        if let MemoryOp::Open = op {
            window::open();
            return Ok(json!({}));
        }
        let gated = !matches!(op, MemoryOp::Forget { .. });
        if gated && !self.enabled() {
            return Err(Refusal::memory_off());
        }
        let s = self.store().ok_or_else(Refusal::no_memory)?;
        match op {
            MemoryOp::Remember { text, key, qualifier, value, kind, importance, from } => {
                let key = match (key, qualifier) {
                    (Some(k), Some(q)) => Some(format!("{k}:{q}")),
                    (k, _) => k,
                };
                let out = s.remember(RememberIn { text, key, value, kind, importance, from }, &now()).map_err(refuse)?;
                drop(s);
                match out.id() {
                    Some(id) => eprintln!("[buddy] memory {} {id}", out.name()),
                    None => eprintln!("[buddy] memory skipped"),
                }
                self.changed();
                Ok(match out.reason() {
                    Some(why) => json!({"id": null, "outcome": out.name(), "reason": why}),
                    None => json!({"id": out.id(), "outcome": out.name()}),
                })
            }
            MemoryOp::Forget { id, key, qualifier } => {
                let ids = s.forget(id, key.as_deref(), qualifier.as_deref()).map_err(refuse)?;
                drop(s);
                for id in &ids {
                    eprintln!("[buddy] memory forgotten {id}");
                }
                self.changed();
                Ok(json!({"forgotten": ids}))
            }
            MemoryOp::List { kind } => Ok(json!({"facts": s.list(kind.as_deref()).map_err(refuse)?})),
            MemoryOp::Search { q, limit } => Ok(json!({"facts": s.search(&q, limit).map_err(refuse)?})),
            MemoryOp::Open => unreachable!(),
        }
    }

    // ---- the window ----

    fn with_store<T>(&self, f: impl FnOnce(&Store, &Now) -> memory::Result<T>) -> Result<T, String> {
        let s = self.store().ok_or("store")?;
        let out = f(&s, &now()).map_err(|e| werr(&e).to_string())?;
        drop(s);
        self.changed();
        Ok(out)
    }

    pub fn add(&self, kind: &str, text: &str) -> Result<Value, String> {
        let id = self.with_store(|s, n| s.add(kind, text, n))?;
        eprintln!("[buddy] memory created {id}");
        Ok(json!({"id": id}))
    }

    pub fn edit(&self, id: i64, text: &str, value: Option<&str>) -> Result<Value, String> {
        let new = self.with_store(|s, n| s.edit(id, text, value, n))?;
        eprintln!("[buddy] memory superseded {new}");
        Ok(json!({"id": new}))
    }

    pub fn forget(&self, id: i64, hard: bool) -> Result<Value, String> {
        self.with_store(|s, _| if hard { s.erase(id) } else { s.soft_forget(id) })?;
        eprintln!("[buddy] memory {} {id}", if hard { "erased" } else { "forgotten" });
        Ok(json!({}))
    }

    pub fn resolve(&self, id: i64, keep: &str) -> Result<Value, String> {
        let keep_new = match keep {
            "new" => true,
            "old" => false,
            _ => return Err("store".into()),
        };
        self.with_store(|s, n| s.resolve(id, keep_new, n))?;
        eprintln!("[buddy] memory resolved {id} {keep}");
        Ok(json!({}))
    }

    /// Forget everything: the files go and a fresh store opens. Works whatever state the old one is
    /// in (it is never opened), so it also recovers a corrupt or newer file.
    pub fn forget_all(&self, confirm: &str) -> Result<Value, String> {
        if confirm.trim() != PHRASE {
            return Err("bad_phrase".into());
        }
        // Take the store, then wait outside the lock for a host call or the jobs thread that still holds
        // it (they need the lock to finish). `tried` is set so nothing reopens the old file meanwhile.
        let old = {
            let mut i = self.lock();
            i.tried = Some(Instant::now());
            i.store.take()
        };
        if let Some(old) = old {
            let t = Instant::now();
            while Arc::strong_count(&old) > 1 && t.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(old);
        }
        let out = {
            let mut i = self.lock();
            if let Some(again) = i.store.take() {
                drop(again); // opened by someone in between: it goes with the files too
            }
            i.tried = Some(Instant::now());
            match Store::forget_all(&self.dir) {
                Ok(s) => {
                    i.gen += 1;
                    i.error = None;
                    let _ = s.write_files();
                    i.store = Some(Arc::new(s));
                    Ok(json!({}))
                }
                Err(e) => {
                    i.error = Some(("store", e.to_string()));
                    Err(werr(&e).to_string())
                }
            }
        };
        eprintln!("[buddy] memory forget everything: {}", if out.is_ok() { "done" } else { "failed" });
        self.changed();
        out
    }

    /// The window's switch (the caller saves `settings.memory.enabled`).
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
        eprintln!("[buddy] memory {}", if on { "on" } else { "off" });
        if let Some(s) = self.store() {
            if !on {
                if let Err(e) = s.clear_pending() {
                    eprintln!("[buddy] memory pending not cleared: {}", werr(&e));
                }
            }
        }
        self.changed();
    }
}

/// The phrase Forget everything asks for.
pub const PHRASE: &str = "forget everything";

/// `path` with the home folder as `~`.
fn tilde(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(h) if path.starts_with(&h) => format!("~/{}", path.strip_prefix(&h).unwrap_or(path).display()),
        _ => path.display().to_string(),
    }
}

/// `harness.rs`: one sidecar record (any thread). Status records feed the pending queue.
pub fn on_record(gen: u64, rec: &Value) {
    if rec["kind"] == "status" {
        if let Some(m) = get() {
            m.on_status(gen, &rec["status"]);
        }
    }
}

#[cfg(test)]
mod tests;
