//! Reaction-brain tests (aliveness p4, AC14, design `## react-brain`): contexts in the picker,
//! the reaction cues (drag, cursor, morning, approval wait, edge) and their gates, on
//! `tests/fixtures/react_pack` (the bear with the p4 `contexts`, `reactions` and r/s/w behaviors).

use std::sync::Arc;

use mascot_protocol::MascotEvent;

use super::*;
use crate::pack;
use crate::test_pack::repo;

fn react_pack() -> Arc<PackMeta> {
    Arc::new(pack::load(&repo().join("crates/mascot-core/tests/fixtures/react_pack")).unwrap_or_else(|e| panic!("{e}")))
}

fn brain(seed: u64) -> Brain {
    Brain::new(react_pack(), BrainConfig { seed, temperament: None, dnd_action: DndAction::Still }, 0)
}

/// Senses at `hh:mm` (afternoon by default: no morning reflex), November.
fn at(minutes: u16) -> Senses {
    Senses { local: LocalTime { minutes, weekday: 2 }, month: 11, ..Senses::default() }
}

const AFTERNOON: u16 = 14 * 60;

/// Ticks at every wake from `from` to `until`, returning each show with its time.
fn run(b: &mut Brain, from: u64, until: u64) -> Vec<(u64, Show)> {
    let mut out = Vec::new();
    let mut t = from;
    loop {
        out.push((t, b.tick(t)));
        let next = b.next_wake_ms().max(t + 1);
        if next > until {
            break;
        }
        t = next;
    }
    out
}

/// A started brain, idle in the afternoon at `t` = 3000.
fn started(seed: u64) -> (Brain, Senses) {
    let mut b = brain(seed);
    let s = at(AFTERNOON);
    b.senses(&s, 0);
    run(&mut b, 0, 3000);
    (b, s)
}

fn react_notes(b: &Brain) -> Vec<String> {
    b.trace().iter().filter(|l| l.what == "react").map(|l| l.note.clone()).collect()
}

/// Behaviors in the order they start (consecutive repeats folded).
fn behaviors(shows: &[(u64, Show)]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (_, s) in shows {
        if v.last() != Some(&s.behavior) {
            v.push(s.behavior.clone());
        }
    }
    v
}

fn clips(shows: &[(u64, Show)]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (_, s) in shows {
        if v.last() != Some(&s.clip) {
            v.push(s.clip.clone());
        }
    }
    v
}

fn cur(dx: i32, dy: i32, over: bool) -> Option<Cursor> {
    Some(Cursor { dx, dy, over })
}

/// Feeds one cursor sample at `t` (the app sends only changes).
fn cursor(b: &mut Brain, s: &mut Senses, c: Option<Cursor>, t: u64) {
    s.cursor = c;
    b.senses(s, t);
}

/// A petting wiggle over the bear from `t0`: 4 horizontal reversals, 150 ms apart.
fn pet(b: &mut Brain, s: &mut Senses, t0: u64) -> u64 {
    let mut t = t0;
    for dx in [0, 16, -8, 16, -8, 16] {
        cursor(b, s, cur(dx, 24, true), t);
        t += 150;
    }
    t
}

// ---- the pack ----

#[test]
fn the_fixture_declares_every_context_and_reaction() {
    let p = react_pack();
    assert_eq!(p.contexts.keys().map(String::as_str).collect::<Vec<_>>(), {
        let mut c = react::CONTEXTS.to_vec();
        c.sort();
        c
    });
    assert_eq!(p.reactions.len(), react::REACTIONS.len());
    assert_eq!(p.reaction_plays(react::DRAG_HOLD), Some("dangle"));
    assert!(p.waiting().is_empty(), "the r/s/w art is in: {:?}", p.waiting().iter().map(|w| &w.name).collect::<Vec<_>>());
}

// ---- cursor: pet, flinch, follow (`#flow-cursor`) ----

