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
fn read_only_commands_and_buddy_verbs_run_and_others_ask_with_an_always_rule() {
    let fx = Fixture::new("say");
    let p = policy();
    assert_eq!(p.decide(&fx.at, &bash("c1", "ls -la && buddy status --json")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &bash("c2", "buddy --help --json")), Decision::Allow);
    // The bear's own verbs run by default (d5–d8).
    for (i, cmd) in ["buddy say \"Hi!\" --emotion happy", "buddy mascot emote wave", "buddy mascot do read --for 5m", "buddy mascot stop", "buddy mascot list --json", "buddy status", "buddy remind --in 20m stretch"].iter().enumerate() {
        assert_eq!(p.decide(&fx.at, &bash(&format!("b{i}"), cmd)), Decision::Allow, "{cmd}");
    }
    let d = p.decide(&fx.at, &bash("c3", "make build"));
    let a = asks(&d);
    assert_eq!(a["title"], "Run make?");
    assert_eq!(a["suggestedRule"], "Shell(make:*)");
    assert_eq!(a["offerAlways"], true);
    // Allow from now on: the rule is stored and the next make runs.
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "c3", "answer": "allow_always" })).unwrap();
    assert_eq!(p.wait(&fx.at, "c3")["decision"], "allow");
    assert_eq!(p.decide(&fx.at, &bash("c4", "make again")), Decision::Allow);
    // Stored on disk, and read back by a fresh policy.
    assert!(load(&fx.at.state_dir).rules.iter().any(|r| r.pattern == "Shell(make:*)"));
    assert_eq!(policy().decide(&fx.at, &bash("c5", "make from disk")), Decision::Allow);
}

#[test]
fn buddy_verbs_still_run_when_tainted_but_tainted_cards_offer_no_always() {
    let fx = Fixture::new("taint-verbs");
    let p = policy();
    assert!(p.taint(&fx.at, "web_fetch", "https://example.com"));
    assert!(!p.taint(&fx.at, "web_fetch", "https://example.com/2"), "already marked");
    assert_eq!(p.decide(&fx.at, &bash("c1", "buddy say found it")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &bash("c2", "buddy remind --in 1h tea")), Decision::Allow);
    let a = asks(&p.decide(&fx.at, &bash("c3", "make build"))).clone();
    assert_eq!(a["offerAlways"], false);
    assert!(a["suggestedRule"].is_null());
}

#[test]
fn web_tools_run_unless_ask_mode_and_children_get_only_web_tools() {
    let fx = Fixture::new("web");
    let p = policy();
    let tool = |call: &str, tool: &str, args: Value| Decide { call_id: call.into(), tool: tool.into(), args, conversation: "root".into(), ..Default::default() };
    assert_eq!(p.decide(&fx.at, &tool("w1", "web_fetch", json!({ "url": "https://example.com" }))), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &tool("w2", "web_search", json!({ "query": "otters" }))), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &tool("w3", "task", json!({ "prompt": "find otters" }))), Decision::Allow);
    // A child (a task helper) may use the web tools and nothing else.
    let child = |d: Decide| Decide { conversation: "child".into(), depth: 1, ..d };
    assert_eq!(p.decide(&fx.at, &child(tool("k1", "web_fetch", json!({ "url": "https://example.com" })))), Decision::Allow);
    assert!(matches!(p.decide(&fx.at, &child(file("k2", "read", "notes/a.md"))), Decision::Deny { .. }));
    assert!(matches!(p.decide(&fx.at, &child(tool("k3", "task", json!({ "prompt": "x" })))), Decision::Deny { .. }));
    // Ask mode asks for the root's web calls, never with an "Always".
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "ask" })).unwrap();
    let a = asks(&p.decide(&fx.at, &tool("w4", "web_search", json!({ "query": "otters" })))).clone();
    assert_eq!(a["kind"], "web");
    assert_eq!(a["offerAlways"], false);
    assert_eq!(a["detail"], "Ask every time is on.");
    // The task card says in plain words that its helper's web calls won't ask (R-ask).
    let t = asks(&p.decide(&fx.at, &tool("w5", "task", json!({ "prompt": "find otters" })))).clone();
    assert_eq!(t["title"], "Send a helper to research this?");
    assert_eq!(t["detail"], "Ask every time is on. Its helper can search and read the web without asking.");
    assert_eq!(t["detail"], crate::TASK_CARD_DETAIL);
    assert_eq!(t["offerAlways"], false);
    assert_eq!(CHILD_TOOLS, ["web_fetch", "web_search"]);
}

#[test]
fn look_is_checked_as_a_read() {
    let fx = Fixture::new("look");
    let p = policy();
    assert_eq!(p.decide(&fx.at, &file("l1", "look", "notes/shot.png")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &file("l2", "look", "/etc/hosts")), Decision::Allow);
    p.taint(&fx.at, "web_fetch", "u");
    asks(&p.decide(&fx.at, &file("l3", "look", "/etc/hosts")));
    assert_eq!(p.decide(&fx.at, &file("l4", "look", "notes/shot.png")), Decision::Allow);
}

