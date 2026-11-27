use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

use super::jobs::{reflect_once, run_once, Ran, REFLECT_IDLE};
use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("buddy-memory-{name}-{}-{}", std::process::id(), host::random_hex(3)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A memory at `<tmp>/memory` that counts its `memory://state` notices.
fn mem(name: &str, on: bool) -> (Arc<Mem>, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let m = Mem::new(dir(&tmp(name)), on, Box::new(move |_| {
        n2.fetch_add(1, Ordering::SeqCst);
    }));
    (Arc::new(m), n)
}

/// The agent seam: records the sections it got and answers extractions from a script.
#[derive(Default)]
struct Fake {
    sections: Mutex<Vec<Option<String>>>,
    asked: Mutex<Vec<Value>>,
    reply: Mutex<Option<Result<Value, String>>>,
    reflected: Mutex<Vec<Value>>,
    reflect_reply: Mutex<Option<Result<Value, String>>>,
    down: AtomicBool,
}

impl Sidecar for Fake {
    fn set_memory(&self, section: Option<String>) {
        self.sections.lock().unwrap().push(section);
    }
    fn extract(&self, args: Value) -> Result<Value, String> {
        self.asked.lock().unwrap().push(args);
        self.reply.lock().unwrap().clone().unwrap_or_else(|| Err("timeout".into()))
    }
    fn reflect(&self, args: Value) -> Result<Value, String> {
        self.reflected.lock().unwrap().push(args);
        self.reflect_reply.lock().unwrap().clone().unwrap_or_else(|| Err("timeout".into()))
    }
    fn running(&self) -> bool {
        !self.down.load(Ordering::SeqCst)
    }
}

fn status(busy: bool, turn: Option<&str>, failure: bool) -> Value {
    let mut s = json!({"busy": busy, "phase": if busy { "thinking" } else { "idle" }, "tools": [], "asking": []});
    if let Some(t) = turn {
        s["turn"] = json!(t);
    }
    if failure {
        s["failure"] = json!({"reason": "model_error"});
    }
    s
}

/// One chat turn `id` running and ending well on run `gen`.
fn turn(m: &Mem, gen: u64, id: &str) {
    m.on_status(gen, &status(true, Some(id), false));
    m.on_status(gen, &status(true, None, false));
    m.on_status(gen, &status(false, None, false));
}

fn pending(m: &Mem) -> Vec<String> {
    let Some(args) = m.store().unwrap().extract_args(10, now().today) else { return vec![] };
    args["turns"].as_array().unwrap().iter().map(|t| t["requestId"].as_str().unwrap().to_string()).collect()
}

fn texts(state: &Value, group: &str) -> Vec<String> {
    state["groups"][group].as_array().unwrap().iter().map(|r| r["text"].as_str().unwrap().to_string()).collect()
}

// ---- the window's commands ----

#[test]
fn only_the_memory_window_may_call_its_commands() {
    assert_eq!(window::mine("memory", "memory_state"), Ok(()));
    for other in ["persona", "bubble", "signin", "studio", "widget-media", ""] {
        assert_eq!(window::mine(other, "memory_forget_all"), Err("not allowed".to_string()), "{other}");
    }
}

#[test]
fn the_page_calls_its_commands_words_their_refusals_and_keeps_to_the_csp() {
    let js = include_str!("../../web/memory/memory.js");
    let html = include_str!("../../web/memory/index.html");
    for cmd in ["memory_state", "memory_history", "memory_add", "memory_edit", "memory_forget", "memory_resolve", "memory_forget_all", "memory_set_enabled", "memory_close"] {
        assert!(js.contains(&format!("\"{cmd}\"")), "the page never calls {cmd}");
    }
    assert!(js.contains(&format!("\"{}\"", window::EVENT)), "the page doesn't listen for {}", window::EVENT);
    for code in ["secret", "empty", "too_long", "not_found", "bad_phrase"] {
        assert!(js.contains(code), "the page has no words for `{code}`");
    }
    // CSP `script-src 'self'; style-src 'self'`: no inline script or style
    assert!(!html.contains("<script>") && !html.contains("style=") && !html.contains("<style"), "inline script or style in index.html");
    assert!(!js.contains("innerHTML"), "text goes in with textContent");
}

#[test]
fn window_errors_are_the_fixed_strings() {
    let (m, _) = mem("errors", true);
    assert_eq!(m.add("profile", "   "), Err("empty".into()));
    assert_eq!(m.add("profile", &"x".repeat(201)), Err("too_long".into()));
    // a key-shaped string, built here
    let key = format!("{}{}", "sk-", "Q7".repeat(12));
    assert_eq!(m.add("preference", &format!("my key is {key}")), Err("secret".into()));
    assert_eq!(m.add("hobby", "Likes kites"), Err("store".into()), "an unknown kind");
    assert_eq!(m.edit(999, "Lives in Saigon", None), Err("not_found".into()));
    assert_eq!(m.forget(999, false), Err("not_found".into()));
    assert_eq!(m.forget(999, true), Err("not_found".into()));
    assert_eq!(m.resolve(999, "new"), Err("not_found".into()));
    assert_eq!(m.resolve(999, "both"), Err("store".into()));
    assert_eq!(m.forget_all("forget everythin"), Err("bad_phrase".into()));
    assert_eq!(m.forget_all("Forget everything"), Err("bad_phrase".into()), "exact phrase");
    assert_eq!(werr(&Error::Newer), "store");
    assert_eq!(werr(&Error::Usage("x".into())), "store");
}

#[test]
fn add_edit_forget_and_forget_all_reach_the_state_the_file_and_the_agent() {
    let (m, notices) = mem("crud", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    assert_eq!(m.initial_section(), None, "nothing stored yet");
    let id = m.add("profile", "Has a cat, Mochi").unwrap()["id"].as_i64().unwrap();
    m.add("preference", "Short replies").unwrap();
    let st = m.state();
    assert_eq!((st["v"].as_u64(), st["enabled"].as_bool(), st["error"].is_null(), st["phrase"].as_str()), (Some(1), Some(true), true, Some(PHRASE)));
    assert_eq!(texts(&st, "about"), ["Has a cat, Mochi"]);
    assert_eq!(texts(&st, "help"), ["Short replies"]);
    assert_eq!(st["groups"]["about"][0]["source"], "window");
    let md = std::fs::read_to_string(m.dir.join("MEMORY.md")).unwrap();
    assert!(md.contains("Has a cat, Mochi"), "MEMORY.md re-rendered");
    let last = fake.sections.lock().unwrap().last().cloned().flatten().unwrap();
    assert!(last.contains("Has a cat, Mochi") && last.contains("Short replies"));
    // an edit: the new row replaces it
    let new = m.edit(id, "Has two cats, Mochi and Lan", None).unwrap()["id"].as_i64().unwrap();
    assert_ne!(new, id);
    assert_eq!(texts(&m.state(), "about"), ["Has two cats, Mochi and Lan"]);
    // soft forget, then erase
    m.forget(new, false).unwrap();
    assert!(texts(&m.state(), "about").is_empty());
    let pid = m.state()["groups"]["help"][0]["id"].as_i64().unwrap();
    m.forget(pid, true).unwrap();
    assert_eq!(m.state()["counts"]["live"], 0);
    assert_eq!(fake.sections.lock().unwrap().last().cloned().unwrap(), None, "an empty section");
    // forget everything: a fresh store
    m.add("promise", "Ask how Mochi's vet visit went").unwrap();
    let sent = fake.sections.lock().unwrap().len();
    m.forget_all("forget everything").unwrap();
    assert_eq!(m.state()["counts"]["live"], 0);
    assert!(fake.sections.lock().unwrap().len() > sent, "the agent forgot it too");
    assert_eq!(fake.sections.lock().unwrap().last().cloned().unwrap(), None);
    assert!(notices.load(Ordering::SeqCst) >= 7, "a memory://state per change");
}

#[test]
fn an_unchanged_section_is_not_sent_again() {
    let (m, _) = mem("rev", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    m.add("profile", "Lives in Saigon").unwrap();
    let n = fake.sections.lock().unwrap().len();
    m.sync_agent();
    m.changed();
    assert_eq!(fake.sections.lock().unwrap().len(), n);
}

// ---- off (C9) ----

#[test]
fn off_records_no_pending_answers_409_and_sends_no_section() {
    let (m, _) = mem("off", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    m.add("profile", "Lives in Saigon").unwrap();
    turn(&m, 1, "u:one");
    assert_eq!(pending(&m), ["u:one"]);
    // switched off: the waiting turn goes, the agent's section empties, new turns aren't kept
    m.set_enabled(false);
    assert!(pending(&m).is_empty());
    assert_eq!(fake.sections.lock().unwrap().last().cloned().unwrap(), None);
    turn(&m, 1, "u:two");
    assert!(pending(&m).is_empty());
    for op in [MemoryOp::List { kind: None }, MemoryOp::Search { q: "Saigon".into(), limit: 5 }, MemoryOp::Remember { text: "Likes tea".into(), key: None, qualifier: None, value: None, kind: None, importance: None, from: None }] {
        let r = m.on_op(op).unwrap_err();
        assert_eq!((r.status, r.code), (409, "memory_off"));
    }
    // forget still works, and the window still shows what is kept
    let st = m.state();
    assert_eq!((st["enabled"].as_bool(), texts(&st, "about").len()), (Some(false), 1));
    let id = st["groups"]["about"][0]["id"].as_i64().unwrap();
    assert_eq!(m.on_op(MemoryOp::Forget { id: Some(id), key: None, qualifier: None }).unwrap()["forgotten"], json!([id]));
    assert_eq!(m.on_op(MemoryOp::Forget { id: Some(id), key: None, qualifier: None }).unwrap_err().status, 404);
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Waiting("off"));
    // on again: the section comes back
    m.add("profile", "Works as a software engineer").unwrap();
    m.set_enabled(true);
    assert!(fake.sections.lock().unwrap().last().cloned().flatten().unwrap().contains("software engineer"));
}

#[test]
fn a_memory_started_off_still_opens_for_the_window_and_clears_waiting_turns() {
    let d = tmp("startoff");
    {
        let s = Store::open(&dir(&d)).unwrap();
        s.add_pending("u:old", "2026-11-27T09:00:00+07:00").unwrap();
    }
    let m = Mem::new(dir(&d), false, Box::new(|_| {}));
    assert!(m.state()["error"].is_null());
    assert!(m.store().unwrap().extract_args(10, now().today).is_none(), "cleared when opened off");
}

// ---- the host routes ----

#[test]
fn the_routes_remember_list_search_and_forget() {
    let (m, _) = mem("routes", true);
    let r = m.on_op(MemoryOp::Remember { text: "Lives in Saigon".into(), key: Some("home_city".into()), qualifier: None, value: Some("Saigon".into()), kind: None, importance: None, from: None }).unwrap();
    assert_eq!(r["outcome"], "created");
    let id = r["id"].as_i64().unwrap();
    let list = m.on_op(MemoryOp::List { kind: None }).unwrap();
    assert_eq!((list["facts"][0]["id"].as_i64(), list["facts"][0]["source"].as_str()), (Some(id), Some("bear")));
    assert_eq!(m.on_op(MemoryOp::Search { q: "saigon".into(), limit: 5 }).unwrap()["facts"].as_array().unwrap().len(), 1);
    let key = format!("{}{}", "ghp_", "Zx9".repeat(12));
    let bad = m.on_op(MemoryOp::Remember { text: format!("token {key}"), key: None, qualifier: None, value: None, kind: None, importance: None, from: None }).unwrap_err();
    assert_eq!((bad.status, bad.code), (400, "secret"));
    assert_eq!(m.on_op(MemoryOp::List { kind: Some("hobby".into()) }).unwrap_err().code, "usage");
    // a remember that isn't kept says why, with no id
    let ep = m.on_op(MemoryOp::Remember { text: "Went hiking on Sunday".into(), key: None, qualifier: None, value: None, kind: Some("episode".into()), importance: None, from: None }).unwrap();
    assert_eq!(ep, json!({"id": null, "outcome": "skipped", "reason": "episode"}));
    assert_eq!(m.on_op(MemoryOp::Forget { id: None, key: Some("home_city".into()), qualifier: None }).unwrap()["forgotten"], json!([id]));
    assert_eq!(m.on_op(MemoryOp::Forget { id: None, key: Some("home_city".into()), qualifier: None }).unwrap_err().code, "not_found");
}

// ---- a store that won't open (C11) ----

#[test]
fn an_unwritable_store_dir_is_the_error_state_and_503_and_the_app_goes_on() {
    let d = tmp("broken");
    // a file where the folder should be: the store can't be made
    std::fs::write(dir(&d), "not a folder").unwrap();
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let m = Mem::new(dir(&d), true, Box::new(move |_| {
        n2.fetch_add(1, Ordering::SeqCst);
    }));
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    assert_eq!(m.initial_section(), None);
    let st = m.state();
    assert_eq!(st["error"]["code"], "store");
    assert!(st["error"]["file"].as_str().unwrap().ends_with("memory/memory.sqlite"));
    assert_eq!((st["enabled"].as_bool(), st["counts"]["live"].as_u64(), st["phrase"].as_str()), (Some(true), Some(0), Some(PHRASE)));
    for op in [MemoryOp::List { kind: None }, MemoryOp::Forget { id: Some(1), key: None, qualifier: None }] {
        let r = m.on_op(op).unwrap_err();
        assert_eq!((r.status, r.code), (503, "no_memory"));
    }
    assert_eq!(m.add("profile", "Lives in Saigon"), Err("store".into()));
    // turns aren't kept, the jobs wait, nothing panics
    turn(&m, 1, "u:x");
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Waiting("no store"));
    // Forget everything still tries (and says store while the folder is blocked)
    assert_eq!(m.forget_all("forget everything"), Err("store".into()));
    // the folder is fixed: the next look after RETRY opens it, and the agent gets its (empty) state
    std::fs::remove_file(dir(&d)).unwrap();
    assert_eq!(m.state()["error"]["code"], "store", "not before 5 s");
    m.lock().tried = Some(Instant::now() - RETRY);
    let st = m.state();
    assert!(st["error"].is_null(), "{st}");
    m.add("profile", "Lives in Saigon").unwrap();
    assert!(fake.sections.lock().unwrap().last().cloned().flatten().unwrap().contains("Saigon"));
}

#[test]
fn a_folder_without_permissions_opens_once_they_come_back() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("chmod");
    let md = dir(&d);
    std::fs::create_dir_all(&md).unwrap();
    std::fs::set_permissions(&md, std::fs::Permissions::from_mode(0o000)).unwrap();
    let m = Mem::new(md.clone(), true, Box::new(|_| {}));
    assert_eq!(m.state()["error"]["code"], "store");
    assert_eq!(m.on_op(MemoryOp::List { kind: None }).unwrap_err().status, 503);
    std::fs::set_permissions(&md, std::fs::Permissions::from_mode(0o755)).unwrap();
    m.lock().tried = Some(Instant::now() - RETRY);
    // the CLI's next call opens it, not only the window's
    assert!(m.on_op(MemoryOp::List { kind: None }).is_ok());
    assert!(m.state()["error"].is_null());
}

#[test]
fn forget_everything_recovers_a_corrupt_file() {
    let d = tmp("corrupt");
    std::fs::create_dir_all(dir(&d)).unwrap();
    std::fs::write(dir(&d).join("memory.sqlite"), "this is not a database, just text that fills a page or two ".repeat(80)).unwrap();
    let m = Mem::new(dir(&d), true, Box::new(|_| {}));
    assert_eq!(m.state()["error"]["code"], "store");
    m.forget_all("forget everything").unwrap();
    assert!(m.state()["error"].is_null());
    m.add("profile", "Lives in Saigon").unwrap();
}

// ---- the pending feed ----

#[test]
fn the_feed_keeps_chat_turns_that_ended_well_only() {
    let mut f = Feed::default();
    // a chat turn: busy with its id, then idle (the idle record has no turn)
    assert_eq!(f.step(1, &status(true, Some("u:a"), false)), None);
    assert_eq!(f.step(1, &status(true, None, false)), None, "a later busy record without turn keeps it");
    assert_eq!(f.step(1, &status(false, None, false)), Some("u:a".into()));
    assert_eq!(f.step(1, &status(false, None, false)), None, "idle again is nothing new");
    // a failed turn, a sense turn, a continue and a retry: none
    for (id, failure) in [("u:b", true), ("s:c", false), ("c:d", false), ("r:e", false)] {
        f.step(1, &status(true, Some(id), false));
        assert_eq!(f.step(1, &status(false, None, failure)), None, "{id}");
    }
    // the sidecar restarted mid-turn: the new run's first idle isn't the old turn's end
    f.step(1, &status(true, Some("u:f"), false));
    assert_eq!(f.step(2, &status(false, None, false)), None);
    assert!(!f.busy);
}

#[test]
fn the_feed_marks_busy_and_adds_pending_only_while_on() {
    let (m, _) = mem("feed", true);
    m.on_status(1, &status(true, Some("u:a"), false));
    assert!(m.busy());
    m.on_status(1, &status(false, None, false));
    assert!(!m.busy());
    m.on_status(1, &status(true, Some("s:b"), false));
    m.on_status(1, &status(false, None, false));
    assert_eq!(pending(&m), ["u:a"]);
}

// ---- the jobs ----

fn reply_for(args: &Value, text: &str) -> Value {
    let id = args["turns"][0]["requestId"].as_str().unwrap();
    json!({
        "items": [{"turn": id, "kind": "profile", "predicate": "home_city", "qualifier": null, "value": "Saigon",
                   "text": text, "importance": 6, "validFrom": null, "quote": "I live in Saigon"}],
        "turns": [{"requestId": id, "said": "I live in Saigon"}],
        "missing": [],
    })
}

#[test]
fn the_jobs_wait_for_the_gate_then_extract_a_batch() {
    let (m, notices) = mem("jobs", true);
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Waiting("no agent"));
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    fake.down.store(true, Ordering::SeqCst);
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Waiting("no agent"));
    fake.down.store(false, Ordering::SeqCst);
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Waiting("nothing pending"));
    turn(&m, 1, "u:a");
    // a turn running: wait
    m.on_status(1, &status(true, Some("u:b"), false));
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Waiting("busy"));
    m.on_status(1, &status(false, None, false));
    // the chat was just now: wait for the idle time
    m.chat_sent();
    assert_eq!(run_once(&m, Duration::from_secs(300)), Ran::Waiting("chatting"));
    assert!(fake.asked.lock().unwrap().is_empty());
    // idle: one batch with both turns
    let args = m.store().unwrap().extract_args(10, now().today).unwrap();
    *fake.reply.lock().unwrap() = Some(Ok(reply_for(&args, "Lives in Saigon")));
    let before = notices.load(Ordering::SeqCst);
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Extracted { turns: 2, items: 1 });
    assert_eq!(fake.asked.lock().unwrap()[0]["turns"].as_array().unwrap().len(), 2);
    assert!(pending(&m).is_empty(), "both turns cleared");
    assert_eq!(texts(&m.state(), "about"), ["Lives in Saigon"]);
    assert_eq!(m.state()["groups"]["about"][0]["source"], "chat");
    assert!(fake.sections.lock().unwrap().last().cloned().flatten().unwrap().contains("Lives in Saigon"));
    assert!(notices.load(Ordering::SeqCst) > before, "memory://state");
}

