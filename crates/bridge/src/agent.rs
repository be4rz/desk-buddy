//! One agent's sidecar, for the app: starts it with its `init`, answers its
//! `cred.*` and `policy.*` requests, gives each start a host API token (revoked
//! on exit), tells the host API the running turn, maps its records to
//! `MascotEvent`s, and remembers how it ended for the next start's `lastExit`.
//! It also marks the agent when it reads the web (`policy.taint`) and keeps
//! the person's reminders in the sidecar (`remind.*`, design §4.6/§4.7), runs
//! pi-ai sign-ins (`auth.login` → `auth.*` requests → the app's `AuthUi`) and sends
//! sense-triggered turns (`s:` request ids).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::Duration;

use host::{Host, Refusal, RemindOp, Reply};
use mascot_protocol::MascotEvent;
use policy::{AgentCtx, Decide, Decision, Production};
use serde_json::{json, Value};

use crate::bridge::{Bridge, Handler, Spawn};
use crate::credentials::Credentials;
use crate::mascot::Mapper;
use crate::protocol::{new_request_id, new_sense_id, platform, HostApi, Init, ModelRef, ProviderDef, V};
use crate::{BridgeError, Res};

/// How long a start may take to send `ready`.
const READY_WAIT: Duration = Duration::from_secs(20);
const CMD_WAIT: Duration = Duration::from_secs(15);
/// A sign-in waits for the person (a browser, a pasted code).
const LOGIN_WAIT: Duration = Duration::from_secs(600);

/// The app's side of a sign-in (design §4.7, pi-ai's login callbacks 1:1). Called on
/// bridge threads; `prompt`/`select` block until the person answers, `Err` = cancelled.
/// `cancelled` turns true when the sidecar gives up on the question.
pub trait AuthUi: Send + Sync {
    /// Open `url` in the browser (it passed `safe_auth_url`) and show it.
    fn open_url(&self, url: &str, instructions: Option<&str>);
    fn device_code(&self, user_code: &str, verification_uri: &str);
    fn progress(&self, message: &str);
    fn prompt(&self, message: &str, placeholder: Option<&str>, cancelled: &AtomicBool) -> Result<String, String>;
    /// Returns the chosen option's id.
    fn select(&self, message: &str, options: &[(String, String)], cancelled: &AtomicBool) -> Result<String, String>;
}

