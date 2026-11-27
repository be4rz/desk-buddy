//! The reflection run (A13, A14, A15's retention) and its trigger. Fixed now: 2026-11-27T09:12:03+07:00.
mod common;
use common::*;
use memory::{Error, ReflectJob, Reflected, Store, MAX_TRIES};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 9 keyless preferences and a profile, noted on 2026-11-01, plus 3 live episodes: 13 live rows.
fn fixture() -> (Tmp, Store, Vec<i64>, Vec<i64>) {
    let (t, s) = store();
    let old = now_on(date(2026, 11, 1));
    let core: Vec<i64> = (0..9).map(|i| s.add("preference", &format!("Likes tea number {i}"), &old).unwrap()).chain([s.add("profile", "Works at Lanternfish Labs", &old).unwrap()]).collect();
    let eps = vec![
        episode(&s, date(2026, 11, 24), "Stayed late to ship the release", 6),
        episode(&s, date(2026, 11, 25), "Skipped lunch to finish the demo", 6),
        episode(&s, date(2026, 11, 26), "Worked late again on the migration", 7),
    ];
    (t, s, core, eps)
}

fn job(s: &Store) -> ReflectJob {
    s.reflect_args(date(2026, 11, 27), &now()).expect("a batch")
}

fn status(dir: &std::path::Path, batch: &str) -> String {
    db(dir).query_row("SELECT status FROM reflections WHERE batch_id = ?1", [batch], |r| r.get(0)).unwrap()
}

fn insight(text: &str, sources: &[i64]) -> Value {
    json!({"text": text, "importance": 6, "sources": sources})
}

fn live(dir: &std::path::Path) -> i64 {
    count(dir, "SELECT count(*) FROM facts WHERE status = 'live'")
}

// ---- the batch ----