#[test]
fn ten_pending_turns_skip_the_idle_wait_and_a_batch_is_ten() {
    let (m, _) = mem("ten", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    for i in 0..12 {
        turn(&m, 1, &format!("u:t{i:02}"));
    }
    m.chat_sent();
    *fake.reply.lock().unwrap() = Some(Ok(json!({"items": [], "turns": [], "missing": []})));
    assert_eq!(run_once(&m, Duration::from_secs(300)), Ran::Extracted { turns: 10, items: 0 });
    assert_eq!(pending(&m).len(), 2);
    assert_eq!(run_once(&m, Duration::from_secs(300)), Ran::Waiting("chatting"), "2 left: the idle wait again");
}

#[test]
fn a_failing_extractor_drops_a_turn_after_three_tries() {
    let (m, _) = mem("fail", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    turn(&m, 1, "u:a");
    *fake.reply.lock().unwrap() = Some(Err("timeout".into()));
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Failed { turns: 1, dropped: vec![] });
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Failed { turns: 1, dropped: vec![] });
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Failed { turns: 1, dropped: vec!["u:a".into()] });
    assert!(pending(&m).is_empty());
    // a reply that isn't one (no items) counts as a failure too
    turn(&m, 1, "u:b");
    *fake.reply.lock().unwrap() = Some(Ok(json!({"oops": true})));
    assert_eq!(run_once(&m, Duration::ZERO), Ran::Failed { turns: 1, dropped: vec![] });
    assert_eq!(pending(&m), ["u:b"]);
}

