//! The canned idle lines on a virtual clock: no sleeps, a seeded `Rng`, and the real bear pack.

use std::path::Path;

use mascot_core::persona::{self, UserPersona};
use mascot_core::PackMeta;

use super::*;

const MIN: u64 = 60_000;
const OK: Env = Env { allowed: true, shown: true, busy: false };
const READY: Ready = Ready { busy: false, can_turn: true };
const SIGNED_OUT: Ready = Ready { busy: false, can_turn: false };

/// The bear pack, loaded once.
fn bear() -> &'static PackMeta {
    static PACK: std::sync::OnceLock<PackMeta> = std::sync::OnceLock::new();
    PACK.get_or_init(|| mascot_core::pack::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots/bear")).unwrap())
}

/// Biscuit with the person's `chattiness` (1.0 = the dice always say yes).
fn biscuit(chattiness: f64) -> EffectivePersona {
    let user = UserPersona { chattiness: Some(chattiness), ..UserPersona::default() };
    persona::resolve(bear(), &user).unwrap()
}

fn lines(chattiness: f64, seed: u64) -> IdleLines {
    let mut il = IdleLines::new(seed);
    il.set_persona(Some(biscuit(chattiness)));
    il
}

fn at(minutes: u16, idle: u64, weekday: u8) -> Senses {
    Senses { local: LocalTime { minutes, weekday }, user_idle_ms: idle, ..Senses::default() }
}

/// A break of 11 minutes (a break resets the work streak), then the person is back at `fire`,
/// at 14:00 on weekday 2 (no morning or evening in the way). Returns what the last sample said.
fn back_at(il: &mut IdleLines, fire: u64, env: Env, tweak: impl Fn(&mut Senses)) -> Option<Line> {
    let mut samples = [at(14 * 60, 0, 2), at(14 * 60, 11 * MIN, 2), at(14 * 60, 1_000, 2)];
    samples.iter_mut().for_each(&tweak);
    let times = [fire - 12 * MIN, fire - 1_000, fire];
    let mut out = None;
    for (s, t) in samples.iter().zip(times) {
        out = il.sample(s, t, env);
    }
    out
}

fn morning(il: &mut IdleLines, t: u64, weekday: u8, env: Env) -> Option<Line> {
    il.sample(&at(8 * 60, 0, weekday), t, env)
}

// ---- the numbers -------------------------------------------------------------------------------

#[test]
fn the_cooldown_runs_from_40_to_20_minutes_and_the_dice_from_40_to_100_percent() {
    assert_eq!(cooldown_ms(0.0), 40 * MIN);
    assert_eq!(cooldown_ms(0.5), 30 * MIN);
    assert_eq!(cooldown_ms(1.0), 20 * MIN);
    assert_eq!(cooldown_ms(7.0), 20 * MIN, "clamped");
    assert!((say_probability(0.0) - 0.4).abs() < 1e-9);
    assert!((say_probability(0.5) - 0.7).abs() < 1e-9);
    assert!((say_probability(1.0) - 1.0).abs() < 1e-9);
}

#[test]
fn the_jitter_keeps_the_cooldown_within_ten_percent_of_its_base() {
    for c in [0.0, 0.3, 0.5, 1.0] {
        for seed in 0..40 {
            let mut il = lines(c, seed);
            il.start_cooldown(0);
            let base = cooldown_ms(c) as f64;
            assert!((il.cooldown as f64) >= base * 0.9 - 1.0 && (il.cooldown as f64) <= base * 1.1 + 1.0, "c={c} seed={seed}: {}", il.cooldown);
        }
    }
}

#[test]
fn the_reflex_rules_map_to_the_persona_moments() {
    assert_eq!(moment_of("morning"), Some("morning"));
    assert_eq!(moment_of("back_from_idle"), Some("back"));
    assert_eq!(moment_of("long_work"), Some("long_session"));
    assert_eq!(moment_of("app_switch"), None);
    for m in ["morning", "back", "long_session", "evening"] {
        assert!(mascot_core::persona::MOMENTS.contains(&m));
    }
}

// ---- a line, with no model ---------------------------------------------------------------------

