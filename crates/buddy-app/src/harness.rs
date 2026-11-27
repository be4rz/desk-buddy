//! The agent harness inside the app (design `## talk`, `## accounts`, §2): the host API
//! (`BUDDY_HOME/host.json`), the policy gate, the credentials and one agent, "main", whose sidecar
//! runs with the §3.1 folders. Its mascot events and host commands go to the main thread through
//! the `Inbox`.
//!
//! Credentials live in the OS keychain (`KeychainStore`, service `desk-buddy`) and in Rust memory;
//! the keychain is read once at start, off the main thread (a permission dialog can hold it; the
//! tray says "Getting ready…" meanwhile), and a read error means no sidecar ("can't read keychain").
//! No key ever reaches the sidecar's or a command's environment (bridge passes a fixed allowlist).
//! The sidecar starts even with no credential (sign-in needs it), with model `null`.
//! Debug builds only: `BUDDY_FAKE_MODEL=<url>` (the scripted fake provider, a session-only key),
//! the app's own `ANTHROPIC_API_KEY` (session-only, never stored), `BUDDY_CRED_STORE=memory|fail`
//! (no real keychain; `fail` denies every read), `BUDDY_FAKE_OAUTH=1` (the sidecar's scripted
//! `fake-oauth` sign-in, its model at `BUDDY_FAKE_MODEL`), `BUDDY_PROMPT=<text>` (one prompt once
//! the agent is ready) and `BUDDY_ANSWER=allow|deny` (answers every approval card).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use bridge::bridge::sidecar_bin;
use bridge::credentials::Credentials;
use bridge::store::{CredentialStore, KeychainStore};
use bridge::protocol::{ModelRef, ProviderDef, ProviderModel};
use bridge::{Agent, AgentEvent, AgentSpec, BridgeError, Res};
use host::{Action, Caller, Host, HostEvent, MascotOp, Refusal, RemindOp, ScreenOp, WidgetOp};
use mascot_core::body::BodyReq;
use mascot_protocol::{Emotion, MascotEvent};
use policy::Production;
use serde_json::{json, Value};

use crate::driver::{post, Inbox};
use crate::settings;
use crate::signin::{self, Provider};

/// The one agent of p3.
pub const AGENT_ID: &str = "main";
/// Restarts after crashes allowed within `RESTART_WINDOW`.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(60);

/// What the tray's agent line shows.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentState {
    /// Running with no model: nothing is signed in.
    SignedOut,
    /// The keychain couldn't be read: no sidecar.
    Keychain(String),
    Starting,
    Running { model: String },
    Stopped,
    Error(String),
}

impl AgentState {
    pub fn line(&self) -> String {
        match self {
            AgentState::SignedOut => "Connect an AI account…".into(),
            AgentState::Keychain(why) => format!("⚠ Can't read the keychain · {why}"),
            AgentState::Starting => "Getting ready…".into(),
            AgentState::Running { model } => format!("Ready · {model}"),
            AgentState::Stopped => "Assistant stopped".into(),
            AgentState::Error(e) => format!("⚠ Assistant problem · {e}"),
        }
    }

    /// Nothing can be asked until an account is connected (the tray line opens the Accounts window).
    pub fn needs_account(&self) -> bool {
        matches!(self, AgentState::SignedOut | AgentState::Keychain(_))
    }
}

/// One entry of the tray's Model menu.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelChoice {
    pub provider: String,
    pub id: String,
    pub name: String,
}

/// Where credentials come from (the keychain, plus session-only dev keys) and the dev hooks.
pub struct Config {
    pub store: Box<dyn CredentialStore>,
    /// Session-only api keys `(provider, key)`: in memory, never stored.
    pub session: Vec<(String, String)>,
    /// Endpoints pi-ai lacks (the fake provider).
    pub providers: Vec<ProviderDef>,
    /// The fake provider's model wins over every sign-in.
    pub fake: bool,
    /// Debug builds' scripted `fake-oauth` sign-in: its model's base URL.
    pub fake_oauth: Option<String>,
    /// The prompt's "Your body" section (`mascot_core::body::prompt_text`); `None` adds none.
    pub body: Option<String>,
    /// The prompt's "Who you are" section (`mascot_core::persona::prompt_text`); `None` adds none.
    pub persona: Option<String>,
}

