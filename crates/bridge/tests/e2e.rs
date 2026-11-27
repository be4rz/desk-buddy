//! End to end: the compiled `buddy-agent` sidecar against the scripted fake
//! model (`agent/test/support/fake.ts`), the real host API and the `buddy` CLI
//! on the commands' PATH. Needs `bun`; builds the sidecar and `buddy` if missing.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use bridge::protocol::{ModelRef, ProviderDef, ProviderModel};
use bridge::{credentials::Credentials, Agent, AgentEvent, AgentSpec};
use host::{Action, Caller, Host, HostEvent, Refusal, RemindOp};
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
    /// Every request the fake gets is also appended to `log` as one `{path, body}` JSON line.
    fn start(script: &Path, log: &Path) -> Fake {
        let mut child = Command::new("bun")
            .arg(repo().join("agent/test/support/fake.ts"))
            .args(["--port", "0", "--script"])
            .arg(script)
            .arg("--log")
            .arg(log)
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
    policy: Arc<Production>,
    /// The fake model's request log (JSONL).
    log: PathBuf,
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
        // The memory extractor's own request (its user message lists "Exchanges:"): first, so no chat matcher wins.
        { "match": "/Exchanges:[\\s\\S]*moved to Hanoi/", "reply": { "text": "{\"items\": [{\"exchange\": 1, \"kind\": \"profile\", \"predicate\": \"home_city\", \"qualifier\": null, \"value\": \"Hanoi\", \"text\": \"Lives in Hanoi\", \"importance\": 8, \"valid_from\": \"2026-10\", \"quote\": \"I moved to Hanoi last month\"}]}" } },
        { "match": "/Exchanges:[\\s\\S]*live in Saigon/", "reply": { "text": "{\"items\": [{\"exchange\": 1, \"kind\": \"profile\", \"predicate\": \"home_city\", \"qualifier\": null, \"value\": \"Saigon\", \"text\": \"Lives in Saigon\", \"importance\": 7, \"valid_from\": null, \"quote\": \"I live in Saigon\"}]}" } },
        { "match": "said", "reply": { "text": "I said hi." } },
        { "match": "set for", "reply": { "text": "Reminder set." } },
        { "match": "remind me", "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "buddy remind --in 2s stretch" } }] } },
        { "match": "Otter facts", "times": 1, "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "make otters" } }] } },
        { "match": "search otters", "reply": { "toolCalls": [{ "name": "web_search", "args": { "query": "otters" } }] } },
        { "match": "gated hi", "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "make greet && buddy say Hi there" } }] } },
        { "match": "/declined/i", "reply": { "text": "Okay, I won’t." } },
        { "match": "say hi", "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "buddy say Hi there --emotion happy" } }] } },
        { "match": "print env", "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "env > env.txt" } }] } },
        { "match": "break", "reply": { "error": { "status": 400, "body": { "error": { "message": "the fake refused" } } } } },
        { "match": "slow", "times": 1, "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "echo $$ > cmd.pid; sleep 30" } }] } },
        // The resumed run, told its call was cut off, speaks.
        { "match": "/interrupted/i", "times": 1, "reply": { "toolCalls": [{ "name": "bash", "args": { "command": "buddy say Back again" } }] } },
    ])
}

fn fixture(name: &str) -> Fx {
    fixture_with(name, json!({}))
}

fn fixture_with(name: &str, settings: Value) -> Fx {
    fixture_full(name, settings, None)
}

fn fixture_full(name: &str, settings: Value, persona: Option<&str>) -> Fx {
    fixture_mem(name, settings, persona, None)
}

fn fixture_mem(name: &str, settings: Value, persona: Option<&str>, memory: Option<&str>) -> Fx {
    let root = std::env::temp_dir().join(format!("buddy-e2e-{name}-{}-{}", std::process::id(), policy::random_hex(3)));
    std::fs::create_dir_all(root.join("home")).unwrap();
    let script_path = root.join("script.json");
    std::fs::write(&script_path, script().to_string()).unwrap();
    let log = root.join("fake.jsonl");
    let fake = Fake::start(&script_path, &log);
    let said = Arc::new(Mutex::new(Vec::new()));
    let s2 = said.clone();
    // The host's sink: reminders go to the agent (as the app does), the rest is recorded.
    let slot: Arc<OnceLock<Weak<Agent>>> = Arc::default();
    let slot2 = slot.clone();
    let host = Arc::new(
        Host::start(&root.join("host.json"), move |e| match &e.action {
            Action::Remind(op) => slot2.get().and_then(Weak::upgrade).map_or_else(|| Err(Refusal::no_agent()), |a| a.remind(op)),
            _ => {
                s2.lock().unwrap().push(e);
                Ok(json!({}))
            }
        })
        .unwrap(),
    );
    host.set_min_remind_ms(1000);
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
        settings: {
            let mut s = json!({ "retryDelayMs": 50 });
            s.as_object_mut().unwrap().extend(settings.as_object().cloned().unwrap_or_default());
            s
        },
        body: None,
        persona: persona.map(str::to_string),
        memory: memory.map(str::to_string),
        private_dirs: vec![],
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let e2 = events.clone();
    let policy = Arc::new(Production::new(|_, _| {}));
    let agent = Agent::new(spec, policy.clone(), host.clone(), creds, move |e| e2.lock().unwrap().push(e));
    let _ = slot.set(Arc::downgrade(&agent));
    agent.start().expect("the sidecar starts");
    Fx { root, agent, events, said, policy, log, _host: host, _fake: fake }
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
    // `buddy say` is a default allow rule (d5): no card.
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
                |e| matches!(e, MascotEvent::Working { kind: WorkKind::Buddy, tool } if tool == "bash"),
                |e| matches!(e, MascotEvent::Speaking { text } if text == "I said hi."),
                |e| matches!(e, MascotEvent::Idle),
            ]
        ),
        "{m:#?}"
    );
    assert!(m.iter().all(|e| !matches!(e, MascotEvent::AwaitingPermission { .. })), "{m:#?}");
    // The key stayed in Rust: no record carries it.
    assert!(fx.records().iter().all(|r| !r.to_string().contains(KEY)));
}

#[test]
fn an_allowed_card_shows_awaiting_permission_then_the_answer_then_working() {
    let fx = fixture("allow-card");
    let from = fx.len();
    // `make` isn't allowed, so the whole line asks.
    fx.agent.prompt("gated hi please").unwrap();
    let (asked_at, call) = fx.wait(from, "an approval card", |e| match e {
        AgentEvent::Mascot(MascotEvent::AwaitingPermission { call_id, .. }) => Some(call_id.clone()),
        _ => None,
    });
    // Nothing runs while the card is up.
    std::thread::sleep(Duration::from_millis(300));
    let answered_at = fx.len();
    assert!(
        fx.mascot(from).iter().all(|e| !matches!(e, MascotEvent::Working { .. })),
        "no Working before the answer: {:#?}",
        fx.mascot(from)
    );
    fx.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call, "answer": "allow" })).unwrap();
    let (working_at, _) = fx.wait(answered_at, "the command running", |e| matches!(e, AgentEvent::Mascot(MascotEvent::Working { kind: WorkKind::Shell, .. })).then_some(()));
    assert!(asked_at < answered_at && answered_at <= working_at, "AwaitingPermission ({asked_at}) → answer ({answered_at}) → Working ({working_at})");
    assert_eq!(fx.run_end(from), MascotEvent::Idle);
    let m = fx.mascot(from);
    assert!(
        subsequence(
            &m,
            &[
                |e| matches!(e, MascotEvent::Thinking),
                |e| matches!(e, MascotEvent::AwaitingPermission { title, .. } if title == "Run make?"),
                |e| matches!(e, MascotEvent::Working { kind: WorkKind::Shell, tool } if tool == "bash"),
                |e| matches!(e, MascotEvent::Idle),
            ]
        ),
        "{m:#?}"
    );
}

