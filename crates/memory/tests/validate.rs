mod common;
use common::*;
use memory::{looks_secret, Error, Outcome, RememberIn};
use serde_json::json;

fn one(s: &memory::Store, v: serde_json::Value, said: &str) -> Outcome {
    extract(s, said, vec![v], &now()).items[0].outcome.clone()
}

fn source_of(s: &memory::Store, id: i64) -> &'static str {
    s.list(None).unwrap().into_iter().find(|f| f.id == id).unwrap().source
}

#[test]
fn unknown_predicate_becomes_a_keyless_note() {
    let (_t, s) = store();
    let Outcome::Created(id) = one(&s, item("profile", Some("shoe_size"), Some("42"), "Wears size 42"), "x") else { panic!() };
    let f = s.list(None).unwrap().into_iter().find(|f| f.id == id).unwrap();
    assert_eq!((f.key, f.value), (None, None));
}

#[test]
fn cardinality_and_kind_come_from_the_registry() {
    let (_t, s) = store();
    // The model calls city a "preference": the registry says profile, one value.
    one(&s, item("preference", Some("home_city"), Some("Hanoi"), "Lives in Hanoi"), "x");
    let out = one(&s, item("preference", Some("home_city"), Some("Hue"), "Lives in Hue"), "x");
    assert!(matches!(out, Outcome::Superseded { .. }), "single-valued: {out:?}");
    assert_eq!(s.list(Some("profile")).unwrap().len(), 1);
    // A known predicate with no value can't be keyed: a note.
    let Outcome::Created(id) = one(&s, item("profile", Some("job"), None, "Works somewhere"), "x") else { panic!() };
    assert_eq!(s.list(None).unwrap().iter().find(|f| f.id == id).unwrap().key, None);
    // A qualifier on a predicate that takes none is dropped.
    let Outcome::Created(id) = one(&s, with(item("profile", Some("job"), Some("baker"), "Bakes"), "qualifier", json!("x")), "x") else { panic!() };
    assert_eq!(s.list(None).unwrap().iter().find(|f| f.id == id).unwrap().key.as_deref(), Some("job"));
}

#[test]
fn bounds_are_checked_per_item() {
    let (_t, s) = store();
    let base = || item("preference", None, None, "Likes tea");
    let long = |n: usize| "a".repeat(n);
    let cases = [
        (with(base(), "importance", json!(0)), "importance"),
        (with(base(), "importance", json!(11)), "importance"),
        (with(base(), "importance", json!(null)), "importance"),
        (with(base(), "text", json!("")), "text"),
        (with(base(), "text", json!(long(201))), "text"),
        (with(base(), "kind", json!("mood")), "kind"),
        (with(base(), "kind", json!("insight")), "kind"),
        (with(base(), "validFrom", json!("2026-13-40")), "valid_from"),
        (with(base(), "validFrom", json!("last month")), "valid_from"),
        (with(base(), "validFrom", json!("2028-01-01")), "valid_from"),
        (with(item("profile", Some("job"), Some(&long(101)), "Works"), "text", json!("Works")), "value"),
    ];
    for (v, why) in cases {
        assert_eq!(one(&s, v, "x"), Outcome::Skipped(why), "{why}");
    }
    assert!(s.list(None).unwrap().is_empty());
    // The edges are fine: 200 chars of text, importance 1 and 10.
    assert!(matches!(one(&s, with(base(), "text", json!("x".repeat(200))), "x"), Outcome::Created(_)));
    assert!(matches!(one(&s, with(with(base(), "text", json!("Other")), "importance", json!(10)), "x"), Outcome::Created(_)));
    assert!(matches!(one(&s, with(with(base(), "text", json!("Another")), "importance", json!(1)), "x"), Outcome::Created(_)));
}