#[test]
fn a_capped_output_reads_back_when_tainted_but_other_buddy_files_still_ask() {
    let fx = Fixture::new("capped");
    let p = policy();
    p.taint(&fx.at, "web_fetch", "u");
    assert_eq!(p.decide(&fx.at, &file("o1", "read", ".buddy/outputs/call_1.txt")), Decision::Allow);
    asks(&p.decide(&fx.at, &file("o2", "read", ".buddy/notes.txt")));
    // Still never written by the agent.
    assert!(matches!(p.decide(&fx.at, &file("o3", "write", ".buddy/outputs/call_1.txt")), Decision::Deny { .. }));
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "ask" })).unwrap();
    asks(&p.decide(&fx.at, &file("o4", "read", ".buddy/outputs/call_1.txt")));
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
    asks(&p.decide(&fx.at, &bash("c1", "make one")));
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
    let first = asks(&p.decide(&fx.at, &bash("c2", "make two"))).clone();
    assert_eq!(asks(&p.decide(&fx.at, &bash("c2", "make two"))), &first);
    assert_eq!(p.view(&fx.at)["pending"], json!(["c2"]));

    // Cancel withdraws the card.
    let (p2, at2) = (p.clone(), fx.at.clone());
    let waiter = thread::spawn(move || p2.wait(&at2, "c2"));
    thread::sleep(Duration::from_millis(50));
    p.cancel(&fx.at, "c2");
    assert_eq!(waiter.join().unwrap()["answer"], "cancelled");

    // Release ends the gen's waits; the card stays for the next sidecar.
    asks(&p.decide(&fx.at, &bash("c3", "make three")));
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
        asks(&p.decide(&fx.at, &bash(&format!("c{i}"), "make x")));
    }
    assert!(matches!(p.decide(&fx.at, &bash("c99", "make x")), Decision::Deny { .. }));
    assert!(matches!(p.decide(&fx.at, &Decide { tool: "frobnicate".into(), ..bash("w1", "") }), Decision::Deny { .. }));
    assert!(matches!(p.decide(&fx.at, &bash("e1", "   ")), Decision::Deny { .. }));
    let child = Decide { conversation: "child".into(), depth: 1, ..bash("k1", "ls") };
    assert!(matches!(p.decide(&fx.at, &child), Decision::Deny { .. }));
    assert!(p.ui(&fx.at, &json!({ "op": "remove_rule", "ruleId": "d1" })).is_err());
}

#[test]
fn full_access_runs_everything_except_what_needs_attention() {
    let mut fx = Fixture::new("full");
    let port = fx.root.join("host.json");
    fs::write(&port, "{}").unwrap();
    fx.at.secrets = vec![port.clone()];
    let p = policy();
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "full" })).unwrap();
    // anything ordinary runs with no card: outside the allowlist, writes, redirects, files outside the work folder
    for (i, cmd) in ["rm notes/x", "npm install", "git commit -am wip && git push", "echo hi > out.txt", "python3 build.py", "echo $(ls)", "mkdir -p a/b && mv a c", "chmod +x run.sh"].iter().enumerate() {
        assert_eq!(p.decide(&fx.at, &bash(&format!("ok{i}"), cmd)), Decision::Allow, "{cmd}");
    }
    assert_eq!(p.decide(&fx.at, &file("f1", "write", "/tmp/buddy-full-test.txt")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &file("f2", "edit", "src/main.rs")), Decision::Allow);
    // what needs attention still asks, with the reason and no "always"
    let risky = [
        ("rm -rf notes", "Run rm?"),
        ("cd notes && sudo make install", "Run sudo?"),
        ("git push --force origin main", "Run git push?"),
        ("git reset --hard HEAD~3", "Run git reset?"),
        ("FOO=1 env rm -fr /tmp/x", "Run FOO=1?"),
        ("find . -name '*.log' -delete", "Run find?"),
        ("osascript -e 'tell app \"Finder\" to empty trash'", "Run osascript?"),
        ("security find-generic-password -s desk-buddy -w", "Run security?"),
        ("cat ~/.ssh/id_ed25519", "Run cat?"),
        ("npm publish", "Run npm?"),
        ("echo $(sudo whoami)", "Run echo?"),
    ];
    for (i, (cmd, _)) in risky.iter().enumerate() {
        let d = p.decide(&fx.at, &bash(&format!("r{i}"), cmd));
        let a = asks(&d);
        assert!(a["detail"].as_str().unwrap().starts_with("Full access still asks"), "{cmd}: {a}");
        assert_eq!(a["offerAlways"], false, "{cmd}");
        p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": format!("r{i}"), "answer": "deny" })).unwrap();
    }
    // an ask rule (the fetch rules) asks as in every mode but skip
    let a = asks(&p.decide(&fx.at, &bash("c1", "curl https://example.com"))).clone();
    assert!(a["detail"].as_str().unwrap().starts_with("A rule asks"), "{a}");
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "c1", "answer": "deny" })).unwrap();
    // sensitive files ask, by any file tool; deny rules and secrets still deny
    for (i, (tool, path)) in [("read", "~/.ssh/id_rsa"), ("write", "~/.zshrc"), ("edit", ".env"), ("write", "~/Library/LaunchAgents/x.plist"), ("write", "/etc/hosts")].iter().enumerate() {
        let d = p.decide(&fx.at, &file(&format!("s{i}"), tool, path));
        assert!(asks(&d)["detail"].as_str().unwrap().starts_with("Full access still asks"), "{tool} {path}");
        p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": format!("s{i}"), "answer": "deny" })).unwrap();
    }
    assert_eq!(p.decide(&fx.at, &file("e1", "read", ".env.example")), Decision::Allow);
    assert!(matches!(p.decide(&fx.at, &file("d1", "write", ".buddy/x")), Decision::Deny { .. }));
    assert!(matches!(p.decide(&fx.at, &file("d2", "write", ".git/config")), Decision::Deny { .. }));
    assert!(matches!(p.decide(&fx.at, &bash("d3", "cat ../host.json")), Decision::Deny { .. }));
    // once the conversation read the web, full access pauses: it behaves as the allowlist
    assert!(p.taint(&fx.at, "browser", "https://example.com/"));
    let a = asks(&p.decide(&fx.at, &bash("t1", "npm install"))).clone();
    assert!(a["detail"].as_str().unwrap().contains("read the web"), "{a}");
    assert_eq!(p.decide(&fx.at, &bash("t2", "ls notes")), Decision::Allow);
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "t1", "answer": "deny" })).unwrap();
    p.ui(&fx.at, &json!({ "op": "clear_taint" })).unwrap();
    assert_eq!(p.decide(&fx.at, &bash("t3", "npm install")), Decision::Allow);
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

