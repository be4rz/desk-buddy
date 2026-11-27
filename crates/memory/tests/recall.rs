//! Recall on a message (A12), `recall_span`, and the FTS-backed search. Fixed now: 2026-11-27T09:12:03+07:00.
mod common;
use common::*;
use memory::{Error, Span, Store, MAX_CHARS, TOP};
use serde_json::json;

const TALK: &str = "Stressed about Friday's talk at the meetup; practised the intro twice";

fn fixture() -> (Tmp, Store) {
    let (t, s) = store();
    episode(&s, date(2026, 11, 20), TALK, 7);
    episode(&s, date(2026, 11, 25), "Cooked noodles for lunch and fixed a bike tyre", 4);
    episode(&s, date(2026, 11, 26), "Fixed the login bug at Lanternfish Labs", 6);
    episode(&s, date(2026, 11, 26), "Walked to the lake with Lan", 5);
    (t, s)
}

fn recall_n(dir: &std::path::Path, id: i64) -> (i64, Option<String>) {
    db(dir).query_row("SELECT recall_n, last_recall FROM facts WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
}

#[test]
fn a_topic_from_last_week_is_picked_by_its_keywords() {
    let (_t, s) = fixture();
    let got = s.recall("I'm nervous about speaking at the meetup again", &now()).unwrap();
    assert_eq!(got, [format!("2026-11-20: {TALK}")]);
}

#[test]
fn an_unrelated_message_recalls_nothing() {
    let (_t, s) = fixture();
    assert!(s.recall("what's a good pasta recipe", &now()).unwrap().is_empty());
    assert!(s.recall("", &now()).unwrap().is_empty());
    assert!(s.recall("what did we talk about", &now()).unwrap().is_empty(), "only stopwords");
    assert!(s.recall("???", &now()).unwrap().is_empty());
}

#[test]
fn a_date_word_returns_that_days_episodes_without_any_keyword() {
    let (_t, s) = fixture();
    let want = ["2026-11-26: Fixed the login bug at Lanternfish Labs", "2026-11-26: Walked to the lake with Lan"];
    for msg in ["what did we talk about yesterday?", "hôm qua mình nói gì nhỉ", "hom qua", "HÔM QUA", "Yesterday"] {
        let mut got = recall_now(&s, msg);
        got.sort();
        assert_eq!(got, want, "{msg}");
    }
    // a day with one episode, other date forms
    assert_eq!(recall_now(&s, "what did I do last Friday"), [format!("2026-11-20: {TALK}")]);
    assert_eq!(recall_now(&s, "thứ 6 tuần trước thế nào"), [format!("2026-11-20: {TALK}")]);
    assert_eq!(recall_now(&s, "what happened on 2026-11-25?"), ["2026-11-25: Cooked noodles for lunch and fixed a bike tyre"]);
    // a day with nothing, and today
    assert!(s.recall("what did we do today?", &now()).unwrap().is_empty());
}

fn recall_now(s: &Store, msg: &str) -> Vec<String> {
    s.recall(msg, &now()).unwrap()
}

#[test]
fn last_week_gives_at_most_three_lines() {
    let (_t, s) = fixture();
    let got = s.recall("tuần trước", &now()).unwrap();
    assert_eq!(got.len(), TOP);
    assert!(got[0].starts_with("2026-11-26") || got[0].starts_with("2026-11-25"), "newest first among equals: {got:?}");
}

#[test]
fn core_rows_are_never_recalled() {
    let (_t, s) = fixture();
    city(&s, "Hanoi", 3, &now());
    s.add("preference", "Likes the quiet meetup mornings", &now()).unwrap(); // listed in the section
    s.add("promise", "Ask how the meetup talk went", &now()).unwrap();
    assert!(s.section().contains("meetup mornings"));
    let got = s.recall("Hanoi meetup mornings", &now()).unwrap();
    assert!(got.iter().all(|l| !l.contains("Hanoi") && !l.contains("mornings") && !l.contains("Ask how")), "{got:?}");
    assert_eq!(got, [format!("2026-11-20: {TALK}")]);
    assert!(s.recall("Hanoi", &now()).unwrap().is_empty());
}

#[test]
fn a_keyless_note_the_section_does_not_list_can_be_recalled() {
    let (_t, s) = store();
    // 5 promises fit the section; the 6th (lowest importance, so last) is cut and so recallable
    for i in 0..6 {
        s.add("promise", &format!("Ask about the zebra project part {i}"), &now_on(date(2026, 11, 20 + i))).unwrap();
    }
    let in_section = s.section();
    let got = s.recall("zebra", &now()).unwrap();
    for l in &got {
        let text = l.split_once(": ").unwrap().1;
        assert!(!in_section.contains(text), "{text} is in the section");
    }
    assert_eq!(got.len(), 1);
}

#[test]
fn at_most_three_lines_and_600_characters() {
    let (_t, s) = store();
    for i in 0..5 {
        episode(&s, date(2026, 11, 26), &format!("Item {i} {}", "word ".repeat(36)), 5);
    }
    let got = s.recall("yesterday", &now()).unwrap();
    assert!(!got.is_empty() && got.len() <= TOP, "{}", got.len());
    assert!(got.iter().map(|l| l.chars().count()).sum::<usize>() <= MAX_CHARS);
    // a line too long for what is left is skipped, and a shorter one still fits
    let (_t, s) = store();
    episode(&s, date(2026, 11, 26), &format!("Long one {}", "lorem ".repeat(31)), 9);
    episode(&s, date(2026, 11, 26), &format!("Long two {}", "ipsum ".repeat(31)), 8);
    episode(&s, date(2026, 11, 26), &format!("Long three {}", "dolor ".repeat(31)), 7);
    episode(&s, date(2026, 11, 26), "Short four", 6);
    let got = s.recall("yesterday", &now()).unwrap();
    assert_eq!(got.len(), 3);
    assert!(got[0].contains("Long one") && got[1].contains("Long two") && got[2].contains("Short four"), "{got:?}");
    assert!(got.iter().map(|l| l.chars().count()).sum::<usize>() <= MAX_CHARS);
}

#[test]
fn only_the_rows_sent_are_marked_as_recalled() {
    let (t, s) = fixture();
    let talk = s.list(Some("episode")).unwrap().into_iter().find(|f| f.text == TALK).unwrap().id;
    let cooked = s.list(Some("episode")).unwrap().into_iter().find(|f| f.text.starts_with("Cooked")).unwrap().id;
    let rev = s.core_rev();
    assert_eq!(s.recall("the meetup talk", &now()).unwrap().len(), 1);
    assert_eq!(recall_n(&t.mem(), talk), (1, Some(now().at)));
    assert_eq!(recall_n(&t.mem(), cooked), (0, None));
    assert_eq!(s.recall("the meetup talk", &now()).unwrap().len(), 1);
    assert_eq!(recall_n(&t.mem(), talk).0, 2);
    assert_eq!(s.core_rev(), rev, "recall never moves the section");
    // nothing sent, nothing written
    s.recall("pasta", &now()).unwrap();
    assert_eq!(count(&t.mem(), "SELECT sum(recall_n) FROM facts"), 2);
}

#[test]
fn a_forgotten_or_superseded_episode_never_appears() {
    let (t, s) = fixture();
    let ids: Vec<i64> = s.recall_span(&Span::Day(date(2026, 11, 26))).unwrap().iter().map(|f| f.id).collect();
    s.soft_forget(ids[0]).unwrap();
    db(&t.mem()).execute("UPDATE facts SET status = 'superseded' WHERE id = ?1", [ids[1]]).unwrap();
    assert!(s.recall("yesterday", &now()).unwrap().is_empty());
    assert!(s.recall("login bug lake", &now()).unwrap().is_empty());
    s.erase(ids[1]).unwrap();
    assert!(s.recall("yesterday", &now()).unwrap().is_empty());
}

#[test]
fn of_two_equal_episodes_the_newer_wins() {
    let (_t, s) = store();
    episode(&s, date(2026, 10, 20), "Rehearsed the meetup talk alone", 5);
    episode(&s, date(2026, 11, 20), "Rehearsed the meetup talk again", 5);
    let got = s.recall("meetup talk", &now()).unwrap();
    assert_eq!(got.len(), 2);
    assert!(got[0].starts_with("2026-11-20") && got[1].starts_with("2026-10-20"), "{got:?}");
}

#[test]
fn old_and_minor_episodes_fall_below_the_minimum_score() {
    let (_t, s) = store();
    episode(&s, date(2026, 8, 1), "Rehearsed the meetup talk", 5); // 118 days: 0.5^3.9 of the weight
    assert!(s.recall("meetup talk", &now()).unwrap().is_empty());
    // but a recent one at the same importance is found
    episode(&s, date(2026, 11, 24), "Rehearsed the meetup talk again", 5);
    assert_eq!(s.recall("meetup talk", &now()).unwrap().len(), 1);
}

#[test]
fn importance_lifts_the_score() {
    let (_t, s) = store();
    // 45 days old: 0.35 × (0.5 + imp/10): imp 3 → 0.28 (kept), so check the order instead
    episode(&s, date(2026, 10, 13), "Rehearsed the meetup talk minor", 3);
    episode(&s, date(2026, 10, 13), "Rehearsed the meetup talk major", 9);
    let got = s.recall("meetup talk", &now()).unwrap();
    assert_eq!(got.len(), 2);
    assert!(got[0].ends_with("major"), "{got:?}");
}

#[test]
fn what_keeps_coming_up_stays_fresh() {
    let (_t, s) = store();
    episode(&s, date(2026, 8, 1), "Rehearsed the meetup talk", 9); // 0.5^3.9 × 1.4 = 0.093: too old
    assert!(s.recall("meetup", &now()).unwrap().is_empty());
    // the same row, recalled yesterday, counts its age from then
    let id = s.list(Some("episode")).unwrap()[0].id;
    let c = db(s.dir());
    c.execute("UPDATE facts SET last_recall = '2026-11-26T09:00:00+07:00' WHERE id = ?1", [id]).unwrap();
    assert_eq!(s.recall("meetup", &now()).unwrap(), ["2026-08-01: Rehearsed the meetup talk"]);
}

#[test]
fn the_query_is_built_safely() {
    let (_t, s) = fixture();
    // quotes, operators and punctuation are plain words
    for msg in ["\"meetup\" OR NOT AND NEAR(x) *", "meetup\" -- ; DROP TABLE facts", "talk:talk (meetup", "meetup OR", "'"] {
        s.recall(msg, &now()).unwrap();
    }
    assert_eq!(s.list(Some("episode")).unwrap().len(), 4);
    // more than 12 words is fine
    let long = (0..30).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ");
    assert!(s.recall(&format!("{long} meetup"), &now()).unwrap().is_empty(), "meetup is past the 12th term");
    assert_eq!(s.recall(&format!("meetup {long}"), &now()).unwrap().len(), 1);
}

#[test]
fn a_read_only_store_recalls_nothing_and_writes_nothing() {
    let (t, s) = fixture();
    drop(s);
    db(&t.mem()).pragma_update(None, "user_version", 7).unwrap();
    let s = Store::open(&t.mem()).unwrap();
    assert!(s.is_newer());
    assert!(s.recall("yesterday", &now()).unwrap().is_empty());
    assert!(s.recall("meetup talk", &now()).unwrap().is_empty());
    assert_eq!(count(&t.mem(), "SELECT sum(recall_n) FROM facts"), 0);
}

#[test]
fn recall_span_lists_a_day_or_a_range_oldest_first_with_their_days() {
    let (_t, s) = fixture();
    let day = s.recall_span(&Span::Day(date(2026, 11, 26))).unwrap();
    assert_eq!(day.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["Fixed the login bug at Lanternfish Labs", "Walked to the lake with Lan"]);
    assert!(day.iter().all(|f| f.day.as_deref() == Some("2026-11-26") && f.kind == "episode"));
    let week = s.recall_span(&Span::Range(date(2026, 11, 20), date(2026, 11, 26))).unwrap();
    let days: Vec<_> = week.iter().map(|f| f.day.clone().unwrap()).collect();
    assert_eq!(days, ["2026-11-20", "2026-11-25", "2026-11-26", "2026-11-26"]);
    assert!(s.recall_span(&Span::Day(date(2026, 11, 21))).unwrap().is_empty());
    // the day shows in JSON for recall, and not for the other lists
    assert_eq!(json!(week[0])["day"], "2026-11-20");
    assert!(json!(s.list(Some("episode")).unwrap()[0]).get("day").is_none());
    // reading a day is not a recall
    assert_eq!(count(s.dir(), "SELECT sum(recall_n) FROM facts"), 0);
}

#[test]
fn recall_span_skips_core_rows_and_forgotten_ones() {
    let (_t, s) = fixture();
    s.add("preference", "Likes tea", &now_on(date(2026, 11, 26))).unwrap();
    let id = s.recall_span(&Span::Day(date(2026, 11, 26))).unwrap()[0].id;
    s.soft_forget(id).unwrap();
    assert_eq!(s.recall_span(&Span::Day(date(2026, 11, 26))).unwrap().len(), 1);
}

// ---- search over FTS5 ----

#[test]
fn search_uses_fts_with_accents_and_case_folded() {
    let (_t, s) = store();
    s.add("profile", "Lives in Hà Nội", &now()).unwrap();
    episode(&s, date(2026, 11, 26), "Found a tea shop in Hà Nội", 5);
    assert_eq!(s.search("HA NOI", 5).unwrap().len(), 2);
    assert_eq!(s.search("hà nội", 5).unwrap().len(), 2);
    assert_eq!(s.search("tea", 5).unwrap().len(), 1);
    // a one-letter word is a word here (recall drops them, search does not)
    assert_eq!(s.search("a tea", 5).unwrap().len(), 1);
    // best BM25 first
    let hits = s.search("tea shop noi", 5).unwrap();
    assert_eq!(hits[0].text, "Found a tea shop in Hà Nội");
    assert!(matches!(s.search("   ", 5), Err(Error::Usage(_))));
    assert!(s.search("!!!", 5).unwrap().is_empty());
}

#[test]
fn search_finds_live_rows_only_and_honors_the_limit() {
    let (_t, s) = store();
    for i in 0..6 {
        episode(&s, date(2026, 11, 20 + i), &format!("Rehearsal number {i} for the meetup"), 5);
    }
    assert_eq!(s.search("meetup", 3).unwrap().len(), 3);
    assert_eq!(s.search("meetup", 100).unwrap().len(), 6);
    let id = s.search("rehearsal", 1).unwrap()[0].id;
    s.soft_forget(id).unwrap();
    assert!(s.search("meetup", 100).unwrap().iter().all(|f| f.id != id));
    s.erase(s.search("meetup", 1).unwrap()[0].id).unwrap();
    assert_eq!(s.search("meetup", 100).unwrap().len(), 4);
}

// ---- review fixes ----

#[test]
fn d_and_dj_match_both_ways() {
    // the index folds accents but not đ: typing "dan bau" must still find "đàn bầu", and the other way
    let (_t, s) = store();
    episode(&s, date(2026, 11, 25), "Học đàn bầu với Lan", 6);
    episode(&s, date(2026, 11, 26), "Mochi dang ngủ trên ghế", 5);
    assert_eq!(recall_now(&s, "dan bau hôm nay sao rồi"), ["2026-11-25: Học đàn bầu với Lan"]);
    assert_eq!(recall_now(&s, "Đàn bầu"), ["2026-11-25: Học đàn bầu với Lan"]);
    assert_eq!(recall_now(&s, "mochi đang ngủ"), ["2026-11-26: Mochi dang ngủ trên ghế"]);
    assert_eq!(s.search("dan", 5).unwrap().len(), 2, "đàn and dang");
    assert_eq!(s.search("ĐÀN", 5).unwrap().len(), 2);
    // many d's stay one bounded query
    s.recall("dddddddddddddd dđdđdđdđdđ", &now()).unwrap();
}

#[test]
fn search_matches_the_start_of_a_word_recall_does_not() {
    let (_t, s) = store();
    episode(&s, date(2026, 11, 26), "Baked biscuits with Minh", 5);
    assert_eq!(s.search("bisc", 5).unwrap().len(), 1, "a prefix");
    assert_eq!(s.search("biscuit", 5).unwrap().len(), 1);
    assert!(s.search("is", 5).unwrap().is_empty(), "two letters stay whole words");
    assert!(s.recall("bisc", &now()).unwrap().is_empty(), "recall stays on whole words");
}

#[test]
fn a_date_word_finds_an_old_day_and_ranks_before_keywords() {
    let (_t, s) = store();
    episode(&s, date(2026, 8, 1), "Moved into the flat by the lake", 3); // far below the minimum by age
    episode(&s, date(2026, 11, 26), "Talked about the lake trip", 9);
    assert_eq!(recall_now(&s, "what happened on 2026-08-01?"), ["2026-08-01: Moved into the flat by the lake"]);
    let got = recall_now(&s, "the lake on 2026-08-01");
    assert_eq!(got[0], "2026-08-01: Moved into the flat by the lake", "date words win: {got:?}");
}

#[test]
fn list_without_a_kind_is_the_core_facts() {
    let (_t, s) = fixture();
    s.add("preference", "Likes tea", &now()).unwrap();
    assert_eq!(s.list(None).unwrap().iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["Likes tea"]);
    assert_eq!(s.list(Some("episode")).unwrap().len(), 4);
}
