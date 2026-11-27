//! The window's History: up to 14 days of episodes and insights, newest first, with paging.
mod common;
use common::*;
use memory::Store;
use serde_json::{json, Value};

fn days(v: &Value) -> Vec<String> {
    v["history"].as_array().unwrap().iter().map(|d| d["day"].as_str().unwrap().to_string()).collect()
}

/// One episode on each of `n` days ending 2026-11-26.
fn many_days(s: &Store, n: i64) {
    let end = date(2026, 11, 26);
    for k in 0..n {
        let d = end.checked_sub(jiff::Span::new().days(k)).unwrap();
        episode(s, d, &format!("Something happened {d}"), 5);
    }
}

#[test]
fn history_lists_days_newest_first_and_items_in_observed_order() {
    let (_t, s) = store();
    let first = episode(&s, date(2026, 11, 25), "Mochi chased a moth", 5);
    let second = episode(&s, date(2026, 11, 25), "Lan lent a book", 5);
    let third = episode(&s, date(2026, 11, 20), "Cleaned the desk", 5);
    let v = s.view(now().today);
    assert_eq!(days(&v), ["2026-11-25", "2026-11-20"]);
    assert_eq!(v["historyMore"], false);
    let items = v["history"][0]["items"].as_array().unwrap();
    assert_eq!(items.iter().map(|i| i["id"].as_i64().unwrap()).collect::<Vec<_>>(), [first, second]);
    assert_eq!(v["history"][1]["items"][0]["id"], third);
    let it = &items[0];
    assert_eq!(it["kind"], "episode");
    assert_eq!(it["text"], "Mochi chased a moth");
    assert_eq!(it["source"], "guess");
    assert_eq!(it["label"], Value::Null);
    assert_eq!(it["disputed"], false);
    assert!(it.get("sources").is_none());
    assert_eq!(v["counts"]["days"], 2);
}

#[test]
fn history_shows_insights_with_their_source_counts_and_hides_the_rest() {
    let (t, s) = store();
    let a = episode(&s, date(2026, 11, 25), "Mochi chased a moth", 5);
    let b = episode(&s, date(2026, 11, 26), "Mochi chased a bird", 5);
    let gone = episode(&s, date(2026, 11, 24), "Tiny forgotten thing", 5);
    s.soft_forget(gone).unwrap();
    for i in 0..10 {
        s.add("preference", &format!("Likes tea number {i}"), &now()).unwrap(); // enough rows that one insight is under the guard
    }
    let j = s.reflect_args(date(2026, 11, 27), &now()).unwrap();
    s.apply_reflect(&j, &json!({"insights": [{"text": "Mochi likes to chase things", "sources": [a, b]}]}), &now()).unwrap();
    let v = s.view(now().today);
    assert_eq!(days(&v), ["2026-11-27", "2026-11-26", "2026-11-25"], "no day for the forgotten episode or the preference");
    let ins = &v["history"][0]["items"][0];
    assert_eq!((ins["kind"].as_str(), ins["sources"].as_i64()), (Some("insight"), Some(2)));
    assert_eq!(v["counts"]["days"], 3);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts WHERE kind = 'insight'"), 1);
}

#[test]
fn the_window_shows_14_days_and_says_when_more_remain() {
    let (_t, s) = store();
    many_days(&s, 14);
    let v = s.view(now().today);
    assert_eq!(days(&v).len(), 14);
    assert_eq!(v["historyMore"], false);
    assert_eq!(v["counts"]["days"], 14);

    let (_t, s) = store();
    many_days(&s, 20);
    let v = s.view(now().today);
    assert_eq!(days(&v).len(), 14);
    assert_eq!(days(&v)[0], "2026-11-26");
    assert_eq!(days(&v)[13], "2026-11-13");
    assert_eq!(v["historyMore"], true);
    assert_eq!(v["counts"]["days"], 20);
}

#[test]
fn history_before_pages_older_days() {
    let (_t, s) = store();
    many_days(&s, 20);
    let (page, more) = s.history(date(2026, 11, 13)).unwrap();
    assert_eq!(page.iter().map(|d| d["day"].as_str().unwrap()).collect::<Vec<_>>(), ["2026-11-12", "2026-11-11", "2026-11-10", "2026-11-09", "2026-11-08", "2026-11-07"]);
    assert!(!more);
    // the bound is exclusive and may be any day
    let (page, more) = s.history(date(2026, 11, 26)).unwrap();
    assert_eq!(page.len(), 14);
    assert_eq!(page[0]["day"], "2026-11-25");
    assert!(more);
    let (page, more) = s.history(date(2026, 1, 1)).unwrap();
    assert!(page.is_empty() && !more);
}

#[test]
fn empty_and_unreadable_stores_have_the_old_shape_where_it_matters() {
    let (_t, s) = store();
    let v = s.view(now().today);
    assert_eq!(v["history"], json!([]));
    assert_eq!(v["historyMore"], false);
    assert_eq!(v["counts"]["days"], 0);
}

#[test]
fn live_episodes_still_count_as_hidden_for_forget_everything() {
    let (_t, s) = store();
    episode(&s, date(2026, 11, 25), "Something small", 5);
    let v = s.view(now().today);
    assert_eq!(v["counts"]["live"], 0);
    assert_eq!(v["counts"]["hidden"], 1, "forget everything stays offered");
}

#[test]
fn history_never_shows_a_forgotten_or_superseded_day_and_follows_edits_to_other_rows() {
    let (_t, s) = store();
    let e = episode(&s, date(2026, 11, 25), "Something small", 5);
    assert_eq!(days(&s.view(now().today)), ["2026-11-25"]);
    s.soft_forget(e).unwrap();
    assert!(days(&s.view(now().today)).is_empty());
    assert_eq!(s.view(now().today)["counts"]["days"], 0);
}
