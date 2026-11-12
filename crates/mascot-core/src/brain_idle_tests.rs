//! Idle-brain tests (aliveness 2, p1, design §1.3–1.6): the variety rule over 10 000 picks (AC17),
//! the relax ladder (non-happy), `for_s` holds and idle station visits (AC18), turn-first from all
//! 8 dirs, `when` eligibility and hidden entries. On `tests/fixtures/variety_pack` and the bear.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mascot_protocol::pack::{PoolEntry, PoolKind, When};
use mascot_protocol::MascotEvent;

use super::*;
use crate::pack;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> Arc<PackMeta> {
    Arc::new(pack::load(&repo().join("crates/mascot-core/tests/fixtures").join(name)).unwrap_or_else(|e| panic!("{e}")))
}

fn bear() -> Arc<PackMeta> {
    Arc::new(pack::load(&repo().join("mascots/bear")).unwrap_or_else(|e| panic!("{e}")))
}

fn brain(pack: Arc<PackMeta>, seed: u64) -> Brain {
    Brain::new(pack, BrainConfig { seed, temperament: None, dnd_action: DndAction::Hide }, 0)
}

/// Ticks at every wake until `until`, returning each show with its time.
fn run_until(b: &mut Brain, from: u64, until: u64) -> Vec<(u64, Show)> {
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

/// Contiguous runs of one behavior: (behavior, from, to).
fn segments(shows: &[(u64, Show)], end: u64) -> Vec<(String, u64, u64)> {
    let mut out: Vec<(String, u64, u64)> = Vec::new();
    for (k, (t, s)) in shows.iter().enumerate() {
        let to = shows.get(k + 1).map_or(end, |n| n.0);
        match out.last_mut() {
            Some((b, _, e)) if *b == s.behavior => *e = to,
            _ => out.push((s.behavior.clone(), *t, to)),
        }
    }
    out
}

/// AC17 mechanism: 10 000 seeded picks, 6 s apart (a fidget's length), keep every rule and
/// never relax.
#[test]
fn ten_thousand_picks_keep_the_variety_rule() {
    let p = fixture("variety_pack");
    let v = p.variety.clone().expect("the fixture has a variety block");
    let mut b = brain(p.clone(), 11);
    let mut seq: Vec<(String, PoolKind)> = Vec::new();
    for i in 0..10_000u64 {
        let t = 300_000 + i * 6_000;
        let (name, _) = b.pick(t);
        b.last_start.insert(name.clone(), t);
        let e = p.idle_pool.iter().find(|e| e.name() == name).unwrap();
        seq.push((e.group_key().to_string(), e.kind));
    }
    assert!(!b.trace().iter().any(|l| l.what == "pick" && l.note.starts_with("relaxed")), "no relaxed pick");
    for k in 1..seq.len() {
        assert_ne!(seq[k].0, seq[k - 1].0, "group {} twice in a row at pick {k}", seq[k].0);
        let from = (k + 1).saturating_sub(v.window as usize);
        let n = seq[from..=k].iter().filter(|g| g.0 == seq[k].0).count();
        assert!(n <= v.max_in_window as usize, "{} {n} times in the window ending at {k}", seq[k].0);
        let moves = seq[..=k].iter().rev().take_while(|g| g.1 == PoolKind::Move).count();
        assert!(moves <= v.max_moves_in_row as usize, "{moves} moves in a row at {k}");
    }
    let distinct: BTreeSet<&str> = seq.iter().map(|g| g.0.as_str()).collect();
    assert!(distinct.len() >= 14, "{distinct:?}");
}

/// Non-happy: three entries cannot keep "≤ 2 in 20": the ladder relaxes `max_in_window`, then
/// cooldowns, then falls back to rest (look_around after a rest); no_repeat is never relaxed.
#[test]
fn too_few_entries_relax_the_rules_in_order() {
    let p = fixture("variety_pack_3");
    let mut b = brain(p.clone(), 5);
    let mut names = Vec::new();
    for i in 0..60u64 {
        let t = 300_000 + i * 1_000;
        let (n, _) = b.pick(t);
        b.last_start.insert(n.clone(), t);
        names.push(n);
    }
    let relaxed: BTreeSet<&str> = b.trace().iter().filter(|l| l.what == "pick").filter_map(|l| l.note.strip_prefix("relaxed ")).collect();
    assert!(relaxed.contains("max_in_window"), "{relaxed:?}");
    assert!(relaxed.contains("cooldown"), "{relaxed:?}");
    for k in 1..names.len() {
        assert_ne!(names[k], names[k - 1], "no_repeat holds at {k}: {names:?}");
    }
    // every pick traces its name and the total
    let pl: Vec<&str> = b.trace().iter().filter(|l| l.what == "pick" && !l.note.starts_with("relaxed")).map(|l| l.note.as_str()).collect();
    assert_eq!(pl.len(), 60);
    assert!(pl.iter().all(|n| n.split_once(" total ").is_some_and(|(name, w)| names.contains(&name.to_string()) && w.parse::<f64>().is_ok())), "{pl:?}");
    // with every entry barred and no ladder step left: rest, or look_around right after a rest
    let mut q = (*p).clone();
    q.idle_pool.retain(|e| e.name() == "rest");
    let mut b = brain(Arc::new(q), 6);
    assert_eq!(b.pick(1000).0, "rest");
    assert_eq!(b.pick(2000).0, "look_around", "rest was last");
    let rules: Vec<&str> = b.trace().iter().filter_map(|l| l.note.strip_prefix("relaxed ")).collect();
    assert_eq!(rules.last(), Some(&"rest"));
}

/// Turn first (§1.5): from each of the 8 dirs a front-only (F5) and a back-only behavior turn to the
/// nearest dir their first clip has, through turn clips, then play it there; already there: no turn.
#[test]
fn turn_first_from_all_eight_dirs() {
    let p = fixture("variety_pack");
    let front = [Dir::S, Dir::SE, Dir::E, Dir::W, Dir::SW];
    let back = [Dir::N, Dir::NE, Dir::NW];
    for (behavior, clip, dirs) in [("ear_twitch", "fid_front", &front[..]), ("peek", "fid_back", &back[..])] {
        for d0 in Dir::ALL {
            let mut b = brain(p.clone(), 3);
            b.dir = d0;
            b.start(behavior, Intent::Idle, 1000);
            let shows = run_until(&mut b, 1000, 9000);
            let first = shows.iter().find(|(_, s)| s.clip == clip).unwrap_or_else(|| panic!("{behavior} from {d0:?} plays"));
            assert!(dirs.contains(&first.1.dir), "{behavior} from {d0:?} plays facing {:?}", first.1.dir);
            let before: Vec<&Show> = shows.iter().take_while(|(_, s)| s.clip != clip).map(|(_, s)| s).collect();
            if dirs.contains(&d0) {
                assert!(before.is_empty(), "{behavior} from {d0:?}: no turn needed");
            } else {
                assert!(!before.is_empty() && before.iter().all(|s| s.clip == "turn_ccw" || s.clip == "turn_cw"), "{behavior} from {d0:?}: only turn clips first");
                let steps = d0.ccw_to(first.1.dir).min(8 - d0.ccw_to(first.1.dir));
                let nearest = dirs.iter().map(|d| d0.ccw_to(*d).min(8 - d0.ccw_to(*d))).min().unwrap();
                assert_eq!(steps, nearest, "{behavior} from {d0:?} turns the shortest way");
            }
            // it never changes facing without a turn clip
            let mut dir = d0;
            for (_, s) in &shows {
                if s.dir != dir {
                    assert!(s.clip == "turn_ccw" || s.clip == "turn_cw" || dir.ccw_to(s.dir) == 1 || dir.ccw_to(s.dir) == 7, "{behavior}: snap {dir:?} -> {:?}", s.dir);
                    dir = s.dir;
                }
            }
        }
    }
}

/// `{"for_s": [a, b]}`: whole loops until a seeded draw in [a, b] s has passed.
#[test]
fn for_s_holds_whole_loops_for_the_draw() {
    let p = fixture("variety_pack");
    let loop_ms: u64 = p.clips["fid_hold"].durs_ms.iter().map(|d| *d as u64).sum();
    for seed in 1..6 {
        let mut b = brain(p.clone(), seed);
        b.dir = Dir::S;
        b.start("sit_long", Intent::Idle, 0);
        let shows = run_until(&mut b, 0, 60_000);
        let seg = &segments(&shows, 60_000)[0];
        assert_eq!(seg.0, "sit_long");
        let len = seg.2 - seg.1;
        assert!((20_000..=40_000 + loop_ms).contains(&len), "seed {seed}: held {len} ms");
        assert_eq!(len % loop_ms, 0, "whole loops");
        let note = b.trace().iter().find(|l| l.what == "hold").map(|l| l.note.clone()).unwrap();
        let d: f64 = note.trim_start_matches("for ").trim_end_matches('s').parse().unwrap();
        assert!((20.0..=40.0).contains(&d) && (len as f64) >= d * 1000.0 - 50.0, "{note} vs {len}");
    }
}

/// AC18 on the bear: read, nap and the desk visit are each held 20–300 s from idle.
#[test]
fn the_bears_long_entries_hold_twenty_to_three_hundred_seconds() {
    let p = bear();
    assert!(p.available("desk"), "desk art is rendered");
    for name in ["read", "nap", "desk"] {
        for seed in 1..4 {
            let mut q = (*p).clone();
            q.idle_pool.retain(|e| e.name() == name || e.name() == "rest");
            for e in &mut q.idle_pool {
                if e.name() == name {
                    e.weight = 1000.0; // its cooldown keeps two holds apart
                }
            }
            q.warmup.exclude.clear();
            let mut b = brain(Arc::new(q), seed);
            let shows = run_until(&mut b, 0, 400_000);
            let segs = segments(&shows, 400_000);
            let seg = segs.iter().find(|s| s.0 == name).unwrap_or_else(|| panic!("{name} seed {seed} plays: {segs:?}"));
            let len = (seg.2 - seg.1) as f64 / 1000.0;
            assert!((20.0..=300.0).contains(&len), "{name} seed {seed}: {len} s");
            assert!(shows.iter().filter(|(_, s)| s.behavior == name).all(|(_, s)| s.intent == Intent::Idle), "{name} is idle throughout");
        }
    }
}

/// An idle desk visit: sit, type for the hold_s draw, stand; a turn while seated takes the seat over.
#[test]
fn an_idle_desk_visit_types_then_stands_and_a_turn_takes_it_over() {
    let mut q = (*bear()).clone();
    q.idle_pool.retain(|e| e.name() == "desk" || e.name() == "rest");
    q.idle_pool.iter_mut().for_each(|e| e.weight = if e.name() == "desk" { 1000.0 } else { 0.001 });
    let q = Arc::new(q);
    let mut b = brain(q.clone(), 2);
    let shows = run_until(&mut b, 0, 300_000);
    let clips: Vec<&str> = shows.iter().filter(|(_, s)| s.behavior == "desk").map(|(_, s)| s.clip.as_str()).collect();
    let first = |c: &str| clips.iter().position(|x| *x == c);
    assert!(first("desk_sit") < first("desk_type") && first("desk_type") < first("desk_stand"), "{clips:?}");
    let seat = b.trace().iter().find(|l| l.what == "seat" && l.note.starts_with("idle ")).expect("hold_s traced").note.clone();
    let d: f64 = seat.trim_start_matches("idle ").trim_end_matches('s').parse().unwrap();
    assert!((45.0..=150.0).contains(&d), "{seat}");
    // a turn opens while typing from idle: the seat becomes the turn's (a seated turn)
    let mut b = brain(q, 2);
    let shows = run_until(&mut b, 0, 30_000);
    assert_eq!(shows.last().unwrap().1.clip, "desk_type", "typing by 30 s");
    b.event(&MascotEvent::Thinking, 30_000);
    let shows = run_until(&mut b, 30_000, 34_000);
    assert!(shows.iter().all(|(_, s)| s.behavior == "desk"), "stays seated");
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Work), "the turn works at the desk");
    assert!(b.idle_seat.is_none());
}