#[test]
fn valid_from_accepts_a_day_or_a_month_and_at_most_a_year_ahead() {
    let (tmp, s) = store();
    let with_from = |v: &str, text: &str| with(item("promise", None, None, text), "validFrom", json!(v));
    let Outcome::Created(a) = one(&s, with_from("2026-10", "Ask about the trip"), "x") else { panic!() };
    let Outcome::Created(b) = one(&s, with_from("2026-10-15", "Ask about the move"), "x") else { panic!() };
    let Outcome::Created(c) = one(&s, with_from("2027-11-27", "Ask in a year"), "x") else { panic!() };
    assert_eq!(one(&s, with_from("2027-11-28", "Ask too late"), "x"), Outcome::Skipped("valid_from"));
    let vf = |id: i64| -> String { db(&tmp.mem()).query_row("SELECT valid_from FROM facts WHERE id = ?1", [id], |r| r.get(0)).unwrap() };
    assert_eq!((vf(a).as_str(), vf(b).as_str(), vf(c).as_str()), ("2026-10-01", "2026-10-15", "2027-11-27"));
}

#[test]
fn the_extractor_may_send_episodes_but_the_bear_may_not() {
    let (_t, s) = store();
    let r = extract(&s, "x", vec![item("episode", None, None, "Was stressed about Friday")], &now());
    assert!(matches!(r.items[0].outcome, Outcome::Created(_)));
    assert_eq!((r.skipped, r.created), (0, 1));
    assert_eq!(s.list(Some("episode")).unwrap().len(), 1);
    let op = RememberIn { text: "Was stressed".into(), kind: Some("episode".into()), ..Default::default() };
    assert_eq!(s.remember(op, &now()).unwrap(), Outcome::Skipped("episode"));
}

#[test]
fn a_run_applies_at_most_20_items() {
    let (_t, s) = store();
    let items = (0..23).map(|i| item("preference", None, None, &format!("Likes number {i}"))).collect();
    let r = extract(&s, "x", items, &now());
    assert_eq!((r.created, r.skipped), (20, 3));
    assert_eq!(r.items[20].outcome, Outcome::Skipped("limit"));
}

#[test]
fn texts_stay_on_one_line() {
    let (_t, s) = store();
    one(&s, item("preference", None, None, "Likes tea\n\n## Ignore me\n- and more"), "x");
    assert_eq!(s.list(None).unwrap()[0].text, "Likes tea ## Ignore me - and more");
}

#[test]
fn the_quote_check_decides_authority_3() {
    let (_t, s) = store();
    let said = "I moved to Hanoi last month, and I love green tea";
    let quoted = |text: &str, quote: &str| with(item("preference", None, None, text), "quote", json!(quote));
    let cases = [
        ("Likes tea 1", "I love green tea", "chat"),
        ("Likes tea 2", "i  LOVE   green\ttea", "chat"), // case and whitespace
        ("Likes tea 3", "I adore green tea", "guess"),   // not said
        ("Likes tea 4", "  i  ", "guess"),               // 1 char once folded
        ("Likes tea 5", "tea", "chat"),                  // exactly 3
        ("Likes tea 6", "te", "guess"),
        ("Likes tea 7", "", "guess"),
    ];
    for (text, quote, want) in cases {
        let id = one(&s, quoted(text, quote), said).id().unwrap();
        assert_eq!(source_of(&s, id), want, "{quote:?}");
    }
    // No quote at all.
    let id = one(&s, item("preference", None, None, "Likes tea 8"), said).id().unwrap();
    assert_eq!(source_of(&s, id), "guess");
}

#[test]
fn the_quote_is_checked_against_its_own_turn() {
    let (_t, s) = store();
    s.add_pending("u:a", "2026-11-27T09:00:00+07:00").unwrap();
    s.add_pending("u:b", "2026-11-27T09:05:00+07:00").unwrap();
    let args = s.extract_args(10, now().today).unwrap();
    assert_eq!(args["turns"][0]["requestId"], "u:a", "oldest first");
    let mk = |turn: &str, text: &str| with(with(item("preference", None, None, text), "turn", json!(turn)), "quote", json!("I like oat milk"));
    let reply = json!({
        "items": [mk("u:a", "Likes oat milk"), mk("u:b", "Likes oat milk too")],
        "turns": [{"requestId": "u:a", "said": "I like oat milk"}, {"requestId": "u:b", "said": "Something else"}],
        "missing": [],
    });
    let r = s.apply_extract(&args, &reply, &now()).unwrap();
    assert_eq!(r.items[0].turn.as_deref(), Some("u:a"));
    assert_eq!(source_of(&s, r.items[0].outcome.id().unwrap()), "chat");
    assert_eq!(source_of(&s, r.items[1].outcome.id().unwrap()), "guess");
}