#[test]
fn widget_reads_run_and_browser_actions_ask_while_marked() {
    let fx = Fixture::new("widgets");
    let p = policy();
    for (i, cmd) in ["buddy music play lofi --json", "buddy music status --json", "buddy widget list --json", "buddy browser snapshot"].iter().enumerate() {
        assert_eq!(p.decide(&fx.at, &bash(&format!("w{i}"), cmd)), Decision::Allow, "{cmd}");
    }
    // not marked: the person's own rule lets the bear click
    let a = asks(&p.decide(&fx.at, &bash("c1", "buddy browser click e3"))).clone();
    assert_eq!(a["suggestedRule"], "Shell(buddy browser:*)");
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "c1", "answer": "allow_always" })).unwrap();
    assert_eq!(p.decide(&fx.at, &bash("c2", "buddy browser click e4")), Decision::Allow);
    // a page read marks it: click, type and goto ask again, even in skip mode, and offer no always
    assert!(p.taint(&fx.at, "browser", "https://example.com/order"));
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "skip" })).unwrap();
    for (i, cmd) in ["buddy browser click e4", "buddy browser type e2 hello", "buddy browser goto https://evil.example", "echo $(buddy browser click e1)", "x=$(buddy  browser  goto https://evil.example)", "env buddy browser click e1", "FOO=1 buddy browser type e2 secret", "sh -c 'buddy browser goto https://evil.example'", "buddy widget open browser https://evil.example/?q=x"].iter().enumerate() {
        let d = p.decide(&fx.at, &bash(&format!("t{i}"), cmd));
        let a = asks(&d);
        assert_eq!(a["offerAlways"], false, "{cmd}");
        assert_eq!(a["title"], "Let the bear use the page?", "{cmd}");
        p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": format!("t{i}"), "answer": "deny" })).unwrap();
    }
    // the same, spelled so the words aren't side by side: flags between them, quotes or
    // backslashes inside them, a word from a variable, eval (review p5 S1)
    let spelled = [
        "buddy --json browser click e1",
        "buddy browser --json click e1",
        "buddy browser click --json e1",
        "buddy widget --json open browser https://evil.example",
        "buddy brow''ser cli\"\"ck e1",
        "bu\\ddy browser goto https://evil.example",
        "b=browser; buddy $b click e1",
        "B=buddy; $B browser type e2 secret",
        "v=goto; buddy browser ${v} https://evil.example",
        "eval buddy browser click e1",
        "eval \"$(printf 'buddy browser click e1')\"",
        "sh -c \"buddy --json browser goto https://evil.example\"",
        "echo `buddy browser click e1`",
    ];
    for (i, cmd) in spelled.iter().enumerate() {
        let d = p.decide(&fx.at, &bash(&format!("s{i}"), cmd));
        let a = asks(&d);
        assert_eq!(a["title"], "Let the bear use the page?", "{cmd}");
        p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": format!("s{i}"), "answer": "deny" })).unwrap();
    }
    // reading and the bear's own verbs still run
    assert_eq!(p.decide(&fx.at, &bash("r1", "buddy browser snapshot --json")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &bash("r1b", "buddy --json browser snapshot")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &bash("r2", "buddy music pause")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &bash("r3", "buddy widget list")), Decision::Allow);
}