#[test]
fn a_moment_says_one_of_the_personas_filled_lines() {
    let user = UserPersona { chattiness: Some(1.0), call_me: Some("Sam".into()), ..UserPersona::default() };
    let e = persona::resolve(bear(), &user).unwrap();
    let mut il = IdleLines::new(1);
    il.set_persona(Some(e.clone()));
    let said = morning(&mut il, 0, 1, OK).expect("a morning line");
    assert_eq!(said.moment, "morning");
    assert!(e.idle("morning").contains(&said.text), "{}", said.text);
    assert!(!said.text.contains('{'), "placeholders are filled: {}", said.text);

    let mut il = lines(1.0, 1);
    let back = back_at(&mut il, 30 * MIN, OK, |_| {}).expect("a back line");
    assert_eq!(back.moment, "back");
    assert!(biscuit(1.0).idle("back").contains(&back.text));
}

#[test]
fn long_work_says_a_long_session_line() {
    let mut il = lines(1.0, 3);
    let mut said = None;
    let mut t = 0;
    // input all the time for over two hours (the minute clock of the senses)
    while t <= 2 * 3_600_000 + 5 * MIN && said.is_none() {
        said = il.sample(&at(14 * 60, 0, 2), t, OK);
        t += MIN;
    }
    let said = said.expect("long_session");
    assert_eq!(said.moment, "long_session");
    assert!(biscuit(1.0).idle("long_session").contains(&said.text));
}

#[test]
fn a_mascot_without_a_persona_says_nothing() {
    let mut il = IdleLines::new(1);
    assert_eq!(morning(&mut il, 0, 1, OK), None);
    il.set_persona(Some(biscuit(1.0)));
    il.set_persona(None);
    assert_eq!(back_at(&mut il, 60 * MIN, OK, |_| {}), None);
}

// ---- suppression -------------------------------------------------------------------------------

#[test]
fn dnd_quiet_hours_fullscreen_a_hidden_bear_a_turn_and_a_bubble_each_say_nothing_and_queue_nothing() {
    type Tweak = fn(&mut Senses);
    let cases: [(&str, Env, Tweak); 8] = [
        ("brain says not allowed (dnd / quiet)", Env { allowed: false, ..OK }, |_| {}),
        ("dnd toggle", OK, |s| s.dnd_toggle = true),
        ("quiet hours", OK, |s| s.quiet_hours = true),
        ("fullscreen", OK, |s| s.fullscreen = true),
        ("screen share", OK, |s| s.screen_share = true),
        ("bear hidden or held still", Env { shown: false, ..OK }, |_| {}),
        ("a turn is running or a bubble is up", Env { busy: true, ..OK }, |_| {}),
        ("all at once", Env { allowed: false, shown: false, busy: true }, |s| s.fullscreen = true),
    ];
    for (why, env, tweak) in cases {
        let mut il = lines(1.0, 5);
        assert_eq!(back_at(&mut il, 30 * MIN, env, tweak), None, "{why}");
        // nothing was queued: the next quiet samples with everything clear say nothing either
        assert_eq!(il.sample(&at(14 * 60, 2_000, 2), 30 * MIN + 2_000, OK), None, "{why}: no line queued");
        assert_eq!(il.sample(&at(14 * 60, 3_000, 2), 31 * MIN, OK), None, "{why}: no line queued");
        // it spent no cooldown either: a later return (clear) speaks
        assert!(back_at(&mut il, 60 * MIN, OK, |_| {}).is_some(), "{why}: free again later");
    }
}

#[test]
fn a_morning_held_back_by_a_bubble_is_gone_not_queued() {
    let mut il = lines(1.0, 5);
    assert_eq!(morning(&mut il, 0, 1, Env { busy: true, ..OK }), None);
    for m in 1..30 {
        assert_eq!(morning(&mut il, m * MIN, 1, OK), None, "minute {m}");
    }
    // the next morning is another day
    assert!(morning(&mut il, 24 * 60 * MIN, 2, OK).is_some());
}

#[test]
fn a_morning_in_dnd_is_not_said_when_dnd_ends() {
    let mut il = lines(1.0, 5);
    let mut s = at(8 * 60, 0, 1);
    s.dnd_toggle = true;
    assert_eq!(il.sample(&s, 0, Env { allowed: false, ..OK }), None);
    assert_eq!(morning(&mut il, 5 * MIN, 1, OK), None, "that morning is spent");
}

// ---- cooldown ----------------------------------------------------------------------------------