#[test]
fn reflect_args_has_the_i1_shape_and_a_hashed_batch_id() {
    let (_t, s, _core, eps) = fixture();
    let j = job(&s);
    let want: String = Sha256::digest(format!("{},{},{}", eps[0], eps[1], eps[2]).as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(j.batch_id, want);
    assert_eq!(j.batch_id.len(), 64);
    let a = &j.args;
    assert_eq!(a["today"], "2026-11-27");
    let e = a["episodes"].as_array().unwrap();
    assert_eq!(e.len(), 3);
    assert_eq!(e[0], json!({"id": eps[0], "day": "2026-11-24", "text": "Stayed late to ship the release", "importance": 6}));
    let known = a["known"].as_array().unwrap();
    assert_eq!(known.len(), 10, "the live core rows");
    assert!(known.iter().all(|k| ["id", "kind", "predicate", "qualifier", "value", "text"].iter().all(|f| k.get(*f).is_some())));
    assert_eq!(a["registry"].as_array().unwrap().len(), 12);
}

#[test]
fn nothing_new_means_no_job_and_the_batch_is_capped_at_50() {
    let (_t, s) = store();
    assert!(s.reflect_args(date(2026, 11, 27), &now()).is_none());
    s.add("profile", "Works at Lanternfish Labs", &now()).unwrap();
    assert!(s.reflect_args(date(2026, 11, 27), &now()).is_none(), "only episodes make a batch");
    for i in 0..55 {
        episode(&s, date(2026, 11, 20), &format!("Episode number {i} of the week"), 5);
    }
    let j = job(&s);
    let e = j.args["episodes"].as_array().unwrap();
    assert_eq!(e.len(), 50);
    assert!(e.windows(2).all(|w| w[0]["id"].as_i64() < w[1]["id"].as_i64()), "oldest first");
    // done with the first 50: the next run takes the other 5
    s.apply_reflect(&j, &json!({}), &now()).unwrap();
    let j2 = job(&s);
    assert_eq!(j2.args["episodes"].as_array().unwrap().len(), 5);
    assert_ne!(j2.batch_id, j.batch_id);
}

// ---- A13: a run is safe to repeat ----

#[test]
fn a_crashed_run_is_rerun_as_the_same_batch_and_applies_once() {
    let (t, s, _core, eps) = fixture();
    let j1 = job(&s);
    assert_eq!(status(&t.mem(), &j1.batch_id), "running");
    drop(s); // the process dies mid-run
    let s = Store::open(&t.mem()).unwrap();
    let j2 = job(&s);
    assert_eq!(j2.batch_id, j1.batch_id);
    assert_eq!(status(&t.mem(), &j2.batch_id), "running");

    let reply = json!({"updates": [], "insights": [insight("Crunch week: worked late and skipped lunch", &[eps[0], eps[1]])], "merges": []});
    let r = s.apply_reflect(&j2, &reply, &now()).unwrap();
    assert_eq!((r.insights, r.updates, r.merges, r.forgotten), (1, 0, 0, 0));
    assert_eq!(r.days, ["2026-11-27"]);
    assert_eq!(status(&t.mem(), &j2.batch_id), "done");
    assert!(s.reflect_args(date(2026, 11, 27), &now()).is_none(), "a done batch is not run again");

    // exactly one insight: live, keyless, authority 1, no source turn, observed now, with its sources
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE kind = 'insight'"), 1);
    let (st, pred, auth, turn, obs, imp): (String, Option<String>, i64, Option<String>, String, i64) = db(&t.mem())
        .query_row("SELECT status, predicate, authority, source_turn, observed_at, importance FROM facts WHERE kind = 'insight'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .unwrap();
    assert_eq!((st.as_str(), pred, auth, turn, obs.as_str(), imp), ("live", None, 1, None, "2026-11-27T09:12:03+07:00", 6));
    let sources: Vec<i64> = {
        let c = db(&t.mem());
        let mut q = c.prepare("SELECT episode_id FROM insight_sources ORDER BY episode_id").unwrap();
        q.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(sources, [eps[0], eps[1]]);

    // applying the same reply again does not duplicate anything
    let again = s.apply_reflect(&j2, &reply, &now()).unwrap();
    assert_eq!(again, Reflected::default());
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE kind = 'insight'"), 1);
    // and a re-run of the same insight text under a new batch is skipped as a duplicate
    let new_ep = episode(&s, date(2026, 11, 27), "Shipped the release at last", 5);
    let j3 = job(&s);
    assert_eq!(j3.args["episodes"].as_array().unwrap().len(), 1);
    let r = s.apply_reflect(&j3, &json!({"insights": [insight("  crunch WEEK: worked late and skipped lunch ", &[new_ep])]}), &now()).unwrap();
    assert_eq!(r.insights, 0);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE kind = 'insight'"), 1);
}

#[test]
fn a_successful_run_resets_the_trigger_and_moves_the_cursor() {
    let (t, s, _core, eps) = fixture();
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("19"));
    let j = job(&s);
    s.apply_reflect(&j, &json!({}), &now()).unwrap();
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("0"));
    assert_eq!(meta(&t.mem(), "last_reflection_at").as_deref(), Some("2026-11-27T09:12:03+07:00"));
    assert_eq!(meta(&t.mem(), "reflect_last_id"), Some(eps[2].to_string()));
    let finished: Option<String> = db(&t.mem()).query_row("SELECT finished_at FROM reflections", [], |r| r.get(0)).unwrap();
    assert_eq!(finished.as_deref(), Some("2026-11-27T09:12:03+07:00"));
    assert!(s.reflect_args(date(2026, 11, 27), &now()).is_none());
    // a new episode makes the next batch
    let e = episode(&s, date(2026, 11, 27), "Rested", 5);
    let j2 = job(&s);
    assert_eq!(j2.args["episodes"][0]["id"], e);
}

// ---- A14: the guard ----

#[test]
fn a_reply_changing_more_than_30_percent_is_rejected_whole() {
    let (t, s, _core, eps) = fixture();
    let j = job(&s);
    let before = live(&t.mem());
    assert_eq!(before, 13);
    // five existing notes extended: 5 of 13 = 38%
    let updates: Vec<Value> = (0..5).map(|i| item("preference", None, None, &format!("Likes tea number {i}"))).collect();
    let reply = json!({"updates": updates, "insights": [insight("Late nights", &[eps[0]])], "merges": []});
    assert_eq!(s.apply_reflect(&j, &reply, &now()), Err(Error::Rejected { pct: 46 }));
    assert_eq!(status(&t.mem(), &j.batch_id), "failed");
    let finished: Option<String> = db(&t.mem()).query_row("SELECT finished_at FROM reflections", [], |r| r.get(0)).unwrap();
    assert!(finished.is_some());
    // nothing changed: no insight, no extended rows, the counters stay
    assert_eq!(live(&t.mem()), before);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE kind = 'insight'"), 0);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE observed_at LIKE '2026-11-27%' AND kind = 'preference'"), 0);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM insight_sources"), 0);
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("19"));
    assert_eq!(meta(&t.mem(), "last_reflection_at"), None);
    assert_eq!(meta(&t.mem(), "reflect_last_id"), None);
    // the next trigger retries the same batch
    assert_eq!(job(&s).batch_id, j.batch_id);
    assert_eq!(status(&t.mem(), &j.batch_id), "running");
}