#[test]
fn a_denied_buddy_say_never_reaches_the_host() {
    let fx = fixture("deny");
    let from = fx.len();
    // `make` isn't allowed, so the whole line asks.
    fx.agent.prompt("gated hi please").unwrap();
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
    // The resumed run's `buddy say` (allowed by default) belongs to the cut-off turn, on time.
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
    let host = Arc::new(Host::start(&root.join("host.json"), |_| Ok(json!({}))).unwrap());
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
        body: None,
        persona: None,
        memory: None,
        private_dirs: vec![],
    };
    let agent = Agent::new(spec, Arc::new(Production::new(|_, _| {})), host, Arc::new(Credentials::new()), |_| {});
    assert!(agent.start().is_err());
    assert_eq!(std::fs::read_to_string(state.join("last-exit")).unwrap(), "quit", "a start that never ran isn’t a crash");
    let _ = std::fs::remove_dir_all(&root);
}

/// The `Reminder` events at index ≥ `from`.
fn reminders(fx: &Fx, from: usize) -> Vec<(String, String)> {
    fx.mascot(from).into_iter().filter_map(|e| if let MascotEvent::Reminder { id, text, .. } = e { Some((id, text)) } else { None }).collect()
}

#[cfg(unix)]
#[test]
fn a_reminder_survives_kill_9_and_fires_once() {
    let fx = fixture("remind");
    let from = fx.len();
    // The agent runs `buddy remind --in 2s stretch` (allowed by default, d8).
    fx.agent.prompt("remind me to stretch").unwrap();
    assert_eq!(fx.run_end(from), MascotEvent::Idle);
    let list = fx.agent.remind(&RemindOp::List).unwrap();
    let list = list["reminders"].as_array().unwrap().clone();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["text"], "stretch");
    let id = list[0]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("r_") && id.len() == 14, "{id}");
    assert!(fx.records().iter().any(|r| r["kind"] == "entry" && r.to_string().contains("set for")), "the CLI printed its line");

    // Killed before it fires; restarted after its time.
    fx.agent.crash();
    fx.wait(from, "the exit", |e| matches!(e, AgentEvent::Exited { by_app: false, .. }).then_some(()));
    assert!(reminders(&fx, from).is_empty(), "not yet");
    // Nothing runs to hold it.
    let r = fx.agent.remind(&RemindOp::Add { at_ms: 1, text: "x".into() }).unwrap_err();
    assert_eq!((r.status, r.code), (503, "no_agent"));
    std::thread::sleep(Duration::from_millis(2500));
    let restart = fx.len();
    fx.agent.start().unwrap();
    let (_, got) = fx.wait(restart, "the reminder", |e| match e {
        AgentEvent::Mascot(MascotEvent::Reminder { id, text, .. }) => Some((id.clone(), text.clone())),
        _ => None,
    });
    assert_eq!(got, (id.clone(), "stretch".to_string()));
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(reminders(&fx, from).len(), 1, "fired once");
    assert_eq!(fx.agent.remind(&RemindOp::List).unwrap()["reminders"], json!([]));
    // A fired reminder can't be cancelled.
    let r = fx.agent.remind(&RemindOp::Cancel { id: id.clone() }).unwrap_err();
    assert_eq!((r.status, r.code), (404, "not_found"));

    // Another restart doesn't fire it again.
    fx.agent.shutdown(Duration::from_secs(3));
    let again = fx.len();
    fx.agent.start().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(reminders(&fx, again).is_empty(), "{:?}", reminders(&fx, again));
}

