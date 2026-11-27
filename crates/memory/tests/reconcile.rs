mod common;
use common::*;
use memory::{Outcome, RememberIn};
use serde_json::json;

fn note(s: &memory::Store, kind: &str, text: &str, now: &memory::Now) -> Outcome {
    extract(s, "x", vec![item(kind, None, None, text)], now).items[0].outcome.clone()
}

#[test]
fn keyless_creates_then_dedupes_within_30_days() {
    let (tmp, s) = store();
    let Outcome::Created(a) = note(&s, "preference", "Likes quiet mornings", &now()) else { panic!() };
    // Same text, different case and spacing, 10 days later: extended, one row.
    let later = now_on(date(2026, 12, 7));
    assert_eq!(note(&s, "preference", "likes  QUIET mornings", &later), Outcome::Extended(a));
    assert_eq!(s.list(None).unwrap().len(), 1);
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts WHERE observed_at LIKE '2026-12-07%'"), 1);
    // Another kind with the same text is its own row.
    assert!(matches!(note(&s, "profile", "Likes quiet mornings", &later), Outcome::Created(_)));
    // Last seen 40 days ago: a new row.
    let much_later = now_on(date(2027, 1, 16));
    assert!(matches!(note(&s, "preference", "Likes quiet mornings", &much_later), Outcome::Created(_)));
}

#[test]
fn extend_keeps_the_higher_importance_and_authority() {
    let (tmp, s) = store();
    let Outcome::Created(a) = note(&s, "preference", "Likes quiet mornings", &now()) else { panic!() };
    assert_eq!(row(&tmp.mem(), a).3, 1);
    let said = "I like quiet mornings";
    let it = with(with(item("preference", None, None, "Likes quiet mornings"), "quote", json!(said)), "importance", json!(9));
    assert_eq!(extract(&s, said, vec![it], &now()).items[0].outcome, Outcome::Extended(a));
    assert_eq!(row(&tmp.mem(), a).3, 3);
    let imp: i64 = db(&tmp.mem()).query_row("SELECT importance FROM facts WHERE id = ?1", [a], |r| r.get(0)).unwrap();
    assert_eq!(imp, 9);
    // A later guess with lower importance and authority lowers neither.
    let it = with(item("preference", None, None, "Likes quiet mornings"), "importance", json!(2));
    extract(&s, "x", vec![it], &now());
    assert_eq!(row(&tmp.mem(), a).3, 3);
}

#[test]
fn keyed_many_extends_the_same_value_and_creates_others() {
    let (_t, s) = store();
    let likes = |v: &str| item("preference", Some("likes"), Some(v), &format!("Likes {v}"));
    let a = extract(&s, "x", vec![likes("green tea")], &now()).items[0].outcome.clone();
    let b = extract(&s, "x", vec![likes("Green  Tea")], &now()).items[0].outcome.clone();
    assert!(matches!(a, Outcome::Created(_)));
    assert_eq!(b, Outcome::Extended(a.id().unwrap()));
    assert!(matches!(extract(&s, "x", vec![likes("coffee")], &now()).items[0].outcome, Outcome::Created(_)));
    assert_eq!(s.list(Some("preference")).unwrap().len(), 2);
    // The qualifier is part of the slot: a sister and a brother can both be called Lan.
    let people = |q: &str| with(item("profile", Some("people"), Some("Lan"), "Has a relative, Lan"), "qualifier", json!(q));
    let r = extract(&s, "x", vec![people("sister"), people("Sister"), people("brother")], &now());
    assert!(matches!(r.items[0].outcome, Outcome::Created(_)));
    assert!(matches!(r.items[1].outcome, Outcome::Extended(_)));
    assert!(matches!(r.items[2].outcome, Outcome::Created(_)));
    let keys: Vec<_> = s.list(Some("profile")).unwrap().into_iter().filter_map(|f| f.key).collect();
    assert!(keys.contains(&"people:sister".to_string()) && keys.contains(&"people:brother".to_string()));
}

#[test]
fn keyed_one_create_and_extend() {
    let (_t, s) = store();
    let id = city(&s, "Saigon", 3, &now());
    let again = extract(&s, "x", vec![item("profile", Some("home_city"), Some("saigon"), "Lives in Saigon")], &now());
    assert_eq!(again.items[0].outcome, Outcome::Extended(id));
}