#[test]
fn a_reply_just_under_the_guard_is_applied() {
    let (t, s, _core, eps) = fixture();
    let j = job(&s);
    // 3 of 13 = 23%
    let updates: Vec<Value> = (0..2).map(|i| item("preference", None, None, &format!("Likes tea number {i}"))).collect();
    let r = s.apply_reflect(&j, &json!({"updates": updates, "insights": [insight("Late nights", &[eps[0]])]}), &now()).unwrap();
    assert_eq!((r.updates, r.insights, r.pct), (2, 1, 23));
    assert_eq!(status(&t.mem(), &j.batch_id), "done");
    // 4 of 13 = 30.8% is over; exactly 30% is not
    let (t, s, _c, eps) = fixture();
    for i in 0..7 {
        s.add("preference", &format!("Prefers tea number {i}"), &now_on(date(2026, 11, 1))).unwrap();
    }
    assert_eq!(live(&t.mem()), 20);
    let j = job(&s);
    let updates: Vec<Value> = (0..5).map(|i| item("preference", None, None, &format!("Likes tea number {i}"))).collect();
    let r = s.apply_reflect(&j, &json!({"updates": updates, "insights": [insight("Late nights", &[eps[0]])]}), &now()).unwrap();
    assert_eq!(r.pct, 30);
}

#[test]
fn only_updates_that_touch_a_live_row_count_toward_the_guard() {
    let (_t, s, _core, _eps) = fixture();
    let j = job(&s);
    // created and skipped updates do not count
    let updates: Vec<Value> = (0..4).map(|i| item("preference", None, None, &format!("Brand new like {i}"))).chain([item("episode", None, None, "x")]).collect();
    let r = s.apply_reflect(&j, &json!({"updates": updates}), &now()).unwrap();
    assert_eq!((r.updates, r.pct), (4, 0));
}

// ---- the failure case ----

#[test]
fn bad_json_marks_the_run_failed_and_the_next_trigger_retries_it() {
    let (t, s, _core, _eps) = fixture();
    let j = job(&s); // try 1
    assert!(matches!(s.apply_reflect(&j, &json!("not an object"), &now()), Err(Error::Usage(_))));
    assert_eq!(status(&t.mem(), &j.batch_id), "failed");
    let j = job(&s); // try 2: the same batch
    assert_eq!(status(&t.mem(), &j.batch_id), "running");
    assert!(matches!(s.apply_reflect(&j, &json!([1, 2]), &now()), Err(Error::Usage(_))));
    let j2 = job(&s); // try 3
    assert_eq!(j2.batch_id, j.batch_id);
    assert!(matches!(s.apply_reflect(&j2, &Value::Null, &now()), Err(Error::Usage(_))));
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE kind = 'insight'"), 0);
    // missing arrays are empty, wrong types are ignored (a fresh store: the tries are not spent)
    let (_t, s, _core, _eps) = fixture();
    let r = s.apply_reflect(&job(&s), &json!({"updates": "x", "insights": {}}), &now()).unwrap();
    assert_eq!(r, Reflected::default());
}

