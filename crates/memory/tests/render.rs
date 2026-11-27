mod common;
use common::*;
use serde_json::json;

fn said(item: serde_json::Value, quote: &str) -> serde_json::Value {
    with(item, "quote", json!(quote))
}

fn imp(item: serde_json::Value, n: i64) -> serde_json::Value {
    with(item, "importance", json!(n))
}

const SAID: &str = "I live in Hanoi since October. I want short replies. I am a software and systems engineer";

fn fixture() -> (Tmp, memory::Store) {
    let (tmp, s) = store();
    let hanoi = with(imp(said(item("profile", Some("home_city"), Some("Hanoi"), "Lives in Hanoi"), "I live in Hanoi"), 8), "validFrom", json!("2026-10"));
    let r = extract(
        &s,
        SAID,
        vec![
            hanoi,
            imp(item("profile", Some("job"), Some("engineer"), "Software and systems engineer"), 7),
            with(imp(item("profile", Some("pets"), Some("Mochi"), "Has a cat, Mochi"), 5), "qualifier", json!("cat")),
            imp(said(item("preference", Some("reply_style"), Some("short"), "Short replies"), "I want short replies"), 6),
            imp(item("preference", Some("checkin_hours"), Some("not before 10:00"), "No check-ins before 10:00"), 5),
            imp(item("promise", None, None, "Ask how Mochi's vet visit went"), 6),
            // a guess against the stated city
            imp(item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang"), 6),
        ],
        &now(),
    );
    assert_eq!(r.disputed, 1);
    (tmp, s)
}

#[test]
fn the_section_has_the_supplied_layout() {
    let (_t, s) = fixture();
    let want = "\
## What I know about you
These are notes from earlier chats. They can be out of date: if the person
says otherwise, believe them. Never recite this list unprompted.

About them
- Lives in Hanoi (since Oct 2026)
- Software and systems engineer
- Has a cat, Mochi

How they like help
- Short replies
- No check-ins before 10:00

Things you said you'd do
- Ask how Mochi's vet visit went

Not sure yet (ask when it fits, once)
- Maybe lives in Da Nang? You have Hanoi on record.

You remember past chats. If the person asks about the past and it isn't
here, run `buddy memory recall <date>` or `buddy memory search \"<words>\"`.";
    assert_eq!(s.section(), want);
}

#[test]
fn the_section_carries_no_date_of_today() {
    let (_t, s) = fixture();
    let t = s.section();
    for bad in ["2026-11-27", "Nov 2026", "today is", "Today"] {
        assert!(!t.contains(bad), "{bad}");
    }
}

#[test]
fn nothing_live_renders_nothing() {
    let (_t, s) = store();
    assert_eq!(s.section(), "");
    let id = s.add("profile", "Likes tea", &now()).unwrap();
    assert!(s.section().contains("Likes tea"));
    s.soft_forget(id).unwrap();
    assert_eq!(s.section(), "");
}

#[test]
fn rows_come_by_importance_then_newest() {
    let (_t, s) = store();
    let a = imp(item("preference", None, None, "Likes A"), 5);
    let b = imp(item("preference", None, None, "Likes B"), 9);
    extract(&s, "x", vec![a, b], &now_on(date(2026, 11, 1)));
    extract(&s, "x", vec![imp(item("preference", None, None, "Likes C"), 5)], &now_on(date(2026, 11, 20)));
    let lines: Vec<String> = s.section().lines().filter(|l| l.starts_with("- Likes")).map(str::to_string).collect();
    assert_eq!(lines, ["- Likes B", "- Likes C", "- Likes A"]);
}

#[test]
fn at_most_five_promises_and_two_questions() {
    let (_t, s) = store();
    let promises = (0..7).map(|i| imp(item("promise", None, None, &format!("Follow up on thing {i}")), 5 + (i % 3))).collect();
    extract(&s, "x", promises, &now());
    assert_eq!(s.section().matches("Follow up on").count(), 5);

    let said_it = |p: &str, v: &str| said(item("profile", Some(p), Some(v), &format!("Has {p} {v}")), "I said so");
    let guess = |p: &str, v: &str| item("profile", Some(p), Some(v), &format!("Has {p} {v}"));
    extract(&s, "I said so", vec![said_it("home_city", "Hanoi"), said_it("job", "baker"), said_it("employer", "Lanternfish Labs")], &now());
    let r = extract(&s, "x", vec![guess("home_city", "Hue"), guess("job", "chef"), guess("employer", "Moonlight Co")], &now());
    assert_eq!(r.disputed, 3);
    let t = s.section();
    assert_eq!(t.matches("Maybe ").count(), 2);
    assert_eq!(s.view(date(2026, 11, 27))["counts"]["disputes"], 3, "the window still lists all of them");
}

#[test]
fn over_the_cap_the_lowest_importance_rows_go_first() {
    let (_t, s) = store();
    let rows: Vec<_> = (0..60)
        .map(|i| imp(item("preference", None, None, &format!("Likes item imp{:02} n{i:02} {}", 1 + i % 10, "x".repeat(50))), 1 + i % 10))
        .collect();
    for batch in rows.chunks(20) {
        extract(&s, "x", batch.to_vec(), &now()); // a run applies at most 20 items
    }
    let t = s.section();
    assert!(t.chars().count() <= 2000, "{}", t.chars().count());
    let kept: Vec<i64> = t.lines().filter_map(|l| l.split("imp").nth(1)?.get(..2)?.parse().ok()).collect();
    assert!(kept.len() > 10 && kept.len() < 60, "{}", kept.len());
    let lowest_kept = *kept.iter().min().unwrap();
    // every importance above the lowest kept one is fully present
    for level in lowest_kept + 1..=10 {
        assert_eq!(kept.iter().filter(|&&k| k == level).count(), 6, "level {level}");
    }
    assert!(lowest_kept >= 4, "only the low rows were dropped: {lowest_kept}");
    // the fixed lines survive the cut
    assert!(t.starts_with("## What I know about you") && t.ends_with("`buddy memory search \"<words>\"`."));
}

#[test]
fn view_has_the_i6_shape() {
    let (_t, s) = store();
    let r = extract(
        &s,
        "I live in Hanoi",
        vec![
            with(imp(said(item("profile", Some("home_city"), Some("Hanoi"), "Lives in Hanoi"), "I live in Hanoi"), 8), "validFrom", json!("2026-10")),
            item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang"),
        ],
        &now(),
    );
    assert_eq!(r.created + r.disputed, 2);
    let want = json!({
        "v": 1, "today": "2026-11-27", "phrase": "forget everything",
        "groups": {
            "about": [{"id": 1, "kind": "profile", "text": "Lives in Hanoi", "key": "home_city", "label": "City",
                       "value": "Hanoi", "since": "2026-10", "source": "chat", "disputed": true}],
            "help": [], "promises": [],
        },
        "disputes": [{"id": 2, "key": "home_city", "label": "City", "value": "Da Nang", "text": "Lives in Da Nang",
                      "live": {"id": 1, "value": "Hanoi", "source": "chat"}}],
        "history": null, "historyMore": false,
        "counts": {"live": 1, "disputes": 1},
    });
    assert_eq!(s.view(date(2026, 11, 27)), want);

    // A keyless row has null key, label, value and since; a window row says "window".
    let id = s.add("promise", "Ask about Mochi", &now()).unwrap();
    let v = s.view(date(2026, 11, 27));
    assert_eq!(
        v["groups"]["promises"][0],
        json!({"id": id, "kind": "promise", "text": "Ask about Mochi", "key": null, "label": null, "value": null,
               "since": null, "source": "window", "disputed": false})
    );
}

#[test]
fn view_orders_rows_by_importance_then_newest_and_maps_every_source() {
    let (_t, s) = store();
    extract(&s, "x", vec![imp(item("preference", None, None, "Guess A"), 3)], &now());
    s.remember(
        memory::RememberIn { text: "Bear B".into(), kind: Some("preference".into()), importance: Some(7), ..Default::default() },
        &now(),
    )
    .unwrap();
    s.add("preference", "Window C", &now()).unwrap();
    extract(&s, "I said D", vec![imp(said(item("preference", None, None, "Chat D"), "I said D"), 9)], &now());
    let v = s.view(date(2026, 11, 27));
    let rows: Vec<(String, String)> =
        v["groups"]["help"].as_array().unwrap().iter().map(|r| (r["text"].as_str().unwrap().into(), r["source"].as_str().unwrap().into())).collect();
    let pairs = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(rows, [pairs("Chat D", "chat"), pairs("Bear B", "bear"), pairs("Window C", "window"), pairs("Guess A", "guess")]);
}