/// A9: the module's log lines carry ids, counts and codes, never a fact's text, a quote or `said`.
#[test]
fn no_log_line_here_formats_text_quotes_or_said() {
    let src = [include_str!("mod.rs"), include_str!("window.rs"), include_str!("jobs.rs")];
    for file in src {
        for line in file.lines().filter(|l| l.contains("eprintln!")) {
            for bad in ["text", "quote", "said", "{v", "{op", "{args", "{reply", "{r}", "{st"] {
                let args = line.split_once("eprintln!").unwrap().1;
                assert!(!args.contains(bad), "a log line formats {bad:?}: {line}");
            }
        }
    }
}

/// The replay of a turn that opens the window with `buddy memory open` parses, in time order.
#[test]
fn the_memory_fixture_opens_the_window_with_buddy_memory_open() {
    use mascot_protocol::MascotEvent;
    let items = crate::events::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory.jsonl")).unwrap();
    assert!(items.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
    let open = items.iter().position(|i| matches!(&i.event, Some(MascotEvent::Working { tool, .. }) if tool == "buddy memory open"));
    let idle = items.iter().position(|i| i.event == Some(MascotEvent::Idle));
    assert!(open.is_some() && open < idle, "the tool runs during the turn");
}

// ---- history (p2): episodes, recall, reflection, the window's older days ----

/// `n` moments from one chat turn, through the jobs' extraction as in the app.
fn moments(m: &Mem, fake: &Fake, id: &str, texts: &[&str]) {
    turn(m, 1, id);
    let items: Vec<Value> = texts
        .iter()
        .map(|t| json!({"turn": id, "kind": "episode", "predicate": null, "qualifier": null, "value": null, "text": t, "importance": 5, "validFrom": null, "quote": "launch week"}))
        .collect();
    *fake.reply.lock().unwrap() = Some(Ok(json!({"items": items, "turns": [{"requestId": id, "said": "launch week"}], "missing": []})));
    assert_eq!(run_once(m, Duration::ZERO), Ran::Extracted { turns: 1, items: texts.len() });
}

const LAUNCH: [&str; 4] = ["Stayed late for the launch", "Skipped lunch to fix the build", "Walked Mochi after the late shift", "Had ramen with Lan"];

fn insight(text: &str, sources: &[i64]) -> Value {
    json!({"text": text, "importance": 6, "sources": sources})
}

fn episode_ids(fake: &Fake) -> Vec<i64> {
    fake.reflected.lock().unwrap().last().unwrap()["episodes"].as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect()
}

#[test]
fn reflection_waits_for_the_gate_then_a_failed_run_is_retried_as_the_same_batch() {
    let (m, notices) = mem("reflect", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    assert_eq!(reflect_once(&m, REFLECT_IDLE), Ran::Waiting("not due"), "no episode yet");
    moments(&m, &fake, "u:launch", &LAUNCH);
    // the person is at the keyboard, a turn runs, the agent is down, memory is off: wait
    assert_eq!(reflect_once(&m, REFLECT_IDLE - Duration::from_secs(1)), Ran::Waiting("person here"));
    m.on_status(1, &status(true, Some("u:b"), false));
    assert_eq!(reflect_once(&m, REFLECT_IDLE), Ran::Waiting("busy"));
    m.on_status(1, &status(false, None, false));
    fake.down.store(true, Ordering::SeqCst);
    assert_eq!(reflect_once(&m, REFLECT_IDLE), Ran::Waiting("no agent"));
    fake.down.store(false, Ordering::SeqCst);
    // a chat turn still waiting for the extractor holds it too
    turn(&m, 1, "u:waiting");
    assert_eq!(reflect_once(&m, REFLECT_IDLE), Ran::Waiting("not due"));
    m.store().unwrap().clear_pending().unwrap();
    assert!(fake.reflected.lock().unwrap().is_empty());

    // the model call fails: nothing applied, the run is failed
    let Ran::Reflected { batch8, outcome, .. } = reflect_once(&m, REFLECT_IDLE) else { panic!("a run") };
    assert_eq!((batch8.len(), outcome), (8, "failed"));
    let ids = episode_ids(&fake);
    assert_eq!(ids.len(), 4);
    assert!(m.store().unwrap().list(Some("insight")).unwrap().is_empty());
    // the next look sends the same batch, and this time it lands
    *fake.reflect_reply.lock().unwrap() = Some(Ok(json!({"updates": [], "insights": [insight("Works late in launch weeks", &ids[..2])], "merges": []})));
    let before = notices.load(Ordering::SeqCst);
    assert_eq!(reflect_once(&m, REFLECT_IDLE), Ran::Reflected { batch8: batch8.clone(), outcome: "done", pct: 25 });
    assert_eq!(episode_ids(&fake), ids);
    assert!(notices.load(Ordering::SeqCst) > before, "memory://state");
    let st = m.state();
    let day = &st["history"][0];
    assert!(day["items"].as_array().unwrap().iter().any(|r| r["kind"] == "insight" && r["sources"] == json!(2)), "{day}");
    // nothing new: no run, and no duplicate insight
    assert_eq!(reflect_once(&m, REFLECT_IDLE), Ran::Waiting("not due"));
    assert_eq!(m.store().unwrap().list(Some("insight")).unwrap().len(), 1);
    // off: nothing runs
    m.set_enabled(false);
    assert_eq!(reflect_once(&m, REFLECT_IDLE), Ran::Waiting("off"));
}

/// A14 in the app: a reply that changes more than 30% of the live rows is rejected and nothing lands.
#[test]
fn a_reflection_over_the_30_percent_guard_is_rejected() {
    let (m, _) = mem("guard", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    moments(&m, &fake, "u:launch", &LAUNCH);
    moments(&m, &fake, "u:more", &["Fixed the build at midnight", "Slept in on Saturday"]);
    let reply = |ids: &[i64]| json!({"updates": [], "insights": [insight("Works late in launch weeks", ids), insight("Sleeps in after a hard week", ids), insight("Eats out when busy", ids)], "merges": []});
    *fake.reflect_reply.lock().unwrap() = Some(Err("timeout".into()));
    assert!(matches!(reflect_once(&m, REFLECT_IDLE), Ran::Reflected { outcome: "failed", .. }));
    let ids = episode_ids(&fake);
    *fake.reflect_reply.lock().unwrap() = Some(Ok(reply(&ids)));
    // 3 insights over 6 live rows: 50%
    let Ran::Reflected { outcome, pct, .. } = reflect_once(&m, REFLECT_IDLE) else { panic!("a run") };
    assert_eq!((outcome, pct), ("rejected", 50));
    assert!(m.store().unwrap().list(Some("insight")).unwrap().is_empty(), "nothing applied");
}

/// A model that was never asked (signed out, the sidecar down) gives the try back: the batch outlives
/// MAX_TRIES of those, while unusable replies use it up.
#[test]
fn a_reflection_the_model_never_saw_does_not_use_up_its_batch() {
    let (m, _) = mem("unspent", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    moments(&m, &fake, "u:launch", &LAUNCH);
    for _ in 0..memory::MAX_TRIES + 2 {
        *fake.reflect_reply.lock().unwrap() = Some(Err("no_provider".into()));
        assert!(matches!(reflect_once(&m, REFLECT_IDLE), Ran::Reflected { outcome: "failed", .. }));
    }
    let ids = episode_ids(&fake);
    *fake.reflect_reply.lock().unwrap() = Some(Ok(json!({"updates": [], "insights": [insight("Works late in launch weeks", &ids[..2])], "merges": []})));
    assert!(matches!(reflect_once(&m, REFLECT_IDLE), Ran::Reflected { outcome: "done", .. }));
    assert_eq!(episode_ids(&fake), ids, "the same batch");
    assert_eq!(m.store().unwrap().list(Some("insight")).unwrap().len(), 1);
}

#[test]
fn buddy_memory_recall_answers_a_day_and_409_while_off() {
    let (m, _) = mem("recall-route", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    moments(&m, &fake, "u:launch", &LAUNCH[..2]);
    let today = now().today.to_string();
    let r = m.on_op(MemoryOp::Recall { date: "today".into() }).unwrap();
    assert_eq!(r["day"], json!(today));
    let facts = r["facts"].as_array().unwrap();
    assert_eq!(facts.iter().map(|f| (f["text"].as_str().unwrap(), f["day"].as_str().unwrap())).collect::<Vec<_>>(), vec![(LAUNCH[0], today.as_str()), (LAUNCH[1], today.as_str())]);
    assert_eq!(m.on_op(MemoryOp::Recall { date: "yesterday".into() }).unwrap()["facts"], json!([]));
    let week = m.on_op(MemoryOp::Recall { date: "last-week".into() }).unwrap();
    assert!(week["day"].as_str().unwrap() < today.as_str() && week["facts"] == json!([]), "{week}");
    assert!(week["to"].as_str().unwrap() < today.as_str() && week["to"].as_str() > week["day"].as_str(), "{week}");
    assert!(r.get("to").is_none(), "one day has no `to`");
    assert_eq!(m.on_op(MemoryOp::Recall { date: "someday".into() }).unwrap_err().code, "usage");
    // chat recall: a message about the day gets its lines, an unrelated one none
    let lines = m.recall_for("What did we talk about today?");
    assert!(lines.iter().any(|l| l == &format!("{today}: {}", LAUNCH[0])), "{lines:?}");
    assert!(m.recall_for("Can you open the music widget").is_empty());
    m.set_enabled(false);
    let r = m.on_op(MemoryOp::Recall { date: "today".into() }).unwrap_err();
    assert_eq!((r.status, r.code), (409, "memory_off"));
    assert!(m.recall_for("What did we talk about today?").is_empty(), "off: no recall");
}

#[test]
fn the_window_pages_older_days_and_words_a_bad_day_as_store() {
    let (m, _) = mem("history", true);
    let fake = Arc::new(Fake::default());
    m.attach(fake.clone());
    moments(&m, &fake, "u:launch", &LAUNCH[..1]);
    let st = m.state();
    assert_eq!((st["history"].as_array().unwrap().len(), st["historyMore"].as_bool(), st["counts"]["days"].as_i64()), (1, Some(false), Some(1)));
    let tomorrow = now().today.tomorrow().unwrap().to_string();
    let page = m.history(&tomorrow).unwrap();
    assert_eq!((page["days"].as_array().unwrap().len(), page["more"].as_bool()), (1, Some(false)));
    assert_eq!(m.history(&now().today.to_string()).unwrap()["days"], json!([]), "strictly before");
    assert_eq!(m.history("someday"), Err("store".to_string()));
    // the fallback state (no store) has the same keys
    let (closed, _) = mem("history-off", false);
    let fb = closed.state();
    assert!(fb.get("history").is_some() && fb.get("historyMore").is_some() && fb["counts"].get("days").is_some(), "{fb}");
}

/// The replay of a turn that runs `buddy memory recall yesterday` parses, in time order.
#[test]
fn the_memory_history_fixture_recalls_yesterday_during_the_turn() {
    use mascot_protocol::MascotEvent;
    let items = crate::events::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory_history.jsonl")).unwrap();
    assert!(items.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
    let recall = items.iter().position(|i| matches!(&i.event, Some(MascotEvent::Working { tool, .. }) if tool == "buddy memory recall yesterday"));
    let idle = items.iter().position(|i| i.event == Some(MascotEvent::Idle));
    assert!(recall.is_some() && recall < idle, "the tool runs during the turn");
}