#[test]
fn the_next_line_waits_out_the_cooldown_and_a_chattier_bear_waits_less() {
    // chattiness 1.0: 20 min ±10 % (18..22)
    let mut il = lines(1.0, 7);
    assert!(back_at(&mut il, 20 * MIN, OK, |_| {}).is_some());
    assert!((18 * MIN..=22 * MIN).contains(&il.cooldown), "{}", il.cooldown);
    assert_eq!(back_at(&mut il, 20 * MIN + 17 * MIN, OK, |_| {}), None, "17 min after: inside the cooldown");
    // the dropped return did not move the clock: 37 min after the line it is time
    assert!(back_at(&mut il, 20 * MIN + 37 * MIN, OK, |_| {}).is_some(), "37 min after");

    // chattiness 0: 40 min ±10 % (36..44): 30 min is still too soon (the dice come after the clock)
    let seed = (0..200).find(|&s| back_at(&mut lines(0.0, s), 20 * MIN, OK, |_| {}).is_some()).expect("some seed says");
    let mut il = lines(0.0, seed);
    assert!(back_at(&mut il, 20 * MIN, OK, |_| {}).is_some());
    assert!((36 * MIN..=44 * MIN).contains(&il.cooldown), "{}", il.cooldown);
    assert_eq!(back_at(&mut il, 20 * MIN + 30 * MIN, OK, |_| {}), None, "30 min after: a quiet bear is not ready");
}

#[test]
fn a_no_from_the_dice_starts_no_cooldown() {
    // chattiness 0: 40 % say. Find a seed whose first roll says no, then the very next return gets its own roll
    let mut il = (0..200).map(|s| lines(0.0, s)).find(|il| back_at(&mut il.clone(), 20 * MIN, OK, |_| {}).is_none()).expect("a seed that says no");
    assert_eq!(back_at(&mut il, 20 * MIN, OK, |_| {}), None);
    assert_eq!(il.last_spoke, None, "no cooldown was started");
}

#[test]
fn the_dice_follow_the_chattiness_and_are_seeded() {
    let said = |c: f64| (0..400u64).filter(|&seed| morning(&mut lines(c, seed), 0, 1, OK).is_some()).count();
    assert_eq!(said(1.0), 400, "chattiness 1: always");
    let quiet = said(0.0);
    assert!((130..=190).contains(&quiet), "chattiness 0 is about 40 %: {quiet} of 400");
    let mid = said(0.5);
    assert!((250..=310).contains(&mid), "chattiness 0.5 is about 70 %: {mid} of 400");
    // the same seed, the same day: the same line
    let a = morning(&mut lines(1.0, 9), 0, 1, OK);
    let b = morning(&mut lines(1.0, 9), 0, 1, OK);
    assert_eq!(a, b);
}

// ---- variety -----------------------------------------------------------------------------------

/// One morning per day for `days` days, a day apart, with the work streak broken in between.
fn mornings(il: &mut IdleLines, days: u64) -> Vec<Option<String>> {
    let mut out = vec![];
    for d in 0..days {
        let t = d * 24 * 60 * MIN;
        out.push(morning(il, t, (d % 7) as u8, OK).map(|l| l.text));
        il.sample(&at(23 * 60, 11 * MIN, (d % 7) as u8), t + 15 * 60 * MIN, OK);
    }
    out
}

#[test]
fn a_line_never_repeats_back_to_back_and_unseen_lines_come_first() {
    for seed in 0..20 {
        let mut il = lines(1.0, seed);
        let said: Vec<String> = mornings(&mut il, 13).into_iter().map(|l| l.expect("chattiness 1 always says")).collect();
        for w in said.windows(2) {
            assert_ne!(w[0], w[1], "seed {seed}: back to back");
        }
        // the four morning lines each get a turn before any is said again
        let all = biscuit(1.0).idle("morning");
        assert_eq!(all.len(), 4);
        for round in said.chunks(4).filter(|c| c.len() == 4).take(2) {
            let mut r: Vec<&String> = round.iter().collect();
            r.sort();
            r.dedup();
            assert_eq!(r.len(), 4, "seed {seed}: a round has no repeat: {round:?}");
        }
        assert!(said.iter().all(|l| all.contains(l)));
    }
}

