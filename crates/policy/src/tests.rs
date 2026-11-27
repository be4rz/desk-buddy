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
        let at = AgentCtx { agent_id: "a1".into(), work_dir: work.clone(), state_dir: work.join(".buddy").join("agents").join("a1"), skills_dir: None, gen: 1, home: None, secrets: vec![], private_dirs: vec![] };
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

fn tool(call: &str, name: &str, args: Value) -> Decide {
    Decide { call_id: call.into(), tool: name.into(), args, conversation: "root".into(), ..Default::default() }
}

fn from_check_in(mut d: Decide) -> Decide {
    d.trigger = json!({ "requestId": "s:0f8fad5b-d9cb-469f-a165-70867728950e", "source": "sense" });
    d
}

fn denied_with(d: &Decision) -> &str {
    match d {
        Decision::Deny { reason } => reason,
        other => panic!("expected deny, got {other:?}"),
    }
}

const SCREEN_CARD: (&str, &str) = ("Let the bear look at your screen?", "It reads the text on the display the bear is on. Nothing is saved. Password managers and messengers are never read.");

#[test]
fn the_first_screen_look_asks_with_the_screen_card_and_an_always_rule_then_runs() {
    let fx = Fixture::new("screen-ask");
    let p = policy();
    // not in the default rules: it asks, in the allowlist mode that is the default
    let d = p.decide(&fx.at, &bash("s1", "buddy screen look"));
    let a = asks(&d);
    assert_eq!((a["title"].as_str(), a["detail"].as_str()), (Some(SCREEN_CARD.0), Some(SCREEN_CARD.1)));
    assert_eq!((a["suggestedRule"].as_str(), a["offerAlways"].clone()), (Some("Shell(buddy screen look:*)"), json!(true)));
    // the tool form (with and without a picture) is the same card and the same rule
    for (i, args) in [json!({}), json!({ "image": true }), json!({ "image": false })].into_iter().enumerate() {
        let d = p.decide(&fx.at, &tool(&format!("t{i}"), "screen_look", args));
        let a = asks(&d);
        assert_eq!((a["title"].as_str(), a["detail"].as_str(), a["suggestedRule"].as_str()), (Some(SCREEN_CARD.0), Some(SCREEN_CARD.1), Some("Shell(buddy screen look:*)")), "tool form {i}");
        assert_eq!(a["tool"], "screen_look");
    }
    // the person's Always answer makes the next look run
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "s1", "answer": "allow_always" })).unwrap();
    assert_eq!(p.wait(&fx.at, "s1")["decision"], "allow");
    assert_eq!(p.decide(&fx.at, &bash("s2", "buddy screen look --json")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &tool("s3", "screen_look", json!({ "image": true }))), Decision::Allow);
    // other buddy verbs are not affected
    for (i, cmd) in ["buddy say hi", "buddy status", "buddy mascot list --json", "buddy browser snapshot", "echo screen look"].iter().enumerate() {
        assert_eq!(p.decide(&fx.at, &bash(&format!("o{i}"), cmd)), Decision::Allow, "{cmd}");
    }
}

#[test]
fn full_access_still_asks_for_a_screen_look_even_after_an_always_rule() {
    let fx = Fixture::new("screen-full");
    let p = policy();
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "allowlist" })).unwrap();
    let _ = p.decide(&fx.at, &bash("s0", "buddy screen look"));
    p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": "s0", "answer": "allow_always" })).unwrap();
    let _ = p.wait(&fx.at, "s0");
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "full" })).unwrap();
    for (i, d) in [bash("f1", "buddy screen look"), bash("f2", "buddy screen look --json"), tool("f3", "screen_look", json!({})), tool("f4", "screen_look", json!({ "image": true }))].iter().enumerate() {
        let r = p.decide(&fx.at, d);
        let a = asks(&r);
        assert_eq!(a["title"], SCREEN_CARD.0, "{i}");
        assert_eq!(a["detail"], SCREEN_CARD.1, "{i}");
    }
    // and the skip mode (policy.json only, never asks) still asks for a look, on both paths
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "skip" })).unwrap();
    for (i, d) in [bash("g1", "buddy screen look"), tool("g2", "screen_look", json!({ "image": true })), bash("g3", "echo $(buddy screen look)")].iter().enumerate() {
        assert!(matches!(p.decide(&fx.at, d), Decision::Ask(_)), "skip #{i}");
    }
    assert_eq!(p.decide(&fx.at, &bash("g4", "rm -rf build")), Decision::Allow, "skip still skips everything else");
}