/// Every authority pairing for a single-valued slot: the new value wins at `>=`, else it is disputed.
#[test]
fn keyed_one_authority_pairings() {
    let item_authorities: [(u8, &str); 3] = [(1, "guess"), (2, "bear"), (3, "chat")];
    for live_auth in 1..=4u8 {
        for (new_auth, _) in item_authorities {
            let (tmp, s) = store();
            let live = city(&s, "Hanoi", live_auth, &now());
            let out = match new_auth {
                2 => s
                    .remember(
                        RememberIn { text: "Lives in Da Nang".into(), key: Some("home_city".into()), value: Some("Da Nang".into()), ..Default::default() },
                        &now(),
                    )
                    .unwrap(),
                3 => extract(
                    &s,
                    "I live in Da Nang",
                    vec![with(item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang"), "quote", json!("I live in Da Nang"))],
                    &now(),
                )
                .items[0]
                    .outcome
                    .clone(),
                _ => extract(&s, "x", vec![item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang")], &now()).items[0].outcome.clone(),
            };
            if new_auth >= live_auth {
                assert!(matches!(out, Outcome::Superseded { old, .. } if old == live), "live {live_auth} new {new_auth}: {out:?}");
                assert_eq!(row(&tmp.mem(), live).0, "superseded");
            } else {
                assert!(matches!(out, Outcome::Disputed(_)), "live {live_auth} new {new_auth}: {out:?}");
                assert_eq!(row(&tmp.mem(), live).0, "live", "the live row is untouched");
            }
        }
    }
}

/// A1/A2: Saigon is told in October, Hanoi in November with "last month".
#[test]
fn saigon_then_hanoi_supersedes_with_valid_to_last_month() {
    let (tmp, s) = store();
    let oct = now_on(date(2026, 10, 5));
    let saigon = city(&s, "Saigon", 3, &oct);
    assert!(s.section().contains("Lives in Saigon"));

    let said = "I moved to Hanoi last month";
    let it = with(
        with(item("profile", Some("home_city"), Some("Hanoi"), "Lives in Hanoi"), "quote", json!(said)),
        "validFrom",
        json!("2026-10"),
    );
    let out = extract(&s, said, vec![it], &now()).items[0].outcome.clone();
    let Outcome::Superseded { new, old } = out else { panic!("{out:?}") };
    assert_eq!(old, saigon);
    let (status, valid_to, by, _) = row(&tmp.mem(), saigon);
    assert_eq!((status.as_str(), valid_to.as_deref(), by), ("superseded", Some("2026-10-01"), Some(new)));
    assert_eq!(count(&tmp.mem(), &format!("SELECT count(*) FROM facts WHERE id = {new} AND valid_from = '2026-10-01' AND status = 'live'")), 1);
    let section = s.section();
    assert!(section.contains("Lives in Hanoi (since Oct 2026)") && !section.contains("Saigon"), "{section}");
}

#[test]
fn supersede_without_valid_from_ends_the_old_row_today() {
    let (tmp, s) = store();
    let saigon = city(&s, "Saigon", 3, &now_on(date(2026, 10, 5)));
    city_say(&s, "Hanoi", &now());
    assert_eq!(row(&tmp.mem(), saigon).1.as_deref(), Some("2026-11-27"));
}

fn city_say(s: &memory::Store, v: &str, now: &memory::Now) {
    let said = format!("I live in {v}");
    let it = with(item("profile", Some("home_city"), Some(v), &format!("Lives in {v}")), "quote", json!(said));
    extract(s, &said, vec![it], now);
}

/// A3: an inferred value can't replace one the person stated; it waits as a dispute.
#[test]
fn a_guess_disputes_a_stated_fact_and_the_person_resolves_it() {
    let (tmp, s) = store();
    let hanoi = city(&s, "Hanoi", 3, &now());
    let out = extract(&s, "x", vec![item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang")], &now()).items[0].outcome.clone();
    let Outcome::Disputed(d) = out else { panic!("{out:?}") };

    let v = s.view(date(2026, 11, 27));
    assert_eq!(v["counts"], json!({"live": 1, "disputes": 1, "hidden": 0}));
    assert_eq!(v["disputes"][0]["id"], d);
    assert_eq!(v["disputes"][0]["live"], json!({"id": hanoi, "value": "Hanoi", "source": "chat"}));
    assert_eq!(v["groups"]["about"][0]["disputed"], true);
    assert!(s.section().contains("Not sure yet") && s.section().contains("You have Hanoi on record."));

    // A second guess replaces the first dispute: one question per slot.
    let out = extract(&s, "x", vec![item("profile", Some("home_city"), Some("Hue"), "Lives in Hue")], &now()).items[0].outcome.clone();
    let Outcome::Disputed(d2) = out else { panic!() };
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts WHERE status = 'disputed'"), 1);
    let _ = d;

    // Keep the old value: the guess is forgotten and the question is gone.
    s.resolve(d2, false, &now()).unwrap();
    let v = s.view(date(2026, 11, 27));
    assert_eq!((&v["counts"]["live"], &v["counts"]["disputes"]), (&json!(1), &json!(0)));
    assert_eq!(v["groups"]["about"][0]["disputed"], false);
    assert_eq!(row(&tmp.mem(), d2).0, "forgotten");
    assert_eq!(row(&tmp.mem(), hanoi).0, "live");
    assert!(!s.section().contains("Not sure yet"));
    assert_eq!(s.resolve(d2, false, &now()), Err(memory::Error::NotFound));
}

#[test]
fn resolving_for_the_new_value_supersedes_the_live_row_at_authority_4() {
    let (tmp, s) = store();
    let hanoi = city(&s, "Hanoi", 3, &now());
    let Outcome::Disputed(d) =
        extract(&s, "x", vec![item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang")], &now()).items[0].outcome.clone()
    else {
        panic!()
    };
    s.resolve(d, true, &now()).unwrap();
    assert_eq!(row(&tmp.mem(), d).0, "live");
    assert_eq!(row(&tmp.mem(), d).3, 4);
    let (status, valid_to, by, _) = row(&tmp.mem(), hanoi);
    assert_eq!((status.as_str(), valid_to.as_deref(), by), ("superseded", Some("2026-11-27"), Some(d)));
    let v = s.view(date(2026, 11, 27));
    assert_eq!(v["groups"]["about"][0]["value"], "Da Nang");
    assert_eq!(v["groups"]["about"][0]["source"], "window");
    assert_eq!(v["counts"]["disputes"], 0);
    assert!(s.section().contains("Lives in Da Nang") && !s.section().contains("Hanoi"));
}

#[test]
fn a_dispute_clears_when_the_person_later_says_the_same_thing() {
    let (tmp, s) = store();
    city(&s, "Hanoi", 3, &now());
    extract(&s, "x", vec![item("profile", Some("home_city"), Some("Da Nang"), "Lives in Da Nang")], &now());
    city_say(&s, "Da Nang", &now());
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts WHERE status = 'disputed'"), 0);
    assert_eq!(s.view(date(2026, 11, 27))["groups"]["about"][0]["value"], "Da Nang");
}

#[test]
fn saying_the_live_value_again_answers_the_dispute_but_guessing_it_does_not() {
    let (tmp, s) = store();
    let hanoi = city(&s, "Hanoi", 3, &now());
    let guess = |v: &str| extract(&s, "x", vec![item("profile", Some("home_city"), Some(v), &format!("Lives in {v}"))], &now());
    guess("Da Nang");
    let disputes = || count(&tmp.mem(), "SELECT count(*) FROM facts WHERE status = 'disputed'");
    // a guess of the live value leaves the question
    assert_eq!(guess("Hanoi").items[0].outcome, Outcome::Extended(hanoi));
    assert_eq!(disputes(), 1);
    // the bear relays the person's answer: the question is gone
    let op = RememberIn { text: "Lives in Hanoi".into(), key: Some("home_city".into()), value: Some("Hanoi".into()), ..Default::default() };
    assert_eq!(s.remember(op, &now()).unwrap(), Outcome::Extended(hanoi));
    assert_eq!(disputes(), 0);
    assert!(!s.section().contains("Not sure yet"));
}

#[test]
fn the_bear_remember_is_authority_2() {
    let (tmp, s) = store();
    let op = RememberIn { text: "Likes oat milk".into(), key: Some("likes".into()), value: Some("oat milk".into()), ..Default::default() };
    let Outcome::Created(id) = s.remember(op, &now()).unwrap() else { panic!() };
    assert_eq!(row(&tmp.mem(), id).3, 2);
    // A known key sets the kind from the registry.
    assert_eq!(s.list(Some("preference")).unwrap()[0].id, id);
}