#[test]
fn petting_plays_giggle_once_per_cooldown() {
    let (mut b, mut s) = started(1);
    let t = pet(&mut b, &mut s, 3000);
    let shows = run(&mut b, t, t + 400);
    assert!(react_notes(&b).iter().any(|n| n == "pet → giggle"), "{:?}", react_notes(&b));
    assert!(shows.iter().any(|(_, s)| s.behavior == "giggle"));
    // a second wiggle inside the 10 s cooldown: nothing
    cursor(&mut b, &mut s, None, t + 500);
    let t2 = pet(&mut b, &mut s, t + 3000);
    assert_eq!(react_notes(&b).iter().filter(|n| *n == "pet → giggle").count(), 1, "cooldown");
    // after it: again
    cursor(&mut b, &mut s, None, t2 + 100);
    pet(&mut b, &mut s, t + 11_000);
    assert_eq!(react_notes(&b).iter().filter(|n| *n == "pet → giggle").count(), 2);
}

#[test]
fn a_fast_approach_flinches_and_a_slow_one_does_not() {
    let (mut b, mut s) = started(2);
    for (i, x) in [176, 136, 96, 56, 16].iter().enumerate() {
        cursor(&mut b, &mut s, cur(*x, 0, false), 3000 + i as u64 * 100); // 400 pt/s
    }
    assert!(!react_notes(&b).iter().any(|n| n.starts_with("flinch")));
    cursor(&mut b, &mut s, None, 3500);
    cursor(&mut b, &mut s, cur(176, 0, false), 3600);
    cursor(&mut b, &mut s, cur(16, 0, false), 3700); // 1600 pt/s
    assert!(react_notes(&b).iter().any(|n| n == "flinch → flinch"), "{:?}", react_notes(&b));
    let shows = run(&mut b, 3700, 4000);
    assert!(shows.iter().any(|(_, s)| s.behavior == "flinch"));
}

#[test]
fn pet_wins_over_flinch_when_both_hold() {
    let (mut b, mut s) = started(3);
    // reversals over the bear, the last one a fast dive from outside the flinch ring
    for (t, dx, over) in [(3000, 0, true), (3150, 16, true), (3300, -8, true), (3450, 16, true), (3500, 120, false)] {
        cursor(&mut b, &mut s, cur(dx, 8, over), t);
    }
    cursor(&mut b, &mut s, cur(-8, 8, true), 3550);
    let n = react_notes(&b);
    assert_eq!(n.iter().filter(|x| x.ends_with("→ giggle") || x.ends_with("→ flinch")).collect::<Vec<_>>(), vec!["pet → giggle"], "{n:?}");
}

#[test]
fn a_still_cursor_nearby_is_followed_for_twenty_seconds_then_cools_down() {
    let (mut b, mut s) = started(4);
    // parked to the east of the bear, 80 pt away
    cursor(&mut b, &mut s, cur(80, 0, false), 3000);
    let shows = run(&mut b, 3000, 30_000);
    let n = react_notes(&b);
    assert!(n.iter().any(|x| x == "follow → cursor_watch"), "{n:?}");
    let watch: Vec<&(u64, Show)> = shows.iter().filter(|(_, s)| s.behavior == "cursor_watch").collect();
    let first = watch.first().expect("watches").0;
    assert!((4500..5500).contains(&first), "after 1.5 s still: {first}");
    assert!(watch.iter().all(|(_, s)| s.dir == Dir::E || s.clip.starts_with("turn_")), "faces the cursor");
    assert!(n.iter().any(|x| x == "follow end (time)"), "{n:?}");
    // capped at 20 s (the loop that is playing finishes: ≤ 1.7 s more, back on idle#0)
    let last = watch.last().unwrap().0;
    assert!(last < first + 20_000 + 1700, "capped at 20 s: {first}..{last}");
    // condition (e): the parked cursor does not start it again before the 60 s cooldown
    let later = run(&mut b, 30_000, first + 20_000 + 59_000);
    assert!(!later.iter().any(|(_, s)| s.behavior == "cursor_watch"), "cooling down");
    let again = run(&mut b, first + 20_000 + 59_000, first + 20_000 + 70_000);
    assert!(again.iter().any(|(_, s)| s.behavior == "cursor_watch"), "after the cooldown");
}