/// `when`: hours [from, to) wrapping midnight, months (0 = unknown: never), a context never (p4).
#[test]
fn when_gates_entries() {
    let p = fixture("variety_pack");
    let mut b = brain(p, 1);
    let w = |hours: Option<[u8; 2]>, months: Option<Vec<u8>>, context: Option<&str>| When { hours, months, context: context.map(String::from) };
    b.local.minutes = 15 * 60;
    assert!(b.when_ok(&w(Some([14, 18]), None, None)));
    assert!(!b.when_ok(&w(Some([18, 14]), None, None)));
    b.local.minutes = 23 * 60 + 30;
    assert!(b.when_ok(&w(Some([22, 6]), None, None)), "wraps past midnight");
    assert!(!b.when_ok(&w(Some([14, 18]), None, None)));
    assert!(!b.when_ok(&w(None, Some(vec![12, 1, 2]), None)), "month unknown");
    b.month = 1;
    assert!(b.when_ok(&w(None, Some(vec![12, 1, 2]), None)));
    b.month = 7;
    assert!(!b.when_ok(&w(None, Some(vec![12, 1, 2]), None)));
    assert!(!b.when_ok(&w(None, None, Some("music"))), "contexts wait for p4");
    // the senses carry the month
    b.senses(&Senses { month: 12, ..Senses::default() }, 0);
    assert_eq!(b.month, 12);
}

