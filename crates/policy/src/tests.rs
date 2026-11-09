use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::*;

struct Fixture {
    root: PathBuf,
    at: AgentCtx,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("buddy-policy-{name}-{}-{}", std::process::id(), random_hex(3)));
        let work = root.join("work");
        fs::create_dir_all(work.join("notes")).unwrap();
        let at = AgentCtx { agent_id: "a1".into(), work_dir: work.clone(), state_dir: work.join(".buddy").join("agents").join("a1"), skills_dir: None, gen: 1, home: None, secrets: vec![] };
        Fixture { root, at }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn policy() -> Production {
    Production::new(|_, _| {})
}

fn bash(call: &str, command: &str) -> Decide {
    Decide { call_id: call.into(), tool: "bash".into(), args: json!({ "command": command }), conversation: "root".into(), ..Default::default() }
}

fn file(call: &str, tool: &str, path: &str) -> Decide {
    Decide { call_id: call.into(), tool: tool.into(), args: json!({ "path": path }), conversation: "root".into(), ..Default::default() }
}

fn asks(d: &Decision) -> &Value {
    match d {
        Decision::Ask(a) => a,
        other => panic!("expected ask, got {other:?}"),
    }
}

#[test]
fn read_only_commands_run_and_buddy_say_asks_with_an_always_rule() {
    let fx = Fixture::new("say");
    let p = policy();
    assert_eq!(p.decide(&fx.at, &bash("c1", "ls -la && buddy status --json")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &bash("c2", "buddy --help --json")), Decision::Allow);
    let d = p.decide(&fx.at, &bash("c3", "buddy say \"Hi!\" --emotion happy"));
    let a = asks(&d);
    assert_eq!(a["title"], "Run buddy say?");
    assert_eq!(a["suggestedRule"], "Shell(buddy say:*)");
    assert_eq!(a["offerAlways"], true);
    // Allow from now on: the rule is stored and the next say runs.
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "c3", "answer": "allow_always" })).unwrap();
    assert_eq!(p.wait(&fx.at, "c3")["decision"], "allow");
    assert_eq!(p.decide(&fx.at, &bash("c4", "buddy say again")), Decision::Allow);
    // Stored on disk, and read back by a fresh policy.
    assert!(load(&fx.at.state_dir).rules.iter().any(|r| r.pattern == "Shell(buddy say:*)"));
    assert_eq!(policy().decide(&fx.at, &bash("c5", "buddy say from disk")), Decision::Allow);
}

#[test]
fn every_segment_of_a_compound_command_must_be_allowed() {
    let fx = Fixture::new("compound");
    let p = policy();
    let a = asks(&p.decide(&fx.at, &bash("c1", "ls && rm notes/x"))).clone();
    assert_eq!(a["title"], "Run rm?");
    // `$()` makes the line unreadable: it asks, with no rule to offer.
    let a = asks(&p.decide(&fx.at, &bash("c2", "echo $(ls)"))).clone();
    assert_eq!(a["offerAlways"], false);
    assert!(a["detail"].as_str().unwrap().contains("inside another"));
    // A redirect writes, so even a read-only command asks.
    asks(&p.decide(&fx.at, &bash("c3", "echo hi > notes/out.txt")));
    // A heredoc too.
    asks(&p.decide(&fx.at, &bash("c4", "cat <<EOF\nhi\nEOF")));
}

#[test]
fn deny_rules_beat_skip_mode_and_protect_the_apps_records() {
    let fx = Fixture::new("deny");
    let p = policy();
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "skip" })).unwrap();
    assert_eq!(p.decide(&fx.at, &bash("c1", "rm -rf notes")), Decision::Allow);
    for (tool, path) in [("write", ".buddy/agents/a1/policy.json"), ("edit", ".buddy/x"), ("write", "notes/../.buddy/y")] {
        match p.decide(&fx.at, &file("c2", tool, path)) {
            Decision::Deny { reason } => assert!(reason.contains("own records"), "{reason}"),
            other => panic!("{tool} {path}: {other:?}"),
        }
    }
    // A state folder outside the work folder is protected all the same.
    let outside = AgentCtx { state_dir: fx.root.join("state"), ..fx.at.clone() };
    fs::create_dir_all(&outside.state_dir).unwrap();
    let path = outside.state_dir.join("policy.json");
    assert!(matches!(p.decide(&outside, &file("c3", "write", path.to_str().unwrap())), Decision::Deny { .. }));
}