#[test]
fn a_single_line_is_never_said_twice_in_a_row() {
    let mut e = biscuit(1.0);
    e.idle_lines.insert("morning".into(), vec!["Only line.".into()]);
    let mut il = IdleLines::new(1);
    il.set_persona(Some(e));
    let said = mornings(&mut il, 4);
    assert_eq!(said, [Some("Only line.".to_string()), None, None, None], "silent while the only line would repeat");
}

#[test]
fn the_last_line_counts_across_moments() {
    let mut e = biscuit(1.0);
    e.idle_lines.insert("morning".into(), vec!["Same words.".into()]);
    e.idle_lines.insert("back".into(), vec!["Same words.".into()]);
    let mut il = IdleLines::new(1);
    il.set_persona(Some(e));
    assert_eq!(morning(&mut il, 0, 1, OK).map(|l| l.text), Some("Same words.".into()));
    assert_eq!(back_at(&mut il, 60 * MIN, OK, |_| {}), None, "the cooldown is over but it would repeat");
}

// ---- evening -----------------------------------------------------------------------------------

#[test]
fn the_evening_line_comes_once_a_local_day_between_18_and_22() {
    let mut il = lines(1.0, 11);
    // (the test's clock runs faster than the local one, to stay inside one work streak)
    // 17:59 and 22:00 are outside the window
    assert_eq!(il.sample(&at(17 * 60 + 59, 0, 3), 0, OK), None);
    assert_eq!(il.sample(&at(22 * 60, 0, 3), 5 * MIN, OK), None);
    // away from the keyboard at 19:00 is not an evening yet
    assert_eq!(il.sample(&at(19 * 60, 4 * MIN, 3), 60 * MIN, OK), None);
    // 19:00 at the keyboard
    let said = il.sample(&at(19 * 60, 0, 3), 61 * MIN, OK).expect("evening");
    assert_eq!(said.moment, "evening");
    assert!(biscuit(1.0).idle("evening").contains(&said.text));
    // once: not at 19:30, nor at 21:30 with the cooldown long over
    assert_eq!(il.sample(&at(19 * 60 + 30, 0, 3), 91 * MIN, OK), None);
    assert_eq!(il.sample(&at(21 * 60 + 30, 0, 3), 100 * MIN, OK), None);
    // the next local day
    let next = il.sample(&at(19 * 60, 0, 4), 110 * MIN, OK).expect("the next evening");
    assert_eq!(next.moment, "evening");
    assert_ne!(next.text, said.text, "no back-to-back repeat");
}

#[test]
fn an_evening_that_met_dnd_or_a_bubble_is_gone_not_queued() {
    for (why, env, dnd) in [("dnd", Env { allowed: false, ..OK }, true), ("a bubble", Env { busy: true, ..OK }, false)] {
        let mut il = lines(1.0, 11);
        let mut s = at(19 * 60, 0, 3);
        s.dnd_toggle = dnd;
        assert_eq!(il.sample(&s, 0, env), None, "{why}");
        assert_eq!(il.sample(&at(19 * 60 + 5, 0, 3), 5 * MIN, OK), None, "{why}: nothing is said when it clears");
        assert_eq!(il.sample(&at(20 * 60, 0, 3), 65 * MIN, OK), None, "{why}: not later that evening either");
        assert!(il.sample(&at(19 * 60, 0, 4), 24 * 60 * MIN, OK).is_some(), "{why}: tomorrow's evening is its own");
    }
}

// ---- next to the model check-ins ---------------------------------------------------------------

fn senses_step(u: &mut Unprompted, il: &mut IdleLines, s: &Senses, now: u64, ready: Ready) -> Step {
    step(u, il, s, now, OK, ready)
}

#[test]
fn a_model_check_in_for_the_same_moment_drops_the_canned_line() {
    let mut u = Unprompted::new(4);
    let mut il = lines(1.0, 13);
    let s = at(8 * 60, 0, 1);
    let out = senses_step(&mut u, &mut il, &s, 0, READY);
    assert!(out.check_in.as_deref().is_some_and(|c| c.contains("morning")), "{out:?}");
    assert_eq!(out.line, None, "one voice per moment");
    assert_eq!(u.used(0), 1);
    // the check-in is on record, and it started the cooldown
    assert_eq!(il.model.get("morning"), Some(&0));
    assert_eq!(il.last_spoke, Some(0));
}