#[test]
fn the_quote_check_ignores_unicode_normal_form_and_case() {
    let (_t, s) = store();
    // NFC "Mình ở Hà Nội" vs the same words decomposed (NFD).
    use unicode_normalization::UnicodeNormalization;
    let nfc: String = "Mình ở Hà Nội".nfc().collect();
    let nfd: String = nfc.nfd().collect();
    assert_ne!(nfc, nfd);
    for (n, (quote, said)) in [(&nfd, &nfc), (&nfc, &nfd), (&nfc.to_uppercase(), &nfc)].into_iter().enumerate() {
        let v = with(item("profile", Some("home_city"), Some("Hà Nội"), &format!("Lives in Ha Noi {n}")), "quote", json!(quote));
        let out = one(&s, v, said);
        let id = out.id().unwrap();
        assert_eq!(source_of(&s, id), "chat", "pair {n}: {out:?}");
    }
}

#[test]
fn the_secret_filter_matches_the_brief() {
    let hex = "a1b2c3d4".repeat(4); // 32
    let b64 = format!("Qx7{}Zk9", "aB3dE5fG7hJ1kL".repeat(2));
    let yes = [
        hex.clone(),
        format!("key {hex} ok"),
        b64.clone(),
        format!("sk-{}", "proj123"),
        format!("sk_{}", "live12345"),
        format!("{}_{}", "ghp", "abcdef123456"),
        format!("{}_{}", "gho", "abcdef123456"),
        format!("{}_pat_{}", "github", "11AAAA"),
        format!("{}-{}", "xoxb", "12345-abcdef"),
        format!("{}{}", "AKIA", "IOSFODNN7EXAMPLE"),
        format!("{} {}", "-----BEGIN", "PRIVATE KEY-----"),
        "My password is hunter2".to_string(),
        "the PASSCODE IS 4821".to_string(),
        "my pin is 0000".to_string(),
        "wifi password: hunter2".to_string(),
        "password for the router is lantern42".to_string(),
        "door PIN = 4821".to_string(),
        "mật khẩu wifi là abc".to_string(),
        "mật khẩu: abc".to_string(), // `mật khẩu` then `:` is a token
        unicode_normalization::UnicodeNormalization::nfd("mật khẩu wifi").collect::<String>(), // decomposed: still caught
    ];
    for s in &yes {
        assert!(looks_secret(s), "should be secret: {s}");
    }
    let no = [
        "Lives in Hanoi".to_string(),
        "Works at Lanternfish Labs on desk-buddy".to_string(),
        "Has a cat, Mochi".to_string(),
        "Asked about the spin is hard".to_string(),
        "Likes the task-based approach".to_string(),
        "a1b2c3d4".repeat(3), // 24 hex chars: too short
        "Sister Lan visits on the 12th; session notes in a-very-long-but-plain-hyphenated-slug-here".to_string(),
        "Quên mật khẩu.".to_string(),
        "Uses a password manager".to_string(),
        "Forgot the password for the router again".to_string(),
        "2026-11-27T09:12:03+07:00".to_string(),
    ];
    for s in &no {
        assert!(!looks_secret(s), "should not be secret: {s}");
    }
}