/// A sign-in page the sidecar may open: `https:`, or `http:` on a loopback host only.
/// No userinfo, whitespace or control characters; anything else (`file:`,
/// `javascript:`, custom schemes) is refused.
pub fn safe_auth_url(url: &str) -> bool {
    if url.len() > 8192 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    let (https, rest) = match (lower.strip_prefix("https://"), lower.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (None, Some(r)) => (false, r),
        _ => return false,
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return false;
    }
    if https {
        return true;
    }
    // the host without its port; `[::1]:8080` keeps its brackets
    let host = if authority.starts_with('[') { authority.split(']').next().map(|h| format!("{h}]")).unwrap_or_default() } else { authority.split(':').next().unwrap_or("").to_string() };
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

pub struct AgentSpec {
    pub agent_id: String,
    /// Its working folder: cwd of the sidecar and its commands.
    pub work_dir: PathBuf,
    /// The app's records for it (`agent.sqlite`, `policy.json`, `last-exit`); never the agent's to write.
    pub state_dir: PathBuf,
    pub skills_dir: PathBuf,
    /// The `buddy-agent` binary.
    pub program: PathBuf,
    /// `HOME` of its commands.
    pub home: PathBuf,
    /// Folders put first on its commands' `PATH` (where `buddy` is).
    pub path_dirs: Vec<PathBuf>,
    pub model: Option<ModelRef>,
    pub providers: Vec<ProviderDef>,
    /// Merged over the defaults (`stepLimit`, `shutdownMs`, …).
    pub settings: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    Mascot(MascotEvent),
    /// An `ev`, `ready` or `fatal` record of start `gen`.
    Record { gen: u64, rec: Value },
    Exited { gen: u64, code: Option<i32>, by_app: bool },
}

struct Run {
    bridge: Arc<Bridge>,
    /// `cred.modify_begin` leases not ended yet.
    leases: Vec<String>,
    /// `last-exit` before this start; it stands again if the start never got to `ready`.
    before: Option<String>,
}

type Sink = Box<dyn Fn(AgentEvent) + Send + Sync>;

pub struct Agent {
    spec: AgentSpec,
    policy: Arc<Production>,
    host: Arc<Host>,
    creds: Arc<Credentials>,
    sink: Sink,
    run: Mutex<Option<Run>>,
    gen: AtomicU64,
    /// The last start whose exit was handled (its bridge may still read as running
    /// while the `Exited` event is delivered).
    exited: AtomicU64,
    /// The last start that sent `ready`.
    readied: AtomicU64,
    mapper: Mutex<Mapper>,
    ready: Mutex<Option<mpsc::Sender<Value>>>,
    auth: Mutex<Option<Arc<dyn AuthUi>>>,
    /// The model the next start uses; `set_model` changes it (after a sign-in).
    model: Mutex<Option<ModelRef>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Agent {
    pub fn new(spec: AgentSpec, policy: Arc<Production>, host: Arc<Host>, creds: Arc<Credentials>, sink: impl Fn(AgentEvent) + Send + Sync + 'static) -> Arc<Agent> {
        let spec_model = spec.model.clone();
        Arc::new(Agent { spec, policy, host, creds, sink: Box::new(sink), run: Mutex::new(None), gen: AtomicU64::new(0), exited: AtomicU64::new(0), readied: AtomicU64::new(0), mapper: Mutex::new(Mapper::new()), ready: Mutex::new(None), auth: Mutex::new(None), model: Mutex::new(spec_model) })
    }

    pub fn id(&self) -> &str {
        &self.spec.agent_id
    }

    /// Where the sidecar's sign-in questions go (the sign-in window).
    pub fn set_auth_ui(&self, ui: Arc<dyn AuthUi>) {
        *lock(&self.auth) = Some(ui);
    }

    pub fn creds(&self) -> &Arc<Credentials> {
        &self.creds
    }

    pub fn gen(&self) -> u64 {
        self.gen.load(Ordering::SeqCst)
    }

    pub fn ctx(&self, gen: u64) -> AgentCtx {
        AgentCtx { agent_id: self.spec.agent_id.clone(), work_dir: self.spec.work_dir.clone(), state_dir: self.spec.state_dir.clone(), skills_dir: Some(self.spec.skills_dir.clone()), gen, home: Some(self.spec.home.clone()), secrets: vec![self.host.file().to_path_buf()] }
    }

    fn exit_file(&self) -> PathBuf {
        self.spec.state_dir.join("last-exit")
    }

    /// How the previous start ended: `clean`, `quit` or `crash`. A start that
    /// never recorded its end (the app itself died) counts as a crash.
    pub fn last_exit(&self) -> &'static str {
        match fs::read_to_string(self.exit_file()).unwrap_or_default().trim() {
            "clean" | "" => "clean",
            "quit" => "quit",
            _ => "crash",
        }
    }

    fn shell_env(&self) -> BTreeMap<String, String> {
        let mut path: Vec<String> = self.spec.path_dirs.iter().map(|p| p.display().to_string()).collect();
        path.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(String::from));
        let mut env = BTreeMap::from([
            ("PATH".into(), path.join(":")),
            ("HOME".into(), self.spec.home.display().to_string()),
            ("LANG".into(), "en_US.UTF-8".into()),
            ("TERM".into(), "dumb".into()),
        ]);
        // The allowlist's `git status|diff|log|show` must not run a program a repo's
        // config names: no fsmonitor hook, and no bare repo found in the work folder
        // (command-scope config wins over the repo's).
        let git = [("core.fsmonitor", "false"), ("safe.bareRepository", "explicit")];
        for (i, (k, v)) in git.iter().enumerate() {
            env.insert(format!("GIT_CONFIG_KEY_{i}"), k.to_string());
            env.insert(format!("GIT_CONFIG_VALUE_{i}"), v.to_string());
        }
        env.insert("GIT_CONFIG_COUNT".into(), git.len().to_string());
        env
    }

    /// The current start's bridge while its process runs and its exit isn't handled yet.
    fn live(&self, run: &Option<Run>) -> Option<Arc<Bridge>> {
        run.as_ref().map(|r| r.bridge.clone()).filter(|b| b.is_running() && b.gen > self.exited.load(Ordering::SeqCst))
    }

    /// Start the next sidecar and wait for its `ready`.
    pub fn start(self: &Arc<Self>) -> Res<Value> {
        let mut run = lock(&self.run);
        if self.live(&run).is_some() {
            return Err(BridgeError::new("running", "The agent is already running."));
        }
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let mut settings = json!({
            "stepLimit": 50,
            "shutdownMs": 3000,
            "context": { "maxLines": 200, "maxBytes": 16384, "clearBatch": 8, "compactPct": 80 },
            "subagents": { "max": 3 },
        });
        if let (Some(s), Value::Object(extra)) = (settings.as_object_mut(), self.spec.settings.clone()) {
            s.extend(extra);
        }
        settings["lastExit"] = json!(self.last_exit());
        fs::create_dir_all(&self.spec.state_dir).map_err(|e| BridgeError::new("io", e.to_string()))?;
        fs::create_dir_all(&self.spec.work_dir).map_err(|e| BridgeError::new("io", e.to_string()))?;
        let token = self.host.issue(&self.spec.agent_id, gen);
        // Until its exit is recorded, a start that vanishes was a crash.
        let before = fs::read_to_string(self.exit_file()).ok();
        let _ = fs::write(self.exit_file(), "running");
        let init = Init {
            t: "init",
            v: V,
            agent_id: self.spec.agent_id.clone(),
            work_dir: self.spec.work_dir.display().to_string(),
            storage_path: self.spec.state_dir.join("agent.sqlite").display().to_string(),
            skills_dir: self.spec.skills_dir.display().to_string(),
            model: lock(&self.model).clone(),
            providers: self.spec.providers.clone(),
            shell_env: self.shell_env(),
            shell_path: "/bin/bash".into(),
            platform: platform(),
            host: Some(HostApi { url: self.host.url(), token }),
            settings,
        };
        // The sidecar's own environment: nothing of the app's, no key.
        let env = vec![
            ("HOME".to_string(), self.spec.home.display().to_string()),
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("TMPDIR".to_string(), std::env::temp_dir().display().to_string()),
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ];
        let (tx, rx) = mpsc::channel();
        *lock(&self.ready) = Some(tx);
        let handler: Arc<dyn Handler> = Arc::new(Hooks(Arc::downgrade(self)));
        let spawn = Spawn { program: self.spec.program.clone(), cwd: self.spec.work_dir.clone(), env, init: serde_json::to_value(&init).unwrap(), gen };
        let bridge = Bridge::spawn(spawn, handler).map_err(|e| {
            self.host.revoke_gen(&self.spec.agent_id, gen);
            self.restore_exit(&before);
            BridgeError::new("spawn", format!("Couldn’t start the agent: {e}"))
        })?;
        *run = Some(Run { bridge: bridge.clone(), leases: vec![], before });
        drop(run);
        match rx.recv_timeout(READY_WAIT) {
            Ok(r) if r["t"] == "ready" => Ok(r),
            Ok(r) => Err(BridgeError::new("fatal", r["message"].as_str().unwrap_or("The agent failed to start.").to_string())),
            Err(_) => {
                bridge.kill();
                Err(BridgeError::new("timeout", "The agent didn’t start in time."))
            }
        }
    }

    /// Nothing ran: the previous start's end still stands.
    fn restore_exit(&self, before: &Option<String>) {
        let _ = match before {
            Some(b) => fs::write(self.exit_file(), b),
            None => fs::remove_file(self.exit_file()),
        };
    }

    fn bridge(&self) -> Res<Arc<Bridge>> {
        self.live(&lock(&self.run)).ok_or_else(|| BridgeError::new("not_running", "The agent has stopped."))
    }

    pub fn cmd(&self, op: &str, args: Value) -> Res<Value> {
        self.bridge()?.cmd(op, args, CMD_WAIT)
    }

    /// The model of this and every later start; a running sidecar switches now.
    pub fn set_model(&self, model: Option<ModelRef>) -> Res<Value> {
        *lock(&self.model) = model.clone();
        if !self.is_running() {
            return Ok(json!({}));
        }
        self.cmd("set_model", json!({ "model": model }))
    }

    pub fn model(&self) -> Option<ModelRef> {
        lock(&self.model).clone()
    }

    /// Send the person's message; returns its request id.
    pub fn prompt(&self, text: &str) -> Res<String> {
        let id = new_request_id();
        self.cmd("prompt", json!({ "requestId": id, "text": text, "source": "chat" }))?;
        Ok(id)
    }

    /// A sense-triggered turn (the attention budget allowed it); returns its `s:` request id.
    pub fn prompt_sense(&self, summary: &str) -> Res<String> {
        let id = new_sense_id();
        self.cmd("prompt", json!({ "requestId": id, "text": summary, "source": "sense" }))?;
        Ok(id)
    }

    /// Run pi-ai's OAuth sign-in for `provider_id` (§4.7 `auth.login`); blocks until it
    /// ends (up to 10 min). The credential arrives through `cred.modify`.
    pub fn auth_login(&self, provider_id: &str) -> Res<Value> {
        self.bridge()?.cmd("auth.login", json!({ "providerId": provider_id }), LOGIN_WAIT)
    }

    /// End the sign-in in progress (its `auth_login` returns `cancelled`).
    pub fn auth_cancel(&self) -> Res<Value> {
        self.bridge()?.cmd("auth.cancel", json!({}), Duration::from_secs(5))
    }

    /// A `buddy remind` op for the host's sink: the reminder lives in the
    /// sidecar's store (a durable timer), so with no sidecar there is none.
    pub fn remind(&self, op: &RemindOp) -> Reply {
        let refusal = |e: BridgeError| match e.code {
            "not_running" => Refusal::no_agent(),
            "usage" => Refusal::new(400, "usage", e.message),
            "too_many" => Refusal::new(409, "too_many", e.message),
            "timeout" => Refusal::new(504, "timeout", e.message),
            code => Refusal::new(502, code, e.message),
        };
        if !self.is_running() {
            return Err(Refusal::no_agent());
        }
        match op {
            RemindOp::Add { at_ms, text } => {
                // `reminderId`: `id` is the command record's own.
                let id = format!("r_{}", host::random_hex(6));
                let r = self.cmd("remind.add", json!({ "reminderId": id, "atMs": at_ms, "text": text })).map_err(refusal)?;
                Ok(json!({ "id": r["id"], "atMs": r["atMs"] }))
            }
            RemindOp::List => {
                let r = self.cmd("remind.list", json!({})).map_err(refusal)?;
                Ok(json!({ "reminders": r["reminders"].as_array().cloned().unwrap_or_default() }))
            }
            RemindOp::Cancel { id } => {
                let r = self.cmd("remind.cancel", json!({ "reminderId": id })).map_err(refusal)?;
                if r["cancelled"] == true {
                    Ok(json!({}))
                } else {
                    Err(Refusal::new(404, "not_found", format!("There’s no reminder {id}.")))
                }
            }
        }
    }

    /// The UI's policy ops (`approval_answer`, `set_mode`, …) for this agent.
    pub fn policy_ui(&self, op: &Value) -> Res<Value> {
        self.policy.ui(&self.ctx(self.gen()), op).map_err(|e| BridgeError::new(e.code, e.message))
    }

    /// Ask it to stop; kill it after `grace`.
    pub fn shutdown(&self, grace: Duration) {
        if let Ok(b) = self.bridge() {
            b.shutdown(grace);
        }
    }

    /// SIGKILL its process group, as a crash would (tests and QA).
    #[cfg(unix)]
    pub fn crash(&self) {
        if let Ok(b) = self.bridge() {
            b.kill_group();
        }
    }

    pub fn is_running(&self) -> bool {
        self.bridge().is_ok()
    }

    /// The running sidecar's pid (its process group).
    pub fn pid(&self) -> Option<u32> {
        self.bridge().ok().map(|b| b.pid)
    }

    fn mascot(&self, e: Option<MascotEvent>) {
        if let Some(e) = e {
            (self.sink)(AgentEvent::Mascot(e));
        }
    }

    fn on_record(&self, gen: u64, rec: Value) {
        match rec["t"].as_str() {
            Some("ready" | "fatal") => {
                if rec["t"] == "ready" {
                    self.readied.fetch_max(gen, Ordering::SeqCst);
                }
                if let Some(tx) = lock(&self.ready).take() {
                    let _ = tx.send(rec.clone());
                }
            }
            Some("ev") => {
                // The status names the request a busy run answers (a resumed run too).
                if rec["kind"] == "status" {
                    let s = &rec["status"];
                    self.host.set_turn(&self.spec.agent_id, if s["busy"] == true { s["turn"].as_str() } else { None });
                }
                let e = lock(&self.mapper).on_record(&rec);
                self.mascot(e);
            }
            _ => {}
        }
        (self.sink)(AgentEvent::Record { gen, rec });
    }

    fn on_request(&self, gen: u64, op: &str, req: &Value, cancelled: &AtomicBool) -> Result<Value, (String, String)> {
        let at = self.ctx(gen);
        match op {
            "cred.modify_begin" => {
                let out = self.creds.request(op, req)?;
                if let Some(l) = out["lease"].as_str() {
                    // The sidecar gave up waiting: nobody will end this lease.
                    if cancelled.load(Ordering::SeqCst) {
                        let _ = self.creds.request("cred.modify_end", &json!({ "lease": l }));
                    } else if let Some(r) = lock(&self.run).as_mut().filter(|r| r.bridge.gen == gen) {
                        r.leases.push(l.into());
                    } else {
                        let _ = self.creds.request("cred.modify_end", &json!({ "lease": l }));
                    }
                }
                Ok(out)
            }
            "cred.modify_end" => {
                if let (Some(r), Some(l)) = (lock(&self.run).as_mut(), req["lease"].as_str()) {
                    r.leases.retain(|x| x != l);
                }
                self.creds.request(op, req)
            }
            o if o.starts_with("cred.") => self.creds.request(op, req),
            "policy.decide" => {
                let d: Decide = serde_json::from_value(req.clone()).map_err(|e| ("bad_request".to_string(), e.to_string()))?;
                let decision = self.policy.decide(&at, &d);
                if let Decision::Ask(a) = &decision {
                    // Cancelled while deciding: the gate won't wait on this card.
                    if cancelled.load(Ordering::SeqCst) {
                        self.policy.cancel(&at, &d.call_id);
                    } else {
                        lock(&self.mapper).titled(&d.call_id, a["title"].as_str().unwrap_or("Allow this?"));
                    }
                }
                Ok(decision.json())
            }
            "policy.wait" => Ok(self.policy.wait(&at, req["callId"].as_str().unwrap_or(""))),
            // A web tool read the web: mark the agent before the content reaches the model.
            "policy.taint" => {
                let source = req["source"].as_str().unwrap_or("");
                if !matches!(source, "web_fetch" | "web_search") {
                    return Err(("bad_request".into(), format!("Unknown taint source: {source}")));
                }
                Ok(json!({ "newly": self.policy.taint(&at, source, req["detail"].as_str().unwrap_or("")) }))
            }
            o if o.starts_with("auth.") => self.on_auth(o, req, cancelled),
            _ => Err(("unknown_op".into(), format!("Unknown request: {op}"))),
        }
    }

    /// A sign-in question from pi-ai's login flow, for the window.
    fn on_auth(&self, op: &str, req: &Value, cancelled: &AtomicBool) -> Result<Value, (String, String)> {
        let Some(ui) = lock(&self.auth).clone() else {
            return Err(("no_ui".into(), "Sign in from desk-buddy’s Sign in… window.".into()));
        };
        let s = |k: &str| req[k].as_str().unwrap_or("");
        let opt = |k: &str| req[k].as_str().filter(|v| !v.is_empty());
        match op {
            "auth.open_url" => {
                let url = s("url");
                if !safe_auth_url(url) {
                    return Err(("auth".into(), "That sign-in page isn’t an https address.".into()));
                }
                ui.open_url(url, opt("instructions"));
                Ok(json!({}))
            }
            "auth.device_code" => {
                ui.device_code(s("userCode"), s("verificationUri"));
                Ok(json!({}))
            }
            "auth.progress" => {
                ui.progress(s("message"));
                Ok(json!({}))
            }
            "auth.prompt" => ui.prompt(s("message"), opt("placeholder"), cancelled).map(|v| json!({ "value": v })).map_err(|e| ("cancelled".into(), e)),
            "auth.select" => {
                let options: Vec<(String, String)> = req["options"].as_array().into_iter().flatten().filter_map(|o| Some((o["id"].as_str()?.to_string(), o["label"].as_str().unwrap_or("").to_string()))).collect();
                ui.select(s("message"), &options, cancelled).map(|id| json!({ "id": id })).map_err(|e| ("cancelled".into(), e))
            }
            _ => Err(("unknown_op".into(), format!("Unknown request: {op}"))),
        }
    }

    fn on_cancelled(&self, gen: u64, op: &str, req: &Value, ended: bool) {
        let at = self.ctx(gen);
        match op {
            // Its card stays up for the next start.
            "policy.wait" if ended => self.policy.release(&at),
            // An abort, while waiting or still deciding: the gate drops the call, so its card goes.
            "policy.wait" | "policy.decide" if !ended => self.policy.cancel(&at, req["callId"].as_str().unwrap_or("")),
            _ => {}
        }
    }

    fn on_exited(&self, gen: u64, code: Option<i32>, by_app: bool) {
        let how = match (by_app, lock(&self.mapper).busy()) {
            (true, false) => "clean",
            (true, true) => "quit",
            (false, _) => "crash",
        };
        let (leases, before) = lock(&self.run).as_mut().map(|r| (std::mem::take(&mut r.leases), r.before.take())).unwrap_or_default();
        // A start that timed out or failed before `ready` ran nothing: no crash to report.
        if self.readied.load(Ordering::SeqCst) >= gen {
            let _ = fs::write(self.exit_file(), how);
        } else {
            self.restore_exit(&before);
        }
        self.host.revoke_gen(&self.spec.agent_id, gen);
        self.host.set_turn(&self.spec.agent_id, None);
        self.policy.release(&self.ctx(gen));
        self.creds.release_all(&leases);
        if let Some(tx) = lock(&self.ready).take() {
            let _ = tx.send(json!({ "t": "fatal", "code": "exited", "message": format!("The agent exited ({code:?}) before it was ready.") }));
        }
        let e = lock(&self.mapper).reset();
        self.mascot(e);
        // Handled: a sink may start the next gen on this event.
        self.exited.fetch_max(gen, Ordering::SeqCst);
        (self.sink)(AgentEvent::Exited { gen, code, by_app });
    }
}