#[test]
fn note_turn_drops_that_moment_and_starts_the_cooldown() {
    let mut il = lines(1.0, 13);
    il.note_turn("morning", 0);
    assert_eq!(morning(&mut il, 0, 1, OK), None);
    // a different moment right behind a model check-in waits for the cooldown
    let mut il = lines(1.0, 13);
    il.note_turn("morning", 0);
    assert_eq!(back_at(&mut il, 15 * MIN, OK, |_| {}), None, "inside the cooldown");
    assert!(back_at(&mut il, 40 * MIN, OK, |_| {}).is_some(), "after it");
    // no persona: nothing to cool
    let mut none = IdleLines::new(1);
    none.note_turn("morning", 0);
    assert_eq!(none.last_spoke, None);
}

#[test]
fn canned_lines_spend_none_of_the_check_in_budget_and_still_speak_when_it_is_spent() {
    // signed out: no model check-in can run, the canned line speaks, the counter stays 0 of 4
    let mut u = Unprompted::new(4);
    let mut il = lines(1.0, 17);
    let out = senses_step(&mut u, &mut il, &at(8 * 60, 0, 1), 0, SIGNED_OUT);
    assert!(out.check_in.is_none() && out.line.is_some(), "{out:?}");
    assert_eq!(u.used(0), 0);
    assert_eq!(u.line(0, None), "Check-ins: 0 of 4 this hour");

    // one an hour: the morning check-in is the budget; the later return is over budget, so a free line says it
    let mut u = Unprompted::new(1);
    let mut il = lines(1.0, 17);
    let first = senses_step(&mut u, &mut il, &at(8 * 60, 0, 1), 0, READY);
    assert!(first.check_in.is_some() && first.line.is_none());
    let t = 30 * MIN;
    let mut later = Step::default();
    for (s, at_ms) in [(at(14 * 60, 0, 1), t), (at(14 * 60, 11 * MIN, 1), t + 11 * MIN), (at(14 * 60, 1_000, 1), t + 11 * MIN + 1_000)] {
        later = senses_step(&mut u, &mut il, &s, at_ms, READY);
    }
    assert_eq!(later.check_in, None, "over budget");
    assert_eq!(later.line.as_ref().map(|l| l.moment), Some("back"));
    assert_eq!(u.used(t + 12 * MIN), 1, "still 1 of 1");
    assert_eq!(u.line(t + 12 * MIN, None), "Check-ins: 1 of 1 this hour");
}

#[test]
fn a_canned_line_said_while_signed_out_is_the_moments_one_greeting() {
    // signed out at 08:00: the canned morning line speaks and the check-in waits for a model
    let mut u = Unprompted::new(4);
    let mut il = lines(1.0, 17);
    let first = senses_step(&mut u, &mut il, &at(8 * 60, 0, 1), 0, SIGNED_OUT);
    assert_eq!(first.line.as_ref().map(|l| l.moment), Some("morning"), "{first:?}");
    // signed in a minute later: that morning is spoken for, so no model check-in greets again
    for i in 1..=5u64 {
        let out = senses_step(&mut u, &mut il, &at(8 * 60 + i as u16, 0, 1), i * MIN, READY);
        assert_eq!(out.check_in, None, "sample {i}: {out:?}");
    }
    assert_eq!(u.used(5 * MIN), 0);

    // the dice said no to the canned line: the held-back check-in still greets once a model runs
    let (mut u, mut il) = (0..20u64)
        .map(|seed| (Unprompted::new(4), lines(0.0, seed)))
        .find_map(|(mut u, mut il)| senses_step(&mut u, &mut il, &at(8 * 60, 0, 1), 0, SIGNED_OUT).line.is_none().then_some((u, il)))
        .expect("some seed rolls a no");
    let out = senses_step(&mut u, &mut il, &at(8 * 60 + 1, 0, 1), MIN, READY);
    assert!(out.check_in.is_some() && out.line.is_none(), "{out:?}");
}

#[test]
fn a_busy_turn_holds_back_the_check_in_and_the_canned_line_alike() {
    let mut u = Unprompted::new(4);
    let mut il = lines(1.0, 17);
    let busy = Ready { busy: true, can_turn: true };
    let out = senses_step(&mut u, &mut il, &at(8 * 60, 0, 1), 0, busy);
    assert_eq!(out, Step::default());
    assert_eq!(u.used(0), 0);
}
