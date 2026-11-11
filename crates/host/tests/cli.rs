//! The `buddy` binary against a live host: `remind` and its exit codes
//! (0 done, 1 refused, 2 usage).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use host::{random_hex, Action, Host, Refusal, RemindOp};
use serde_json::{json, Value};

struct Fx {
    host: Host,
    dir: PathBuf,
    added: Arc<Mutex<Vec<(u64, String)>>>,
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(name: &str, agent: bool) -> Fx {
    let dir = std::env::temp_dir().join(format!("buddy-cli-{name}-{}-{}", std::process::id(), random_hex(3)));
    let added: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let a2 = added.clone();
    let host = Host::start(&dir.join("host.json"), move |e| {
        if !agent {
            return Err(Refusal::no_agent());
        }
        match e.action {
            Action::Remind(RemindOp::Add { at_ms, text }) => {
                a2.lock().unwrap().push((at_ms, text));
                Ok(json!({ "id": "r_0123456789ab", "atMs": at_ms }))
            }
            Action::Remind(RemindOp::List) => {
                let list: Vec<Value> = a2.lock().unwrap().iter().map(|(at, t)| json!({ "id": "r_0123456789ab", "atMs": at, "text": t })).collect();
                Ok(json!({ "reminders": list }))
            }
            Action::Remind(RemindOp::Cancel { .. }) => Err(Refusal::new(404, "not_found", "No such reminder.")),
            _ => Ok(json!({})),
        }
    })
    .unwrap();
    host.set_min_remind_ms(1000);
    Fx { host, dir, added }
}

fn buddy(file: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_buddy")).args(args).env_remove("BUDDY_API").env_remove("BUDDY_TOKEN").env_remove("BUDDY_TURN").env("BUDDY_HOST_FILE", file).output().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

#[test]
fn remind_sets_lists_and_reports_refusals() {
    let fx = start("ok", true);
    let file = fx.host.file().to_path_buf();
    let o = buddy(&file, &["remind", "--in", "2s", "stretch", "now"]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("reminder r_0123456789ab set for "));
    assert_eq!(fx.added.lock().unwrap()[0].1, "stretch now");
    let o = buddy(&file, &["remind", "list", "--json"]);
    assert_eq!(code(&o), 0);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["reminders"][0]["text"], "stretch now");
    let o = buddy(&file, &["remind", "list"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("r_0123456789ab"));
    if cfg!(unix) {
        assert_eq!(code(&buddy(&file, &["remind", "--at", "07:30", "tea"])), 0);
    }
    // Unknown id: refused, not usage.
    let o = buddy(&file, &["remind", "cancel", "r_ffffffffffff"]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stderr).contains("No such reminder"));
    // Under the host's floor: the host's usage error.
    assert_eq!(code(&buddy(&file, &["remind", "--in", "500ms", "x"])), 2);
}

#[test]
fn remind_usage_errors_exit_2_before_any_request() {
    let fx = start("usage", true);
    let file = fx.host.file().to_path_buf();
    for args in [
        &["remind", "--in", "0s", "x"][..],
        &["remind", "--in", "8d", "x"],
        &["remind", "--in", "soon", "x"],
        &["remind", "--in", "20m"],
        &["remind", "x"],
        &["remind"],
        &["remind", "--in", "1m", "--at", "10:00", "x"],
        &["remind", "--at", "25:00", "x"],
        &["remind", "cancel"],
    ] {
        let o = buddy(&file, args);
        assert_eq!(code(&o), 2, "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }
    assert!(fx.added.lock().unwrap().is_empty());
}

#[test]
fn remind_with_no_agent_exits_1() {
    let fx = start("none", false);
    let o = buddy(fx.host.file(), &["remind", "--in", "20m", "x"]);
    assert_eq!(code(&o), 1);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("desk-buddy’s agent isn’t running") && err.contains("503"), "{err}");
}

/// `buddy` from the person's terminal: only `BUDDY_HOME` (no agent env, no `BUDDY_HOST_FILE`).
fn buddy_home(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_buddy"))
        .args(args)
        .env_remove("BUDDY_API")
        .env_remove("BUDDY_TOKEN")
        .env_remove("BUDDY_TURN")
        .env_remove("BUDDY_HOST_FILE")
        .env("BUDDY_HOME", home)
        .output()
        .unwrap()
}

/// A host whose sink records who said what.
fn recording(name: &str) -> (Host, PathBuf, Arc<Mutex<Vec<host::HostEvent>>>) {
    let dir = std::env::temp_dir().join(format!("buddy-cli-{name}-{}-{}", std::process::id(), random_hex(3)));
    let seen: Arc<Mutex<Vec<host::HostEvent>>> = Arc::default();
    let s2 = seen.clone();
    let host = Host::start(&dir.join("host.json"), move |e| {
        s2.lock().unwrap().push(e);
        Ok(json!({}))
    })
    .unwrap();
    (host, dir, seen)
}

#[test]
fn say_from_a_terminal_reads_buddy_home_and_reports_the_app_gone() {
    let (host, dir, seen) = recording("home");
    let o = buddy_home(&dir, &["say", "hello", "--emotion", "happy"]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].caller, host::Caller::User);
        assert_eq!(seen[0].action, Action::Say { text: "hello".into(), emotion: Some("happy".into()) });
    }
    // A stale file (the app died without removing it): nothing listens on its port.
    let stale = std::fs::read(host.file()).unwrap();
    drop(host);
    assert!(!dir.join("host.json").exists(), "dropping the host removes its file");
    std::fs::write(dir.join("host.json"), &stale).unwrap();
    let o = buddy_home(&dir, &["say", "hi"]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stderr).contains("desk-buddy isn't running"), "{}", String::from_utf8_lossy(&o.stderr));
    // No file at all.
    std::fs::remove_file(dir.join("host.json")).unwrap();
    let o = buddy_home(&dir, &["say", "hi"]);
    assert_eq!(code(&o), 1);
    assert!(String::from_utf8_lossy(&o.stderr).contains("desk-buddy isn't running"), "{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_agent_token_env_wins_over_buddy_home() {
    let (host, dir, seen) = recording("agentfirst");
    let token = host.issue("main", 7);
    let o = Command::new(env!("CARGO_BIN_EXE_buddy"))
        .args(["say", "from the agent"])
        .env_remove("BUDDY_HOST_FILE")
        .env_remove("BUDDY_TURN")
        .env("BUDDY_HOME", &dir)
        .env("BUDDY_API", host.url())
        .env("BUDDY_TOKEN", &token)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(seen.lock().unwrap()[0].caller, host::Caller::Agent { agent_id: "main".into(), gen: 7 });
    drop(host);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A host whose sink answers `buddy mascot …` like the app (body-tools).
fn mascot_host(name: &str) -> (Host, PathBuf, Arc<Mutex<Vec<host::MascotOp>>>) {
    let dir = std::env::temp_dir().join(format!("buddy-cli-{name}-{}-{}", std::process::id(), random_hex(3)));
    let seen: Arc<Mutex<Vec<host::MascotOp>>> = Arc::default();
    let s2 = seen.clone();
    let host = Host::start(&dir.join("host.json"), move |e| {
        let Action::Mascot(op) = e.action else { return Ok(json!({})) };
        s2.lock().unwrap().push(op.clone());
        Ok(match op {
            host::MascotOp::Do { name, .. } if name == "juggle" => json!({ "error": "unknown activity \"juggle\"", "have": ["read", "nap"], "message": "" }),
            host::MascotOp::Do { name, .. } if name == "busy" => return Err(Refusal::new(503, "busy", "busy")),
            host::MascotOp::Do { name, .. } if name == "nap" => json!({ "status": "refused", "activity": "nap", "reason": "dnd", "message": "Not now: do not disturb is on." }),
            host::MascotOp::Do { name, for_ms } => json!({ "status": "queued", "activity": name, "forMs": for_ms, "message": "Started reading for 5m (after this reply)." }),
            host::MascotOp::Stop => json!({ "status": "stopped", "activity": "read", "message": "Stopped reading." }),
            host::MascotOp::List => json!({ "activities": [{ "name": "read", "about": "sit down and read a book", "kind": "pastime", "defaultForS": 120 }], "emotes": [], "maxForS": 1800, "playing": null, "message": "Activities:\n  read      sit down and read a book (default 2m)" }),
            host::MascotOp::Emote { name } => json!({ "status": "played", "emote": name, "message": "Waved." }),
        })
    })
    .unwrap();
    (host, dir, seen)
}

#[test]
fn mascot_commands_print_the_reply_and_exit_by_the_frozen_codes() {
    let (host, dir, seen) = mascot_host("mascot");
    let file = host.file().to_path_buf();
    let out = |o: &Output| (code(o), String::from_utf8_lossy(&o.stdout).trim().to_string(), String::from_utf8_lossy(&o.stderr).trim().to_string());

    let o = buddy(&file, &["mascot", "do", "read", "--for", "5m"]);
    assert_eq!(out(&o), (0, "Started reading for 5m (after this reply).".into(), String::new()));
    assert_eq!(seen.lock().unwrap().last(), Some(&host::MascotOp::Do { name: "read".into(), for_ms: Some(300_000) }));
    let o = buddy(&file, &["mascot", "do", "read", "--json"]);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!((code(&o), v["ok"].clone(), v["status"].clone()), (0, json!(true), json!("queued")), "--json prints the reply");
    // refused is an answer: exit 0
    let o = buddy(&file, &["mascot", "do", "nap"]);
    assert_eq!(out(&o), (0, "Not now: do not disturb is on.".into(), String::new()));
    // an unknown name: usage, with the names it has
    let o = buddy(&file, &["mascot", "do", "juggle"]);
    assert_eq!(out(&o), (2, String::new(), "buddy: unknown activity \"juggle\" (have: read, nap)".into()));
    // busy: exit 1
    let o = buddy(&file, &["mascot", "do", "busy"]);
    assert_eq!(code(&o), 1, "{}", String::from_utf8_lossy(&o.stderr));
    let n = seen.lock().unwrap().len();
    // out-of-range --for: usage before any request
    for d in ["5s", "31m", "2h", "soon"] {
        let o = buddy(&file, &["mascot", "do", "read", "--for", d]);
        assert_eq!(code(&o), 2, "--for {d}");
    }
    assert_eq!(out(&buddy(&file, &["mascot", "do", "read", "--for", "5s"])).2, "buddy: --for must be between 10s and 30m");
    assert_eq!(code(&buddy(&file, &["mascot", "do"])), 2);
    assert_eq!(code(&buddy(&file, &["mascot", "dance"])), 2);
    assert_eq!(seen.lock().unwrap().len(), n, "usage errors send nothing");

    assert_eq!(out(&buddy(&file, &["mascot", "stop"])).1, "Stopped reading.");
    assert_eq!(out(&buddy(&file, &["mascot", "emote", "wave"])).1, "Waved.");
    let o = buddy(&file, &["mascot", "list"]);
    assert_eq!(out(&o).1, "Activities:\n  read      sit down and read a book (default 2m)");
    let v: Value = serde_json::from_slice(&buddy(&file, &["mascot", "list", "--json"]).stdout).unwrap();
    assert_eq!((v["maxForS"].clone(), v["playing"].clone()), (json!(1800), Value::Null));

    // --help --json: the mascot commands, no static emote list
    let v: Value = serde_json::from_slice(&buddy(&file, &["--help", "--json"]).stdout).unwrap();
    assert_eq!(v["mascot"], "run `buddy mascot list` for activities and emotes");
    let usages: Vec<&str> = v["commands"].as_array().unwrap().iter().filter_map(|c| c["usage"].as_str()).collect();
    for u in ["buddy mascot list [--json]", "buddy mascot do <activity> [--for <dur>]", "buddy mascot stop", "buddy mascot emote <name>"] {
        assert!(usages.contains(&u), "{u}");
    }
    assert!(v["commands"].as_array().unwrap().iter().all(|c| c.get("emotes").is_none()));

    // the app gone: exit 1
    drop(host);
    assert_eq!(code(&buddy(&file, &["mascot", "list"])), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// A late `buddy mascot do` (its turn ended) still acts and says so, like `buddy say`.
#[test]
fn a_late_mascot_command_says_after_its_turn_ended() {
    let (host, dir, seen) = mascot_host("mascot-late");
    let token = host.issue("main", 1);
    host.set_turn("main", Some("u:t2"));
    let o = Command::new(env!("CARGO_BIN_EXE_buddy"))
        .args(["mascot", "do", "read"])
        .env_remove("BUDDY_HOST_FILE")
        .env("BUDDY_HOME", &dir)
        .env("BUDDY_API", host.url())
        .env("BUDDY_TOKEN", &token)
        .env("BUDDY_TURN", "u:t1")
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "Started reading for 5m (after this reply). (after its turn ended)");
    assert_eq!(seen.lock().unwrap().len(), 1, "it still acted");
    drop(host);
    let _ = std::fs::remove_dir_all(&dir);
}
