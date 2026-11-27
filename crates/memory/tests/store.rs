mod common;
use common::*;
use memory::{Error, Outcome, RememberIn, Store};
use serde_json::json;
use std::sync::Arc;

fn said_city(s: &Store, v: &str, now: &memory::Now) -> i64 {
    city(s, v, 3, now)
}

fn guess_city(s: &Store, v: &str) -> Outcome {
    extract(s, "x", vec![item("profile", Some("home_city"), Some(v), &format!("Lives in {v}"))], &now()).items[0].outcome.clone()
}

// ---- open, files, versions ----

#[test]
fn open_creates_a_wal_store_at_user_version_1() {
    let tmp = Tmp::new();
    let dir = tmp.0.join("a").join("memory");
    let s = Store::open(&dir).unwrap();
    assert!(!s.is_newer());
    assert_eq!(s.dir(), dir);
    let c = db(&dir);
    assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(c.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0)).unwrap(), "wal");
    for t in ["facts", "pending_turns", "reflections", "insight_sources", "meta"] {
        assert_eq!(count(&dir, &format!("SELECT count(*) FROM sqlite_master WHERE name = '{t}'")), 1, "{t}");
    }
    assert_eq!(count(&dir, "SELECT count(*) FROM pragma_table_info('pending_turns') WHERE name = 'tries'"), 1);
    // Reopening keeps the rows.
    s.add("profile", "Likes tea", &now()).unwrap();
    drop(s);
    assert_eq!(Store::open(&dir).unwrap().list(None).unwrap().len(), 1);
}

#[test]
fn an_uncreatable_dir_is_a_store_error() {
    let tmp = Tmp::new();
    let file = tmp.0.join("file");
    std::fs::write(&file, "x").unwrap();
    assert!(matches!(Store::open(&file.join("memory")), Err(Error::Store(_))));
}

#[test]
fn the_store_is_send_sync_and_shared_across_threads() {
    fn needs<T: Send + Sync>() {}
    needs::<Store>();
    let (_t, s) = store();
    let s = Arc::new(s);
    let hs: Vec<_> = (0..4)
        .map(|t| {
            let s = s.clone();
            std::thread::spawn(move || {
                for i in 0..10 {
                    s.add("preference", &format!("Likes thing {t}-{i}"), &now()).unwrap();
                }
            })
        })
        .collect();
    hs.into_iter().for_each(|h| h.join().unwrap());
    assert_eq!(s.list(None).unwrap().len(), 40);
}

fn newer_store(tmp: &Tmp) -> Store {
    let s = Store::open(&tmp.mem()).unwrap();
    s.add("profile", "Likes tea", &now()).unwrap();
    drop(s);
    db(&tmp.mem()).pragma_update(None, "user_version", 7).unwrap();
    Store::open(&tmp.mem()).unwrap()
}

#[test]
fn a_newer_file_opens_read_only() {
    let tmp = Tmp::new();
    let s = newer_store(&tmp);
    assert!(s.is_newer());
    // reads still work
    assert_eq!(s.view(date(2026, 11, 27))["counts"]["live"], 1);
    assert_eq!(s.list(None).unwrap().len(), 1);
    assert_eq!(s.search("tea", 5).unwrap().len(), 1);
    // the prompt carries nothing and nothing is extracted
    assert_eq!(s.section(), "");
    assert!(s.extract_args(10, now().today).is_none());
    // every write is Newer
    let op = RememberIn { text: "Likes oat milk".into(), ..Default::default() };
    assert_eq!(s.remember(op, &now()), Err(Error::Newer));
    assert_eq!(s.add("profile", "x", &now()), Err(Error::Newer));
    assert_eq!(s.edit(1, "x", None, &now()), Err(Error::Newer));
    assert_eq!(s.soft_forget(1), Err(Error::Newer));
    assert_eq!(s.erase(1), Err(Error::Newer));
    assert_eq!(s.forget(Some(1), None, None), Err(Error::Newer));
    assert_eq!(s.resolve(1, true, &now()), Err(Error::Newer));
    assert_eq!(s.add_pending("u:a", "2026-11-27T09:00:00+07:00"), Err(Error::Newer));
    assert_eq!(s.clear_pending(), Err(Error::Newer));
    assert_eq!(s.apply_extract(&json!({"turns": []}), &json!({"items": []}), &now()), Err(Error::Newer));
    assert_eq!(s.extract_failed(&json!({"turns": []})), Err(Error::Newer));
    assert_eq!(s.write_files(), Err(Error::Newer));
    drop(s);
    assert_eq!(count(&tmp.mem(), "PRAGMA user_version"), 7, "the file was not touched");
    // Forget everything still recovers it.
    let fresh = Store::forget_all(&tmp.mem()).unwrap();
    assert!(!fresh.is_newer());
    assert_eq!(fresh.list(None).unwrap().len(), 0);
    assert_eq!(count(&tmp.mem(), "PRAGMA user_version"), 1);
    fresh.add("profile", "Likes tea", &now()).unwrap();
}

