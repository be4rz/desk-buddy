//! Spike S2 driver: one prompt through the whole harness, printing what the
//! mascot would see. A thin wrapper over `crates/bridge` (plan §7 S2).
//!
//!   cargo build -p host --bin buddy && cargo run -p s2-harness -- [--yes] [--fake <url>] "<prompt>"
//!
//! `--fake <url>` uses the scripted fake model (`bun agent/test/support/fake.ts
//! --port <p> --script <file>`; pass its base URL, without `/v1`). Without it, the
//! model is Anthropic's (`S2_MODEL`, default `claude-sonnet-5-5`) and the key comes
//! from this process's `ANTHROPIC_API_KEY` into the in-memory credential store:
//! it never reaches the sidecar's or a command's environment.
//! `--yes` allows every approval card; otherwise each one asks here (y/n/a).
//! Prints one JSON line per mascot event and per `buddy say`/`emote`.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use bridge::bridge::sidecar_bin;
use bridge::credentials::Credentials;
use bridge::protocol::{ModelRef, ProviderDef, ProviderModel};
use bridge::{Agent, AgentEvent, AgentSpec};
use host::{Action, Host};
use mascot_protocol::MascotEvent;
use policy::Production;
use serde_json::json;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let yes = take_flag(&mut args, "--yes");
    let fake = take_value(&mut args, "--fake");
    let prompt = args.join(" ");
    if prompt.trim().is_empty() {
        eprintln!("usage: s2-harness [--yes] [--fake <url>] \"<prompt>\"");
        return ExitCode::from(2);
    }
    let Some(program) = sidecar_bin() else {
        eprintln!("s2-harness: no sidecar; build it with `cd agent && bun run build`, or set BUDDY_AGENT_BIN (debug builds)");
        return ExitCode::from(1);
    };
    let root = std::env::var_os("S2_HOME").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("desk-buddy-s2"));
    let creds = Arc::new(Credentials::new());
    let (model, providers) = match &fake {
        Some(url) => {
            creds.set_api_key("fake", "sk-fake-s2");
            let m = ProviderModel { id: "fake-1".into(), name: "Fake 1".into(), context_window: 128000, max_tokens: 4096, reasoning: false, input: vec!["text".into()], compat: None };
            (ModelRef { provider: "fake".into(), model: "fake-1".into() }, vec![ProviderDef { id: "fake".into(), name: "Fake".into(), api: "openai-completions".into(), base_url: format!("{}/v1", url.trim_end_matches('/')), models: vec![m] }])
        }
        None => {
            let Some(key) = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty()) else {
                eprintln!("s2-harness: set ANTHROPIC_API_KEY, or pass --fake <url>");
                return ExitCode::from(2);
            };
            creds.set_api_key("anthropic", &key);
            (ModelRef { provider: "anthropic".into(), model: std::env::var("S2_MODEL").unwrap_or_else(|_| "claude-sonnet-5-5".into()) }, vec![])
        }
    };

    let out = Arc::new(Mutex::new(std::io::stdout()));
    let print = {
        let out = out.clone();
        move |v: serde_json::Value| {
            let mut o = out.lock().unwrap();
            let _ = writeln!(o, "{v}");
        }
    };
    let p2 = print.clone();
    let host = match Host::start(&root.join("host.json"), move |e| {
        let (kind, detail) = match e.action {
            Action::Say { text, emotion } => ("say", json!({ "text": text, "emotion": emotion })),
            Action::Emote { name } => ("emote", json!({ "name": name })),
        };
        p2(json!({ "host": kind, "detail": detail, "turn": e.turn, "afterTurn": e.after_turn }));
    }) {
        Ok(h) => Arc::new(h),
        Err(e) => {
            eprintln!("s2-harness: the host API didn’t start: {e}");
            return ExitCode::from(1);
        }
    };
    // The agent's `buddy` CLI: the host crate's binary, built beside this one.
    let buddy_dir: Vec<PathBuf> = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).into_iter().collect();
    if !buddy_dir.iter().any(|d| d.join("buddy").is_file()) {
        eprintln!("s2-harness: no `buddy` beside this binary; build it first with `cargo build -p host --bin buddy` (same profile)");
        return ExitCode::from(1);
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let spec = AgentSpec {
        agent_id: "s2".into(),
        work_dir: root.join("work"),
        state_dir: root.join("state").join("agents").join("s2"),
        skills_dir: repo.join("agent/skills"),
        program,
        home: root.join("home"),
        path_dirs: buddy_dir,
        model: Some(model),
        providers,
        settings: json!({}),
    };
    let _ = std::fs::create_dir_all(&spec.home);
    let (tx, rx) = mpsc::channel::<MascotEvent>();
    let tx = Mutex::new(tx);
    let p3 = print.clone();
    let agent = Agent::new(spec, Arc::new(Production::new(|_, _| {})), host.clone(), creds, move |e| {
        if let AgentEvent::Mascot(m) = e {
            p3(json!({ "mascot": m }));
            let _ = tx.lock().unwrap().send(m);
        }
    });
    if let Err(e) = agent.start() {
        eprintln!("s2-harness: {e}");
        return ExitCode::from(1);
    }
    if let Err(e) = agent.prompt(&prompt) {
        eprintln!("s2-harness: {e}");
        agent.shutdown(Duration::from_secs(3));
        return ExitCode::from(1);
    }
    let mut busy = false;
    let code = loop {
        let Ok(m) = rx.recv_timeout(Duration::from_secs(300)) else {
            eprintln!("s2-harness: no progress for 5 minutes");
            break ExitCode::from(1);
        };
        match m {
            MascotEvent::AwaitingPermission { call_id, title } => {
                let answer = if yes { "allow" } else { ask(&title) };
                if let Err(e) = agent.policy_ui(&json!({ "op": "approval_answer", "callId": call_id, "answer": answer })) {
                    eprintln!("s2-harness: {e}");
                }
            }
            MascotEvent::Idle if busy => break ExitCode::SUCCESS,
            MascotEvent::Error { .. } => break ExitCode::from(1),
            MascotEvent::Idle => {}
            _ => busy = true,
        }
    };
    agent.shutdown(Duration::from_secs(3));
    code
}

fn ask(title: &str) -> &'static str {
    eprint!("{title} [y]es / [n]o / [a]lways: ");
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    match line.trim() {
        "y" | "yes" => "allow",
        "a" | "always" => "allow_always",
        _ => "deny",
    }
}

fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != name);
    args.len() != before
}

fn take_value(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}