/// The fake provider at `url` (no `/v1`).
/// `get()` once it is `Some`, polling for at most `max`.
fn wait_for<T>(max: Duration, get: impl Fn() -> Option<T>) -> Option<T> {
    let t = Instant::now();
    loop {
        if let Some(v) = get() {
            return Some(v);
        }
        if t.elapsed() >= max {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn fake_def(url: &str) -> ProviderDef {
    let m = ProviderModel { id: "fake-1".into(), name: "Fake 1".into(), context_window: 128000, max_tokens: 4096, reasoning: false, input: vec!["text".into()], compat: None };
    ProviderDef { id: "fake".into(), name: "Fake".into(), api: "openai-completions".into(), base_url: format!("{}/v1", url.trim_end_matches('/')), models: vec![m] }
}

impl Config {
    /// From the environment (see the module doc).
    pub fn from_env() -> Config {
        // the scripted fake model is a dev credential: release builds never read it
        #[cfg(debug_assertions)]
        let fake_url = std::env::var("BUDDY_FAKE_MODEL").ok().filter(|u| !u.trim().is_empty());
        #[cfg(not(debug_assertions))]
        let fake_url: Option<String> = None;
        let mut c = Config { store: Box::new(KeychainStore::new()), session: vec![], providers: vec![], fake: false, fake_oauth: None, body: None, persona: None };
        #[cfg(debug_assertions)]
        {
            match std::env::var("BUDDY_CRED_STORE").ok().as_deref() {
                Some("memory") => c.store = Box::new(bridge::store::MemoryStore::new()),
                Some("fail") => c.store = Box::new(bridge::store::MemoryStore::failing()),
                _ => {}
            }
            // read once: the key lives in `Credentials` from here on, never in any child's env
            if let Some(k) = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.trim().is_empty()) {
                c.session.push(("anthropic".into(), k));
            }
            if std::env::var("BUDDY_FAKE_OAUTH").ok().as_deref() == Some("1") {
                c.fake_oauth = Some(format!("{}/v1", fake_url.as_deref().unwrap_or("http://127.0.0.1:9").trim_end_matches('/')));
            }
        }
        if let Some(url) = fake_url {
            c.providers.push(fake_def(&url));
            // under the fake sign-in, the fake model comes only from signing in
            if c.fake_oauth.is_none() {
                c.fake = true;
                c.session.push(("fake".into(), "sk-fake-buddy".into()));
            }
        }
        c
    }

    /// Tests: an in-memory store and no dev hooks.
    #[cfg(test)]
    pub fn memory(store: bridge::store::MemoryStore) -> Config {
        Config { store: Box::new(store), session: vec![], providers: vec![], fake: false, fake_oauth: None, body: None, persona: None }
    }
}

/// The model to run: the fake one; else `settings.model` when its provider has a credential;
/// else the first provider of the sign-in list that has one (with its default model); else none.
pub fn choose_model(wanted: &settings::Model, fake: bool, catalog: &[Provider], has: impl Fn(&str) -> bool) -> Option<ModelRef> {
    if fake {
        return Some(ModelRef { provider: "fake".into(), model: "fake-1".into() });
    }
    if !wanted.provider.is_empty() && has(&wanted.provider) {
        return Some(ModelRef { provider: wanted.provider.clone(), model: wanted.model.clone() });
    }
    catalog.iter().find(|p| has(&p.id)).map(|p| ModelRef { provider: p.id.clone(), model: p.model.clone() })
}

/// The agent's folders under `BUDDY_HOME` (§3.1).
pub fn agent_dirs(home: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let base = home.join("agents").join(AGENT_ID);
    (base.join("state"), base.join("work"), base.join("home"))
}

pub struct Harness {
    host: Mutex<Option<Arc<Host>>>,
    host_file: PathBuf,
    agent: Arc<Agent>,
    policy: Arc<Production>,
    creds: Arc<Credentials>,
    /// `settings.model` (the sign-in may set it)
    wanted: Mutex<settings::Model>,
    catalog: Vec<Provider>,
    fake: bool,
    /// A sidecar program was found.
    sidecar: bool,
    /// The launch read of the keychain finished (until then the signed-in set is partial).
    loaded: std::sync::atomic::AtomicBool,
    state: Mutex<AgentState>,
    crashes: Mutex<Vec<Instant>>,
    closing: std::sync::atomic::AtomicBool,
}

static HARNESS: OnceLock<Arc<Harness>> = OnceLock::new();

/// The app's harness, once `start` ran.
pub fn get() -> Option<Arc<Harness>> {
    HARNESS.get().cloned()
}

/// Debug builds: `BUDDY_ANSWER=allow|deny` answers every card.
fn auto_answer() -> Option<&'static str> {
    #[cfg(debug_assertions)]
    {
        match std::env::var("BUDDY_ANSWER").ok().as_deref() {
            Some("allow") => return Some("allow"),
            Some("deny") => return Some("deny"),
            _ => {}
        }
    }
    None
}

/// Debug builds: `BUDDY_PROMPT=<text>` is submitted once the agent is ready.
fn launch_prompt() -> Option<String> {
    #[cfg(debug_assertions)]
    {
        if let Some(p) = std::env::var("BUDDY_PROMPT").ok().filter(|p| !p.trim().is_empty()) {
            return Some(p);
        }
    }
    None
}

/// What the host's sink does with a command; `agent` holds reminders, `policy` the conversation's
/// web mark (a browser page the agent read sets it).
fn on_host(e: HostEvent, agent: Option<Arc<Agent>>, policy: Option<Arc<Production>>) -> host::Reply {
    let who = match &e.caller {
        Caller::User => "user",
        Caller::Agent { .. } => "agent",
    };
    match e.action {
        Action::Say { text, emotion } => {
            eprintln!("[buddy] host say {who}{}: {}", emotion.as_ref().map(|e| format!(" ({e})")).unwrap_or_default(), serde_json::to_string(&text).unwrap_or_default());
            let emo = emotion.as_deref().and_then(|s| serde_json::from_value::<Emotion>(json!(s)).ok());
            // the agent's own line during its turn joins the turn's bubble; anything else is its own bubble
            let merge = matches!(e.caller, Caller::Agent { .. }) && !e.after_turn;
            let t2 = text.clone();
            let emo2 = emotion.clone();
            post(Inbox::Call(Box::new(move |a| a.bubble_say(t2, emo2, merge))));
            post(Inbox::Event(MascotEvent::Say { text, emotion: emo }));
            Ok(json!({}))
        }
        Action::Mascot(op) => body_call(who, op, e.after_turn),
        Action::Screen(ScreenOp::Look { image }) => screen_call(&e, who, image, agent, policy, crate::screen::host_look),
        // the app's store (`memory/`); none started (a test sink) is 503
        Action::Memory(op) => crate::memory::get().map_or_else(|| Err(Refusal::no_memory()), |m| m.on_op(op)),
        Action::Remind(op) => {
            eprintln!("[buddy] host remind {who}: {op:?}");
            agent.map_or_else(|| Err(Refusal::no_agent()), |a| a.remind(&op))
        }
        Action::Widget(op) => {
            eprintln!("[buddy] host widget {who}: {}", widget_line(&op));
            // the host answers before widgets::start (it needs the host's port): wait for it at launch
            let m = wait_for(Duration::from_secs(5), crate::widgets::manager).ok_or_else(|| Refusal::new(503, "no_widgets", "Widgets aren't running."))?;
            let refuse = |e: widgets::WidgetError| Refusal::new(e.status, e.code, e.message);
            match op {
                WidgetOp::Open { kind, url } => m.open(&kind, url.as_deref()).map_err(refuse),
                WidgetOp::List => Ok(m.list()),
                WidgetOp::Close { id } => m.close(matches!(e.caller, Caller::Agent { .. }), &id).map_err(refuse),
                WidgetOp::Commands => Ok(m.commands()),
                WidgetOp::Action { widget, action, args } => {
                    let done = m.action(matches!(e.caller, Caller::Agent { .. }), &widget, &action, &args).map_err(refuse)?;
                    // the agent read a web page: mark the conversation before it sees the text
                    if let (Some(url), Caller::Agent { gen, .. }, Some(a), Some(p)) = (&done.taint, &e.caller, &agent, &policy) {
                        if p.taint(&a.ctx(*gen), "browser", url) {
                            eprintln!("[buddy] marked: the agent read {url}");
                        }
                    }
                    Ok(done.value)
                }
            }
        }
    }
}

/// `buddy screen look` (D18): refused in a check-in turn, else `look` (the host's capture path); an
/// agent's successful look marks the conversation like a browser read does, before the reply carries
/// the text (the mark's detail names the source, never the content). The person's own terminal
/// is not marked.
fn screen_call(e: &HostEvent, who: &str, image: bool, agent: Option<Arc<Agent>>, policy: Option<Arc<Production>>, look: impl FnOnce(bool) -> host::Reply) -> host::Reply {
    // the policy denies this already; a command that reaches the host from a check-in is refused here too
    if crate::screen::is_sense_turn(e.turn.as_deref()) {
        eprintln!("[buddy] screen look refused: sense_turn");
        return Err(Refusal::new(403, "sense_turn", policy::SCREEN_SENSE_DENIED));
    }
    // each look is approved inside a running turn: a loop left running after it (`while …; do buddy screen look`) is not
    if e.after_turn && matches!(e.caller, Caller::Agent { .. }) {
        eprintln!("[buddy] screen look refused: turn_ended");
        return Err(Refusal::new(403, "turn_ended", "Screen look only runs while the turn that asked for it is running."));
    }
    eprintln!("[buddy] host screen look {who}{}", if image { " +image" } else { "" });
    let done = look(image)?;
    if let (Caller::Agent { gen, .. }, Some(a), Some(p)) = (&e.caller, &agent, &policy) {
        if p.taint(&a.ctx(*gen), "screen", "screen look") {
            eprintln!("[buddy] marked: the agent looked at the screen");
        }
    }
    Ok(done)
}

/// How long a `buddy mascot …` waits for the main thread before it answers busy (design `#host-api`).
const BODY_WAIT: Duration = Duration::from_secs(2);

/// `buddy mascot …`: the brain answers on the main thread (`App::body`); no answer in `BODY_WAIT`
/// is HTTP 503 `busy`.
/// `late`: from a turn that already ended (it still acts, like `say`; the log and reply say so).
fn body_call(who: &str, op: MascotOp, late: bool) -> host::Reply {
    let who = if late { format!("{who} (after turn)") } else { who.to_string() };
    let req = match op {
        MascotOp::Emote { name } => BodyReq::Emote { name },
        MascotOp::Do { name, for_ms } => BodyReq::Do { name, for_ms },
        MascotOp::Stop => BodyReq::Stop,
        MascotOp::List => BodyReq::List,
    };
    let what = match &req {
        BodyReq::Do { name, .. } | BodyReq::Emote { name } => format!("{} {name}", req.op()),
        _ => req.op().to_string(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    // `true` once the caller was answered busy. The main thread checks it and acts under the same
    // lock, so a call either happens and its reply is read, or never happens.
    let gave_up = Arc::new(std::sync::Mutex::new(false));
    let g2 = gave_up.clone();
    post(Inbox::Call(Box::new(move |a| {
        let g = g2.lock().unwrap_or_else(|e| e.into_inner());
        // already answered busy: the caller was told it did not happen, so it must not happen now
        if *g {
            eprintln!("[buddy] host mascot: a late {} dropped", req.op());
            return;
        }
        let _ = tx.send(a.body(&req));
    })));
    let got = rx.recv_timeout(BODY_WAIT).or_else(|_| {
        let mut g = gave_up.lock().unwrap_or_else(|e| e.into_inner());
        // the main thread may have acted just as the wait ran out: then its reply is here
        rx.try_recv().inspect_err(|_| *g = true)
    });
    match got {
        Ok(v) => {
            let out = v["status"].as_str().or(v["error"].as_str().map(|_| "error")).unwrap_or("ok");
            eprintln!("[buddy] host mascot {who}: {what} -> {out}");
            Ok(v)
        }
        Err(_) => {
            eprintln!("[buddy] host mascot {who}: {what} -> busy");
            Err(Refusal::new(503, "busy", "The bear didn't answer in time."))
        }
    }
}

/// A widget command for the log: never the typed text or a page's address in full.
fn widget_line(op: &WidgetOp) -> String {
    match op {
        WidgetOp::Open { kind, url } => format!("open {kind}{}", if url.is_some() { " <url>" } else { "" }),
        WidgetOp::List => "list".into(),
        WidgetOp::Close { id } => format!("close {id}"),
        WidgetOp::Commands => "commands".into(),
        WidgetOp::Action { widget, action, .. } => format!("{widget} {action}"),
    }
}

impl Harness {
    /// Start the host API, the policy, the credentials and (unless the keychain can't be read)
    /// the agent, in the background. Its state changes, mascot events and policy views arrive in
    /// the inbox. An `Err` is the host API's (the caller shows it in the tray).
    pub fn start(home: &Path, wanted: &settings::Model) -> std::io::Result<Arc<Harness>> {
        let mut config = Config::from_env();
        // the pack's "Your body" and "Who you are" prompt sections with the person's layer (`settings.persona`),
        // run on the main thread, after driver::start. A later change swaps "Who you are" in place
        // (`persona::send_prompt`); the body names no gesture rate when a persona sets it, so it never goes stale.
        crate::driver::with_app(|a| {
            let user = &a.settings.persona.user;
            config.body = mascot_core::body::prompt_text_with(&a.pack, a.pack.persona.is_some());
            config.persona = mascot_core::persona::prompt_text(&a.pack, user);
        });
        let h = Harness::build(home, wanted, config, sidecar_bin())?;
        let _ = HARNESS.set(h.clone());
        h.clone().launch();
        Ok(h)
    }

    /// Everything but starting the sidecar (tests use this with a `MemoryStore` and no sidecar).
    pub fn build(home: &Path, wanted: &settings::Model, config: Config, program: Option<PathBuf>) -> std::io::Result<Arc<Harness>> {
        std::fs::create_dir_all(home)?;
        let slot: Arc<OnceLock<Weak<Agent>>> = Arc::default();
        let slot2 = slot.clone();
        let pslot: Arc<OnceLock<Weak<Production>>> = Arc::default();
        let pslot2 = pslot.clone();
        let host_file = home.join("host.json");
        let host = Arc::new(Host::start(&host_file, move |e| on_host(e, slot2.get().and_then(Weak::upgrade), pslot2.get().and_then(Weak::upgrade)))?);
        let policy = Arc::new(Production::new(|_, view| {
            post(Inbox::Call(Box::new(move |a| a.policy_changed(&view))));
        }));
        let _ = pslot.set(Arc::downgrade(&policy));
        let Config { store, session, providers, fake, fake_oauth, body, persona } = config;
        let creds = Arc::new(Credentials::with_store(store));
        for (p, k) in &session {
            creds.set_api_key(p, k);
        }
        let catalog = signin::catalog(fake_oauth.is_some());
        let (state_dir, work_dir, agent_home) = agent_dirs(home);
        let _ = std::fs::create_dir_all(&agent_home);
        // the person's `buddy`, built beside the app (`cargo build -p host --bin buddy`)
        let buddy_dir: Vec<PathBuf> = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).filter(|d| d.join("buddy").is_file()).into_iter().collect();
        if buddy_dir.is_empty() {
            eprintln!("[buddy] no `buddy` beside the app: the agent can't speak or set reminders (cargo build -p host --bin buddy)");
        }
        #[cfg(debug_assertions)]
        let agent_settings = match &fake_oauth {
            Some(url) => json!({ "fakeOAuth": { "baseUrl": url } }),
            None => json!({}),
        };
        // release builds never turn the scripted sign-in on (lead condition b)
        #[cfg(not(debug_assertions))]
        let agent_settings = json!({});
        let repo_skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agent/skills");
        let spec = AgentSpec {
            agent_id: AGENT_ID.into(),
            work_dir,
            state_dir,
            skills_dir: repo_skills,
            program: program.clone().unwrap_or_default(),
            home: agent_home,
            path_dirs: buddy_dir,
            // chosen once the stored credentials are read (`load`)
            model: None,
            providers,
            settings: agent_settings,
            body,
            persona,
            // the "What I know about you" section; its folder is the agent's private dir (never read by its tools)
            memory: crate::memory::get().and_then(|m| m.initial_section()),
            private_dirs: vec![crate::memory::dir(home)],
        };
        let answer = auto_answer();
        let me: Arc<OnceLock<Weak<Harness>>> = Arc::default();
        let me2 = me.clone();
        let agent = Agent::new(spec, policy.clone(), host.clone(), creds.clone(), move |e| match e {
            AgentEvent::Mascot(m) => {
                let auto = match (answer, &m) {
                    (Some(ans), MascotEvent::AwaitingPermission { call_id, .. }) => Some((ans, call_id.clone())),
                    _ => None,
                };
                post(Inbox::Event(m));
                // after the card is up, as if clicked
                if let Some((ans, id)) = auto {
                    eprintln!("[buddy] BUDDY_ANSWER={ans} for {id}");
                    post(Inbox::Call(Box::new(move |a| a.card_answer(id, ans.to_string()))));
                }
            }
            AgentEvent::Record { gen, rec } => {
                // status records: a chat turn that ended well joins memory's pending queue
                crate::memory::on_record(gen, &rec);
                let e = &rec["entry"];
                if rec["kind"] == "entry" && e["kind"] == "buddy.event" && e["data"]["type"] == "approval_asked" {
                    if let Some(id) = e["data"]["callId"].as_str().map(str::to_string) {
                        let data = e["data"].clone();
                        post(Inbox::Call(Box::new(move |a| a.bubble_approval(&id, data))));
                    }
                }
            }
            AgentEvent::Exited { by_app, code, .. } => {
                if let Some(h) = me2.get().and_then(Weak::upgrade) {
                    h.exited(by_app, code);
                }
            }
        });
        let _ = slot.set(Arc::downgrade(&agent));
        let h = Arc::new(Harness {
            host: Mutex::new(Some(host)),
            host_file,
            agent,
            policy,
            creds,
            wanted: Mutex::new(wanted.clone()),
            catalog,
            fake,
            sidecar: program.is_some(),
            loaded: Default::default(),
            state: Mutex::new(AgentState::Starting),
            crashes: Mutex::new(vec![]),
            closing: Default::default(),
        });
        let _ = me.set(Arc::downgrade(&h));
        if let Some(m) = crate::memory::get() {
            m.attach(h.agent.clone());
        }
        Ok(h)
    }

    fn set_state(&self, s: AgentState) {
        eprintln!("[buddy] {}", s.line().trim_start_matches("⚠ "));
        *self.state.lock().unwrap() = s.clone();
        post(Inbox::Call(Box::new(move |a| a.agent_changed(&s))));
    }

    pub fn state(&self) -> AgentState {
        self.state.lock().unwrap().clone()
    }

    /// On a background thread: read the stored credentials (a keychain dialog can hold this for as
    /// long as it stays open; the tray says "Getting ready…" meanwhile and the bear keeps moving),
    /// then start the sidecar (it waits for `ready`), then the launch prompt.
    fn launch(self: Arc<Self>) {
        self.set_state(AgentState::Starting);
        std::thread::Builder::new()
            .name("buddy-agent-start".into())
            .spawn(move || {
                let st = self.load();
                if st != AgentState::Starting {
                    match &st {
                        AgentState::Error(_) => eprintln!("[buddy] no sidecar: build it with `cd agent && bun run build` (or set BUDDY_AGENT_BIN in debug builds)"),
                        AgentState::Keychain(_) => eprintln!("[buddy] the agent doesn't start without the keychain; desk-buddy needs “Always Allow” for its keychain items"),
                        _ => {}
                    }
                    self.set_state(st);
                    self.launch_prompt(); // with no agent: the bubble shows the error
                    return;
                }
                if self.start_agent() {
                    self.launch_prompt();
                }
            })
            .expect("spawn agent start");
    }

    /// Read the stored credentials once and choose the model: the state to go on with (`Starting`
    /// when the sidecar should start). A read error is the tray's keychain line and no sidecar.
    fn load(&self) -> AgentState {
        match self.creds.load() {
            Ok(n) => {
                eprintln!("[buddy] credentials: {n} stored");
                let wanted = self.wanted.lock().unwrap().clone();
                let _ = self.agent.set_model(choose_model(&wanted, self.fake, &self.catalog, |p| self.creds.has(p)));
                self.loaded.store(true, std::sync::atomic::Ordering::SeqCst);
                if self.sidecar {
                    AgentState::Starting
                } else {
                    AgentState::Error("no sidecar".into())
                }
            }
            Err(e) => AgentState::Keychain(e.message),
        }
    }

    /// `BUDDY_PROMPT` (debug builds), as if typed into the bubble.
    fn launch_prompt(&self) {
        if let Some(p) = launch_prompt() {
            eprintln!("[buddy] BUDDY_PROMPT: {}", serde_json::to_string(&p).unwrap_or_default());
            post(Inbox::Call(Box::new(move |a| a.submit(p))));
        }
    }

    fn start_agent(&self) -> bool {
        match self.agent.start() {
            Ok(_) => {
                self.set_state(self.running_state());
                true
            }
            Err(e) => {
                eprintln!("[buddy] the agent didn't start: {e}");
                self.set_state(AgentState::Error(format!("{} ({})", e.code, e.message)));
                false
            }
        }
    }

    /// A sidecar exit: a crash restarts it (at most `MAX_RESTARTS` a minute).
    fn exited(self: &Arc<Self>, by_app: bool, code: Option<i32>) {
        if by_app || self.closing.load(std::sync::atomic::Ordering::SeqCst) {
            self.set_state(AgentState::Stopped);
            return;
        }
        eprintln!("[buddy] the agent exited unexpectedly ({code:?})");
        let mut c = self.crashes.lock().unwrap();
        c.retain(|t| t.elapsed() < RESTART_WINDOW);
        if c.len() >= MAX_RESTARTS {
            drop(c);
            self.set_state(AgentState::Error("crashed".into()));
            return;
        }
        c.push(Instant::now());
        drop(c);
        self.set_state(AgentState::Starting);
        let h = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            if !h.closing.load(std::sync::atomic::Ordering::SeqCst) {
                h.start_agent();
            }
        });
    }

    fn running_state(&self) -> AgentState {
        match self.agent.model() {
            Some(m) => AgentState::Running { model: m.model },
            None => AgentState::SignedOut,
        }
    }

    pub fn has_model(&self) -> bool {
        self.agent.model().is_some()
    }

    /// A model is chosen and the sidecar is up: an unprompted turn could run now.
    pub fn can_turn(&self) -> bool {
        self.has_model() && self.agent.is_running()
    }

    pub fn model(&self) -> Option<ModelRef> {
        self.agent.model()
    }

    /// The models the person can pick (tray "Model"): every model the sidecar's catalog lists for the
    /// providers that have a credential, as (provider id, model id, name). Asks the sidecar (call off the
    /// main thread); empty while it is down or nothing is signed in.
    pub fn models(&self) -> Vec<ModelChoice> {
        let providers: Vec<&str> = if self.fake { vec!["fake"] } else { self.catalog.iter().filter(|p| self.creds.has(&p.id)).map(|p| p.id.as_str()).collect() };
        if providers.is_empty() || !self.agent.is_running() {
            return Vec::new();
        }
        let reply = match self.agent.cmd("models", json!({ "providers": providers })) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[buddy] models: {e}");
                return Vec::new();
            }
        };
        let list = reply["models"].as_array().cloned().unwrap_or_default();
        list.iter()
            .filter_map(|m| {
                let provider = m["provider"].as_str()?.to_string();
                let id = m["id"].as_str()?.to_string();
                let name = m["name"].as_str().unwrap_or(&id).to_string();
                Some(ModelChoice { provider, id, name })
            })
            .collect()
    }

    /// The person picked a model (tray "Model"): it is `settings.model` from now on and the sidecar
    /// switches at once. Call off the main thread.
    pub fn pick_model(&self, provider: &str, model: &str) {
        if self.fake || !self.creds.has(provider) {
            return;
        }
        let saved = settings::Model { provider: provider.into(), model: model.into(), ..self.wanted.lock().unwrap().clone() };
        *self.wanted.lock().unwrap() = saved.clone();
        post(Inbox::Call(Box::new(move |a| a.model_chosen(saved))));
        eprintln!("[buddy] model: {provider}/{model}");
        if let Err(e) = self.agent.set_model(Some(ModelRef { provider: provider.into(), model: model.into() })) {
            eprintln!("[buddy] set_model: {e}");
        }
        if self.agent.is_running() {
            self.set_state(self.running_state());
        }
    }

    fn no_model(&self) -> BridgeError {
        match self.state() {
            AgentState::Keychain(why) => BridgeError::new("keychain", format!("Can't read the keychain: {why}.")),
            AgentState::Starting if !self.loaded.load(std::sync::atomic::Ordering::SeqCst) => BridgeError::new("starting", "Still reading the keychain; try again in a moment."),
            _ => BridgeError::new("no_model", NO_MODEL),
        }
    }

    /// The person's message; returns its request id. Blocks for the sidecar's ack (call off the main thread).
    pub fn prompt(&self, text: &str) -> Res<String> {
        if !self.has_model() {
            return Err(self.no_model());
        }
        // memory's extraction waits until the chat has been quiet a while
        if let Some(m) = crate::memory::get() {
            m.chat_sent();
        }
        self.agent.prompt(text)
    }

    /// A sense-triggered turn the attention budget allowed (`source: "sense"`, an `s:` id).
    /// `summary` is coarse: the rule and the time of day, never an app or window name.
    pub fn prompt_sense(&self, summary: &str) -> Res<String> {
        if !self.has_model() {
            return Err(self.no_model());
        }
        self.agent.prompt_sense(summary)
    }

    // --- sign-in (the Sign in… window) ------------------------------------------------------

    pub fn creds(&self) -> &Arc<Credentials> {
        &self.creds
    }

    pub fn catalog(&self) -> &[Provider] {
        &self.catalog
    }

    /// Whether the sidecar is up for an OAuth sign-in.
    pub fn can_oauth(&self) -> bool {
        self.agent.is_running()
    }

    /// A pasted API key, straight to the keychain (never to the sidecar). Errors name the
    /// store's reason only, never the key.
    pub fn save_api_key(&self, provider_id: &str, key: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Paste a key first.".into());
        }
        // no sidecar started and the stored sign-ins never loaded: a save here couldn't be used
        if let AgentState::Keychain(why) = self.state() {
            return Err(format!("Can't read the keychain: {why}. Allow desk-buddy's keychain access, then quit and reopen it."));
        }
        self.creds.put(provider_id, json!({ "type": "api_key", "key": key })).map_err(|e| format!("Couldn't save the key: {e}."))?;
        self.credentials_changed(provider_id);
        Ok(())
    }

    /// pi-ai's OAuth sign-in through the sidecar; blocks until it ends (call off the main thread).
    /// The credential arrives through `cred.modify`, so it is in the keychain when this returns.
    pub fn oauth_login(&self, provider_id: &str) -> Result<(), BridgeError> {
        self.agent.auth_login(provider_id)?;
        self.credentials_changed(provider_id);
        Ok(())
    }

    pub fn cancel_login(&self) {
        let _ = self.agent.auth_cancel();
    }

    /// Sign out / Remove: the keychain item goes, then `credentials_changed`.
    pub fn sign_out(&self, provider_id: &str) -> Result<(), String> {
        self.creds.remove(provider_id).map_err(|e| format!("Couldn't remove it from the keychain: {e}."))?;
        self.credentials_changed(provider_id);
        Ok(())
    }

    /// A credential came or went: the sidecar drops its cache, and the model follows. A sign-in
    /// to a provider other than `settings.model`'s, while that one has no credential, makes it the
    /// new `settings.model` (with the provider's default model).
    fn credentials_changed(&self, provider_id: &str) {
        if self.agent.is_running() {
            if let Err(e) = self.agent.cmd("credentials_changed", json!({ "providerId": provider_id })) {
                eprintln!("[buddy] credentials_changed: {e}");
            }
        }
        // mid-load the set is partial: the load picks the model once it has them all
        if !self.loaded.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let wanted = self.wanted.lock().unwrap().clone();
        let current = self.agent.model();
        let next = choose_model(&wanted, self.fake, &self.catalog, |p| self.creds.has(p));
        if next == current {
            return;
        }
        if let Some(m) = next.as_ref().filter(|m| m.provider == provider_id && m.provider != wanted.provider) {
            let saved = settings::Model { provider: m.provider.clone(), model: m.model.clone(), ..wanted.clone() };
            *self.wanted.lock().unwrap() = saved.clone();
            post(Inbox::Call(Box::new(move |a| a.model_chosen(saved))));
        }
        eprintln!("[buddy] model: {}", next.as_ref().map_or("none".to_string(), |m| format!("{}/{}", m.provider, m.model)));
        if let Err(e) = self.agent.set_model(next) {
            eprintln!("[buddy] set_model: {e}");
        }
        if self.agent.is_running() {
            self.set_state(self.running_state());
        }
    }

    /// An approval card's answer (`allow`, `allow_always`, `deny`) to the policy gate.
    pub fn answer(&self, call_id: &str, answer: &str) -> Res<Value> {
        self.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call_id, "answer": answer }))
    }

    /// Stop the current turn (the bubble's ■).
    pub fn abort(&self) -> Res<Value> {
        self.agent.cmd("abort", json!({}))
    }

    pub fn agent(&self) -> &Arc<Agent> {
        &self.agent
    }

    /// The host API's port (the browser widget never opens it).
    pub fn host_port(&self) -> Option<u16> {
        self.host.lock().unwrap().as_ref().map(|h| h.port())
    }

    pub fn policy_view(&self) -> Value {
        self.policy.view(&self.agent.ctx(self.agent.gen()))
    }

    /// The bubble's Snooze: the same text again in `minutes`, through the reminder path.
    pub fn snooze(&self, text: &str, minutes: u32) -> host::Reply {
        self.agent.remind(&RemindOp::Add { at_ms: policy::now_ms() + minutes as u64 * 60_000, text: text.to_string() })
    }

    /// Quit: stop the sidecar and remove `host.json` (also after a crash: the host outlives the sidecar).
    pub fn shutdown(&self) {
        self.closing.store(true, std::sync::atomic::Ordering::SeqCst);
        self.agent.shutdown(Duration::from_secs(3));
        // the agent holds the host too: drop ours, and remove the file outright
        self.host.lock().unwrap().take();
        let _ = std::fs::remove_file(&self.host_file);
    }

    pub fn host_file(&self) -> &Path {
        &self.host_file
    }
}