#[test]
fn a_corrupt_file_fails_to_open_and_forget_all_recovers() {
    let tmp = Tmp::new();
    std::fs::create_dir_all(tmp.mem()).unwrap();
    std::fs::write(tmp.mem().join("memory.sqlite"), "this is not a database ".repeat(500)).unwrap();
    assert!(matches!(Store::open(&tmp.mem()), Err(Error::Store(_))));
    let s = Store::forget_all(&tmp.mem()).unwrap();
    s.add("profile", "Likes tea", &now()).unwrap();
    assert_eq!(s.list(None).unwrap().len(), 1);
}

#[test]
fn forget_all_removes_every_file_and_row() {
    let tmp = Tmp::new();
    let s = Store::open(&tmp.mem()).unwrap();
    s.add("profile", "Works at Lanternfish Labs", &now()).unwrap();
    s.write_files().unwrap();
    std::fs::create_dir_all(tmp.mem().join("notes")).unwrap();
    std::fs::write(tmp.mem().join("notes/2026-11-26.md"), "Works at Lanternfish Labs").unwrap();
    drop(s);
    let s = Store::forget_all(&tmp.mem()).unwrap();
    assert!(s.list(None).unwrap().is_empty());
    assert_eq!(s.section(), "");
    assert!(!tmp.mem().join("MEMORY.md").exists() && !tmp.mem().join("notes").exists());
    for e in std::fs::read_dir(tmp.mem()).unwrap() {
        let bytes = std::fs::read(e.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("Lanternfish"));
    }
    // nothing to delete is fine too
    drop(s);
    Store::forget_all(&tmp.0.join("never-existed")).unwrap();
}

#[test]
fn memory_md_is_generated_with_its_header_first() {
    let (tmp, s) = store();
    let header = "<!-- Generated by desk-buddy from memory.sqlite. Edit in the What I remember window; changes here are overwritten. -->";
    s.write_files().unwrap();
    let text = std::fs::read_to_string(tmp.mem().join("MEMORY.md")).unwrap();
    assert_eq!(text.lines().next(), Some(header));
    assert!(text.contains("Nothing yet") && !text.contains("## What I know about you"));
    s.add("profile", "Has a cat, Mochi", &now()).unwrap();
    s.write_files().unwrap();
    let text = std::fs::read_to_string(tmp.mem().join("MEMORY.md")).unwrap();
    assert_eq!(text.lines().next(), Some(header));
    assert!(text.contains(&s.section()) && !text.contains("Nothing yet"));
    assert!(!tmp.mem().join("MEMORY.md.tmp").exists());
}

// ---- pending and extraction failures ----

#[test]
fn add_pending_ignores_anything_but_chat_turns() {
    let (tmp, s) = store();
    for id in ["s:sense-1", "c:continue", "r:retry", "u"] {
        s.add_pending(id, "2026-11-27T09:00:00+07:00").unwrap();
    }
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM pending_turns"), 0);
    s.add_pending("u:a", "2026-11-27T09:00:00+07:00").unwrap();
    s.add_pending("u:a", "2026-11-27T09:00:00+07:00").unwrap();
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM pending_turns"), 1);
    s.clear_pending().unwrap();
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM pending_turns"), 0);
    assert!(s.extract_args(10, now().today).is_none());
}