#[test]
fn a_look_inside_a_longer_command_keeps_the_commands_own_card() {
    let fx = Fixture::new("screen-compound");
    let p = policy();
    for mode in ["allowlist", "full", "ask"] {
        p.ui(&fx.at, &json!({ "op": "set_mode", "mode": mode })).unwrap();
        for (i, cmd) in ["buddy screen look && rm -rf ~/Documents", "sudo true; buddy screen look"].iter().enumerate() {
            let r = p.decide(&fx.at, &bash(&format!("{mode}{i}"), cmd));
            let a = asks(&r);
            assert_ne!(a["title"], SCREEN_CARD.0, "{mode} {cmd}");
            let detail = a["detail"].as_str().unwrap();
            assert!(detail.ends_with("It also looks at your screen."), "{mode} {cmd}: {detail}");
            if mode == "full" {
                // the riskier command is the reason, not the look
                assert!(!detail.contains("reads the screen"), "{mode} {cmd}: {detail}");
            }
            assert_eq!(a["command"], *cmd);
        }
    }
}

#[test]
fn a_tainted_conversation_asks_for_a_screen_look_with_no_always() {
    let fx = Fixture::new("screen-taint");
    let p = policy();
    p.taint(&fx.at, "browser", "https://example.com/");
    for (i, d) in [bash("t1", "buddy screen look"), tool("t2", "screen_look", json!({ "image": true }))].iter().enumerate() {
        let r = p.decide(&fx.at, d);
        let a = asks(&r);
        assert_eq!((a["title"].as_str(), a["detail"].as_str(), a["offerAlways"].clone()), (Some(SCREEN_CARD.0), Some(SCREEN_CARD.1), json!(false)), "{i}");
        assert!(a["suggestedRule"].is_null() || a["suggestedRule"] == "", "{i}: no rule to save while marked: {}", a["suggestedRule"]);
    }
    // a look from the screen itself marks it too, and the next one asks again
    let fx = Fixture::new("screen-taint2");
    let p = policy();
    assert!(p.taint(&fx.at, "screen", "screen look"));
    assert!(matches!(p.decide(&fx.at, &bash("t3", "buddy screen look")), Decision::Ask(_)));
}

#[test]
fn a_check_in_turn_never_looks_at_the_screen() {
    let fx = Fixture::new("screen-sense");
    let p = policy();
    let msg = "Screen look only runs when the person asked in chat, not during a check-in";
    // even with an Always rule, and in full access
    for mode in ["allowlist", "full", "ask"] {
        p.ui(&fx.at, &json!({ "op": "set_mode", "mode": mode })).unwrap();
        for (i, d) in [
            bash("c1", "buddy screen look"),
            bash("c2", "buddy screen look --image"),
            tool("c3", "screen_look", json!({})),
            tool("c4", "screen_look", json!({ "image": true })),
            bash("c5", "echo $(buddy screen look)"),
            bash("c6", "buddy  \"screen\"  look"),
            bash("c7", "eval buddy screen look"),
            bash("c8", "x=look; buddy screen $x"),
            bash("c9", "sh -c 'buddy screen look'"),
        ]
        .into_iter()
        .enumerate()
        {
            let r = p.decide(&fx.at, &from_check_in(d));
            assert_eq!(denied_with(&r), msg, "{mode} #{i}");
        }
        // ordinary check-in work is unchanged (ask mode asks for everything)
        if mode != "ask" {
            assert_eq!(p.decide(&fx.at, &from_check_in(bash("ok", "buddy say hi"))), Decision::Allow, "{mode}");
            assert_eq!(p.decide(&fx.at, &from_check_in(bash("ok2", "buddy say \"a screen look\""))), Decision::Allow, "{mode}");
            assert_eq!(p.decide(&fx.at, &from_check_in(bash("ok3", "buddy say \"take a look at your $TASK\""))), Decision::Allow, "{mode}");
        }
    }
    // in chat, the same call is only an ask
    assert!(matches!(p.decide(&fx.at, &bash("chat", "buddy screen look")), Decision::Ask(_)));
    let mut d = bash("chat2", "buddy screen look");
    d.trigger = json!({ "requestId": "u:0f8fad5b-d9cb-469f-a165-70867728950e", "source": "user" });
    assert!(matches!(p.decide(&fx.at, &d), Decision::Ask(_)));
}

