//! The bear pack against the real art (aliveness A4: AC12, AC17).
//!
//! - every A4 storyboard behavior is rendered (available) and in the idle pool, none hidden;
//! - the pack carries the bear's `variety` block;
//! - a long seeded run keeps the variety rules with an energetic temperament and in quiet hours;
//! - (A5: AC13) each of the 11 intents plays its own clip, the new gestures are listed emotes, and
//!   without their art the intents fall back down their chains;
//! - (p4: AC14) the reactions and contexts play stage r/s/w art;
//! - (p5: AC19, AC18) every prop activity and rare surprise is rendered, in the idle pool and listed,
//!   rare ones on a long cooldown, hats and winter props only in their months; nothing waits for art.
//!
//! `buddy studio audit` (AC11, AC17) measures time shares and targets; this file pins what the
//! pack itself must hold, so a pack edit that drops an A4 entry or breaks the rules fails here.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mascot_core::{body, pack, replay, Brain, BrainConfig, DndAction, Intent, LocalTime, NowPlaying, PackMeta, Senses};
use mascot_protocol::pack::{Loops, PoolKind, Temperament};

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
    run_senses(seed, minutes, temperament, Senses { quiet_hours, ..Senses::default() })
}

/// `run` with these senses held all along (10:00, November).
fn run_senses(seed: u64, minutes: u64, temperament: Option<Temperament>, senses: Senses) -> (Vec<Pick>, Vec<String>) {
    run_at(seed, minutes, temperament, Senses { local: LocalTime { minutes: 10 * 60, weekday: 0 }, month: 11, ..senses })
}

/// `run` with exactly these senses (local time and month included) held all along.
fn run_at(seed: u64, minutes: u64, temperament: Option<Temperament>, senses: Senses) -> (Vec<Pick>, Vec<String>) {
    let p = bear();
    let dur = minutes * 60_000;
    let mut brain = Brain::new(p.clone(), BrainConfig { seed, temperament, dnd_action: DndAction::Hide }, 0);
    brain.senses(&senses, 0);
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

/// The 11 A5 intents (AC13): each has its own clip, rendered by stage e (gestures-a) or by the p2
/// stages (`stretch` from g, `spin` from t).
const A5_INTENTS: [&str; 11] = ["wave", "nod", "shake", "greet", "stretch", "sad", "surprised", "jump", "spin", "dodge", "remind"];

/// The A5 emotes the agent can use: stage x's 10 gestures (gestures-b) and the intents that were not emotes yet.
const A5_EMOTES: [&str; 16] = [
    "shrug", "facepalm", "thumbs_up", "clap", "point_bubble", "shy_hide", "laugh", "pout", "proud", "head_scratch", // x
    "greet", "stretch", "sad", "surprised", "dodge", "remind",
];

fn clips_of(p: &PackMeta, behavior: &str) -> Vec<String> {
    p.behavior(behavior).map_or_else(Vec::new, |b| b.seq.iter().map(|s| s.clip.clone()).collect())
}

#[test]
fn each_a5_intent_plays_its_own_clip() {
    let p = bear();
    let firsts: Vec<(&str, String)> = A5_INTENTS.iter().map(|n| (*n, p.resolve(Intent::parse(n).unwrap()).to_string())).collect();
    // own behavior playing exactly its own clip (named as the intent) is stricter than the
    // prototype's rule "no other intent's first behavior uses its clips": the 11 names differ
    for (intent, b) in &firsts {
        assert_eq!(b, intent, "{intent}: resolves to {b}, not its own behavior");
        assert_eq!(clips_of(&p, b), [intent.to_string()], "{intent}: {b} plays more than its own clip");
        // a fallback chain stays behind it
        assert!(p.intents[&Intent::parse(intent).unwrap()].len() >= 2, "{intent}: no fallback behind its own behavior");
    }
}

#[test]
fn the_a5_gestures_are_listed_and_in_the_prompt() {
    let p = bear();
    let listed = body::emotes(&p);
    let prompt = body::prompt_text(&p).unwrap();
    for name in A5_EMOTES {
        let e = listed.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name}: not in `buddy mascot list`"));
        assert_eq!(e.group, "gesture", "{name}");
        assert!(!e.about.is_empty(), "{name}: no about");
        assert!(prompt.contains(&format!("- {name}: {}\n", e.about)), "{name}: not in the prompt's gestures");
    }
    let waiting: Vec<String> = p.waiting().into_iter().map(|w| w.name).filter(|n| A5_EMOTES.contains(&n.as_str())).collect();
    assert!(waiting.is_empty(), "A5 emotes wait for art: {waiting:?}");
}

