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
