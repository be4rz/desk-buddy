//! The agent harness inside the app (design `## talk`, §2): the host API (`BUDDY_HOME/host.json`),
//! the policy gate, the in-memory credentials and one agent, "main", whose sidecar runs with the
//! §3.1 folders. Its mascot events and host commands go to the main thread through the `Inbox`.
//!
//! Credentials (dev, until sign-in in p4): `BUDDY_FAKE_MODEL=<url>` (the scripted fake provider,
//! as s2-harness) or the app's own `ANTHROPIC_API_KEY`, read once into `Credentials`. The key
//! never reaches the sidecar's or a command's environment (bridge passes a fixed allowlist).
//! Debug builds only: `BUDDY_PROMPT=<text>` submits one prompt once the agent is ready, and
//! `BUDDY_ANSWER=allow|deny` answers every approval card.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use bridge::bridge::sidecar_bin;
use bridge::credentials::Credentials;
use bridge::protocol::{ModelRef, ProviderDef, ProviderModel};
use bridge::{Agent, AgentEvent, AgentSpec, BridgeError, Res};
use host::{Action, Caller, Host, HostEvent, Refusal, RemindOp};
use mascot_protocol::{EmoteName, Emotion, MascotEvent};
use policy::Production;
use serde_json::{json, Value};

use crate::driver::{post, Inbox};
use crate::settings;

/// The one agent of p3.
pub const AGENT_ID: &str = "main";
/// Restarts after crashes allowed within `RESTART_WINDOW`.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(60);

/// What the tray's agent line shows.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentState {
    NoKey,
    Starting,
    Running { model: String },
    Stopped,
    Error(String),
}

impl AgentState {
    pub fn line(&self) -> String {
        match self {
            AgentState::NoKey => "⚠ Agent: no key (sign in arrives in p4)".into(),
            AgentState::Starting => "Agent: starting…".into(),
            AgentState::Running { model } => format!("Agent: running · {model}"),
            AgentState::Stopped => "Agent: stopped".into(),
            AgentState::Error(e) => format!("⚠ Agent: error · {e}"),
        }
    }
}

/// The model the agent uses and the credential that goes with it (in memory only).
#[derive(Clone, PartialEq)]
pub struct ModelChoice {
    pub model: ModelRef,
    pub providers: Vec<ProviderDef>,
    pub provider_id: String,
    pub key: String,
}

/// Never prints the key.
impl std::fmt::Debug for ModelChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelChoice").field("model", &self.model).field("provider_id", &self.provider_id).field("key", &"<redacted>").finish_non_exhaustive()
    }
}