#[test]
fn the_screen_look_token_scan_finds_the_command_however_it_is_spelled_and_nothing_else() {
    for yes in [
        "buddy screen look",
        "buddy screen look --image",
        "  buddy   screen   look  --json",
        "ls && buddy screen look",
        "FOO=1 buddy screen look",
        "env buddy screen look",
        "echo $(buddy screen look)",
        "echo `buddy screen look`",
        "sh -c 'buddy screen look'",
        "buddy 'screen' \"look\"",
        "buddy scr\\een look",
        "eval buddy screen look",
        "v=look; buddy screen ${v}",
        "b=screen; buddy $b look",
        "s=scr; l=lo; buddy ${s}een ${l}ok",
        "$(echo buddy) screen look",
        "$B screen look",
    ] {
        assert!(rules::screen_look(yes), "{yes}");
    }
    for no in ["buddy say \"take a look at your $TASK\"", "buddy say \"a screen look $X\"", "echo \"look at the screen $HOME\"", "cp $A $B $C", "buddy say screen look", "buddy status", "buddy mascot do read", "echo hello", "ls screen look", "buddy browser snapshot", "buddy screenshot", "look at the screen"] {
        assert!(!rules::screen_look(no), "{no}");
    }
}

// ------------------------------------------------------------ memory (I3)

fn memory_fixture(name: &str) -> (Fixture, PathBuf) {
    let mut fx = Fixture::new(name);
    let home = fx.root.join("home");
    let dir = home.join("memory");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("memory.sqlite"), "x").unwrap();
    fs::create_dir_all(home.join("memoryx")).unwrap();
    fs::write(home.join("memoryx").join("a.txt"), "x").unwrap();
    fx.at.home = Some(home);
    fx.at.private_dirs = vec![dir.clone()];
    (fx, dir)
}

fn denied_private(d: &Decision) -> bool {
    matches!(d, Decision::Deny { reason } if reason == "That folder holds what the bear remembers about the person: reach it only with buddy memory.")
}

#[test]
fn the_memory_folder_is_private_to_every_tool_and_shell_word_even_in_skip_mode() {
    let (fx, dir) = memory_fixture("private");
    let p = policy();
    p.ui(&fx.at, &json!({ "op": "set_mode", "mode": "skip" })).unwrap();
    let db = dir.join("memory.sqlite").display().to_string();
    let dirs = dir.display().to_string();
    for tool in ["read", "write", "edit", "look"] {
        for path in [db.as_str(), dirs.as_str(), "~/memory/memory.sqlite", "~/memory", "~/memory/../memory/x.db"] {
            let d = p.decide(&fx.at, &file("f", tool, path));
            assert!(denied_private(&d), "{tool} {path}: {d:?}");
        }
    }
    if cfg!(target_os = "macos") {
        for path in [db.to_uppercase(), dirs.to_uppercase(), "~/MEMORY/Memory.SQLite".to_string()] {
            let d = p.decide(&fx.at, &file("f", "read", &path));
            assert!(denied_private(&d), "{path}: {d:?}");
        }
    }
    for cmd in [format!("cat {db}"), format!("sqlite3 {db} .tables"), format!("X={db} env"), format!("cd {dirs} && ls"), format!("ls '{dirs}'"), "cat ~/memory/memory.sqlite".to_string()] {
        let d = p.decide(&fx.at, &bash("b", &cmd));
        assert!(denied_private(&d), "{cmd}: {d:?}");
    }
    // A sibling whose name only starts the same is not inside it.
    let sibling = fx.root.join("home").join("memoryx").join("a.txt").display().to_string();
    assert_eq!(p.decide(&fx.at, &file("s1", "read", &sibling)), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &file("s2", "read", "~/memoryx/a.txt")), Decision::Allow);
    assert_eq!(p.decide(&fx.at, &bash("s3", &format!("cat {sibling}"))), Decision::Allow);
    // Nothing changes without private dirs.
    let none = AgentCtx { private_dirs: vec![], ..fx.at.clone() };
    assert_eq!(p.decide(&none, &file("n1", "read", &db)), Decision::Allow);
    // A private dir that doesn't exist yet is still guarded.
    let later = fx.root.join("home").join("later");
    let ahead = AgentCtx { private_dirs: vec![later.clone()], ..fx.at.clone() };
    assert!(denied_private(&p.decide(&ahead, &file("a1", "write", &later.join("memory.sqlite").display().to_string()))));
}