/// A one-page "DuckDuckGo" on 127.0.0.1 for every request.
fn ddg_server() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/html/", l.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut s = s;
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let body = r#"<html><body><div class="result"><a class="result__a" href="https://example.com/otters">Otter facts</a><a class="result__snippet">Otters hold hands.</a></div></body></html>"#;
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    url
}

#[test]
fn a_web_search_marks_the_agent_and_its_next_card_offers_no_always() {
    let fx = fixture_with("web", json!({ "web": { "allowLoopback": true, "searchUrl": ddg_server() } }));
    let at = fx.agent.ctx(fx.agent.gen());
    assert!(fx.policy.view(&at)["taint"].is_null());
    let from = fx.len();
    fx.agent.prompt("search otters").unwrap();
    // The results led the model to `make otters`, which asks: tainted, so no "Always".
    let call = fx.asked(from);
    let taint = fx.policy.view(&at)["taint"].clone();
    assert_eq!(taint["sources"][0]["source"], "web_search", "{taint}");
    let (_, card) = fx.wait(from, "the card's entry", |e| match e {
        AgentEvent::Record { rec, .. } if rec["kind"] == "entry" && rec["entry"]["data"]["type"] == "approval_asked" => Some(rec["entry"]["data"].clone()),
        _ => None,
    });
    assert_eq!(card["offerAlways"], false, "{card}");
    assert!(fx.records().iter().any(|r| r["kind"] == "entry" && r["entry"]["data"]["type"] == "taint_set"));
    fx.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call, "answer": "deny" })).unwrap();
    assert_eq!(fx.run_end(from), MascotEvent::Idle);
}

// --- accounts (p4) ---------------------------------------------------------------------------