#[test]
fn the_cursor_leaving_mid_follow_ends_the_watch() {
    let (mut b, mut s) = started(5);
    cursor(&mut b, &mut s, cur(-64, 32, false), 3000);
    let shows = run(&mut b, 3000, 8000);
    assert!(shows.iter().any(|(_, s)| s.behavior == "cursor_watch"));
    cursor(&mut b, &mut s, None, 8000);
    assert!(react_notes(&b).iter().any(|x| x == "follow end (gone)"), "{:?}", react_notes(&b));
    // the playing loop finishes (≤ 20 frames), then the watch is over
    let after = run(&mut b, 8000, 12_000);
    assert!(after.iter().filter(|(t, _)| *t > 8000 + 1700).all(|(_, s)| s.behavior != "cursor_watch"), "{:?}", behaviors(&after));
    assert_ne!(b.tick(12_000).behavior, "cursor_watch");
}

#[test]
fn follow_turns_toward_the_cursor_when_it_moves_round() {
    let (mut b, mut s) = started(6);
    cursor(&mut b, &mut s, cur(80, 0, false), 3000);
    run(&mut b, 3000, 6000);
    assert_eq!(b.tick(6000).behavior, "cursor_watch");
    // slowly round to the west (≤ 200 pt/s, so still), a second apart
    let mut t = 6000;
    for (dx, dy) in [(64, -48), (0, -72), (-64, -48), (-80, 0)] {
        t += 1100;
        run(&mut b, t - 1100, t);
        cursor(&mut b, &mut s, cur(dx, dy, false), t);
    }
    let shows = run(&mut b, t, t + 3000);
    assert!(react_notes(&b).iter().any(|x| x.starts_with("follow turn")), "{:?}", react_notes(&b));
    let last = shows.last().unwrap().1.clone();
    assert_eq!((last.behavior.as_str(), last.dir), ("cursor_watch", Dir::W), "{:?}", clips(&shows));
}

#[test]
fn petting_during_a_turn_does_nothing() {
    let (mut b, mut s) = started(7);
    b.event(&MascotEvent::Thinking, 3000);
    let t = pet(&mut b, &mut s, 3100);
    cursor(&mut b, &mut s, cur(80, 0, false), t);
    let shows = run(&mut b, t, t + 5000);
    assert!(react_notes(&b).is_empty(), "{:?}", react_notes(&b));
    assert!(shows.iter().all(|(_, s)| s.intent == Intent::Think), "{:?}", behaviors(&shows));
}

#[test]
fn a_click_cuts_a_reaction() {
    let (mut b, mut s) = started(8);
    let t = pet(&mut b, &mut s, 3000);
    // (a posture stands up first: wait for the giggle itself)
    let mut g = t;
    while b.tick(g).behavior != "giggle" {
        g = b.next_wake_ms().max(g + 1);
        assert!(g < t + 10_000, "giggles");
    }
    g += 100;
    assert_eq!(b.tick(g).behavior, "giggle");
    b.poke(Poke::Click, g);
    b.poke(Poke::PromptOpen, g);
    assert!(react_notes(&b).iter().any(|x| x == "cut (click)"));
    // (the frame on screen finishes, then listen)
    let shows = run(&mut b, g, g + 1000);
    assert!(shows.iter().skip(1).all(|(_, s)| s.behavior != "giggle"), "{:?}", shows.iter().map(|(t, s)| (*t, s.clip.clone(), s.frame)).collect::<Vec<_>>());
}

// ---- drag (`#flow-drag`) ----

fn panel(x: i32, y: i32) -> WinRect {
    WinRect { x, y, w: 284, h: 198, owner_pid: 1, layer: 3 }
}

/// Drags the panel from `t0` over `ms`, in 100 ms steps of `step` pt to the right.
fn drag(b: &mut Brain, s: &mut Senses, t0: u64, ms: u64, step: i32) -> u64 {
    s.panel = Some(panel(400, 400));
    b.senses(s, t0);
    b.poke(Poke::DragStart, t0);
    let mut t = t0;
    let mut x = 400;
    while t < t0 + ms {
        t += 100;
        x += step;
        s.panel = Some(panel(x, 400));
        b.senses(s, t);
    }
    b.poke(Poke::DragEnd, t);
    t
}