pub const NO_MODEL: &str = "There's no model to talk to yet. Sign in to one first.";

#[cfg(test)]
mod tests {
    use super::*;
    use bridge::store::MemoryStore;

    fn wanted() -> settings::Model {
        settings::Model { provider: "anthropic".into(), model: "claude-sonnet-5-5".into(), ..Default::default() }
    }

    fn home(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("buddy-harness-{name}-{}-{}", std::process::id(), host::random_hex(3)))
    }

    /// Built, with the stored credentials read as `launch`'s thread does (no sidecar start).
    fn built(home: &Path, config: Config, program: Option<PathBuf>) -> Arc<Harness> {
        let h = Harness::build(home, &wanted(), config, program).unwrap();
        let st = h.load();
        h.set_state(st);
        h
    }

    /// A store whose `list` waits until told (a keychain dialog nobody has answered yet).
    struct Held(MemoryStore, Mutex<std::sync::mpsc::Receiver<()>>);

    impl CredentialStore for Held {
        fn get(&self, p: &str) -> bridge::store::StoreRes<Option<Value>> {
            self.0.get(p)
        }
        fn put(&self, p: &str, c: &Value) -> bridge::store::StoreRes<()> {
            self.0.put(p, c)
        }
        fn delete(&self, p: &str) -> bridge::store::StoreRes<()> {
            self.0.delete(p)
        }
        fn list(&self) -> bridge::store::StoreRes<Vec<String>> {
            let _ = self.1.lock().unwrap().recv();
            self.0.list()
        }
    }

    fn look_event(caller: host::Caller, turn: Option<&str>) -> HostEvent {
        HostEvent { caller, action: host::Action::Screen(host::ScreenOp::Look { image: false }), turn: turn.map(String::from), after_turn: false }
    }

    #[test]
    fn a_screen_look_marks_the_conversation_only_when_an_agent_got_text_back() {
        let home = home("screen");
        let h = built(&home, Config::memory(MemoryStore::new()), None);
        let gen = h.agent.gen();
        let agent = || host::Caller::Agent { agent_id: "main".into(), gen };
        let ok = |_: bool| -> host::Reply { Ok(json!({ "text": "[screen]", "truncated": false })) };
        let taint = |h: &Harness| h.policy_view()["taint"].clone();
        let before = taint(&h);

        // the person's terminal: the text comes back, nothing is marked
        let r = screen_call(&look_event(host::Caller::User, None), "user", false, Some(h.agent.clone()), Some(h.policy.clone()), ok).unwrap();
        assert_eq!(r["text"], "[screen]");
        assert_eq!(taint(&h), before, "the person's own look marks nothing");

        // a refused look (off, permission, deny-listed) marks nothing
        let refuse = |_: bool| -> host::Reply { Err(Refusal::new(403, "screen_look_off", "Screen look is turned off.")) };
        let e = screen_call(&look_event(agent(), Some("u:a")), "agent", false, Some(h.agent.clone()), Some(h.policy.clone()), refuse).unwrap_err();
        assert_eq!((e.status, e.code), (403, "screen_look_off"));
        assert_eq!(taint(&h), before, "no text came back");

        // an agent's look marks it, source screen, and the detail never holds what was read
        screen_call(&look_event(agent(), Some("u:a")), "agent", true, Some(h.agent.clone()), Some(h.policy.clone()), ok).unwrap();
        let t = taint(&h);
        let marks = serde_json::to_string(&t).unwrap();
        assert!(marks.contains("\"source\":\"screen\"") && marks.contains("\"detail\":\"screen look\""), "{marks}");
        assert!(!marks.contains("[screen]"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_screen_look_from_a_check_in_turn_is_refused_before_it_looks() {
        let home = home("screen-sense");
        let h = built(&home, Config::memory(MemoryStore::new()), None);
        let gen = h.agent.gen();
        let called = std::cell::Cell::new(false);
        let look = |_: bool| -> host::Reply {
            called.set(true);
            Ok(json!({}))
        };
        let e = look_event(host::Caller::Agent { agent_id: "main".into(), gen }, Some("s:0f8fad5b-d9cb-469f-a165-70867728950e"));
        let err = screen_call(&e, "agent", false, Some(h.agent.clone()), Some(h.policy.clone()), look).unwrap_err();
        assert_eq!((err.status, err.code, err.message.as_str()), (403, "sense_turn", "Screen look only runs when the person asked in chat, not during a check-in"));
        assert!(!called.get(), "nothing was captured");
        assert!(!serde_json::to_string(&h.policy_view()["taint"]).unwrap().contains("screen"), "nothing was marked");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_screen_look_after_its_turn_ended_is_refused_before_it_looks() {
        let home = home("screen-late");
        let h = built(&home, Config::memory(MemoryStore::new()), None);
        let gen = h.agent.gen();
        let called = std::cell::Cell::new(false);
        let look = |_: bool| -> host::Reply {
            called.set(true);
            Ok(json!({}))
        };
        let mut e = look_event(host::Caller::Agent { agent_id: "main".into(), gen }, Some("u:0f8fad5b-d9cb-469f-a165-70867728950e"));
        e.after_turn = true;
        let err = screen_call(&e, "agent", false, Some(h.agent.clone()), Some(h.policy.clone()), look).unwrap_err();
        assert_eq!((err.status, err.code), (403, "turn_ended"));
        assert!(!called.get(), "nothing was captured");
        // the person's own terminal has no turn to end
        let mut e = look_event(host::Caller::User, None);
        e.after_turn = true;
        assert!(screen_call(&e, "you", false, None, None, |_| Ok(json!({}))).is_ok());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_keychain_is_read_off_the_calling_thread_and_the_tray_says_starting_meanwhile() {
        let home = home("held");
        let store = MemoryStore::new();
        store.put("anthropic", &json!({ "type": "api_key", "key": "sk-ant-held" })).unwrap();
        let (answer, rx) = std::sync::mpsc::channel();
        let config = Config { store: Box::new(Held(store, Mutex::new(rx))), ..Config::memory(MemoryStore::new()) };
        // neither returns while the dialog is open
        let t = Instant::now();
        let h = Harness::build(&home, &wanted(), config, None).unwrap();
        h.clone().launch();
        assert!(t.elapsed() < Duration::from_secs(2), "build and launch don't wait for the keychain");
        assert_eq!(h.state().line(), "Getting ready…");
        assert!(!h.has_model(), "nothing read yet");
        assert_eq!(h.prompt("hi").unwrap_err().code, "starting", "not “sign in”: the read is still running");
        // a key saved meanwhile doesn't replace settings.model's provider, which simply hasn't loaded yet
        h.save_api_key("openrouter", "sk-or-v1-meanwhile").unwrap();
        answer.send(()).unwrap();
        let t = Instant::now();
        while h.state() == AgentState::Starting {
            assert!(t.elapsed() < Duration::from_secs(5), "the load finished");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(h.model().unwrap().provider, "anthropic", "the stored key, read on the start thread, still preferred");
        assert_eq!(h.state(), AgentState::Error("no sidecar".into()));
        h.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_widget_call_at_launch_waits_for_the_widgets_then_gives_up() {
        let t = Instant::now();
        let n = std::sync::atomic::AtomicU32::new(0);
        assert_eq!(wait_for(Duration::from_secs(2), || (n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3).then_some(7)), Some(7));
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!(wait_for(Duration::from_millis(120), || None::<u8>), None);
    }

    #[test]
    fn the_model_is_the_fake_then_settings_then_the_first_signed_in_provider() {
        let cat = signin::catalog(false);
        let has = |set: &'static [&'static str]| move |p: &str| set.contains(&p);
        assert_eq!(choose_model(&wanted(), true, &cat, has(&[])), Some(ModelRef { provider: "fake".into(), model: "fake-1".into() }));
        assert_eq!(choose_model(&wanted(), false, &cat, has(&["anthropic", "openai"])), Some(ModelRef { provider: "anthropic".into(), model: "claude-sonnet-5-5".into() }));
        // settings.model's provider has nothing: the first signed-in one, with its default model
        assert_eq!(choose_model(&wanted(), false, &cat, has(&["openrouter", "xai"])), Some(ModelRef { provider: "openrouter".into(), model: "anthropic/claude-sonnet-5.5".into() }));
        assert_eq!(choose_model(&wanted(), false, &cat, has(&[])), None);
    }

    #[test]
    fn no_credential_means_signed_out_a_no_model_prompt_and_quit_removes_host_json() {
        let home = home("none");
        let h = built(&home, Config::memory(MemoryStore::new()), None);
        assert!(!h.has_model());
        assert_eq!(AgentState::SignedOut.line(), "Connect an AI account…");
        assert!(home.join("host.json").is_file(), "the host API is up for `buddy say`");
        let e = h.prompt("hi").unwrap_err();
        assert_eq!(e.code, "no_model");
        assert_eq!(h.prompt_sense("back from idle · morning").unwrap_err().code, "no_model");
        assert!(!h.agent().is_running());
        assert_eq!(h.policy_view()["mode"], "allowlist");
        // a reminder with no agent: 503 no_agent, as `buddy remind` reports it
        assert_eq!(h.snooze("x", 10).unwrap_err().code, "no_agent");
        h.shutdown();
        assert!(!home.join("host.json").exists(), "quitting removes host.json");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_keychain_read_error_is_the_tray_line_and_no_sidecar_start() {
        let home = home("fail");
        // a sidecar program is set: only the keychain error keeps it from starting
        let h = built(&home, Config::memory(MemoryStore::failing()), Some(PathBuf::from("/nonexistent/buddy-agent")));
        assert_eq!(h.state(), AgentState::Keychain("access denied".into()));
        assert_eq!(h.state().line(), "⚠ Can't read the keychain · access denied");
        assert!(!h.agent().is_running());
        assert_eq!(h.agent().gen(), 0, "no start was even tried");
        let e = h.prompt("hi").unwrap_err();
        assert_eq!(e.code, "keychain");
        // a pasted key isn't saved where it couldn't be used, and the error never echoes it
        let e = h.save_api_key("openrouter", "sk-or-pasted-in-keychain-error").unwrap_err();
        assert!(e.starts_with("Can't read the keychain: access denied."), "{e}");
        assert!(!e.contains("sk-or-pasted"));
        assert!(!h.has_model() && !h.can_turn());
        assert_eq!(h.prompt("hi").unwrap_err().code, "keychain");
        h.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_stored_key_is_loaded_and_chosen_and_saving_never_echoes_the_key() {
        let home = home("stored");
        let store = MemoryStore::new();
        store.put("openrouter", &json!({ "type": "api_key", "key": "sk-or-stored-1" })).unwrap();
        let h = built(&home, Config::memory(store), None);
        assert_eq!(h.model(), Some(ModelRef { provider: "openrouter".into(), model: "anthropic/claude-sonnet-5.5".into() }));
        assert_eq!(h.save_api_key("mistral", "   ").unwrap_err(), "Paste a key first.");
        h.save_api_key("anthropic", " sk-ant-pasted-2 ").unwrap();
        assert!(h.creds().has("anthropic"));
        // settings.model's provider now has a key: it wins
        assert_eq!(h.model().unwrap().provider, "anthropic");
        h.sign_out("anthropic").unwrap();
        assert_eq!(h.model().unwrap().provider, "openrouter");
        h.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn agent_lines_match_the_prototype() {
        assert_eq!(AgentState::Running { model: "claude-sonnet-5-5".into() }.line(), "Ready · claude-sonnet-5-5");
        assert_eq!(AgentState::Stopped.line(), "Assistant stopped");
        assert!(AgentState::Error("crashed".into()).line().starts_with("⚠ Assistant problem"));
        // only a missing account makes the line a way in
        assert!(AgentState::SignedOut.needs_account() && AgentState::Keychain("x".into()).needs_account());
        assert!(!AgentState::Starting.needs_account() && !AgentState::Stopped.needs_account());
    }
}