/// Non-happy: without stages e and x the intents fall back down their chains and the gestures
/// wait for art (left out of the list, named in the load log), the pack still loads.
#[test]
fn without_stages_e_and_x_the_intents_fall_back_and_the_gestures_wait() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots/bear");
    let dir = std::env::temp_dir().join(format!("mascot-bear-no-ex-{}", std::process::id()));
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(src.join("pack.json")).unwrap()).unwrap();
    let abs = |rel: &serde_json::Value| serde_json::Value::String(src.join(rel.as_str().unwrap()).canonicalize().unwrap().to_string_lossy().into_owned());
    v["art_root"] = abs(&v["art_root"]);
    v["palettes"] = v["palettes"].as_array().unwrap().iter().map(abs).collect();
    let sources = v["sources"].as_array_mut().unwrap();
    sources.retain(|s| !s["path"].as_str().unwrap().starts_with("anim_e/") && !s["path"].as_str().unwrap().starts_with("anim_x/"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pack.json"), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let p = pack::load(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    let p = p.expect("the bear pack loads without stages e and x");
    let want = [("wave", "happy"), ("nod", "happy"), ("shake", "oops"), ("greet", "happy"), ("sad", "oops"), ("surprised", "oops"), ("jump", "celebrate"), ("dodge", "dash"), ("remind", "ask"), ("stretch", "stretch"), ("spin", "spin")];
    for (intent, b) in want {
        assert_eq!(p.resolve(Intent::parse(intent).unwrap()), b, "{intent}");
    }
    let listed: Vec<String> = body::emotes(&p).into_iter().map(|e| e.name).collect();
    for name in ["remind", "shrug", "facepalm", "thumbs_up", "clap", "point_bubble", "shy_hide", "laugh", "pout", "proud", "head_scratch"] {
        assert!(!listed.iter().any(|n| n == name), "{name}: listed without its art");
        assert!(p.waiting().iter().any(|w| w.kind == "emote" && w.name == name), "{name}: not waiting");
    }
    for name in ["greet", "sad", "surprised", "dodge", "stretch"] {
        assert!(listed.iter().any(|n| n == name), "{name}: an intent emote stays listed (it falls back)");
    }
    let line = p.waiting_line().expect("a waiting line");
    assert!(line.contains("wait for art") && line.contains("remind"), "{line}");
}

/// AC13: `crates/buddy-app/tests/fixtures/emotes.jsonl` (each gesture emote once, 5 s apart) plays
/// every emote's own behavior through the brain.
#[test]
fn the_emotes_fixture_plays_each_gesture_with_its_own_behavior() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../buddy-app/tests/fixtures/emotes.jsonl");
    let items: Vec<replay::Item> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    let names: Vec<String> = items.iter().filter_map(|i| match &i.event {
        Some(mascot_protocol::MascotEvent::Emote { name }) => Some(name.clone()),
        _ => None,
    }).collect();
    let p = bear();
    let gestures: std::collections::BTreeSet<String> = body::emotes(&p).into_iter().filter(|e| e.group == "gesture").map(|e| e.name).collect();
    assert_eq!(names.iter().cloned().collect::<std::collections::BTreeSet<_>>(), gestures, "the fixture has each gesture emote once");
    assert_eq!(names.len(), gestures.len());
    let end = items.last().unwrap().at_ms + 8000;
    // seed 1: no idle desk spell or long prop set overlaps an emote (an emote at the desk is refused as busy, one
    // during a prop set waits for its exit; p5's sets moved seed 3's picks)
    let tl = replay::Timeline { timeline_version: replay::TIMELINE_VERSION, seed: 1, duration_ms: end, start_local: "10:00".into(), weekday: 2, items };
    let log = replay::run(p, &tl);
    let items = &tl.items;
    for (k, name) in names.iter().enumerate() {
        let (from, to) = (items[k].at_ms, items.get(k + 1).map_or(end, |i| i.at_ms));
        // a `plays` emote runs under intent emote, an `intent` emote under its own intent
        let played: Vec<&str> = log
            .iter()
            .filter(|l| (from..to).contains(&l.t_ms) && (l.intent == Intent::Emote || l.intent.as_str() == name))
            .map(|l| l.behavior.as_str())
            .collect();
        assert!(played.contains(&name.as_str()), "emote {name} at {from} ms played {played:?}");
        assert!(played.iter().all(|b| b == name), "emote {name} at {from} ms also played {played:?}");
    }
}