#[test]
fn a_long_drag_dangles_lands_and_is_dizzy_and_a_short_one_is_not() {
    let (mut b, mut s) = started(9);
    let shows = {
        b.poke(Poke::DragStart, 3000);
        let s1 = run(&mut b, 3000, 3100);
        b.poke(Poke::DragEnd, 3100);
        s1
    };
    assert_eq!(shows[0].1.behavior, "dangle", "drag_hold replaces hold");
    // 2.1 s, 21 × 40 pt = 840 pt
    let end = drag(&mut b, &mut s, 10_000, 2100, 40);
    let after = run(&mut b, end, end + 6000);
    let bs = behaviors(&after);
    assert_eq!(&bs[..2], ["land_squash", "dizzy"], "{bs:?}");
    // a 200 ms drag: no dizzy
    let end = drag(&mut b, &mut s, 20_000, 200, 40);
    let after = run(&mut b, end, end + 4000);
    let bs = behaviors(&after);
    assert_eq!(bs[0], "land_squash");
    assert!(!bs.contains(&"dizzy".to_string()), "{bs:?}");
}

#[test]
fn a_drag_outranks_the_agent() {
    let (mut b, mut s) = started(10);
    b.event(&MascotEvent::Thinking, 3000);
    run(&mut b, 3000, 4000);
    b.poke(Poke::DragStart, 4000);
    assert_eq!(b.tick(4000).behavior, "dangle");
    let end = drag(&mut b, &mut s, 4100, 300, 10);
    let after = run(&mut b, end, end + 3000);
    let bs = behaviors(&after);
    assert_eq!(bs[0], "land_squash", "{bs:?}");
    assert_eq!(after.last().unwrap().1.intent, Intent::Think, "then the turn again");
}

/// p4 integration QA: dizzy is a reaction, so a long drag during a turn (think, the ask card)
/// lands with land_squash and goes straight back to the turn, never dizzy over it.
#[test]
fn a_long_drag_during_a_turn_lands_without_dizzy() {
    for ev in [MascotEvent::Thinking, MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run it?".into() }] {
        let (mut b, mut s) = started(10);
        b.poke(Poke::PromptSent, 3000);
        b.event(&ev, 3000);
        run(&mut b, 3000, 4000);
        let end = drag(&mut b, &mut s, 4000, 2100, 40);
        let after = run(&mut b, end, end + 4000);
        let bs = behaviors(&after);
        assert_eq!(bs[0], "land_squash", "{ev:?}: {bs:?}");
        assert!(!bs.iter().any(|x| x == "dizzy"), "{ev:?}: no dizzy over the turn {bs:?}");
    }
}

// ---- gates (`#priority`: DND and quiet hours) ----

#[test]
fn quiet_hours_and_dnd_give_no_cursor_music_or_away_reaction_but_a_drag_still_dangles_and_lands() {
    for (name, gate) in [("quiet hours", Senses { quiet_hours: true, ..at(23 * 60 + 30) }), ("dnd", Senses { dnd_toggle: true, ..at(AFTERNOON) })] {
        let mut b = brain(11);
        let mut s = gate.clone();
        b.senses(&s, 0);
        run(&mut b, 0, 3000);
        s.now_playing = Some(NowPlaying { source: "music".into(), title: "x".into(), playing: true });
        s.user_idle_ms = 20 * 60_000;
        b.senses(&s, 3000);
        let t = pet(&mut b, &mut s, 3100);
        cursor(&mut b, &mut s, cur(80, 0, false), t);
        let shows = run(&mut b, t, t + 120_000);
        let bs = behaviors(&shows);
        for no in ["giggle", "cursor_watch", "head_bob", "foot_tap", "dance", "tap_glass", "floor_nap"] {
            assert!(!bs.iter().any(|x| x == no), "{name}: {no} in {bs:?}");
        }
        if name == "dnd" {
            continue; // DND holds still: the dragged bear shows its v1 hold
        }
        s.cursor = None;
        b.senses(&s, 130_000);
        b.poke(Poke::DragStart, 130_000);
        assert_eq!(b.tick(130_000).behavior, "dangle", "{name}");
        b.poke(Poke::DragEnd, 130_000);
        let end = drag(&mut b, &mut s, 131_000, 2100, 40);
        let after = run(&mut b, end, end + 4000);
        let bs = behaviors(&after);
        assert_eq!(bs[0], "land_squash", "{name}: {bs:?}");
        assert!(!bs.contains(&"dizzy".to_string()), "{name}: no dizzy");
    }
}

