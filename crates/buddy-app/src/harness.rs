//! The agent harness inside the app (design `## talk`, `## accounts`, §2): the host API
//! (`BUDDY_HOME/host.json`), the policy gate, the credentials and one agent, "main", whose sidecar
//! runs with the §3.1 folders. Its mascot events and host commands go to the main thread through
//! the `Inbox`.
//!
//! Credentials live in the OS keychain (`KeychainStore`, service `desk-buddy`) and in Rust memory;
//! the keychain is read once at start, and a read error means no sidecar ("can't read keychain").
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
use host::{Action, Caller, Host, HostEvent, Refusal, RemindOp};
use mascot_protocol::{EmoteName, Emotion, MascotEvent};
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
            AgentState::SignedOut => "⚠ Agent: not signed in".into(),
            AgentState::Keychain(why) => format!("⚠ Agent: can't read keychain · {why}"),
            AgentState::Starting => "Agent: starting…".into(),
            AgentState::Running { model } => format!("Agent: running · {model}"),
            AgentState::Stopped => "Agent: stopped".into(),
            AgentState::Error(e) => format!("⚠ Agent: error · {e}"),
        }
    }
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
}

/// The fake provider at `url` (no `/v1`).
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
        let mut c = Config { store: Box::new(KeychainStore::new()), session: vec![], providers: vec![], fake: false, fake_oauth: None };
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
        Config { store: Box::new(store), session: vec![], providers: vec![], fake: false, fake_oauth: None }
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

/// What the host's sink does with a command; `agent` holds reminders.
fn on_host(e: HostEvent, agent: Option<Arc<Agent>>) -> host::Reply {
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
        Action::Emote { name } => {
            eprintln!("[buddy] host emote {who}: {name}");
            match serde_json::from_value::<EmoteName>(json!(name)) {
                Ok(n) => post(Inbox::Event(MascotEvent::Emote { name: n })),
                Err(_) => return Err(Refusal::new(400, "usage", format!("Unknown emote “{name}”."))),
            }
            Ok(json!({}))
        }
        Action::Remind(op) => {
            eprintln!("[buddy] host remind {who}: {op:?}");
            agent.map_or_else(|| Err(Refusal::no_agent()), |a| a.remind(&op))
        }
    }
}

impl Harness {
    /// Start the host API, the policy, the credentials and (unless the keychain can't be read)
    /// the agent, in the background. Its state changes, mascot events and policy views arrive in
    /// the inbox. An `Err` is the host API's (the caller shows it in the tray).
    pub fn start(home: &Path, wanted: &settings::Model) -> std::io::Result<Arc<Harness>> {
        let h = Harness::build(home, wanted, Config::from_env(), sidecar_bin())?;
        let _ = HARNESS.set(h.clone());
        h.clone().launch();
        Ok(h)
    }

    /// Everything but starting the sidecar (tests use this with a `MemoryStore` and no sidecar).
    pub fn build(home: &Path, wanted: &settings::Model, config: Config, program: Option<PathBuf>) -> std::io::Result<Arc<Harness>> {
        std::fs::create_dir_all(home)?;
        let slot: Arc<OnceLock<Weak<Agent>>> = Arc::default();
        let slot2 = slot.clone();
        let host_file = home.join("host.json");
        let host = Arc::new(Host::start(&host_file, move |e| on_host(e, slot2.get().and_then(Weak::upgrade)))?);
        let policy = Arc::new(Production::new(|_, view| {
            post(Inbox::Call(Box::new(move |a| a.policy_changed(&view))));
        }));
        let Config { store, session, providers, fake, fake_oauth } = config;
        let creds = Arc::new(Credentials::with_store(store));
        for (p, k) in &session {
            creds.set_api_key(p, k);
        }
        // once, at start: a read error is the tray's keychain line and no sidecar
        let keychain = match creds.load() {
            Ok(n) => {
                eprintln!("[buddy] credentials: {n} stored");
                None
            }
            Err(e) => Some(e.message),
        };
        let catalog = signin::catalog(fake_oauth.is_some());
        let model_ref = if keychain.is_none() { choose_model(wanted, fake, &catalog, |p| creds.has(p)) } else { None };
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
            model: model_ref,
            providers,
            settings: agent_settings,
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
            AgentEvent::Record { rec, .. } => {
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
        let state = match (keychain, &program) {
            (Some(why), _) => AgentState::Keychain(why),
            (None, None) => AgentState::Error("no sidecar".into()),
            (None, Some(_)) => AgentState::Starting,
        };
        let h = Arc::new(Harness {
            host: Mutex::new(Some(host)),
            host_file,
            agent,
            policy,
            creds,
            wanted: Mutex::new(wanted.clone()),
            catalog,
            fake,
            state: Mutex::new(state),
            crashes: Mutex::new(vec![]),
            closing: Default::default(),
        });
        let _ = me.set(Arc::downgrade(&h));
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

    /// Start the sidecar on a background thread (it waits for `ready`), then the launch prompt.
    fn launch(self: Arc<Self>) {
        let st = self.state();
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
        self.set_state(AgentState::Starting);
        std::thread::Builder::new()
            .name("buddy-agent-start".into())
            .spawn(move || {
                if self.start_agent() {
                    self.launch_prompt();
                }
            })
            .expect("spawn agent start");
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

    fn no_model(&self) -> BridgeError {
        match self.state() {
            AgentState::Keychain(why) => BridgeError::new("keychain", format!("Can't read the keychain: {why}.")),
            _ => BridgeError::new("no_model", NO_MODEL),
        }
    }

    /// The person's message; returns its request id. Blocks for the sidecar's ack (call off the main thread).
    pub fn prompt(&self, text: &str) -> Res<String> {
        if !self.has_model() {
            return Err(self.no_model());
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
        let h = Harness::build(&home, &wanted(), Config::memory(MemoryStore::new()), None).unwrap();
        assert!(!h.has_model());
        assert_eq!(AgentState::SignedOut.line(), "⚠ Agent: not signed in");
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
        let h = Harness::build(&home, &wanted(), Config::memory(MemoryStore::failing()), Some(PathBuf::from("/nonexistent/buddy-agent"))).unwrap();
        assert_eq!(h.state(), AgentState::Keychain("access denied".into()));
        assert_eq!(h.state().line(), "⚠ Agent: can't read keychain · access denied");
        h.clone().launch();
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
        let h = Harness::build(&home, &wanted(), Config::memory(store), None).unwrap();
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
        assert_eq!(AgentState::Running { model: "claude-sonnet-5-5".into() }.line(), "Agent: running · claude-sonnet-5-5");
        assert_eq!(AgentState::Stopped.line(), "Agent: stopped");
        assert!(AgentState::Error("crashed".into()).line().starts_with("⚠ Agent: error"));
    }
}