/// An entry gated by `when` is never picked; when it holds it can be.
#[test]
fn a_when_entry_is_picked_only_inside_its_window() {
    let mut q = (*fixture("variety_pack")).clone();
    q.idle_pool.push(PoolEntry { behavior: Some("tummy_rub".into()), weight: 50.0, kind: PoolKind::Rare, group: Some("tea".into()), when: Some(When { hours: Some([14, 18]), months: None, context: None }), ..PoolEntry::default() });
    let q = Arc::new(q);
    let count = |minutes: u16| {
        let mut b = brain(q.clone(), 9);
        b.senses(&Senses { local: LocalTime { minutes, weekday: 0 }, ..Senses::default() }, 0);
        (0..200u64).filter(|i| { let (n, _) = b.pick(300_000 + i * 6000); b.last_start.insert(n.clone(), 300_000 + i * 6000); n == "tummy_rub" && b.picks.front().is_some_and(|p| p.group == "tea") }).count()
    };
    assert_eq!(count(10 * 60), 0);
    assert!(count(15 * 60) > 0);
}

/// The bear without the art of some sources (`anim_f/`, …): a temp copy of its `pack.json`
/// pointing at the real art, those sources left out. Removed on drop.
fn bear_without(dirs: &[&str]) -> (crate::test_pack::TempPack, Arc<PackMeta>) {
    let (guard, r) = crate::test_pack::copy("mascots/bear", &format!("without-{}", dirs.join("-")), |v, _| {
        v["sources"].as_array_mut().unwrap().retain(|s| !dirs.iter().any(|d| s["path"].as_str().unwrap().starts_with(&format!("{d}/"))));
    });
    (guard, Arc::new(r.unwrap_or_else(|e| panic!("{e}"))))
}