// ---- contexts (`#pack-contexts`) ----

/// Idle picks (`pick` trace notes' behavior) between `from` and `to`.
fn picks(b: &Brain, from: u64, to: u64) -> Vec<String> {
    b.trace()
        .iter()
        .filter(|l| l.what == "pick" && (from..to).contains(&l.t_ms))
        .map(|l| l.note.split_whitespace().next().unwrap_or("").to_string())
        .collect()
}

#[test]
fn music_adds_its_entries_while_it_plays_and_none_after() {
    // (4 of the pool's ~36 weight: about one pick in nine while it plays)
    for seed in [1, 2, 3] {
        let (mut b, mut s) = started(seed);
        s.now_playing = Some(NowPlaying { source: "spotify".into(), title: "x".into(), playing: true });
        b.senses(&s, 5000);
        run(&mut b, 5000, 600_000);
        let during = picks(&b, 5000, 600_000);
        assert!(during.iter().any(|p| ["head_bob", "foot_tap", "dance"].contains(&p.as_str())), "seed {seed}: {during:?}");
        s.now_playing.as_mut().unwrap().playing = false;
        b.senses(&s, 600_000);
        run(&mut b, 600_000, 1_200_000);
        let after = picks(&b, 600_001, 1_200_000);
        assert!(!after.iter().any(|p| ["head_bob", "foot_tap", "dance"].contains(&p.as_str())), "seed {seed}: {after:?}");
    }
}

#[test]
fn evening_yawns_in_the_evening_only() {
    let mut b = brain(13);
    b.senses(&at(21 * 60), 0);
    run(&mut b, 0, 900_000);
    assert!(picks(&b, 0, 900_000).iter().any(|p| p == "evening_yawn"));
    let mut b = brain(13);
    b.senses(&at(AFTERNOON), 0);
    run(&mut b, 0, 900_000);
    assert!(!picks(&b, 0, 900_000).iter().any(|p| p == "evening_yawn"));
}

#[test]
fn away_picks_tap_glass_or_wave_and_away_long_holds_the_floor_nap_until_input() {
    let (mut b, mut s) = started(14);
    s.user_idle_ms = 6 * 60_000;
    b.senses(&s, 5000);
    run(&mut b, 5000, 300_000);
    let p = picks(&b, 5000, 300_000);
    assert!(p.iter().any(|x| x == "tap_glass" || x == "wave_small"), "{p:?}");
    // 16 minutes: only floor_nap, held well past its for_s [30, 120]
    s.user_idle_ms = 16 * 60_000;
    b.senses(&s, 300_000);
    let shows = run(&mut b, 300_000, 900_000);
    let nap_from = shows.iter().find(|(_, s)| s.clip == "curl_nap").expect("curls up").0;
    let tail: Vec<&(u64, Show)> = shows.iter().filter(|(t, _)| *t >= nap_from + 1000).collect();
    assert!(tail.iter().all(|(_, s)| s.clip == "curl_nap"), "held: {:?}", clips(&shows[shows.len() - 5..]));
    assert!(picks(&b, 300_000, 900_000).iter().all(|x| x == "floor_nap"));
    // input returns: curl_up, then the greeting
    s.user_idle_ms = 0;
    b.senses(&s, 900_000);
    let back = run(&mut b, 900_000, 910_000);
    let c = clips(&back);
    let up = c.iter().position(|x| x == "curl_up").unwrap_or_else(|| panic!("{c:?}"));
    assert!(back.iter().any(|(_, s)| s.intent == Intent::Greet), "greets: {c:?}");
    assert!(up < c.len() - 1);
}