/// p4 (react-brain, AC14): the stage r/s/w art is wired. Every reaction plays a rendered behavior
/// (morning's `greet` is an intent) and every context has its own rendered pool entries. Since p5
/// nothing waits for art and nightcap (set bed_cap) is a quiet-hours pick.
#[test]
fn the_p4_reactions_and_contexts_play_on_the_real_art() {
    let p = bear();
    let want = ["approval_wait", "dizzy", "drag_hold", "drag_land", "edge_land", "flinch", "follow", "morning", "pet"];
    assert_eq!(p.reactions.keys().map(String::as_str).collect::<Vec<_>>(), want);
    for (name, r) in &p.reactions {
        for b in r.names() {
            assert!(p.available(b) || (name == "morning" && Intent::parse(b).is_some()), "reaction {name}: {b} has no art");
        }
    }
    for (name, c) in &p.contexts {
        let own: Vec<&str> = p.idle_pool.iter().filter(|e| e.when.as_ref().and_then(|w| w.context.as_deref()) == Some(name)).map(|e| e.name()).collect();
        assert!(!own.is_empty() || c.hold.is_some(), "context {name} has no entries");
        for b in own.iter().copied().chain(c.hold.as_deref()) {
            assert!(p.available(b), "context {name}: {b} has no art");
        }
    }
    let waiting: Vec<(&str, String)> = p.waiting().into_iter().map(|w| (w.kind, w.name)).collect();
    assert_eq!(waiting, [], "nothing waits for art after p5");
    assert!(p.available("nightcap") && p.behavior("nightcap").and_then(|b| b.set.as_deref()) == Some("bed_cap"));
    // its idle entry holds 23:00-07:00 only: a 23:30 quiet hour picks it, a 10:00 one never does
    let at = |minutes, seed| {
        let s = Senses { quiet_hours: true, local: LocalTime { minutes, weekday: 0 }, month: 11, ..Senses::default() };
        run_at(seed, 60, None, s).0.into_iter().map(|k| k.1).collect::<Vec<String>>()
    };
    let night: Vec<String> = (1..=3).flat_map(|seed| at(23 * 60 + 30, seed)).collect();
    assert!(night.iter().any(|n| n == "nightcap"), "nightcap is a quiet-hours pick: {night:?}");
    assert!(night.iter().all(|n| ["nap", "rest", "nightcap"].contains(&n.as_str())), "sleepy pool only: {night:?}");
    let day: Vec<String> = (1..=3).flat_map(|seed| at(10 * 60, seed)).collect();
    assert!(!day.iter().any(|n| n == "nightcap"), "nightcap outside its hours: {day:?}");
}

/// p4: the drag fixture on the real bear: it dangles while held, lands with a squash, and the
/// long drag (not the 0.2 s one) makes it dizzy.
#[test]
fn the_drag_fixture_dangles_and_lands_on_the_real_bear() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../buddy-app/tests/fixtures/drag.jsonl");
    let items: Vec<replay::Item> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    let tl = replay::Timeline { timeline_version: replay::TIMELINE_VERSION, seed: 4, duration_ms: 20_000, start_local: "10:00".into(), weekday: 2, items };
    let mut seq: Vec<(u64, String)> = Vec::new();
    for l in replay::run(bear(), &tl) {
        if seq.last().map(|x| &x.1) != Some(&l.behavior) {
            seq.push((l.t_ms, l.behavior));
        }
    }
    let names = |a: u64, b: u64| -> Vec<&str> { seq.iter().filter(|(t, _)| (a..b).contains(t)).map(|(_, x)| x.as_str()).collect() };
    let first = names(0, 15_000);
    let i = first.iter().position(|b| *b == "dangle").unwrap_or_else(|| panic!("{first:?}"));
    assert_eq!(&first[i..i + 3], ["dangle", "land_squash", "dizzy"], "{first:?}");
    let second = names(15_000, 20_000);
    let j = second.iter().position(|b| *b == "dangle").unwrap_or_else(|| panic!("{second:?}"));
    assert_eq!(second.get(j + 1), Some(&"land_squash"), "{second:?}");
    assert!(!second.contains(&"dizzy"), "{second:?}");
}