#[test]
fn a_failed_call_is_retried_with_the_same_batch() {
    let (t, s, _core, _eps) = fixture();
    let first = job(&s);
    for _ in 1..MAX_TRIES {
        s.reflect_failed(&first, &now()).unwrap();
        assert_eq!(status(&t.mem(), &first.batch_id), "failed");
        let again = job_or_none(&s).expect("retried");
        assert_eq!(again.batch_id, first.batch_id);
        assert_eq!(status(&t.mem(), &first.batch_id), "running");
    }
    // a failure never overwrites a finished run
    s.apply_reflect(&first, &json!({}), &now()).unwrap();
    s.reflect_failed(&first, &now()).unwrap();
    assert_eq!(status(&t.mem(), &first.batch_id), "done");
}

fn job_or_none(s: &Store) -> Option<ReflectJob> {
    s.reflect_args(date(2026, 11, 27), &now())
}

#[test]
fn a_batch_tried_three_times_is_given_up_and_the_next_run_moves_on() {
    let (t, s, _core, eps) = fixture();
    for n in 1..=MAX_TRIES {
        let j = job_or_none(&s).unwrap_or_else(|| panic!("try {n}"));
        s.reflect_failed(&j, &now()).unwrap();
    }
    let given_up = job_or_none(&s);
    assert!(given_up.is_none(), "the 4th ask gives up");
    let batch: String = Sha256::digest(format!("{},{},{}", eps[0], eps[1], eps[2]).as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(status(&t.mem(), &batch), "failed");
    assert_eq!(meta(&t.mem(), "reflect_last_id"), Some(eps[2].to_string()));
    assert!(job_or_none(&s).is_none());
    // a later episode is a new batch with a fresh count
    let e = episode(&s, date(2026, 11, 27), "Rested", 5);
    let j = job_or_none(&s).unwrap();
    assert_eq!(j.args["episodes"].as_array().unwrap().len(), 1);
    assert_eq!(j.args["episodes"][0]["id"], e);
    assert_ne!(j.batch_id, batch);
}

// ---- what a reply may do ----

#[test]
fn merges_follow_the_rules() {
    let (t, s, core, eps) = fixture();
    let city = city(&s, "Hanoi", 3, &now_on(date(2026, 11, 1))); // keyed profile row
    // the extractor's guesses (authority 1) may be merged; what the person wrote (4) is never dropped
    db(&t.mem()).execute("UPDATE facts SET authority = 1 WHERE id IN (?1, ?2)", [core[1], core[6]]).unwrap();
    let j = job(&s);
    let reply = json!({"merges": [
        {"keep": core[0], "drop": [core[1]]},                // ok: keyless, same kind, a guess dropped
        {"keep": core[6], "drop": [core[7]]},                // the person's row as the drop: refused
        {"keep": core[2], "drop": [core[9]]},                // preference vs profile: refused
        {"keep": city, "drop": [core[9]]},                   // keyed keep: refused
        {"keep": core[9], "drop": [city]},                   // keyed drop: refused
        {"keep": core[3], "drop": [core[3]]},                // keep in drop: refused
        {"keep": core[3], "drop": [core[4], core[4]]},       // a drop twice: refused
        {"keep": core[3], "drop": [core[1]]},                // the drop is not live any more: refused
        {"keep": core[3], "drop": []},                       // nothing to drop
        {"keep": 9999, "drop": [core[5]]},                   // unknown keep
        {"keep": core[3], "drop": [9999]},                   // unknown drop
        {"keep": eps[0], "drop": [core[5]]},                 // episode vs preference: refused
    ]});
    let r = s.apply_reflect(&j, &reply, &now()).unwrap();
    assert_eq!(r.merges, 1);
    assert_eq!(row(&t.mem(), core[1]).0, "superseded");
    assert_eq!(row(&t.mem(), core[1]).2, Some(core[0]));
    for id in [core[0], core[2], core[3], core[4], core[5], core[6], core[7], core[9], city] {
        assert_eq!(row(&t.mem(), id).0, "live", "{id}");
    }
    // two episodes may merge into one: they leave History
    let (_t, s, _core, eps) = fixture();
    let j = job(&s);
    let r = s.apply_reflect(&j, &json!({"merges": [{"keep": eps[1], "drop": [eps[0]]}]}), &now()).unwrap();
    assert_eq!(r.merges, 1);
    assert_eq!(r.days, ["2026-11-24"]);
}

#[test]
fn an_update_at_authority_1_cannot_overwrite_a_stated_fact() {
    let (t, s, _core, _eps) = fixture();
    let hanoi = city(&s, "Hanoi", 3, &now_on(date(2026, 11, 1)));
    let j = job(&s);
    let upd = with(item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang"), "quote", json!("I live in Da Nang"));
    let r = s.apply_reflect(&j, &json!({"updates": [upd]}), &now()).unwrap();
    assert_eq!(r.updates, 1);
    assert_eq!(row(&t.mem(), hanoi).0, "live", "the person's word stays");
    assert_eq!(s.view(now().today)["disputes"][0]["value"], "Da Nang", "the guess waits for the person");
    // the quote was ignored: still a guess, never authority 3
    assert_eq!(count(&t.mem(), "SELECT authority FROM facts WHERE status = 'disputed'"), 1);
}

#[test]
fn updates_create_extend_and_supersede_guesses_and_files_a_new_fact() {
    let (t, s, _core, _eps) = fixture();
    let j = job(&s);
    let upd = item("profile", Some("job"), Some("Systems engineer"), "Works as a systems engineer");
    let r = s.apply_reflect(&j, &json!({"updates": [upd]}), &now()).unwrap();
    assert_eq!(r.updates, 1);
    let list = s.list(Some("profile")).unwrap();
    assert!(list.iter().any(|f| f.key.as_deref() == Some("job") && f.source == "guess"));
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE kind = 'insight'"), 0);
}

#[test]
fn episodes_insights_and_secrets_are_refused_as_updates() {
    let (t, s, _core, eps) = fixture();
    let j = job(&s);
    let key = format!("{}{}", "sk", "-live-abc123def456");
    let reply = json!({"updates": [
        item("episode", None, None, "Another episode"),
        item("insight", None, None, "A pattern"),
        item("preference", None, None, &format!("Uses key {key}")),
        item("preference", None, None, &"x ".repeat(120)),
        item("bogus", None, None, "Bad kind"),
        json!("not an item"),
    ], "insights": [
        insight(&format!("Pattern with {key}"), &[eps[0]]),
        insight(&"y ".repeat(120), &[eps[0]]),
        insight("", &[eps[0]]),
        insight("Sources outside the batch", &[999, eps[0] + 1000]),
        insight("No sources at all", &[]),
        {"text": "No sources field"},
    ]});
    let r = s.apply_reflect(&j, &reply, &now()).unwrap();
    assert_eq!((r.updates, r.insights, r.pct), (0, 0, 0));
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts"), 13);
}

#[test]
fn insights_are_capped_at_three_clamped_and_cite_only_this_batch() {
    let (t, s, core, eps) = fixture();
    let j = job(&s);
    let reply = json!({"insights": [
        {"text": "One", "importance": 99, "sources": [eps[0], eps[0], core[0], eps[1]]},
        {"text": "Two", "sources": [eps[2]]},
        {"text": "Three", "importance": 0, "sources": [eps[1]]},
        {"text": "Four is ignored", "sources": [eps[1]]},
    ]});
    let r = s.apply_reflect(&j, &reply, &now()).unwrap();
    assert_eq!(r.insights, 3);
    let imp = |t_: &str| -> i64 { db(&t.mem()).query_row("SELECT importance FROM facts WHERE text = ?1", [t_], |r| r.get(0)).unwrap() };
    assert_eq!((imp("One"), imp("Two"), imp("Three")), (10, 5, 1));
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM insight_sources WHERE insight_id = (SELECT id FROM facts WHERE text = 'One')"), 2, "the preference id was dropped");
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE text LIKE 'Four%'"), 0);
}

#[test]
fn an_insight_never_cites_an_episode_that_is_gone() {
    let (t, s, _core, eps) = fixture();
    let j = job(&s);
    s.erase(eps[0]).unwrap(); // erased while the model was thinking
    s.soft_forget(eps[1]).unwrap();
    let reply = json!({"insights": [insight("Gone", &[eps[0], eps[1]]), insight("Kept", &[eps[0], eps[2]])]});
    let r = s.apply_reflect(&j, &reply, &now()).unwrap();
    assert_eq!(r.insights, 1);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM insight_sources"), 1);
}

#[test]
fn a_reflection_that_changes_the_section_moves_core_rev() {
    let (_t, s, _core, _eps) = fixture();
    let rev = s.core_rev();
    let j = job(&s);
    assert_eq!(s.core_rev(), rev, "starting a run changes nothing");
    s.apply_reflect(&j, &json!({"insights": [insight("Late nights", &[1])]}), &now()).unwrap();
    assert_eq!(s.core_rev(), rev, "an insight is not in the section");
    let (_t, s, _core, _eps) = fixture();
    let rev = s.core_rev();
    let j = job(&s);
    s.apply_reflect(&j, &json!({"updates": [item("preference", None, None, "Likes oat milk")]}), &now()).unwrap();
    assert!(s.core_rev() > rev);
}

#[test]
fn a_read_only_store_does_not_reflect() {
    let (t, s, _core, _eps) = fixture();
    let j = job(&s);
    drop(s);
    db(&t.mem()).pragma_update(None, "user_version", 9).unwrap();
    let s = Store::open(&t.mem()).unwrap();
    assert!(s.reflect_args(date(2026, 11, 27), &now()).is_none());
    assert!(!s.reflect_due(&now()));
    assert_eq!(s.apply_reflect(&j, &json!({}), &now()), Err(Error::Newer));
    assert_eq!(s.reflect_failed(&j, &now()), Err(Error::Newer));
}

// ---- retention ----

#[test]
fn retention_forgets_only_old_minor_never_recalled_episodes() {
    let (t, s) = store();
    let today = date(2026, 11, 27); // 90 days back is 2026-08-29
    let old = date(2026, 6, 1);
    let forgotten_a = episode(&s, old, "Old and minor", 3);
    let forgotten_b = episode(&s, date(2026, 8, 28), "Ninety-one days, importance four", 4);
    let kept_edge = episode(&s, date(2026, 8, 29), "Exactly ninety days", 3);
    let kept_important = episode(&s, old, "Old but important", 5);
    let kept_recalled = episode(&s, old, "Old, minor and recalled", 3);
    let kept_null = episode(&s, old, "Old with no importance", 3);
    let kept_recent = episode(&s, date(2026, 11, 20), "Recent and minor", 3);
    db(&t.mem()).execute("UPDATE facts SET recall_n = 1 WHERE id = ?1", [kept_recalled]).unwrap();
    db(&t.mem()).execute("UPDATE facts SET importance = NULL WHERE id = ?1", [kept_null]).unwrap();
    // an old profile, promise and insight stay however minor
    let profile = s.add("profile", "Works at Lanternfish Labs", &now_on(old)).unwrap();
    let promise = s.add("promise", "Ask about Mochi", &now_on(old)).unwrap();
    db(&t.mem())
        .execute(
            "INSERT INTO facts (kind, text, importance, authority, observed_at) VALUES ('insight', 'Old pattern', 1, 1, '2026-06-02T09:00:00+07:00')",
            [],
        )
        .unwrap();
    let insight_id: i64 = db(&t.mem()).query_row("SELECT id FROM facts WHERE kind = 'insight'", [], |r| r.get(0)).unwrap();
    for e in [forgotten_a, kept_important] {
        db(&t.mem()).execute("INSERT INTO insight_sources VALUES (?1, ?2)", [insight_id, e]).unwrap();
    }

    let j = s.reflect_args(today, &now()).unwrap();
    let r = s.apply_reflect(&j, &json!({}), &now()).unwrap();
    assert_eq!(r.forgotten, 2);
    assert_eq!(r.days, ["2026-06-01", "2026-08-28"]);
    for id in [forgotten_a, forgotten_b] {
        assert_eq!(row(&t.mem(), id).0, "forgotten", "{id}");
    }
    for id in [kept_edge, kept_important, kept_recalled, kept_null, kept_recent, profile, promise, insight_id] {
        assert_eq!(row(&t.mem(), id).0, "live", "{id}");
    }
    // the forgotten episode no longer backs the insight; the insight stays
    let backing: Vec<i64> = {
        let c = db(&t.mem());
        let mut q = c.prepare("SELECT episode_id FROM insight_sources").unwrap();
        q.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(backing, [kept_important]);
}

#[test]
fn a_rejected_run_forgets_nothing() {
    let (t, s) = store();
    for i in 0..4 {
        s.add("preference", &format!("Likes tea number {i}"), &now_on(date(2026, 11, 1))).unwrap();
    }
    let old = episode(&s, date(2026, 6, 1), "Old and minor", 3);
    let j = s.reflect_args(date(2026, 11, 27), &now()).unwrap();
    let updates: Vec<Value> = (0..4).map(|i| item("preference", None, None, &format!("Likes tea number {i}"))).collect();
    assert!(matches!(s.apply_reflect(&j, &json!({"updates": updates}), &now()), Err(Error::Rejected { .. })));
    assert_eq!(row(&t.mem(), old).0, "live");
}

// ---- the trigger ----

#[test]
fn due_by_importance() {
    let (_t, s) = store();
    assert!(!s.reflect_due(&now()));
    for i in 0..4 {
        episode(&s, date(2026, 11, 26), &format!("Episode {i}"), 6);
    }
    // 24 of 30: only the 24 h rule could make it due, and there has never been a run, so it is
    assert!(s.reflect_due(&now()));
}

#[test]
fn due_by_importance_30_even_right_after_a_run() {
    let (t, s) = store();
    episode(&s, date(2026, 11, 26), "First", 5);
    let j = s.reflect_args(date(2026, 11, 27), &now()).unwrap();
    s.apply_reflect(&j, &json!({}), &now()).unwrap();
    assert!(!s.reflect_due(&now()), "nothing new");
    for i in 0..4 {
        episode(&s, date(2026, 11, 27), &format!("Episode {i}"), 7);
    }
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("28"));
    assert!(!s.reflect_due(&now()), "28 is under 30 and the last run was just now");
    episode(&s, date(2026, 11, 27), "One more", 2 + 1);
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("31"));
    assert!(s.reflect_due(&now()));
}

#[test]
fn due_after_a_day_with_a_new_episode() {
    let (_t, s) = store();
    episode(&s, date(2026, 11, 26), "First", 5);
    let j = s.reflect_args(date(2026, 11, 27), &now()).unwrap();
    s.apply_reflect(&j, &json!({}), &now()).unwrap(); // at 2026-11-27T09:12:03+07:00
    episode(&s, date(2026, 11, 27), "Second", 5);
    assert!(!s.reflect_due(&now()));
    assert!(!s.reflect_due(&now_at(date(2026, 11, 28), "09:12:02")), "one second short of 24 h");
    assert!(s.reflect_due(&now_at(date(2026, 11, 28), "09:12:03")));
    assert!(s.reflect_due(&now_at(date(2026, 12, 5), "09:00:00")));
}

#[test]
fn a_day_without_a_new_episode_is_not_due() {
    let (_t, s) = store();
    episode(&s, date(2026, 11, 26), "First", 5);
    let j = s.reflect_args(date(2026, 11, 27), &now()).unwrap();
    s.apply_reflect(&j, &json!({}), &now()).unwrap();
    assert!(!s.reflect_due(&now_at(date(2026, 12, 5), "09:00:00")));
}

#[test]
fn not_due_while_a_chat_turn_waits_for_the_extractor() {
    let (_t, s) = store();
    for i in 0..5 {
        episode(&s, date(2026, 11, 26), &format!("Episode {i}"), 6);
    }
    assert!(s.reflect_due(&now()));
    s.add_pending("u:waiting", "2026-11-27T09:00:00+07:00").unwrap();
    assert!(!s.reflect_due(&now()));
    s.clear_pending().unwrap();
    assert!(s.reflect_due(&now()));
}

#[test]
fn extraction_counts_the_importance_of_created_and_extended_items() {
    let (t, s) = store();
    let n = now();
    extract(
        &s,
        "x",
        vec![
            with(item("episode", None, None, "Counted"), "importance", json!(5)),
            with(item("preference", None, None, "Likes tea"), "importance", json!(4)),
            with(item("episode", None, None, "Too minor"), "importance", json!(2)),              // dropped: low
            with(item("preference", None, None, &format!("Key {}{}", "sk", "-abc123def456")), "importance", json!(9)), // refused: secret
        ],
        &n,
    );
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("9"));
    // the same note said again extends it: counted again
    extract(&s, "x", vec![with(item("preference", None, None, "Likes tea"), "importance", json!(3))], &n);
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("12"));
    // the window and the bear's own commands are not extraction
    s.add("preference", "Likes oat milk", &n).unwrap();
    assert_eq!(meta(&t.mem(), "importance_since_reflection").as_deref(), Some("12"));
}