#[test]
fn secrets_are_refused_on_every_write_path() {
    let (_t, s) = store();
    let key = format!("{}{}", "AKIA", "IOSFODNN7EXAMPLE");
    let said = format!("my password is {key}");

    // remember: text and value
    for (text, value) in [(said.as_str(), None), ("Has a key", Some(key.as_str()))] {
        let op = RememberIn { text: text.into(), key: value.map(|_| "projects".into()), value: value.map(str::to_string), ..Default::default() };
        assert_eq!(s.remember(op, &now()), Err(Error::Secret));
    }
    // remember: a key's qualifier (`--key people:<token>`)
    let op = RememberIn { text: "Has a sister".into(), key: Some(format!("people:{key}")), value: Some("Lan".into()), ..Default::default() };
    assert_eq!(s.remember(op, &now()), Err(Error::Secret));
    // add and edit
    assert_eq!(s.add("profile", &said, &now()), Err(Error::Secret));
    let id = s.add("profile", "Likes tea", &now()).unwrap();
    assert_eq!(s.edit(id, &said, None, &now()), Err(Error::Secret));
    assert_eq!(s.edit(id, "Likes tea", Some(&key), &now()), Err(Error::Secret));
    // extract: text, value and quote
    let r = extract(
        &s,
        &said,
        vec![
            item("profile", None, None, &said),
            item("profile", Some("projects"), Some(&key), "Has a project"),
            with(item("profile", None, None, "Has a secret"), "quote", json!(said)),
        ],
        &now(),
    );
    assert_eq!(r.skipped, 3);
    assert!(r.items.iter().all(|i| i.outcome == Outcome::Skipped("secret")));
    assert_eq!(s.list(None).unwrap().len(), 1, "only 'Likes tea' exists");
}

#[test]
fn remember_checks_its_arguments() {
    let (_t, s) = store();
    let op = |text: &str| RememberIn { text: text.into(), ..Default::default() };
    assert!(matches!(s.remember(op(""), &now()), Err(Error::Usage(_))));
    assert!(matches!(s.remember(op("   "), &now()), Err(Error::Usage(_))));
    assert!(matches!(s.remember(op(&"a".repeat(201)), &now()), Err(Error::Usage(_))));
    assert!(matches!(s.remember(RememberIn { kind: Some("mood".into()), ..op("Likes tea") }, &now()), Err(Error::Usage(_))));
    assert!(matches!(s.remember(RememberIn { importance: Some(11), ..op("Likes tea") }, &now()), Err(Error::Usage(_))));
    assert!(matches!(s.remember(RememberIn { from: Some("soon".into()), ..op("Likes tea") }, &now()), Err(Error::Usage(_))));
    assert!(matches!(s.remember(RememberIn { key: Some("home_city".into()), ..op("Lives in Hue") }, &now()), Err(Error::Usage(_))));
    // An unknown key is a note; "pred:qual" splits.
    let Outcome::Created(a) = s.remember(RememberIn { key: Some("shoe_size".into()), value: Some("42".into()), ..op("Wears 42") }, &now()).unwrap() else { panic!() };
    assert_eq!(s.list(None).unwrap()[0].key, None);
    let Outcome::Created(b) =
        s.remember(RememberIn { key: Some("pets:cat".into()), value: Some("Mochi".into()), ..op("Has a cat, Mochi") }, &now()).unwrap()
    else {
        panic!()
    };
    assert_ne!(a, b);
    assert_eq!(s.list(None).unwrap().iter().find(|f| f.id == b).unwrap().key.as_deref(), Some("pets:cat"));
    let Outcome::Created(c) = s.remember(RememberIn { from: Some("2026-10".into()), ..op("Started a new job") }, &now()).unwrap() else { panic!() };
    assert_eq!(s.list(None).unwrap().iter().find(|f| f.id == c).unwrap().since.as_deref(), Some("2026-10"));
}