/// Lead condition (e): reactions cannot dominate idle. Thirty minutes of music on the bear keep
/// the variety rules, and the music entries stay a minority of the picks.
#[test]
fn thirty_minutes_of_music_keep_the_variety_rule_and_react_entries_stay_a_minority() {
    let music = Senses { now_playing: Some(NowPlaying { source: "music".into(), title: "x".into(), playing: true }), ..Senses::default() };
    for seed in [1, 2, 3] {
        let (picks, relaxed) = run_senses(seed, 30, None, music.clone());
        assert_eq!(violations(&picks), Vec::<String>::new(), "seed {seed}");
        assert!(relaxed.is_empty(), "seed {seed}: {relaxed:?}");
        let react = picks.iter().filter(|p| p.3 == PoolKind::React).count();
        let share = react as f64 / picks.len() as f64;
        assert!(react > 0 && share <= 0.35, "seed {seed}: react {react} of {} picks", picks.len());
    }
}

/// Storyboard rows 71–86 (prototype `#sb-props-*`): (behavior, set, body group).
const A6_PROPS: [(&str, &str, &str); 16] = [
    ("tea", "tea", "pastime"), ("snack", "snack", "pastime"), ("knit", "knit", "pastime"),
    ("notebook", "notebook", "pastime"), ("laptop", "laptop", "pastime"), ("headphones", "phones", "pastime"),
    ("umbrella", "umbrella", "play"),
    ("ball", "ball", "play"), ("bubbles", "bubbles", "play"), ("yoyo", "yoyo", "play"), ("blocks", "blocks", "play"),
    ("balloon", "balloon", "play"),
    ("plant", "plant", "pastime"), ("sweep", "sweep", "pastime"), ("snowman", "snowman", "play"),
    ("fishing", "fishing", "pastime"),
];

/// Storyboard rows 88–98 (prototype `#sb-rare`): (behavior, months it may play in, empty = all year).
const A6_RARE: [(&str, &[u8]); 11] = [
    ("windmill", &[]), ("hiccups", &[]), ("find_coin", &[]), ("snowflake_nose", &[12, 1, 2]),
    ("butterfly_ear", &[4, 5, 6, 7, 8, 9]), ("peekaboo", &[]), ("moonwalk", &[]),
    ("hat_spring", &[3, 4, 5]), ("hat_summer", &[6, 7, 8]), ("hat_autumn", &[9, 10, 11]), ("hat_winter", &[12, 1, 2]),
];