#[test]
fn a_turn_is_dropped_after_three_failed_tries() {
    let (tmp, s) = store();
    s.add_pending("u:a", "2026-11-27T09:00:00+07:00").unwrap();
    s.add_pending("u:b", "2026-11-27T09:01:00+07:00").unwrap();
    let args = s.extract_args(10, now().today).unwrap();
    assert_eq!(s.extract_failed(&args).unwrap(), Vec::<String>::new());
    // re-adding a waiting turn does not reset its tries
    s.add_pending("u:a", "2026-11-27T09:00:00+07:00").unwrap();
    assert_eq!(s.extract_failed(&args).unwrap(), Vec::<String>::new());
    assert_eq!(count(&tmp.mem(), "SELECT tries FROM pending_turns WHERE request_id = 'u:a'"), 2);
    assert_eq!(s.extract_failed(&args).unwrap(), ["u:a", "u:b"]);
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM pending_turns"), 0);
    // a later batch is not blocked
    s.add_pending("u:c", "2026-11-27T09:05:00+07:00").unwrap();
    assert_eq!(s.extract_args(10, now().today).unwrap()["turns"][0]["requestId"], "u:c");
}

#[test]
fn a_turn_is_dropped_after_seven_days_pending() {
    let (_t, s) = store();
    s.add_pending("old", "x").unwrap(); // ignored, not a chat turn
    s.add_pending("u:old", "2026-11-20T09:00:00+07:00").unwrap(); // 7 days
    s.add_pending("u:six", "2026-11-21T09:00:00+07:00").unwrap(); // 6 days
    let args = s.extract_args(10, now().today).unwrap();
    assert_eq!(s.extract_failed(&args).unwrap(), ["u:old"]);
    let args = s.extract_args(10, now().today).unwrap();
    assert_eq!(args["turns"].as_array().unwrap().len(), 1);
    assert_eq!(args["turns"][0]["requestId"], "u:six");
}

// ---- forget, erase ----

#[test]
fn soft_forget_hides_a_row_everywhere_but_keeps_it_on_disk() {
    let (tmp, s) = store();
    let id = said_city(&s, "Hanoi", &now());
    assert!(s.section().contains("Hanoi"));
    assert_eq!(s.forget(Some(id), None, None).unwrap(), [id]);
    assert_eq!(row(&tmp.mem(), id).0, "forgotten");
    assert!(s.list(None).unwrap().is_empty());
    assert!(s.search("hanoi", 5).unwrap().is_empty());
    assert_eq!(s.section(), "");
    assert_eq!(s.view(date(2026, 11, 27))["counts"]["live"], 0);
    assert_eq!(s.forget(Some(id), None, None), Err(Error::NotFound));
    assert_eq!(s.soft_forget(id), Err(Error::NotFound));
    assert_eq!(s.forget(Some(999), None, None), Err(Error::NotFound));
    assert!(matches!(s.forget(None, None, None), Err(Error::Usage(_))));
}

#[test]
fn forget_by_key_and_qualifier() {
    let (_t, s) = store();
    let people = |q: &str, v: &str| with(item("profile", Some("people"), Some(v), &format!("Has a {q}, {v}")), "qualifier", json!(q));
    let likes = |v: &str| item("preference", Some("likes"), Some(v), &format!("Likes {v}"));
    extract(&s, "x", vec![people("sister", "Lan"), people("brother", "Minh"), likes("tea"), likes("coffee")], &now());
    assert_eq!(s.forget(None, Some("people:sister"), None).unwrap().len(), 1);
    assert_eq!(s.forget(None, Some("people"), Some("brother")).unwrap().len(), 1);
    assert_eq!(s.forget(None, Some("people:sister"), None), Err(Error::NotFound));
    assert_eq!(s.forget(None, Some("nothing_here"), None), Err(Error::NotFound));
    assert_eq!(s.forget(None, Some("likes"), None).unwrap().len(), 2);
    assert!(s.list(None).unwrap().is_empty());
}