#[test]
fn buddy_memory_reads_run_and_writes_ask_only_while_marked_and_never_offer_always() {
    let (fx, _) = memory_fixture("memory-verbs");
    let p = policy();
    let reads = ["buddy memory list", "buddy memory list --kind fact --json", "buddy memory search \"sister\" --limit 3", "buddy memory open --json", "buddy memory open", "buddy memory recall yesterday", "buddy memory recall 2026-11-26 --json"];
    let writes = ["buddy memory remember \"I live in Hanoi\" --key home_city", "buddy memory forget 3", "buddy memory forget --key people:sister"];
    for (i, cmd) in reads.iter().chain(writes.iter()).enumerate() {
        assert_eq!(p.decide(&fx.at, &bash(&format!("u{i}"), cmd)), Decision::Allow, "untainted {cmd}");
    }
    assert!(p.taint(&fx.at, "web_fetch", "https://example.com"));
    for (i, cmd) in reads.iter().enumerate() {
        assert_eq!(p.decide(&fx.at, &bash(&format!("r{i}"), cmd)), Decision::Allow, "tainted read {cmd}");
    }
    let spelled = [
        "buddy memory remember x",
        "buddy memory forget 3",
        "buddy memory remember \"I live in Hanoi\" --json",
        "buddy --json memory remember x",
        "buddy memory --json forget 3",
        "sh -c 'buddy memory remember x'",
        "bu''ddy memory remember x",
        "bu\\ddy mem\"\"ory for'g'et 3",
        "echo $(buddy memory forget 3)",
        "m=memory; buddy $m remember x",
        "eval buddy memory remember x",
        "env buddy memory forget 3",
    ];
    for (i, cmd) in spelled.iter().chain(writes.iter()).enumerate() {
        let d = p.decide(&fx.at, &bash(&format!("w{i}"), cmd));
        let a = asks(&d);
        assert_eq!(a["title"], "Let the bear change what it remembers?", "{cmd}");
        assert_eq!(a["detail"], "This conversation read a web page, so changing memory asks every time.", "{cmd}");
        assert_eq!(a["kind"], "command");
        assert_eq!(a["command"], *cmd);
        assert_eq!(a["offerAlways"], false, "{cmd}");
        assert!(a["suggestedRule"].is_null(), "{cmd}");
        p.ui(&fx.at, &json!({ "op": "approval_answer", "callId": format!("w{i}"), "answer": "deny" })).unwrap();
    }
    // full access pauses while marked, skip still asks for a memory write
    for mode in ["full", "skip"] {
        p.ui(&fx.at, &json!({ "op": "set_mode", "mode": mode })).unwrap();
        let d = p.decide(&fx.at, &bash(&format!("m-{mode}"), "buddy memory remember x"));
        assert_eq!(asks(&d)["title"], "Let the bear change what it remembers?", "{mode}");
        assert_eq!(p.decide(&fx.at, &bash(&format!("l-{mode}"), "buddy memory list")), Decision::Allow, "{mode}");
    }
    // d12 is a default rule, the twelfth
    let rules = p.view(&fx.at)["rules"].as_array().unwrap().clone();
    assert_eq!(rules.iter().filter(|r| r["origin"] == "default").count(), 12);
    assert!(rules.iter().any(|r| r["id"] == "d12" && r["effect"] == "allow" && r["pattern"] == "Shell(buddy memory:*)"));
}