/// p5 (props-wire, AC19, AC18): every prop activity and rare surprise is rendered, in the idle pool
/// (props `long` 20–300 s, rare ones weight ≤ 0.3 and cooldown ≥ 1800 s), and listed for the agent.
#[test]
fn every_a6_prop_and_rare_surprise_is_rendered_in_the_pool_and_listed() {
    let p = bear();
    let (acts, emos, prompt) = (body::activities(&p), body::emotes(&p), body::prompt_text(&p).unwrap());
    let range = p.variety.as_ref().and_then(|v| v.targets.as_ref()).unwrap().long_len_s;
    let mut bad = vec![];
    for (name, set, group) in A6_PROPS {
        let b = p.behavior(name).unwrap_or_else(|| panic!("{name}: no behavior"));
        if !p.available(name) || b.set.as_deref() != Some(set) {
            bad.push(format!("{name}: available {} set {:?} (want {set})", p.available(name), b.set));
        }
        let entries: Vec<_> = p.idle_pool.iter().filter(|e| e.name() == name).collect();
        if entries.is_empty() {
            bad.push(format!("{name}: not in the idle pool"));
        }
        for e in &entries {
            if e.kind != PoolKind::Long || e.weight <= 0.0 || !(600.0..=1800.0).contains(&e.cooldown_s) {
                bad.push(format!("{name}: kind {:?} weight {} cooldown {}", e.kind, e.weight, e.cooldown_s));
            }
        }
        // the loop step's for_s keeps a visit inside targets.long_len_s with its enter and exit
        let held = b.seq.iter().find_map(|st| if let Loops::For { for_s } = st.loops { Some(for_s) } else { None });
        if !held.is_some_and(|[lo, hi]| lo >= range[0] && hi <= range[1] - 30.0) {
            bad.push(format!("{name}: loop for_s {held:?} outside {range:?}"));
        }
        match acts.iter().find(|a| a.name == name) {
            Some(a) if a.group == group && prompt.contains(&format!("- {name}: {}\n", a.about)) => {}
            other => bad.push(format!("{name}: activity {other:?}, not group {group} in list and prompt")),
        }
    }
    assert!(acts.iter().any(|a| a.name == "nightcap"), "nightcap is an activity");
    let cap = p.behavior("nightcap").unwrap().seq.iter().find_map(|st| if let Loops::For { for_s } = st.loops { Some(for_s) } else { None });
    assert!(cap.is_some_and(|[lo, hi]| lo >= range[0] && hi <= range[1] - 30.0), "nightcap for_s {cap:?}");
    assert!(p.warmup.exclude.iter().any(|n| n == "nightcap"), "no bed at launch");
    for (name, months) in A6_RARE {
        if !p.available(name) {
            bad.push(format!("{name}: not rendered ({:?})", p.missing_clip(name)));
        }
        match p.idle_pool.iter().find(|e| e.name() == name) {
            None => bad.push(format!("{name}: not in the idle pool")),
            Some(e) => {
                let want_kind = if name == "moonwalk" { PoolKind::Move } else { PoolKind::Rare };
                let got: &[u8] = e.when.as_ref().and_then(|w| w.months.as_deref()).unwrap_or(&[]);
                if e.kind != want_kind || !(e.weight > 0.0 && e.weight <= 0.3) || e.cooldown_s < 1800.0 || got != months {
                    bad.push(format!("{name}: kind {:?} weight {} cooldown {} months {got:?}", e.kind, e.weight, e.cooldown_s));
                }
            }
        }
        if !emos.iter().any(|e| e.name == name && e.group == "surprise") {
            bad.push(format!("{name}: not a surprise emote"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    assert!(prompt.contains("\nSurprises: windmill, "), "the prompt lists the surprises:\n{prompt}");
    assert_eq!(p.waiting().len(), 0, "{:?}", p.waiting().iter().map(|w| &w.name).collect::<Vec<_>>());
}

/// Non-happy (AC19): a month-gated entry never plays outside its months. Two July hours on three
/// seeds may show the summer hat, never another hat, the snowflake, the snowman or ice fishing;
/// and in winter the snowman shows up.
#[test]
fn hats_and_winter_props_keep_to_their_months() {
    let at = |month: u8, hour: u16| Senses { local: LocalTime { minutes: hour * 60, weekday: 2 }, month, ..Senses::default() };
    let july: Vec<String> = (1..=3).flat_map(|seed| run_at(seed, 120, None, at(7, 10)).0).map(|k| k.1).collect();
    for never in ["hat_spring", "hat_autumn", "hat_winter", "snowflake_nose", "snowman", "fishing", "tea"] {
        assert!(!july.iter().any(|n| n == never), "July 10:00: {never} played");
    }
    let winter: Vec<String> = (1..=3).flat_map(|seed| run_at(seed, 120, None, at(12, 15)).0).map(|k| k.1).collect();
    for want in ["snowman", "fishing", "tea"] {
        assert!(winter.iter().any(|n| n == want), "December 15:00: no {want}");
    }
    assert!(!winter.iter().any(|n| n == "hat_summer" || n == "butterfly_ear"), "December: a summer surprise played");
}