#[test]
fn forgetting_the_live_value_forgets_the_question_about_it() {
    let (tmp, s) = store();
    let hanoi = said_city(&s, "Hanoi", &now());
    let Outcome::Disputed(d) = guess_city(&s, "Da Nang") else { panic!() };
    s.soft_forget(hanoi).unwrap();
    assert_eq!(row(&tmp.mem(), d).0, "forgotten");
    assert_eq!(s.view(date(2026, 11, 27))["counts"], json!({"live": 0, "disputes": 0}));
}

#[test]
fn erase_removes_the_whole_chain_in_both_directions() {
    let (tmp, s) = store();
    let a = said_city(&s, "Saigon", &now_on(date(2026, 9, 1)));
    let b = said_city(&s, "Hanoi", &now_on(date(2026, 10, 1)));
    let c = said_city(&s, "Da Nang", &now());
    let other = s.add("preference", "Likes tea", &now()).unwrap();
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts"), 4);
    // from the middle of the chain
    s.erase(b).unwrap();
    for gone in [a, b, c] {
        assert_eq!(count(&tmp.mem(), &format!("SELECT count(*) FROM facts WHERE id = {gone}")), 0);
    }
    assert_eq!(count(&tmp.mem(), &format!("SELECT count(*) FROM facts WHERE id = {other}")), 1);
    assert_eq!(s.section().contains("Da Nang"), false);
    assert_eq!(s.erase(b), Err(Error::NotFound));

    // from the oldest, with a forgotten head
    let a = said_city(&s, "Saigon", &now_on(date(2026, 9, 1)));
    let b = said_city(&s, "Hanoi", &now());
    s.soft_forget(b).unwrap();
    s.erase(a).unwrap();
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts"), 1);
}

#[test]
fn erasing_the_live_value_takes_the_question_with_it_and_zeroes_the_bytes() {
    let (tmp, s) = store();
    let hanoi = said_city(&s, "Hue", &now());
    guess_city(&s, "Da Nang");
    s.erase(hanoi).unwrap();
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts"), 0);
    // checkpoint from another connection, then look for the text in the main file
    db(&tmp.mem()).query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
    let bytes = std::fs::read(tmp.mem().join("memory.sqlite")).unwrap();
    // (the schema's own comments quote 'Hanoi', so this test uses another city)
    assert!(!String::from_utf8_lossy(&bytes).contains("Lives in Hue"));
}

#[test]
fn erasing_one_row_leaves_unrelated_rows() {
    let (tmp, s) = store();
    let b = said_city(&s, "Hanoi", &now());
    let other = s.add("promise", "Ask about Mochi", &now()).unwrap();
    s.erase(other).unwrap();
    assert_eq!(row(&tmp.mem(), b).0, "live");
}

// ---- the window ----

#[test]
fn add_makes_authority_4_keyless_rows_and_checks_its_input() {
    let (tmp, s) = store();
    let id = s.add("promise", "Ask how Mochi's vet visit went", &now()).unwrap();
    assert_eq!(row(&tmp.mem(), id).3, 4);
    assert_eq!(s.add("promise", "ask how  mochi's vet visit went", &now()).unwrap(), id, "the same note extends");
    assert!(matches!(s.add("episode", "Had lunch", &now()), Err(Error::Usage(_))));
    assert!(matches!(s.add("insight", "Pattern", &now()), Err(Error::Usage(_))));
    assert_eq!(s.add("profile", "   ", &now()), Err(Error::Empty));
    assert_eq!(s.add("profile", &"x".repeat(201), &now()), Err(Error::TooLong));
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts"), 1);
}

