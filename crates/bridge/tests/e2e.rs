//! End to end: the compiled `buddy-agent` sidecar against the scripted fake
//! model (`agent/test/support/fake.ts`), the real host API and the `buddy` CLI
//! on the commands' PATH. Needs `bun`; builds the sidecar and `buddy` if missing.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bridge::protocol::{ModelRef, ProviderDef, ProviderModel};
use bridge::{credentials::Credentials, Agent, AgentEvent, AgentSpec};
use host::{Action, Caller, Host, HostEvent};
use mascot_protocol::{MascotEvent, WorkKind};
use policy::Production;
use serde_json::{json, Value};

const KEY: &str = "sk-fake-e2e-91d2";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The sidecar under test: `BUDDY_AGENT_BIN`, else `agent/dist/buddy-agent`, built once per run.
fn sidecar() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(p) = std::env::var_os("BUDDY_AGENT_BIN").filter(|v| !v.is_empty()) {
            return PathBuf::from(p);
        }
        let ok = Command::new("bun").arg(repo().join("agent/scripts/build.ts")).current_dir(repo().join("agent")).status().expect("bun is needed for the e2e tests");
        assert!(ok.success(), "building the sidecar failed");
        repo().join("agent/dist/buddy-agent")
    })
    .clone()
}

/// The folder holding a fresh `buddy` CLI.
fn buddy_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let ok = Command::new(env!("CARGO")).args(["build", "-q", "-p", "host", "--bin", "buddy"]).current_dir(repo()).status().unwrap();
        assert!(ok.success(), "building buddy failed");
        // target/debug/deps/e2e-… → target/debug
        let dir = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
        assert!(dir.join("buddy").is_file(), "no buddy in {}", dir.display());
        dir
    })
    .clone()
}

struct Fake {
    child: Child,
    url: String,
}

