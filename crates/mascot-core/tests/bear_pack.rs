//! The bear pack against the real art (aliveness A4: AC12, AC17).
//!
//! - every A4 storyboard behavior is rendered (available) and in the idle pool, none hidden;
//! - the pack carries the bear's `variety` block;
//! - a long seeded run keeps the variety rules with an energetic temperament and in quiet hours.
//!
//! `buddy studio audit` (AC11, AC17) measures time shares and targets; this file pins what the
//! pack itself must hold, so a pack edit that drops an A4 entry or breaks the rules fails here.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mascot_core::{pack, Brain, BrainConfig, DndAction, LocalTime, PackMeta, Senses};
use mascot_protocol::pack::{PoolKind, Temperament};

fn bear() -> Arc<PackMeta> {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots/bear");
    Arc::new(pack::load(&dir).expect("the bear pack loads"))
}

/// Storyboard rows 1–33 (prototype `#storyboard`): stage lean (p1) and stages f g p m t (p2).
const A4: [&str; 33] = [
    "lean", // stage lean
    "weight_shift", "ear_twitch", "look_up", "look_down", "tummy_rub", "wave_small", "peek", "butt_wiggle", // f
    "rock_heels", "head_tilt", "scratch", "stretch", "yawn", "shake_off", "sneeze", "nod_off", // g
    "sit_floor", "sit_sway", "lie_belly", "lie_back", "floor_nap", "squat", // p
    "tiptoe", "skip", "shuffle", "back_waddle", // m
    "hop", "spin", "roll", "trip", "chase", "slide", // t
];

/// The A4 behaviors that travel: the variety rule's `max_moves_in_row` counts them.
const A4_MOVES: [&str; 10] = ["tiptoe", "skip", "shuffle", "back_waddle", "hop", "spin", "roll", "trip", "chase", "slide"];