#[test]
fn not_due_without_a_new_episode_whatever_the_importance_counter_says() {
    let (t, s) = store();
    for i in 0..5 {
        episode(&s, date(2026, 11, 26), &format!("Episode {i}"), 7);
    }
    let j = s.reflect_args(date(2026, 11, 27), &now()).unwrap();
    s.apply_reflect(&j, &json!({}), &now()).unwrap();
    // a counter left over (a batch given up on, its episodes forgotten): no batch to run, so not due
    db(&t.mem()).execute("UPDATE meta SET value = '35' WHERE key = 'importance_since_reflection'", []).unwrap();
    assert!(s.reflect_args(date(2026, 11, 27), &now()).is_none());
    assert!(!s.reflect_due(&now()));
}

#[test]
fn an_unspent_try_is_given_back() {
    let (t, s, _core, _eps) = fixture();
    let j = job(&s);
    s.reflect_unspent(&j, &now()).unwrap();
    assert_eq!(status(&t.mem(), &j.batch_id), "failed");
    // MAX_TRIES more real tries still fit: the unspent one never counted
    for _ in 0..MAX_TRIES {
        let again = job(&s);
        assert_eq!(again.batch_id, j.batch_id);
        s.reflect_failed(&again, &now()).unwrap();
    }
    assert!(s.reflect_args(date(2026, 11, 27), &now()).is_none(), "then given up");
}