#[test]
fn edit_replaces_a_row_and_keeps_the_old_one_as_history() {
    let (tmp, s) = store();
    let old = city(&s, "Hanoi", 3, &now_on(date(2026, 10, 5)));
    let it = with(item("profile", Some("home_city"), Some("Hanoi"), "Lives in Hanoi"), "validFrom", json!("2026-10"));
    extract(&s, "x", vec![it], &now_on(date(2026, 10, 5))); // extends: nothing to carry yet
    // text only: the value, the predicate and the importance carry over
    let n1 = s.edit(old, "Lives in Hanoi, Vietnam", None, &now()).unwrap();
    let (status, valid_to, by, _) = row(&tmp.mem(), old);
    assert_eq!((status.as_str(), valid_to.as_deref(), by), ("superseded", Some("2026-11-27"), Some(n1)));
    assert_eq!(row(&tmp.mem(), n1).3, 4);
    let f = &s.view(date(2026, 11, 27))["groups"]["about"][0];
    assert_eq!((&f["id"], &f["key"], &f["value"], &f["source"]), (&json!(n1), &json!("home_city"), &json!("Hanoi"), &json!("window")));
    assert_eq!(f["text"], "Lives in Hanoi, Vietnam");
    // a new value
    let n2 = s.edit(n1, "Lives in Da Nang", Some("Da Nang"), &now()).unwrap();
    let f = &s.view(date(2026, 11, 27))["groups"]["about"][0];
    assert_eq!((&f["id"], &f["value"], &f["since"]), (&json!(n2), &json!("Da Nang"), &json!(null)));
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts WHERE status = 'superseded'"), 2);
    // later extraction compares against the edited value
    assert!(matches!(guess_city(&s, "Da Nang"), Outcome::Extended(id) if id == n2));
    // the edited row is not live any more
    assert_eq!(s.edit(old, "x", None, &now()), Err(Error::NotFound));
    assert_eq!(s.edit(n2, "", None, &now()), Err(Error::Empty));
    assert_eq!(s.edit(n2, &"x".repeat(201), None, &now()), Err(Error::TooLong));
    assert_eq!(s.edit(n2, "Lives in Hue", Some(&"x".repeat(101)), &now()), Err(Error::TooLong));
}

#[test]
fn edit_keeps_since_when_only_the_text_changes_and_ignores_value_on_notes() {
    let (_t, s) = store();
    let said = "I live in Hanoi";
    let it = with(with(item("profile", Some("home_city"), Some("Hanoi"), "Lives in Hanoi"), "quote", json!(said)), "validFrom", json!("2026-10"));
    let old = extract(&s, said, vec![it], &now()).items[0].outcome.id().unwrap();
    s.edit(old, "Lives in Hanoi city", None, &now()).unwrap();
    assert_eq!(s.view(date(2026, 11, 27))["groups"]["about"][0]["since"], "2026-10");
    let note = s.add("preference", "Likes tea", &now()).unwrap();
    let new = s.edit(note, "Likes green tea", Some("ignored"), &now()).unwrap();
    assert_eq!(s.list(Some("preference")).unwrap()[0].value, None);
    assert_eq!(s.list(Some("preference")).unwrap()[0].id, new);
}

#[test]
fn editing_to_the_disputed_value_closes_the_question() {
    let (_t, s) = store();
    let hanoi = said_city(&s, "Hanoi", &now());
    guess_city(&s, "Da Nang");
    s.edit(hanoi, "Lives in Da Nang", Some("Da Nang"), &now()).unwrap();
    assert_eq!(s.view(date(2026, 11, 27))["counts"]["disputes"], 0);
}

#[test]
fn resolve_needs_a_disputed_row() {
    let (_t, s) = store();
    let live = said_city(&s, "Hanoi", &now());
    assert_eq!(s.resolve(live, true, &now()), Err(Error::NotFound));
    assert_eq!(s.resolve(404, false, &now()), Err(Error::NotFound));
}

// ---- reading ----