#[test]
fn modes_ask_and_allowlist_for_files() {
    let fx = Fixture::new("files");
    let p = policy();
    assert_eq!(p.decide(&fx.at, &file("c1", "write", "notes/a.md")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &file("c2", "read", "/etc/hosts")), Decision::Allow);
    let a = asks(&p.decide(&fx.at, &file("c3", "write", "/tmp/elsewhere.txt"))).clone();
    assert_eq!(a["kind"], "file");
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "ask" })).unwrap();
    asks(&p.decide(&fx.at, &bash("c4", "ls")));
    asks(&p.decide(&fx.at, &file("c5", "read", "notes/a.md")));
    assert!(p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "yolo" })).is_err());
}

#[test]
fn an_approved_fetch_marks_the_agent_and_marked_agents_lose_shell_allow_rules() {
    let fx = Fixture::new("taint");
    let p = policy();
    // Before the mark, a person's rule lets git push through.
    fs::create_dir_all(&fx.at.state_dir).unwrap();
    let f = PolicyFile { rules: vec![Rule { id: "r1".into(), effect: "allow".into(), pattern: "Shell(git push:*)".into(), origin: "person".into(), at: 0 }], ..Default::default() };
    fs::write(policy_path(&fx.at.state_dir), serde_json::to_vec(&f).unwrap()).unwrap();
    assert_eq!(p.decide(&fx.at, &bash("c0", "git push")), Decision::Allow);
    let a = asks(&p.decide(&fx.at, &bash("c1", "curl -s https://example.com"))).clone();
    assert!(a["detail"].as_str().unwrap().contains("Shell(curl:*)"));
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "c1", "answer": "allow" })).unwrap();
    let w = p.wait(&fx.at, "c1");
    assert_eq!(w["taint"]["source"], "shell");
    assert!(p.view(&fx.at)["taint"].is_object());
    // Marked: the person's shell rule stops applying and no "always" is offered.
    let a = asks(&p.decide(&fx.at, &bash("c2", "git push"))).clone();
    assert_eq!(a["offerAlways"], false);
    assert!(a["detail"].as_str().unwrap().contains("read the web"));
    // Reading outside the folder asks while marked.
    asks(&p.decide(&fx.at, &file("c3", "read", "/etc/hosts")));
    p.ui(&fx.at, &json!({ "op": "clear_taint" })).unwrap();
    assert_eq!(p.decide(&fx.at, &file("c4", "read", "/etc/hosts")), Decision::Allow);
}

#[test]
fn a_damaged_policy_file_starts_marked() {
    let fx = Fixture::new("damaged");
    fs::create_dir_all(&fx.at.state_dir).unwrap();
    fs::write(policy_path(&fx.at.state_dir), b"{ not json").unwrap();
    let f = load(&fx.at.state_dir);
    assert_eq!(f.mode, "allowlist");
    assert!(f.taint.is_some());
}

#[test]
fn wait_blocks_until_answered_and_returns_on_deny_cancel_or_release() {
    let fx = Fixture::new("wait");
    let p = Arc::new(policy());
    asks(&p.decide(&fx.at, &bash("c1", "buddy say one")));
    let (p2, at2) = (p.clone(), fx.at.clone());
    let waiter = thread::spawn(move || p2.wait(&at2, "c1"));
    thread::sleep(Duration::from_millis(100));
    assert!(!waiter.is_finished());
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "c1", "answer": "deny" })).unwrap();
    let w = waiter.join().unwrap();
    assert_eq!((w["decision"].as_str(), w["reason"].as_str()), (Some("deny"), Some(DECLINED)));
    // Answering twice is refused.
    assert_eq!(p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "c1", "answer": "allow" })).unwrap_err().code, "not_found");

    // The same call cut off and asked again gets the same card, not a second one.
    let first = asks(&p.decide(&fx.at, &bash("c2", "buddy say two"))).clone();
    assert_eq!(asks(&p.decide(&fx.at, &bash("c2", "buddy say two"))), &first);
    assert_eq!(p.view(&fx.at)["pending"], json!(["c2"]));

    // Cancel withdraws the card.
    let (p2, at2) = (p.clone(), fx.at.clone());
    let waiter = thread::spawn(move || p2.wait(&at2, "c2"));
    thread::sleep(Duration::from_millis(50));
    p.cancel(&fx.at, "c2");
    assert_eq!(waiter.join().unwrap()["answer"], "cancelled");

    // Release ends the gen's waits; the card stays for the next sidecar.
    asks(&p.decide(&fx.at, &bash("c3", "buddy say three")));
    let (p2, at2) = (p.clone(), fx.at.clone());
    let waiter = thread::spawn(move || p2.wait(&at2, "c3"));
    thread::sleep(Duration::from_millis(50));
    p.release(&fx.at);
    assert_eq!(waiter.join().unwrap()["answer"], "cancelled");
    let next = AgentCtx { gen: 2, ..fx.at.clone() };
    assert_eq!(p.view(&next)["pending"], json!(["c3"]));
}