#[test]
fn on_an_edge_only_edge_entries_play_none_moves_and_it_stands_on_leaving() {
    let mut b = brain(15);
    let feet = [140, 150];
    let win = WinRect { x: 100, y: 300, w: 800, h: 500, owner_pid: 42, layer: 0 };
    let start = panel(900, 900);
    let dropped = WinRect { x: 250 - feet[0], y: 290 - feet[1], ..start };
    let mut s = Senses { panel: Some(start), windows: vec![win], ..at(AFTERNOON) };
    b.senses(&s, 0);
    run(&mut b, 0, 3000);
    b.set_edges(Some(feet));
    b.poke(Poke::DragStart, 3000);
    s.panel = Some(dropped);
    b.senses(&s, 3100);
    b.poke(Poke::DragEnd, 3200);
    assert!(b.sitting());
    let shows = run(&mut b, 3200, 400_000);
    let bs = behaviors(&shows);
    assert_eq!(bs[0], "edge_sit", "{bs:?}");
    assert!(bs[1..].iter().all(|x| x.starts_with("edge_")), "{bs:?}");
    assert!(shows.iter().all(|(_, s)| s.moved_px == [0, 0]), "nothing moves the bear on an edge");
    // the window closes
    s.windows.clear();
    s.panel = Some(WinRect { x: 250 - feet[0], y: 300 - feet[1], ..start });
    b.senses(&s, 400_000);
    let off = run(&mut b, 400_000, 403_000);
    assert!(behaviors(&off).contains(&"edge_stand".to_string()), "{:?}", behaviors(&off));
}

// ---- morning, approval wait ----

#[test]
fn the_morning_plays_morning_stretch_then_greets() {
    let mut b = brain(16);
    let mut s = Senses { user_idle_ms: 9 * 3_600_000, ..at(7 * 60 + 30) };
    b.senses(&s, 0);
    run(&mut b, 0, 2000);
    s.user_idle_ms = 0;
    b.senses(&s, 2000);
    let shows = run(&mut b, 2000, 12_000);
    let bs = behaviors(&shows);
    let i = bs.iter().position(|x| x == "morning_stretch").unwrap_or_else(|| panic!("{bs:?}"));
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Greet), "{bs:?}");
    let greet_at = shows.iter().position(|(_, s)| s.intent == Intent::Greet).unwrap();
    let stretch_at = shows.iter().position(|(_, s)| s.behavior == "morning_stretch").unwrap();
    assert!(stretch_at < greet_at && i < bs.len());
}