/// The bridge's handler; weak, so the bridge never keeps its agent alive.
struct Hooks(Weak<Agent>);

impl Handler for Hooks {
    fn record(&self, gen: u64, rec: Value) {
        if let Some(a) = self.0.upgrade() {
            a.on_record(gen, rec);
        }
    }

    fn request(&self, gen: u64, op: &str, req: &Value, cancelled: &AtomicBool) -> Result<Value, (String, String)> {
        match self.0.upgrade() {
            Some(a) => a.on_request(gen, op, req, cancelled),
            None => Err(("closing".into(), "The app is closing.".into())),
        }
    }

    fn cancelled(&self, gen: u64, op: &str, req: &Value, ended: bool) {
        if let Some(a) = self.0.upgrade() {
            a.on_cancelled(gen, op, req, ended);
        }
    }

    fn exited(&self, gen: u64, code: Option<i32>, by_app: bool) {
        if let Some(a) = self.0.upgrade() {
            a.on_exited(gen, code, by_app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::safe_auth_url;

    #[test]
    fn sign_in_pages_are_https_or_loopback_http_only() {
        for ok in ["https://claude.ai/oauth/authorize?x=1", "HTTPS://github.com/login/device", "http://127.0.0.1:1455/auth", "http://localhost/cb", "http://[::1]:8080/x"] {
            assert!(safe_auth_url(ok), "{ok}");
        }
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "vscode://x",
            "x-apple.systempreferences:",
            "http://example.com/login",
            "http://127.0.0.1.evil.com/",
            "http://localhost@evil.com/",
            "https://user@claude.ai/",
            "https://",
            "https:// claude.ai",
            "https://claude.ai/\nx",
            "",
            "-a Calculator",
        ] {
            assert!(!safe_auth_url(bad), "{bad}");
        }
    }
}