impl Fake {
    fn start(script: &Path) -> Fake {
        let mut child = Command::new("bun")
            .arg(repo().join("agent/test/support/fake.ts"))
            .args(["--port", "0", "--script"])
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let url = loop {
            let l = lines.next().expect("the fake exited").unwrap();
            if let Some(rest) = l.strip_prefix("fake provider on ") {
                break rest.split_whitespace().next().unwrap().to_string();
            }
        };
        std::thread::spawn(move || for _ in lines {});
        Fake { child, url }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Fx {
    root: PathBuf,
    agent: Arc<Agent>,
    events: Arc<Mutex<Vec<AgentEvent>>>,
    said: Arc<Mutex<Vec<HostEvent>>>,
    _host: Arc<Host>,
    _fake: Fake,
}

impl Drop for Fx {
    fn drop(&mut self) {
        self.agent.shutdown(Duration::from_secs(3));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn script() -> Value {
    json!([
        { "match": "said", "reply": { "text": "I said hi." } },
        { "match": "/declined/i", "reply": { "text": "Okay, I won’t." } },
        { "match": "say hi", "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "buddy say Hi there --emotion happy" } }] } },
        { "match": "break", "reply": { "error": { "status": 400, "body": { "error": { "message": "the fake refused" } } } } },
        { "match": "slow", "times": 1, "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "echo $$ > cmd.pid; sleep 30" } }] } },
        // The resumed run, told its call was cut off, speaks.
        { "match": "/interrupted/i", "times": 1, "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "buddy say Back again" } }] } },
    ])
}

fn fixture(name: &str) -> Fx {
    let root = std::env::temp_dir().join(format!("buddy-e2e-{name}-{}-{}", std::process::id(), policy::random_hex(3)));
    std::fs::create_dir_all(root.join("home")).unwrap();
    let script_path = root.join("script.json");
    std::fs::write(&script_path, script().to_string()).unwrap();
    let fake = Fake::start(&script_path);
    let said = Arc::new(Mutex::new(Vec::new()));
    let s2 = said.clone();
    let host = Arc::new(Host::start(&root.join("host.json"), move |e| s2.lock().unwrap().push(e)).unwrap());
    let creds = Arc::new(Credentials::new());
    creds.set_api_key("fake", KEY);
    let spec = AgentSpec {
        agent_id: "a1".into(),
        work_dir: root.join("work"),
        state_dir: root.join("state").join("agents").join("a1"),
        skills_dir: repo().join("agent/skills"),
        program: sidecar(),
        home: root.join("home"),
        path_dirs: vec![buddy_dir()],
        model: Some(ModelRef { provider: "fake".into(), model: "fake-1".into() }),
        providers: vec![ProviderDef {
            id: "fake".into(),
            name: "Fake".into(),
            api: "openai-completions".into(),
            base_url: format!("{}/v1", fake.url),
            models: vec![ProviderModel { id: "fake-1".into(), name: "Fake 1".into(), context_window: 128000, max_tokens: 4096, reasoning: false, input: vec!["text".into()], compat: None }],
        }],
        settings: json!({ "retryDelayMs": 50 }),
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let e2 = events.clone();
    let agent = Agent::new(spec, Arc::new(Production::new(|_, _| {})), host.clone(), creds, move |e| e2.lock().unwrap().push(e));
    agent.start().expect("the sidecar starts");
    Fx { root, agent, events, said, _host: host, _fake: fake }
}

impl Fx {
    /// The first event at index ≥ `from` that `pred` picks, and its index.
    fn wait<T>(&self, from: usize, what: &str, pred: impl Fn(&AgentEvent) -> Option<T>) -> (usize, T) {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            {
                let ev = self.events.lock().unwrap();
                if let Some((i, t)) = ev.iter().enumerate().skip(from).find_map(|(i, e)| pred(e).map(|t| (i, t))) {
                    return (i, t);
                }
                if Instant::now() > end {
                    let tail: Vec<String> = ev.iter().rev().take(12).map(|e| format!("{e:?}").chars().take(300).collect()).collect();
                    panic!("timed out waiting for {what}; newest events:\n{}", tail.join("\n"));
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    fn asked(&self, from: usize) -> String {
        self.wait(from, "an approval card", |e| match e {
            AgentEvent::Mascot(MascotEvent::AwaitingPermission { call_id, .. }) => Some(call_id.clone()),
            _ => None,
        })
        .1
    }

    /// The run that started after `from` ended: the mascot went idle (or showed an error) after being busy.
    fn run_end(&self, from: usize) -> MascotEvent {
        let (busy, _) = self.wait(from, "a busy status", |e| matches!(e, AgentEvent::Record { rec, .. } if rec["kind"] == "status" && rec["status"]["busy"] == true).then_some(()));
        self.wait(busy, "the run's end", |e| match e {
            AgentEvent::Mascot(m @ (MascotEvent::Idle | MascotEvent::Error { .. })) => Some(m.clone()),
            _ => None,
        })
        .1
    }

    fn mascot(&self, from: usize) -> Vec<MascotEvent> {
        self.events.lock().unwrap()[from..].iter().filter_map(|e| if let AgentEvent::Mascot(m) = e { Some(m.clone()) } else { None }).collect()
    }

    fn records(&self) -> Vec<Value> {
        self.events.lock().unwrap().iter().filter_map(|e| if let AgentEvent::Record { rec, .. } = e { Some(rec.clone()) } else { None }).collect()
    }

    fn snapshot(&self) -> Vec<Value> {
        self.agent.cmd("snapshot", json!({})).unwrap()["entries"].as_array().unwrap().clone()
    }
}

/// Tool calls in a transcript with no result entry.
fn unanswered(entries: &[Value]) -> Vec<String> {
    let msgs: Vec<&Value> = entries.iter().flat_map(|e| e["model"].as_array().into_iter().flatten()).collect();
    let results: Vec<&str> = msgs.iter().filter(|m| m["role"] == "toolResult").filter_map(|m| m["toolCallId"].as_str()).collect();
    msgs.iter()
        .filter(|m| m["role"] == "assistant")
        .flat_map(|m| m["content"].as_array().into_iter().flatten())
        .filter(|c| c["type"] == "toolCall")
        .filter_map(|c| c["id"].as_str())
        .filter(|id| !results.contains(id))
        .map(String::from)
        .collect()
}

/// `want`'s kinds appear in `got` in order (others may sit between).
fn subsequence(got: &[MascotEvent], want: &[fn(&MascotEvent) -> bool]) -> bool {
    let mut it = got.iter();
    want.iter().all(|w| it.any(w))
}

#[test]
fn an_allowed_buddy_say_reaches_the_host_with_the_turn_and_maps_to_mascot_events() {
    let fx = fixture("allow");
    let from = fx.len();
    let request_id = fx.agent.prompt("Please say hi to me").unwrap();
    let call = fx.asked(from);
    fx.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call, "answer": "allow" })).unwrap();
    assert_eq!(fx.run_end(from), MascotEvent::Idle);

    let said = fx.said.lock().unwrap().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].action, Action::Say { text: "Hi there".into(), emotion: Some("happy".into()) });
    assert_eq!(said[0].caller, Caller::Agent { agent_id: "a1".into(), gen: 1 });
    assert_eq!(said[0].turn.as_deref(), Some(request_id.as_str()), "BUDDY_TURN is the prompt's request id");
    assert!(!said[0].after_turn);

    let m = fx.mascot(from);
    assert!(
        subsequence(
            &m,
            &[
                |e| matches!(e, MascotEvent::Thinking),
                |e| matches!(e, MascotEvent::AwaitingPermission { title, .. } if title == "Run buddy say?"),
                |e| matches!(e, MascotEvent::Working { kind: WorkKind::Buddy, tool } if tool == "bash"),
                |e| matches!(e, MascotEvent::Speaking { text } if text == "I said hi."),
                |e| matches!(e, MascotEvent::Idle),
            ]
        ),
        "{m:#?}"
    );
    // Working never comes before the card was answered.
    let asked = m.iter().position(|e| matches!(e, MascotEvent::AwaitingPermission { .. })).unwrap();
    assert!(m[..asked].iter().all(|e| !matches!(e, MascotEvent::Working { .. })), "{m:#?}");
    // The key stayed in Rust: no record carries it.
    assert!(fx.records().iter().all(|r| !r.to_string().contains(KEY)));
}

#[test]
fn a_denied_buddy_say_never_reaches_the_host() {
    let fx = fixture("deny");
    let from = fx.len();
    fx.agent.prompt("Please say hi").unwrap();
    let call = fx.asked(from);
    fx.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call, "answer": "deny" })).unwrap();
    assert_eq!(fx.run_end(from), MascotEvent::Idle);
    assert!(fx.said.lock().unwrap().is_empty());
    let m = fx.mascot(from);
    assert!(m.iter().all(|e| !matches!(e, MascotEvent::Working { .. })), "a denied call never shows as working: {m:#?}");
    assert!(m.iter().any(|e| matches!(e, MascotEvent::Speaking { text } if text.starts_with("Okay"))), "{m:#?}");
}

#[test]
fn a_model_error_maps_to_error() {
    let fx = fixture("error");
    let from = fx.len();
    fx.agent.prompt("please break").unwrap();
    match fx.run_end(from) {
        MascotEvent::Error { reason, .. } => assert_eq!(reason, "model_error"),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn a_crash_mid_tool_restarts_and_records_interrupted() {
    let fx = fixture("crash");
    let from = fx.len();
    let request_id = fx.agent.prompt("do the slow thing").unwrap();
    let call = fx.asked(from);
    fx.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call, "answer": "allow" })).unwrap();
    fx.wait(from, "the command running", |e| matches!(e, AgentEvent::Mascot(MascotEvent::Working { kind: WorkKind::Shell, .. })).then_some(()));
    // Its shell has written its pid: reap it after the crash (pi-durable starts commands detached).
    let pid_file = fx.root.join("work").join("cmd.pid");
    let end = Instant::now() + Duration::from_secs(10);
    while !pid_file.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    fx.agent.crash();
    fx.wait(from, "the exit", |e| matches!(e, AgentEvent::Exited { by_app: false, .. }).then_some(()));
    if let Ok(pid) = std::fs::read_to_string(&pid_file) {
        let _ = Command::new("kill").args(["-KILL", &format!("-{}", pid.trim())]).status();
    }
    assert_eq!(fx.agent.last_exit(), "crash");

    let restart = fx.len();
    fx.agent.start().unwrap();
    assert_eq!(fx.agent.gen(), 2);
    let (_, by) = fx.wait(restart, "interrupted", |e| match e {
        AgentEvent::Record { gen: 2, rec } if rec["kind"] == "entry" && rec["entry"]["data"]["type"] == "interrupted" => Some(rec["entry"]["data"]["by"].clone()),
        _ => None,
    });
    assert_eq!(by, "crash");
    // The resumed run's `buddy say` belongs to the cut-off turn, on time.
    let call = fx.asked(restart);
    fx.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call, "answer": "allow" })).unwrap();
    fx.run_end(restart);
    let said = fx.said.lock().unwrap().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].caller, Caller::Agent { agent_id: "a1".into(), gen: 2 });
    assert_eq!(said[0].turn.as_deref(), Some(request_id.as_str()));
    assert!(!said[0].after_turn, "a resumed run's say is on time");
    assert_eq!(unanswered(&fx.snapshot()), Vec::<String>::new());
    // The first start's token died with it.
    fx.agent.shutdown(Duration::from_secs(3));
    assert_eq!(fx.agent.last_exit(), "clean");
}

#[cfg(unix)]
#[test]
fn a_start_that_dies_before_ready_keeps_the_last_exit() {
    let root = std::env::temp_dir().join(format!("buddy-e2e-unready-{}-{}", std::process::id(), policy::random_hex(3)));
    let state = root.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("last-exit"), "quit").unwrap();
    let host = Arc::new(Host::start(&root.join("host.json"), |_| {}).unwrap());
    let spec = AgentSpec {
        agent_id: "a1".into(),
        work_dir: root.join("work"),
        state_dir: state.clone(),
        skills_dir: repo().join("agent/skills"),
        // Exits at once, before any `ready`.
        program: PathBuf::from("/usr/bin/false"),
        home: root.join("home"),
        path_dirs: vec![],
        model: None,
        providers: vec![],
        settings: json!({}),
    };
    let agent = Agent::new(spec, Arc::new(Production::new(|_, _| {})), host, Arc::new(Credentials::new()), |_| {});
    assert!(agent.start().is_err());
    assert_eq!(std::fs::read_to_string(state.join("last-exit")).unwrap(), "quit", "a start that never ran isn’t a crash");
    let _ = std::fs::remove_dir_all(&root);
}