#[test]
fn a_standing_ask_taps_its_foot_or_checks_its_watch_every_six_to_twelve_seconds() {
    let (mut b, _) = started(17);
    b.event(&MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run it?".into() }, 3000);
    let shows = run(&mut b, 3000, 43_000);
    let starts: Vec<(u64, String)> = {
        let mut v: Vec<(u64, String)> = Vec::new();
        for (t, s) in &shows {
            if v.last().map(|x| &x.1) != Some(&s.behavior) {
                v.push((*t, s.behavior.clone()));
            }
        }
        v.into_iter().filter(|(_, b)| b == "foot_tap" || b == "check_watch").collect()
    };
    assert!(starts.len() >= 3, "{starts:?}");
    assert!(starts[0].0 >= 11_000, "after 8 s: {starts:?}");
    for w in starts.windows(2) {
        assert_ne!(w[0].1, w[1].1, "no repeat: {starts:?}");
    }
    let notes: Vec<u64> = b.trace().iter().filter(|l| l.what == "react" && l.note.starts_with("approval_wait")).map(|l| l.t_ms).collect();
    for w in notes.windows(2) {
        assert!((6000..=12_100).contains(&(w[1] - w[0])), "every 6–12 s: {notes:?}");
    }
    assert!(shows.iter().filter(|(t, _)| *t > 11_000).any(|(_, s)| s.intent == Intent::Ask), "back to ask between");
}

// ---- interrupts through a posture's exit ----

#[test]
fn a_reaction_stands_a_sitting_bear_up_through_its_exit() {
    // find a seed whose day sits on the floor early
    for seed in 1..200 {
        let mut b = brain(seed);
        let mut s = at(AFTERNOON);
        b.senses(&s, 0);
        let shows = run(&mut b, 0, 600_000);
        let Some(&(t, _)) = shows.iter().find(|(t, s)| s.clip == "sit_floor" && *t > 3000) else { continue };
        let mut b = brain(seed);
        b.senses(&s, 0);
        run(&mut b, 0, t + 500);
        assert_eq!(b.tick(t + 500).clip, "sit_floor", "seed {seed}");
        let t2 = pet(&mut b, &mut s, t + 500);
        let after = run(&mut b, t2, t2 + 6000);
        let c = clips(&after);
        let up = c.iter().position(|x| x == "sit_floor_up").unwrap_or_else(|| panic!("seed {seed}: {c:?}"));
        let giggle = c.iter().position(|x| x == "giggle").unwrap_or_else(|| panic!("seed {seed}: {c:?}"));
        assert!(up < giggle, "seed {seed}: {c:?}");
        return;
    }
    panic!("no seed sits on the floor in 10 minutes");
}

// ---- a pack without contexts or reactions ----

#[test]
fn without_reactions_a_drag_holds_and_the_cursor_is_ignored() {
    let p = react_pack();
    let mut v1 = (*p).clone();
    v1.contexts.clear();
    v1.reactions.clear();
    let mut b = Brain::new(Arc::new(v1), BrainConfig { seed: 1, temperament: None, dnd_action: DndAction::Still }, 0);
    let mut s = at(AFTERNOON);
    b.senses(&s, 0);
    run(&mut b, 0, 3000);
    let t = pet(&mut b, &mut s, 3000);
    cursor(&mut b, &mut s, cur(80, 0, false), t);
    let shows = run(&mut b, t, t + 30_000);
    assert!(react_notes(&b).is_empty());
    assert!(!behaviors(&shows).iter().any(|x| x == "giggle" || x == "cursor_watch"));
    b.poke(Poke::DragStart, 40_000);
    assert_eq!(b.tick(40_000).behavior, "hold");
    b.poke(Poke::DragEnd, 42_000);
    assert_ne!(b.tick(42_000).behavior, "land_squash");
}

// ---- month (p1 leftover: `when.months` once the month is fed) ----

#[test]
fn a_months_entry_plays_in_its_months_only() {
    let p = react_pack();
    let mut m = (*p).clone();
    let e = m.idle_pool.iter_mut().find(|e| e.name() == "evening_yawn").unwrap();
    e.when = Some(mascot_protocol::pack::When { months: Some(vec![12, 1, 2]), ..Default::default() });
    e.weight = 50.0;
    let m = Arc::new(m);
    let count = |month: u8| {
        let mut b = Brain::new(m.clone(), BrainConfig { seed: 3, temperament: None, dnd_action: DndAction::Still }, 0);
        b.senses(&Senses { month, ..at(AFTERNOON) }, 0);
        run(&mut b, 0, 600_000);
        picks(&b, 0, 600_000).iter().filter(|x| *x == "evening_yawn").count()
    };
    assert_eq!(count(7), 0, "July");
    assert_eq!(count(0), 0, "unknown month");
    assert!(count(12) > 0, "December");
}


/// Review fix: a follow whose 1.5 s came while the bear was busy (an agent turn) starts once it is
/// free, with no new cursor sample (the past deadline is a new one, not the one already checked).
#[test]
fn a_follow_due_during_a_turn_starts_when_the_turn_ends() {
    let (mut b, mut s) = started(4);
    b.event(&MascotEvent::Thinking, 3000);
    cursor(&mut b, &mut s, cur(80, 0, false), 3000);
    // a sense during the turn: react_eval runs while not free
    b.senses(&s, 6000);
    let during = run(&mut b, 3000, 8000);
    assert!(!during.iter().any(|(_, s)| s.behavior == "cursor_watch"), "not during the turn");
    b.event(&MascotEvent::Idle, 8000);
    let after = run(&mut b, 8000, 20_000);
    assert!(after.iter().any(|(_, s)| s.behavior == "cursor_watch"), "{:?}", behaviors(&after));
}

#[test]
fn a_flinch_cuts_the_follow_watch() {
    let (mut b, mut s) = started(4);
    cursor(&mut b, &mut s, cur(112, 0, false), 3000);
    run(&mut b, 3000, 6000);
    assert_eq!(b.tick(6000).behavior, "cursor_watch");
    cursor(&mut b, &mut s, cur(-40, 0, false), 6050); // 152 pt in one sample gap: 1520 pt/s, to 40 pt
    let n = react_notes(&b);
    assert!(n.iter().any(|x| x == "follow end (flinch)"), "{n:?}");
    assert!(n.iter().any(|x| x == "flinch → flinch"), "{n:?}");
    assert!(run(&mut b, 6050, 8000).iter().any(|(_, s)| s.behavior == "flinch"));
}

#[test]
fn a_turn_held_back_by_the_one_second_limit_still_happens() {
    let (mut b, mut s) = started(6);
    cursor(&mut b, &mut s, cur(80, 0, false), 3000);
    run(&mut b, 3000, 8000);
    assert_eq!(b.tick(8000).dir, Dir::E);
    // south, then west 400 ms later (inside the one-second limit), then parked: no more samples
    cursor(&mut b, &mut s, cur(0, -80, false), 8000);
    run(&mut b, 8000, 8400);
    cursor(&mut b, &mut s, cur(-80, 0, false), 8400);
    let shows = run(&mut b, 8400, 14_000);
    let last = shows.last().unwrap().1.clone();
    assert_eq!((last.behavior.as_str(), last.dir), ("cursor_watch", Dir::W), "{:?}", clips(&shows));
}

#[test]
fn hold_is_refused_on_any_context_but_away_long() {
    let (_tmp, r) = crate::test_pack::copy("crates/mascot-core/tests/fixtures/react_pack", "hold-on-away", |v, _| {
        let h = v["contexts"]["away_long"]["hold"].clone();
        v["contexts"]["away"]["hold"] = h;
    });
    let e = r.err().expect("refused").to_string();
    assert!(e.contains("contexts: away: hold is for away_long only"), "{e}");
}

#[test]
fn follow_without_a_cooldown_is_refused() {
    let (_tmp, r) = crate::test_pack::copy("crates/mascot-core/tests/fixtures/react_pack", "follow-no-cooldown", |v, _| {
        v["reactions"]["follow"].as_object_mut().unwrap().remove("cooldown_s");
    });
    let e = r.err().expect("refused").to_string();
    assert!(e.contains("reactions: follow needs cooldown_s > 0"), "{e}");
}

#[test]
fn the_agents_gesture_cuts_the_watch_and_no_flinch_ends_it_twice() {
    let (mut b, mut s) = started(4);
    cursor(&mut b, &mut s, cur(112, 0, false), 3000);
    run(&mut b, 3000, 6000);
    assert_eq!(b.tick(6000).behavior, "cursor_watch");
    b.event(&MascotEvent::Emote { name: "wave".into() }, 6000);
    let n = react_notes(&b);
    assert!(n.iter().any(|x| x == "cut (agent)") && n.iter().any(|x| x == "follow end (cut)"), "{n:?}");
    cursor(&mut b, &mut s, cur(-40, 0, false), 6050);
    assert!(!react_notes(&b).iter().any(|x| x == "follow end (flinch)"), "the watch is already over");
    // and the agent's gesture does not wait out the 20 s watch
    let shows = run(&mut b, 6050, 9000);
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Wave || s.behavior == "wave"), "{:?}", behaviors(&shows));
}

#[test]
fn the_dir_the_watch_faced_stays_hidden_until_the_bear_faces_elsewhere() {
    let (mut b, mut s) = started(4);
    cursor(&mut b, &mut s, cur(80, 0, false), 3000);
    run(&mut b, 3000, 6000);
    assert!(b.hides_dir(Dir::E) && b.hides_dir(Dir::S), "the watch is on: every dir");
    cursor(&mut b, &mut s, None, 6000);
    let after = run(&mut b, 6000, 9000);
    assert!(b.hides_dir(Dir::E) || after.iter().any(|(_, s)| s.dir != Dir::E), "still facing east: hidden");
    let end = run(&mut b, 9000, 120_000);
    let elsewhere = end.iter().position(|(_, s)| s.dir != Dir::E).expect("turns away at some point");
    let later = end[elsewhere].0;
    run(&mut b, later, later + 1);
    assert!(!b.hides_dir(Dir::E), "faced elsewhere since");
    assert!(!b.hides_dir(Dir::W), "a dir the watch never faced");
}