#[test]
fn extract_args_has_the_i1_shape() {
    let (_t, s) = store();
    assert!(s.extract_args(10, now().today).is_none());
    city(&s, "Hanoi", 3, &now());
    for i in 0..12 {
        s.add_pending(&format!("u:{i:02}"), &format!("2026-11-27T09:{i:02}:00+07:00")).unwrap();
    }
    s.add_pending("s:sense", "2026-11-27T08:00:00+07:00").unwrap();
    s.add_pending("c:continue", "2026-11-27T08:00:00+07:00").unwrap();
    let a = s.extract_args(10, now().today).unwrap();
    let turns = a["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 10);
    assert_eq!(turns[0], json!({"requestId": "u:00", "endedAt": "2026-11-27T09:00:00+07:00"}));
    assert_eq!(s.extract_args(3, now().today).unwrap()["turns"].as_array().unwrap().len(), 3);
    assert_eq!(a["today"], "2026-11-27");
    assert_eq!(a["episodes"], true);
    let known = a["known"].as_array().unwrap();
    assert_eq!((known.len(), &known[0]["predicate"], &known[0]["value"], &known[0]["subject"]), (1, &json!("home_city"), &json!("Hanoi"), &json!("user")));
    assert!(known[0]["id"].is_i64() && known[0].get("qualifier").is_some());
    let reg = a["registry"].as_array().unwrap();
    assert_eq!(reg.len(), 12);
    assert!(reg.contains(&json!({"key": "people", "kind": "profile", "one": false, "qualifier": "relation"})));
    assert!(reg.contains(&json!({"key": "home_city", "kind": "profile", "one": true, "qualifier": null})));
    assert!(reg.contains(&json!({"key": "pets", "kind": "profile", "one": false, "qualifier": "species"})));
}

#[test]
fn apply_extract_clears_the_pending_turns_including_missing_ones() {
    let (tmp, s) = store();
    for id in ["u:a", "u:b", "u:c"] {
        s.add_pending(id, "2026-11-27T09:00:00+07:00").unwrap();
    }
    let args = s.extract_args(2, now().today).unwrap(); // u:a and u:b
    let reply = json!({"items": [], "turns": [{"requestId": "u:a", "said": "hi"}], "missing": ["u:b"]});
    let r = s.apply_extract(&args, &reply, &now()).unwrap();
    assert_eq!(r.cleared, 2);
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM pending_turns"), 1, "u:c is still waiting");
    // A reply with no items array is a usage error and clears nothing.
    let args = s.extract_args(1, now().today).unwrap();
    assert!(matches!(s.apply_extract(&args, &json!({}), &now()), Err(Error::Usage(_))));
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM pending_turns"), 1);
}

#[test]
fn an_item_naming_no_known_turn_is_a_guess_with_no_source() {
    let (_t, s) = store();
    let v = with(with(item("preference", None, None, "Likes tea"), "turn", json!("u:other")), "quote", json!("I like tea"));
    let r = extract(&s, "I like tea", vec![v], &now());
    assert_eq!(r.items[0].turn, None);
    assert_eq!(source_of(&s, r.items[0].outcome.id().unwrap()), "guess");
}

#[test]
fn an_episode_must_be_keyless_and_at_least_importance_3() {
    let (t, s) = store();
    let r = extract(
        &s,
        "x",
        vec![
            item("episode", Some("home_city"), Some("Hanoi"), "Was in Hanoi"),
            with(item("episode", None, None, "Minor thing"), "importance", json!(2)),
            with(item("episode", None, None, "Borderline thing"), "importance", json!(3)),
            item("insight", None, None, "A pattern the extractor made up"),
        ],
        &now(),
    );
    let o: Vec<&Outcome> = r.items.iter().map(|i| &i.outcome).collect();
    assert_eq!(o[0], &Outcome::Skipped("predicate"));
    assert_eq!(o[1], &Outcome::Skipped("low"));
    assert!(matches!(o[2], Outcome::Created(_)));
    assert_eq!(o[3], &Outcome::Skipped("kind"));
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts"), 1);
    // the bear's command and the window can't make an insight either
    let op = RememberIn { text: "A pattern".into(), kind: Some("insight".into()), ..Default::default() };
    assert!(matches!(s.remember(op, &now()), Ok(Outcome::Skipped(_)) | Err(_)));
    assert!(s.add("insight", "A pattern", &now()).is_err());
}

#[test]
fn an_episode_with_a_secret_is_refused_and_not_counted() {
    let (t, s) = store();
    let secret = format!("Pasted {}{}", "sk", "-live-abc123def456ghi");
    let r = extract(&s, "x", vec![with(item("episode", None, None, &secret), "importance", json!(9))], &now());
    assert_eq!(r.created, 0);
    assert_eq!(meta(&t.mem(), "importance_since_reflection"), None);
}