#[test]
fn too_many_pending_cards_deny_and_unknown_tools_deny() {
    let fx = Fixture::new("limits");
    let p = policy();
    for i in 0..MAX_PENDING {
        asks(&p.decide(&fx.at, &bash(&format!("c{i}"), "buddy say x")));
    }
    assert!(matches!(p.decide(&fx.at, &bash("c99", "buddy say x")), Decision::Deny { .. }));
    assert!(matches!(p.decide(&fx.at, &Decide { tool: "web_fetch".into(), ..bash("w1", "") }), Decision::Deny { .. }));
    assert!(matches!(p.decide(&fx.at, &bash("e1", "   ")), Decision::Deny { .. }));
    let child = Decide { conversation: "child".into(), depth: 1, ..bash("k1", "ls") };
    assert!(matches!(p.decide(&fx.at, &child), Decision::Deny { .. }));
    assert!(p.ui(&fx.at, &json!({ "op": "remove_rule", "ruleId": "d1" })).is_err());
}

#[test]
fn the_port_file_and_git_config_are_off_limits_even_in_skip_mode() {
    let mut fx = Fixture::new("secret");
    let port = fx.root.join("host.json");
    fs::write(&port, "{}").unwrap();
    fx.at.secrets = vec![port.clone()];
    let p = policy();
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "skip" })).unwrap();
    let denied = |d: Decision| matches!(d, Decision::Deny { .. });
    // By the read tool, relative or absolute, and named in a command.
    assert!(denied(p.decide(&fx.at, &file("c1", "read", "../host.json"))));
    assert!(denied(p.decide(&fx.at, &file("c2", "read", &port.display().to_string()))));
    assert!(denied(p.decide(&fx.at, &bash("c3", "cat ../host.json"))));
    assert!(denied(p.decide(&fx.at, &bash("c4", &format!("ls && jq .token {}", port.display())))));
    // `~` is the agent's HOME, not the app's.
    let agent_home = AgentCtx { home: Some(fx.root.join("home")), ..fx.at.clone() };
    assert!(denied(p.decide(&agent_home, &file("c4d", "read", "~/../host.json"))));
    assert!(denied(p.decide(&agent_home, &bash("c4e", "cat ~/../host.json"))));
    if cfg!(target_os = "macos") {
        // Its file system ignores case: another spelling is the same file.
        assert!(denied(p.decide(&fx.at, &file("c4b", "read", "../HOST.JSON"))));
        assert!(denied(p.decide(&fx.at, &bash("c4c", "cat ../Host.json"))));
    }
    // Writing a repo's config (whose programs `git status` runs) is denied; reading it isn't.
    for (tool, path) in [("write", ".git/config"), ("edit", "sub/.git/hooks/x"), ("write", ".GIT/config")] {
        assert!(denied(p.decide(&fx.at, &file("c5", tool, path))), "{tool} {path}");
    }
    assert_eq!(p.decide(&fx.at, &file("c6", "read", ".git/config")), Decision::Allow);
    // Everything else still runs in skip mode.
    assert_eq!(p.decide(&fx.at, &bash("c7", "cat notes/host.json")), Decision::Allow);
}