#[test]
fn every_a4_behavior_is_rendered_and_in_the_idle_pool() {
    let p = bear();
    let mut missing = vec![];
    for name in A4 {
        if !p.available(name) {
            missing.push(format!("{name}: not rendered ({:?})", p.missing_clip(name)));
        }
        match p.idle_pool.iter().find(|e| e.name() == name) {
            None => missing.push(format!("{name}: not in the idle pool")),
            Some(e) if e.weight <= 0.0 => missing.push(format!("{name}: weight {} in the idle pool", e.weight)),
            Some(e) if A4_MOVES.contains(&name) && e.kind != PoolKind::Move => missing.push(format!("{name}: kind {:?}, not move", e.kind)),
            Some(_) => {}
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
    let waiting: Vec<String> = p.waiting().into_iter().map(|w| w.name).filter(|n| A4.contains(&n.as_str())).collect();
    assert!(waiting.is_empty(), "A4 entries wait for art: {waiting:?}");
}

#[test]
fn every_idle_entry_plays_its_first_clip_in_some_direction() {
    // turn-first (design §1.5) needs at least one direction; a set behavior has oris instead
    let p = bear();
    for e in &p.idle_pool {
        let name = e.name();
        if e.station.is_some() || p.behavior(name).is_some_and(|b| b.set.is_some()) {
            continue;
        }
        assert!(!p.first_dirs(name).is_empty(), "{name}: its first clip has no direction");
    }
}

#[test]
fn the_bear_carries_the_variety_rule() {
    let p = bear();
    let v = p.variety.as_ref().expect("mascots/bear/pack.json has a variety block");
    assert!(v.no_repeat);
    assert_eq!((v.window, v.max_in_window, v.max_moves_in_row), (20, 2, 2));
    let t = v.targets.as_ref().expect("variety.targets");
    assert_eq!((t.minutes, t.stand_max, t.stand_move_max, t.distinct_min, t.share_max), (10.0, 0.25, 0.45, 20, 0.15));
    assert_eq!(t.long_len_s, [20.0, 300.0]);
    // 2 in 20 keeps every group at or under 10% of picks, under the 15% share target
    assert!((v.max_in_window as f64) / (v.window as f64) < t.share_max);
}

/// One pick from the trace: (time, name, group, kind).
type Pick = (u64, String, String, PoolKind);

/// Runs the brain on a quiet day for `minutes` and returns its picks and its `pick relaxed …` lines.
fn run(seed: u64, minutes: u64, temperament: Option<Temperament>, quiet_hours: bool) -> (Vec<Pick>, Vec<String>) {
    let p = bear();
    let dur = minutes * 60_000;
    let mut brain = Brain::new(p.clone(), BrainConfig { seed, temperament, dnd_action: DndAction::Hide }, 0);
    brain.senses(&Senses { local: LocalTime { minutes: 10 * 60, weekday: 0 }, month: 11, quiet_hours, ..Senses::default() }, 0);
    let mut trace = vec![];
    let mut t = 0;
    brain.tick(0);
    trace.extend(brain.drain_trace());
    while t < dur {
        t = brain.next_wake_ms().max(t + 1).min(dur);
        brain.tick(t);
        trace.extend(brain.drain_trace());
    }
    let (mut picks, mut relaxed) = (vec![], vec![]);
    for l in trace.iter().filter(|l| l.what == "pick") {
        if let Some(rule) = l.note.strip_prefix("relaxed ") {
            relaxed.push(rule.to_string());
        } else if let Some((name, _)) = l.note.split_once(" total ") {
            let e = p.idle_pool.iter().find(|e| e.name() == name);
            let group = e.map_or(name.to_string(), |e| e.group_key().to_string());
            picks.push((l.t_ms, name.to_string(), group, e.map_or(PoolKind::Other, |e| e.kind)));
        }
    }
    (picks, relaxed)
}

/// The hard rules of the pack's variety block, checked on a pick sequence (as the audit does).
fn violations(picks: &[Pick]) -> Vec<String> {
    let v = bear().variety.clone().unwrap();
    let mut out = vec![];
    for k in 0..picks.len() {
        let (at, name, group, _) = &picks[k];
        if k > 0 && picks[k - 1].2 == *group {
            out.push(format!("{at} ms: {name}: group {group} twice in a row"));
        }
        let from = (k + 1).saturating_sub(v.window as usize);
        let n = picks[from..=k].iter().filter(|p| p.2 == *group).count();
        if n > v.max_in_window as usize {
            out.push(format!("{at} ms: {name}: group {group} {n} times in the last {}", v.window));
        }
        let moves = picks[..=k].iter().rev().take_while(|p| p.3 == PoolKind::Move).count();
        if moves > v.max_moves_in_row as usize {
            out.push(format!("{at} ms: {name}: {moves} moves in a row"));
        }
    }
    out
}

#[test]
fn thirty_minutes_keep_the_variety_rule_with_any_temperament() {
    let energetic = Temperament { energy: 1.0, ..Temperament::default() };
    let sleepy = Temperament { energy: 0.0, sleepiness: 1.0, ..Temperament::default() };
    for (label, temp) in [("pack", None), ("energy=1.0", Some(energetic)), ("sleepiness=1.0", Some(sleepy))] {
        for seed in [1, 2, 3] {
            let (picks, relaxed) = run(seed, 30, temp.clone(), false);
            assert!(picks.len() >= 60, "{label} seed {seed}: only {} picks in 30 min", picks.len());
            assert_eq!(violations(&picks), Vec::<String>::new(), "{label} seed {seed}");
            assert!(relaxed.is_empty(), "{label} seed {seed}: relaxed picks {relaxed:?}");
            let distinct: std::collections::BTreeSet<&str> = picks.iter().map(|p| p.1.as_str()).collect();
            assert!(distinct.len() >= 20, "{label} seed {seed}: {} behaviors", distinct.len());
        }
    }
}

#[test]
fn quiet_hours_never_repeat_and_say_when_they_relax() {
    // the quiet pool (nap, rest) cannot fill a window of 20 with 2 per group: the brain relaxes the
    // window cap or a cooldown, says so in the trace, and still never repeats a group back to back
    // nor moves
    let p = bear();
    for seed in [1, 2, 3] {
        let (picks, relaxed) = run(seed, 60, None, true);
        assert!(picks.len() >= 4, "seed {seed}: {} picks", picks.len());
        for (at, name, ..) in &picks {
            assert!(p.quiet_hours.pool.iter().any(|q| q == name), "seed {seed}: {at} ms: {name} is not in the quiet pool");
        }
        for (k, w) in picks.windows(2).enumerate() {
            assert_ne!(w[0].2, w[1].2, "seed {seed}: pick {k}: {} twice in a row", w[1].2);
        }
        for r in &relaxed {
            assert!(r == "max_in_window" || r == "cooldown", "seed {seed}: quiet hours relaxed {r:?}");
        }
    }
}

#[test]
fn every_posture_and_long_entry_with_a_get_up_clip_exits_through_it() {
    // a click, an agent turn or a reaction interrupts a posture: without `exit` (or a set) the
    // brain cuts it at once and the bear jumps from lying on the floor to listen_in
    let p = bear();
    let mut checked = 0;
    for e in &p.idle_pool {
        if e.station.is_some() || !matches!(e.kind, PoolKind::Posture | PoolKind::Long) {
            continue;
        }
        let name = e.name();
        let def = p.behavior(name).unwrap_or_else(|| panic!("{name}: no behavior"));
        let last = &def.seq.last().unwrap_or_else(|| panic!("{name}: empty seq")).clip;
        if def.seq.len() > 1 && (last.ends_with("_up") || last.ends_with("_out")) {
            assert!(def.set.is_some() || def.exit, "{name}: ends in {last} but has no exit: an interrupt cuts it");
            checked += 1;
        }
    }
    assert!(checked >= 6, "only {checked} entries end in a get-up clip");
}