#[test]
fn core_rev_moves_when_the_section_changes() {
    let (_t, s) = store();
    assert_eq!(s.core_rev(), 0);
    s.add_pending("u:a", "2026-11-27T09:00:00+07:00").unwrap();
    assert_eq!(s.core_rev(), 0, "the queue is not memory");
    let id = s.add("preference", "Likes tea", &now()).unwrap();
    let r1 = s.core_rev();
    assert!(r1 > 0);
    s.add("preference", "Likes tea", &now()).unwrap(); // extends: same section
    assert_eq!(s.core_rev(), r1);
    extract(&s, "x", vec![item("episode", None, None, "Was tired")], &now()); // skipped
    assert_eq!(s.core_rev(), r1);
    let Outcome::Created(_) = guess_city(&s, "Hanoi") else { panic!() };
    let r2 = s.core_rev();
    assert!(r2 > r1);
    said_city(&s, "Hanoi", &now()); // extends the guess to authority 3: same text
    let r3 = s.core_rev();
    guess_city(&s, "Da Nang"); // a dispute opens
    let r4 = s.core_rev();
    assert!(r4 > r3);
    let d = s.view(date(2026, 11, 27))["disputes"][0]["id"].as_i64().unwrap();
    s.resolve(d, false, &now()).unwrap(); // a dispute closes
    assert!(s.core_rev() > r4);
    let r5 = s.core_rev();
    s.soft_forget(id).unwrap();
    assert!(s.core_rev() > r5);
    // a new Store counts from 0 again, but the rev still means "section changed since I looked"
    assert_eq!(Store::open(s.dir()).unwrap().core_rev(), 0);
}

#[test]
fn list_and_search() {
    let (_t, s) = store();
    let r = extract(
        &s,
        "x",
        vec![
            with(item("profile", Some("home_city"), Some("Hà Nội"), "Lives in Hà Nội"), "importance", json!(9)),
            item("preference", Some("likes"), Some("green tea"), "Likes green tea"),
            item("preference", Some("likes"), Some("black tea"), "Likes black tea"),
            item("promise", None, None, "Ask about the tea shop in Hà Nội"),
        ],
        &now(),
    );
    assert_eq!(r.created, 4);
    assert_eq!(s.list(None).unwrap().len(), 4);
    assert_eq!(s.list(Some("preference")).unwrap().len(), 2);
    assert_eq!(s.list(None).unwrap()[0].text, "Lives in Hà Nội", "importance first");
    assert!(matches!(s.list(Some("mood")), Err(Error::Usage(_))));
    // case-insensitive, accents kept
    assert_eq!(s.search("GREEN", 5).unwrap().len(), 1);
    assert_eq!(s.search("hà nội", 5).unwrap().len(), 2);
    assert_eq!(s.search("ha noi", 5).unwrap().len(), 0);
    // more matched words first, and the key and value are searched too
    let hits = s.search("tea shop", 5).unwrap();
    assert_eq!(hits[0].text, "Ask about the tea shop in Hà Nội");
    assert_eq!(hits.len(), 3);
    assert_eq!(s.search("home_city", 5).unwrap().len(), 1);
    assert_eq!(s.search("tea", 2).unwrap().len(), 2);
    assert_eq!(s.search("tea", 0).unwrap().len(), 1, "limit is at least 1");
    assert!(matches!(s.search("  ", 5), Err(Error::Usage(_))));
}

// ---- the crate's own rules ----

/// A9: the crate never logs or prints; callers log ids and counts.
#[test]
fn the_crate_never_logs() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for e in std::fs::read_dir(&src).unwrap() {
        let path = e.unwrap().path();
        if path.extension().is_some_and(|x| x == "rs") {
            let code = std::fs::read_to_string(&path).unwrap();
            for bad in ["eprintln!", "println!", "eprint!", "print!", "dbg!", "log::", "tracing::"] {
                assert!(!code.contains(bad), "{} uses {bad}", path.display());
            }
            checked += 1;
        }
    }
    assert!(checked >= 9, "read the crate's sources: {checked}");
}

/// The crate never reads the clock: time comes in through `Now`.
#[test]
fn the_crate_never_reads_the_clock() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for e in std::fs::read_dir(&src).unwrap() {
        let code = std::fs::read_to_string(e.unwrap().path()).unwrap();
        for bad in ["SystemTime", "Instant::now", "Zoned::now", "Timestamp::now", "date::today"] {
            assert!(!code.contains(bad), "{bad}");
        }
    }
}