/// `#hidden-log`: wired entries whose art is missing load, wait, and never play. The bear has all
/// its A4 art since p2, so the test drops the five p2 stages' sources from a copy of it.
#[test]
fn hidden_entries_wait_and_never_play() {
    let (_tmp, p) = bear_without(&["anim_f", "anim_g", "anim_p", "anim_m", "anim_t"]);
    let w = p.waiting();
    let names: BTreeSet<&str> = w.iter().map(|x| x.name.as_str()).collect();
    for n in ["weight_shift", "butt_wiggle", "sit_floor", "tiptoe", "chase", "spin", "stretch", "ear_twitch"] {
        assert!(names.contains(n), "{n} waits: {names:?}");
    }
    // (the real bear has every A4 entry: tests/bear_pack.rs, AC12)
    // the art kit rendered set_lean (p1): lean is a live long entry with face layers from its face part
    assert!(!names.contains("lean") && p.available("lean"), "lean plays");
    let face = p.face.as_ref().expect("face");
    for ori in [Ori::A, Ori::B] {
        assert!(face.set_face("lean", ori).is_some(), "lean {ori:?} has a face");
    }
    let line = p.waiting_line().unwrap();
    let head = format!("pack: {} entries wait for art: weight_shift, ear_twitch, ", w.len());
    assert!(line.starts_with(&head) && line.contains(", … (idle "), "{line}");
    let mut b = brain(p.clone(), 4);
    let shows = run_until(&mut b, 0, 1_800_000);
    let played: BTreeSet<&str> = shows.iter().map(|(_, s)| s.behavior.as_str()).collect();
    assert!(played.iter().all(|n| !names.contains(n)), "{played:?}");
    let picked: BTreeSet<String> = b.trace().iter().filter(|l| l.what == "pick").filter_map(|l| l.note.split_once(" total ").map(|x| x.0.to_string())).collect();
    assert!(!picked.is_empty() && picked.iter().all(|n| !names.contains(n.as_str())), "{picked:?}");
    // an emote waiting for art is not offered, and `buddy mascot list` leaves waiting entries out
    let r = b.body(&crate::body::BodyReq::Emote { name: "ear_twitch".into() }, 1_800_000);
    assert!(r.error.is_some(), "{r:?}");
    let list = serde_json::to_value(b.body(&crate::body::BodyReq::List, 1_800_000)).unwrap();
    for (k, kind) in [("activities", "activity"), ("emotes", "emote")] {
        // by kind: `spin` the idle entry waits for its clip while `spin` the gesture emote plays its intent's
        let waits: BTreeSet<&str> = w.iter().filter(|x| x.kind == kind).map(|x| x.name.as_str()).collect();
        let listed: Vec<&str> = list[k].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
        assert!(!waits.is_empty() && !listed.is_empty() && listed.iter().all(|n| !waits.contains(n)), "{k}: {listed:?}, waiting {waits:?}");
    }
    let listed: Vec<&str> = list["emotes"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert!(!listed.contains(&"ear_twitch") && !listed.contains(&"hop"), "{listed:?}");
}


/// Review fixes: `do read` asked during an idle desk visit stands the bear up at the next loop end and
/// reads; `do read` during an idle read keeps reading until its own deadline, not the idle for_s draw.
#[test]
fn an_activity_takes_over_an_idle_desk_visit_and_an_idle_read() {
    use crate::body::BodyReq;
    let mut q = (*bear()).clone();
    q.idle_pool.retain(|e| e.name() == "desk" || e.name() == "rest");
    q.idle_pool.iter_mut().for_each(|e| e.weight = if e.name() == "desk" { 1000.0 } else { 0.001 });
    let mut b = brain(Arc::new(q), 2);
    let shows = run_until(&mut b, 0, 30_000);
    assert_eq!(shows.last().unwrap().1.clip, "desk_type");
    let r = b.body(&BodyReq::Do { name: "read".into(), for_ms: Some(60_000) }, 30_000);
    assert!(r.error.is_none(), "{r:?}");
    let shows = run_until(&mut b, 30_000, 45_000);
    let stand = shows.iter().position(|(_, s)| s.clip == "desk_stand").expect("stands up from the desk");
    assert!(shows[stand].0 < 36_000, "at the next loop end: {}", shows[stand].0);
    assert!(shows.iter().any(|(_, s)| s.behavior == "read" && s.intent == Intent::Do), "then reads");
    // an idle read relabelled by `do read --for 5m` holds to the deadline
    let mut q = (*bear()).clone();
    q.idle_pool.retain(|e| e.name() == "read" || e.name() == "rest");
    q.idle_pool.iter_mut().for_each(|e| e.weight = if e.name() == "read" { 1000.0 } else { 0.001 });
    q.warmup.exclude.clear();
    let mut b = brain(Arc::new(q), 3);
    let shows = run_until(&mut b, 0, 20_000);
    assert!(shows.last().is_some_and(|(_, s)| s.behavior == "read"), "reading idly by 20 s");
    let r = b.body(&BodyReq::Do { name: "read".into(), for_ms: Some(300_000) }, 20_000);
    assert!(r.error.is_none(), "{r:?}");
    let shows = run_until(&mut b, 20_000, 330_000);
    let up = shows.iter().find(|(_, s)| s.clip == "stand_up").expect("stands up at the end").0;
    assert!(up >= 319_000, "held to the deadline, not the idle draw: {up}");
}

/// Review fix: a v1 pack keeps the v1 rules (every free clip in S, no turn first).
#[test]
fn a_v1_pack_keeps_needing_s_and_never_turns_first() {
    let mut q = (*fixture("variety_pack")).clone();
    assert!(q.available("peek"), "v2: a back-only fidget plays (after a turn)");
    q.pack_version = 1;
    assert!(!q.available("peek"), "v1: no S, not available");
    assert!(q.available("ear_twitch"));
    let mut b = brain(Arc::new(q), 3);
    b.dir = Dir::N;
    b.start("ear_twitch", Intent::Idle, 1000);
    let shows = run_until(&mut b, 1000, 3000);
    assert!(shows.iter().all(|(_, s)| !s.clip.starts_with("turn_")), "no turn first in v1");
}