/// The dev credential: the fake provider wins over a real key; none means no agent.
pub fn choose_model(fake: Option<&str>, anthropic_key: Option<&str>, wanted: &settings::Model) -> Option<ModelChoice> {
    if let Some(url) = fake.filter(|u| !u.trim().is_empty()) {
        let m = ProviderModel { id: "fake-1".into(), name: "Fake 1".into(), context_window: 128000, max_tokens: 4096, reasoning: false, input: vec!["text".into()], compat: None };
        let def = ProviderDef { id: "fake".into(), name: "Fake".into(), api: "openai-completions".into(), base_url: format!("{}/v1", url.trim_end_matches('/')), models: vec![m] };
        return Some(ModelChoice { model: ModelRef { provider: "fake".into(), model: "fake-1".into() }, providers: vec![def], provider_id: "fake".into(), key: "sk-fake-buddy".into() });
    }
    let key = anthropic_key.filter(|k| !k.trim().is_empty())?;
    // p3 has only the Anthropic key: another provider in settings needs sign-in (p4).
    if wanted.provider != "anthropic" {
        return None;
    }
    Some(ModelChoice { model: ModelRef { provider: "anthropic".into(), model: wanted.model.clone() }, providers: vec![], provider_id: "anthropic".into(), key: key.to_string() })
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
    model: Option<String>,
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
    /// Start the host API, the policy and (when there is a credential) the agent, in the background.
    /// Its state changes, mascot events and policy views arrive in the inbox.
    pub fn start(home: &Path, wanted: &settings::Model) -> std::io::Result<Arc<Harness>> {
        let fake = std::env::var("BUDDY_FAKE_MODEL").ok();
        // read once: the key lives in `Credentials` from here on, never in any child's env
        let key = std::env::var("ANTHROPIC_API_KEY").ok();
        let choice = choose_model(fake.as_deref(), key.as_deref(), wanted);
        drop(key);
        let h = Harness::build(home, choice, sidecar_bin())?;
        let _ = HARNESS.set(h.clone());
        h.clone().launch();
        Ok(h)
    }

    /// Everything but starting the sidecar (tests use this with no credential and no sidecar).
    pub fn build(home: &Path, choice: Option<ModelChoice>, program: Option<PathBuf>) -> std::io::Result<Arc<Harness>> {
        std::fs::create_dir_all(home)?;
        let slot: Arc<OnceLock<Weak<Agent>>> = Arc::default();
        let slot2 = slot.clone();
        let host_file = home.join("host.json");
        let host = Arc::new(Host::start(&host_file, move |e| on_host(e, slot2.get().and_then(Weak::upgrade)))?);
        let policy = Arc::new(Production::new(|_, view| {
            post(Inbox::Call(Box::new(move |a| a.policy_changed(&view))));
        }));
        let creds = Arc::new(Credentials::new());
        let (state_dir, work_dir, agent_home) = agent_dirs(home);
        let _ = std::fs::create_dir_all(&agent_home);
        // the person's `buddy`, built beside the app (`cargo build -p host --bin buddy`)
        let buddy_dir: Vec<PathBuf> = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).filter(|d| d.join("buddy").is_file()).into_iter().collect();
        if buddy_dir.is_empty() {
            eprintln!("[buddy] no `buddy` beside the app: the agent can't speak or set reminders (cargo build -p host --bin buddy)");
        }
        let model = choice.as_ref().map(|c| c.model.model.clone());
        let (model_ref, providers) = match &choice {
            Some(c) => {
                creds.set_api_key(&c.provider_id, &c.key);
                (Some(c.model.clone()), c.providers.clone())
            }
            None => (None, vec![]),
        };
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
            settings: json!({}),
        };
        let answer = auto_answer();
        let me: Arc<OnceLock<Weak<Harness>>> = Arc::default();
        let me2 = me.clone();
        let agent = Agent::new(spec, policy.clone(), host.clone(), creds, move |e| match e {
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
        let state = if choice.is_none() { AgentState::NoKey } else if program.is_none() { AgentState::Error("no sidecar".into()) } else { AgentState::Starting };
        let h = Arc::new(Harness { host: Mutex::new(Some(host)), host_file, agent, policy, model, state: Mutex::new(state), crashes: Mutex::new(vec![]), closing: Default::default() });
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
            if let AgentState::Error(_) = st {
                eprintln!("[buddy] no sidecar: build it with `cd agent && bun run build` (or set BUDDY_AGENT_BIN in debug builds)");
            }
            self.set_state(st);
            self.launch_prompt(); // with no key: the bubble shows the no_model error
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
                self.set_state(AgentState::Running { model: self.model.clone().unwrap_or_default() });
                true
            }
            Err(e) => {
                eprintln!("[buddy] the agent didn't start: {e}");
                self.set_state(AgentState::Error(e.code.to_string()));
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

    pub fn has_model(&self) -> bool {
        self.model.is_some()
    }

    /// The person's message; returns its request id. Blocks for the sidecar's ack (call off the main thread).
    pub fn prompt(&self, text: &str) -> Res<String> {
        if !self.has_model() {
            return Err(BridgeError::new("no_model", NO_MODEL));
        }
        self.agent.prompt(text)
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

pub const NO_MODEL: &str = "There's no model key yet. Sign-in arrives in a later version; for now start desk-buddy with ANTHROPIC_API_KEY set.";

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted() -> settings::Model {
        settings::Model { provider: "anthropic".into(), model: "claude-sonnet-5-5".into(), ..Default::default() }
    }

    #[test]
    fn the_fake_model_wins_and_no_key_means_no_agent() {
        let c = choose_model(Some("http://127.0.0.1:9/"), Some("sk-real"), &wanted()).unwrap();
        assert_eq!(c.model, ModelRef { provider: "fake".into(), model: "fake-1".into() });
        assert_eq!(c.providers[0].base_url, "http://127.0.0.1:9/v1");
        let c = choose_model(None, Some("sk-real"), &wanted()).unwrap();
        assert_eq!((c.provider_id.as_str(), c.key.as_str(), c.model.model.as_str()), ("anthropic", "sk-real", "claude-sonnet-5-5"));
        assert!(c.providers.is_empty());
        assert_eq!(choose_model(None, None, &wanted()), None);
        assert_eq!(choose_model(Some(" "), Some(""), &wanted()), None);
        let other = settings::Model { provider: "openai".into(), ..wanted() };
        assert_eq!(choose_model(None, Some("sk"), &other), None, "only the Anthropic key exists before p4");
    }

    #[test]
    fn no_key_means_no_agent_and_a_no_model_prompt_and_quit_removes_host_json() {
        let home = std::env::temp_dir().join(format!("buddy-harness-{}-{}", std::process::id(), host::random_hex(3)));
        let h = Harness::build(&home, None, None).unwrap();
        assert_eq!(h.state(), AgentState::NoKey);
        assert_eq!(h.state().line(), "⚠ Agent: no key (sign in arrives in p4)");
        assert!(home.join("host.json").is_file(), "the host API is up for `buddy say`");
        let e = h.prompt("hi").unwrap_err();
        assert_eq!(e.code, "no_model");
        assert!(!h.agent().is_running());
        assert_eq!(h.policy_view()["mode"], "allowlist");
        // a reminder with no agent: 503 no_agent, as `buddy remind` reports it
        assert_eq!(h.snooze("x", 10).unwrap_err().code, "no_agent");
        h.shutdown();
        assert!(!home.join("host.json").exists(), "quitting removes host.json");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn agent_lines_match_the_prototype() {
        assert_eq!(AgentState::Running { model: "claude-sonnet-5-5".into() }.line(), "Agent: running · claude-sonnet-5-5");
        assert_eq!(AgentState::Stopped.line(), "Agent: stopped");
        assert!(AgentState::Error("crashed".into()).line().starts_with("⚠ Agent: error"));
    }
}