/// `ps eww <pid>`: the process's command line and environment.
fn ps_env(pid: u32) -> String {
    let out = Command::new("/bin/ps").args(["eww", "-o", "command=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn no_key_is_in_the_sidecars_env_nor_a_commands_env() {
    let secrets = [KEY, "ANTHROPIC_API_KEY", "_API_KEY="];
    let fx = fixture("envhyg");
    let env = ps_env(fx.agent.pid().expect("running"));
    assert!(env.contains("buddy-agent"), "ps saw the sidecar: {env:?}");
    for bad in secrets {
        assert!(!env.contains(bad), "{bad} in the sidecar's env");
    }
    // a command the agent runs (`env` isn't allowlisted: answer its card)
    let from = fx.len();
    fx.agent.prompt("print env").unwrap();
    let call = fx.asked(from);
    fx.agent.policy_ui(&json!({ "op": "approval_answer", "callId": call, "answer": "allow" })).unwrap();
    assert_eq!(fx.run_end(from), MascotEvent::Idle);
    let out = std::fs::read_to_string(fx.root.join("work").join("env.txt")).expect("the command ran");
    assert!(out.contains("PATH="), "{out}");
    for bad in secrets {
        assert!(!out.contains(bad), "{bad} in a command's env:\n{out}");
    }
}

/// The sign-in window, scripted: records what the sidecar asked and answers each code prompt.
struct ScriptedUi {
    code: String,
    seen: Mutex<Vec<String>>,
}

impl bridge::agent::AuthUi for ScriptedUi {
    fn open_url(&self, url: &str, _instructions: Option<&str>) {
        self.seen.lock().unwrap().push(format!("open_url {url}"));
    }
    fn device_code(&self, user_code: &str, _verification_uri: &str) {
        self.seen.lock().unwrap().push(format!("device_code {user_code}"));
    }
    fn progress(&self, message: &str) {
        self.seen.lock().unwrap().push(format!("progress {message}"));
    }
    fn prompt(&self, message: &str, _placeholder: Option<&str>, _cancelled: &std::sync::atomic::AtomicBool) -> Result<String, String> {
        self.seen.lock().unwrap().push(format!("prompt {message}"));
        Ok(self.code.clone())
    }
    fn select(&self, _message: &str, options: &[(String, String)], _cancelled: &std::sync::atomic::AtomicBool) -> Result<String, String> {
        Ok(options.first().map(|o| o.0.clone()).unwrap_or_default())
    }
}

#[test]
fn a_fake_oauth_sign_in_stores_the_credential_and_a_wrong_code_is_invalid_grant() {
    let fake_oauth = json!({ "fakeOAuth": { "baseUrl": "http://127.0.0.1:9/v1" } });
    let fx = fixture_with("oauth", fake_oauth.clone());
    let ui = Arc::new(ScriptedUi { code: "good".into(), seen: Mutex::default() });
    fx.agent.set_auth_ui(ui.clone());
    fx.agent.auth_login("fake-oauth").expect("signed in");
    let seen = ui.seen.lock().unwrap().clone();
    // open_url and the prompt are separate requests, handled on their own threads: either order
    for want in ["open_url http://127.0.0.1:1/fake-oauth/authorize", "prompt Paste the code"] {
        assert!(seen.iter().any(|s| s == want), "{want} in {seen:?}");
    }
    assert!(fx.agent.creds().kinds().iter().any(|(id, kind)| id == "fake-oauth" && kind == "oauth"), "{:?}", fx.agent.creds().kinds());

    // a wrong code: pi-ai's invalid_grant, nothing stored
    let fx2 = fixture_with("oauth-bad", fake_oauth.clone());
    fx2.agent.set_auth_ui(Arc::new(ScriptedUi { code: "abc123#state".into(), seen: Mutex::default() }));
    let e = fx2.agent.auth_login("fake-oauth").unwrap_err();
    assert_eq!(e.code, "auth", "{e}");
    assert!(e.message.contains("invalid_grant"), "{e}");
    assert!(!fx2.agent.creds().kinds().iter().any(|(id, _)| id == "fake-oauth"));

    // no window: the sign-in fails at once instead of hanging
    let fx3 = fixture_with("oauth-noui", fake_oauth);
    assert!(fx3.agent.auth_login("fake-oauth").is_err());
}

#[test]
fn a_sense_prompt_runs_with_an_s_id() {
    let fx = fixture("sense");
    let from = fx.len();
    let id = fx.agent.prompt_sense("[unprompted · back_from_idle · afternoon] said hi").unwrap();
    assert!(id.starts_with("s:"), "{id}");
    fx.run_end(from);
    let input = fx.snapshot().into_iter().find(|e| e["kind"] == "buddy.input").expect("the turn's input");
    assert_eq!(input["data"]["source"], "sense", "{input}");
    assert_eq!(input["data"]["requestId"], id.as_str());
}

/// The transcript, system entries included.
fn shown(fx: &Fx) -> String {
    Value::Array(fx.snapshot()).to_string()
}

#[test]
fn the_persona_section_reaches_the_model_and_set_persona_swaps_it_for_the_next_turn() {
    let biscuit = "## Who you are\n\nYour name is Biscuit, a cheeky sidekick.";
    let mochi = "## Who you are\n\nYour name is Mochi, a gentle companion.";
    let fx = fixture_full("persona", json!({}), Some(biscuit));
    let from = fx.len();
    fx.agent.prompt("said hello").unwrap();
    fx.run_end(from);
    assert!(shown(&fx).contains("Biscuit, a cheeky sidekick"), "Init.persona is in the first request");

    // a running sidecar swaps it now
    fx.agent.set_persona(Some(mochi.into())).unwrap();
    assert_eq!(fx.agent.persona().as_deref(), Some(mochi));
    let from = fx.len();
    fx.agent.prompt("said hello again").unwrap();
    fx.run_end(from);
    // (that the old text is gone from that request is checked against the fake's recorded bodies: agent/test/scenarios/persona.test.ts)
    assert!(shown(&fx).contains("Mochi, a gentle companion"), "the next request carries the new persona");

    // an oversized text is refused and never kept: the running persona and the next start's stay Mochi
    assert!(fx.agent.set_persona(Some("x".repeat(20_001))).is_err());
    assert_eq!(fx.agent.persona().as_deref(), Some(mochi));
}

// ---- memory (I1/I2): the core section in the system prompt, and the extractor op ----

/// One chat turn, to its end: the request id.
fn chat(fx: &Fx, text: &str) -> String {
    let from = fx.len();
    let id = fx.agent.prompt(text).unwrap();
    assert_eq!(fx.run_end(from), MascotEvent::Idle);
    id
}

/// Every request the fake model got, as `{path, body}` with `body` parsed.
fn requests(fx: &Fx) -> Vec<Value> {
    let lines: Vec<Value> = std::fs::read_to_string(&fx.log).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    lines.into_iter().map(|mut r| {
        r["body"] = r["body"].as_str().and_then(|b| serde_json::from_str(b).ok()).unwrap_or(Value::Null);
        r
    }).collect()
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn is_extract(req: &Value) -> bool {
    req["body"]["messages"].as_array().and_then(|m| m.last()).is_some_and(|m| text_of(&m["content"]).contains("Exchanges:"))
}

/// The system prompt of every chat request (the extractor's own are left out), oldest first.
fn system_prompts(fx: &Fx) -> Vec<String> {
    requests(fx)
        .iter()
        .filter(|r| r["body"]["messages"].is_array() && !is_extract(r))
        .filter_map(|r| r["body"]["messages"].as_array().and_then(|m| m.first()).filter(|m| m["role"] == "system").map(|m| text_of(&m["content"])))
        .collect()
}

fn now() -> memory::Now {
    memory::Now::new(jiff::civil::date(2026, 11, 27), "2026-11-27T10:00:00+07:00")
}

/// Ends the turn in the store, extracts it through the sidecar and applies the reply, as the app does.
fn learn(fx: &Fx, store: &memory::Store, request_id: &str) -> memory::Applied {
    store.add_pending(request_id, "2026-11-27T09:12:03+07:00").unwrap();
    let args = store.extract_args(10, jiff::civil::date(2026, 11, 27)).expect("a turn is pending");
    let reply = fx.agent.memory_extract(args.clone()).expect("the extractor answers");
    assert_eq!(reply["missing"], json!([]), "{reply}");
    let applied = store.apply_extract(&args, &reply, &now()).unwrap();
    fx.agent.set_memory(Some(store.section())).unwrap();
    applied
}

fn rows(store: &memory::Store) -> Vec<(String, String, Option<String>)> {
    let c = rusqlite::Connection::open(store.dir().join("memory.sqlite")).unwrap();
    let mut st = c.prepare("SELECT value, status, valid_to FROM facts WHERE predicate = 'home_city' ORDER BY id").unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap()
}

#[test]
fn a_remembered_fact_is_in_the_next_start_s_first_system_prompt() {
    let fx = fixture("mem-a1");
    let store = memory::Store::open(&fx.root.join("memory")).unwrap();
    let id = chat(&fx, "I live in Saigon");
    let applied = learn(&fx, &store, &id);
    assert_eq!(applied.created, 1, "{applied:?}");
    let section = store.section();
    assert!(section.contains("Lives in Saigon"), "{section}");
    // The extractor read the person's words from the transcript, not from the app.
    assert_eq!(rows(&store), vec![("Saigon".to_string(), "live".to_string(), None)]);

    // A new start with that section: it is in the very first request's system prompt, last.
    let fx2 = fixture_mem("mem-a1-restart", json!({}), None, Some(&section));
    chat(&fx2, "hello there");
    let first = &system_prompts(&fx2)[0];
    assert!(first.contains("Lives in Saigon"), "{first}");
    assert!(first.trim_end().ends_with(section.trim_end()), "the memory section is listed last:\n{first}");
}

#[test]
fn a_move_supersedes_the_old_city_and_the_section_follows() {
    let fx = fixture("mem-a2");
    let store = memory::Store::open(&fx.root.join("memory")).unwrap();
    let id = chat(&fx, "I live in Saigon");
    learn(&fx, &store, &id);
    let id = chat(&fx, "I moved to Hanoi last month");
    let applied = learn(&fx, &store, &id);
    assert_eq!(applied.superseded, 1, "{applied:?}");
    let section = store.section();
    assert!(section.contains("Hanoi") && !section.contains("Saigon"), "{section}");
    assert_eq!(
        rows(&store),
        vec![
            ("Saigon".to_string(), "superseded".to_string(), Some("2026-10-01".to_string())),
            ("Hanoi".to_string(), "live".to_string(), None),
        ]
    );
    // And the next prompt says so too.
    chat(&fx, "hi again");
    let last = system_prompts(&fx).pop().unwrap();
    assert!(last.contains("Hanoi") && !last.contains("Saigon"), "{last}");
}

#[test]
fn an_erased_fact_leaves_the_next_prompt() {
    let fx = fixture("mem-a4");
    let store = memory::Store::open(&fx.root.join("memory")).unwrap();
    let id = chat(&fx, "I live in Saigon");
    learn(&fx, &store, &id);
    chat(&fx, "do you remember");
    assert!(system_prompts(&fx).pop().unwrap().contains("Lives in Saigon"));

    let fact = store.list(None).unwrap().into_iter().next().expect("one fact");
    store.erase(fact.id).unwrap();
    fx.agent.set_memory(Some(store.section())).unwrap();
    chat(&fx, "and now");
    let last = system_prompts(&fx).pop().unwrap();
    assert!(!last.contains("Saigon") && !last.contains("What I know about you"), "{last}");
}

#[test]
fn with_memory_off_no_extractor_request_is_made_and_the_prompt_has_no_section() {
    let fx = fixture("mem-a5");
    assert_eq!(fx.agent.memory(), None);
    chat(&fx, "I live in Saigon");
    chat(&fx, "I moved to Hanoi last month");
    let all = requests(&fx);
    assert!(!all.is_empty());
    assert!(all.iter().all(|r| !is_extract(r)), "no memory.extract call without the app asking");
    for p in system_prompts(&fx) {
        assert!(!p.contains("What I know about you") && !p.contains("Saigon") && !p.contains("Hanoi"), "{p}");
    }
}

#[test]
fn several_memory_changes_between_turns_change_the_prompt_once() {
    let fx = fixture("mem-a7");
    chat(&fx, "first");
    fx.agent.set_memory(Some("## What I know about you\n- Lives in Saigon".into())).unwrap();
    fx.agent.set_memory(Some("## What I know about you\n- Lives in Hanoi".into())).unwrap();
    // Nothing is sent until a prompt needs it.
    assert_eq!(system_prompts(&fx).len(), 1);
    chat(&fx, "second");
    chat(&fx, "third");
    let prompts = system_prompts(&fx);
    assert_eq!(prompts.len(), 3, "one request per turn");
    let mut distinct = prompts.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), 2, "one change across three turns");
    assert!(!prompts[0].contains("Hanoi"));
    assert!(prompts[1].contains("Lives in Hanoi") && !prompts[1].contains("Saigon"), "{}", prompts[1]);
    assert_eq!(prompts[1], prompts[2]);
}
