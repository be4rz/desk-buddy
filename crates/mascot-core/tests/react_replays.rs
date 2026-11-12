//! The react-brain replays (aliveness p4, design `#ex-react`): each fixture in
//! `crates/buddy-app/tests/fixtures/` (JSONL replay Items, `//` comments) run on
//! `tests/fixtures/react_pack` through `replay::run_with`, and its log checked against the table.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mascot_core::brain::{Brain, Intent};
use mascot_core::pack::{self, PackMeta};
use mascot_core::replay::{self, Item, LogLine, Timeline, TIMELINE_VERSION};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn react_pack() -> Arc<PackMeta> {
    Arc::new(pack::load(&repo().join("crates/mascot-core/tests/fixtures/react_pack")).unwrap_or_else(|e| panic!("{e}")))
}

/// A fixture as a timeline: `//` and blank lines skipped; `start_local` from the first item's
/// `senses.local` when it is at 0 (the morning reflex reads the first sample).
fn load(name: &str, seed: u64, duration_ms: u64) -> Timeline {
    let path = repo().join("crates/buddy-app/tests/fixtures").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let items: Vec<Item> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: {e}: {l}")))
        .collect();
    let first = items.first().filter(|i| i.at_ms == 0).and_then(|i| i.senses.as_ref()).and_then(|s| s.local.clone());
    let (minutes, weekday) = first.map_or((14 * 60, 2), |l| (l.minutes, l.weekday));
    let tl = Timeline {
        timeline_version: TIMELINE_VERSION,
        seed,
        duration_ms,
        start_local: format!("{:02}:{:02}", minutes / 60, minutes % 60),
        weekday,
        items,
    };
    // not through `replay::parse` (a serialized round trip): a patch's cursor never serializes
    assert!(tl.items.windows(2).all(|w| w[0].at_ms <= w[1].at_ms), "{name}: items sorted by at_ms");
    assert!(tl.items.iter().all(|i| i.at_ms <= duration_ms), "{name}: an item past {duration_ms} ms");
    tl
}

fn run(name: &str, seed: u64, duration_ms: u64) -> Vec<LogLine> {
    replay::run(react_pack(), &load(name, seed, duration_ms))
}

/// (start ms, behavior) for each behavior change.
fn starts(log: &[LogLine]) -> Vec<(u64, String)> {
    let mut v: Vec<(u64, String)> = Vec::new();
    for l in log {
        if v.last().map(|x| &x.1) != Some(&l.behavior) {
            v.push((l.t_ms, l.behavior.clone()));
        }
    }
    v
}

const MUSIC: [&str; 3] = ["head_bob", "foot_tap", "dance"];

#[test]
fn music_plays_music_entries_only_while_it_plays() {
    let mut any = false;
    for seed in [1, 2, 3] {
        let log = run("music.jsonl", seed, 600_000);
        let s = starts(&log);
        any |= s.iter().any(|(t, b)| (5_000..240_000).contains(t) && MUSIC.contains(&b.as_str()));
        let after: Vec<_> = s.iter().filter(|(t, b)| *t >= 240_000 && MUSIC.contains(&b.as_str())).collect();
        // one started before the stop may still be finishing at 240 s
        assert!(after.iter().all(|(t, _)| *t < 240_000 + 100), "seed {seed}: {after:?}");
        assert!(!s.iter().any(|(t, b)| *t < 5_000 && MUSIC.contains(&b.as_str())), "seed {seed}: {s:?}");
    }
    assert!(any, "a music entry starts while it plays on one of seeds 1,2,3");
}

#[test]
fn edge_react_sits_plays_only_edge_entries_and_stands_on_leaving() {
    let tl = load("edge_react.jsonl", 15, 303_000);
    let log = replay::run_with(react_pack(), &tl, |b: &mut Brain| b.set_edges(Some([140, 150])));
    let s = starts(&log);
    let on: Vec<_> = s.iter().filter(|(t, _)| (3_200..300_000).contains(t)).collect();
    assert_eq!(on.first().map(|x| x.1.as_str()), Some("edge_sit"), "{s:?}");
    assert!(on.iter().all(|(_, b)| b.starts_with("edge_")), "{on:?}");
    assert!(s.iter().any(|(t, b)| *t >= 300_000 && b == "edge_stand"), "{s:?}");
}

#[test]
fn drag_dangles_lands_and_gets_dizzy_only_after_the_long_drag() {
    let log = run("drag.jsonl", 4, 20_000);
    let s = starts(&log);
    let first: Vec<_> = s.iter().filter(|(t, _)| (3_000..15_000).contains(t)).map(|(_, b)| b.as_str()).collect();
    let i = first.iter().position(|b| *b == "dangle").unwrap_or_else(|| panic!("{first:?}"));
    assert_eq!(&first[i..i + 3], ["dangle", "land_squash", "dizzy"], "{first:?}");
    let second: Vec<_> = s.iter().filter(|(t, _)| *t >= 15_000).map(|(_, b)| b.as_str()).collect();
    let j = second.iter().position(|b| *b == "dangle").unwrap_or_else(|| panic!("{second:?}"));
    assert_eq!(second.get(j + 1), Some(&"land_squash"), "{second:?}");
    assert!(!second.contains(&"dizzy"), "a 0.2 s drag: no dizzy {second:?}");
}

#[test]
fn cursor_follows_flinches_giggles_and_each_watch_ends() {
    let log = run("cursor.jsonl", 5, 95_000);
    let s = starts(&log);
    let between = |a: u64, b: u64| -> Vec<&str> { s.iter().filter(|(t, _)| (a..b).contains(t)).map(|(_, x)| x.as_str()).collect() };
    assert!(between(3_000, 10_000).contains(&"cursor_watch"), "follow: {s:?}");
    assert!(!between(12_000, 20_000).contains(&"cursor_watch"), "None ends the watch: {s:?}");
    // each through the running behavior's exit (the desk stands up first)
    assert!(between(20_000, 25_000).contains(&"flinch"), "flinch: {s:?}");
    assert!(between(30_000, 35_000).contains(&"giggle"), "pet: {s:?}");
    assert!(between(75_000, 85_000).contains(&"cursor_watch"), "follow again after the cooldown: {s:?}");
    let west = log.iter().any(|l| (75_000..85_000).contains(&l.t_ms) && l.behavior == "cursor_watch" && format!("{:?}", l.dir) == "W");
    assert!(west, "faces the cursor (west)");
    assert!(!between(87_000, 95_000).contains(&"cursor_watch"), "the watch ends: {s:?}");
}

#[test]
fn time_has_the_morning_the_evening_yawn_and_quiet_hours() {
    let log = run("time.jsonl", 6, 1_500_000);
    let s = starts(&log);
    let morning: Vec<_> = s.iter().filter(|(t, _)| *t < 60_000).collect();
    let i = morning.iter().position(|(_, b)| b == "morning_stretch").unwrap_or_else(|| panic!("{s:?}"));
    let stretch_t = morning[i].0;
    assert!(log.iter().any(|l| l.t_ms > stretch_t && l.t_ms < 60_000 && l.intent == Intent::Greet), "greet after the stretch: {s:?}");
    assert!(s.iter().any(|(t, b)| (60_000..1_260_000).contains(t) && b == "evening_yawn"), "evening: {s:?}");
    assert!(!s.iter().any(|(t, b)| *t < 60_000 && b == "evening_yawn"), "{s:?}");
    // quiet: no reaction or context entry (nightcap waits for its art in the bear)
    let quiet: Vec<_> = s.iter().filter(|(t, _)| *t >= 1_265_000).map(|(_, b)| b.as_str()).collect();
    for b in ["evening_yawn", "head_bob", "dance", "giggle", "tap_glass", "cursor_watch"] {
        assert!(!quiet.contains(&b), "quiet hours: {quiet:?}");
    }
}

#[test]
fn away_glances_then_naps_on_the_floor_then_greets() {
    let log = run("away.jsonl", 7, 960_000);
    let s = starts(&log);
    assert!(
        s.iter().any(|(t, b)| (5_000..300_000).contains(t) && (b == "tap_glass" || b == "wave_small")),
        "away: {s:?}"
    );
    let long: Vec<_> = s.iter().filter(|(t, _)| (300_000..900_000).contains(t)).map(|(_, b)| b.as_str()).collect();
    assert!(long.contains(&"floor_nap"), "away_long: {long:?}");
    // held: nothing but the nap's own clips once it is down
    let nap = long.iter().position(|b| *b == "floor_nap").unwrap();
    assert!(long[nap..].iter().all(|b| *b == "floor_nap"), "held: {long:?}");
    let back: Vec<_> = log.iter().filter(|l| l.t_ms >= 900_000).collect();
    let up = back.iter().position(|l| l.clip.starts_with("curl_up")).unwrap_or_else(|| panic!("{:?}", starts(&log)));
    assert!(back[up..].iter().any(|l| l.intent == Intent::Greet), "greet after curl_up");
}

#[test]
fn approval_wait_fidgets_standing_and_only_asks_at_the_desk() {
    let log = run("approval_wait.jsonl", 8, 110_000);
    let s = starts(&log);
    let standing: Vec<_> = s.iter().filter(|(t, b)| (3_000..43_000).contains(t) && (b == "foot_tap" || b == "check_watch")).collect();
    assert!(standing.len() >= 3, "{s:?}");
    assert!(standing[0].0 >= 11_000, "after 8 s: {standing:?}");
    for w in standing.windows(2) {
        assert_ne!(w[0].1, w[1].1, "no repeat: {standing:?}");
    }
    // the seated card: the desk's own desk_ask clip, no standing fidget
    let seated: Vec<&LogLine> = log.iter().filter(|l| (70_000..100_000).contains(&l.t_ms)).collect();
    assert!(seated.iter().any(|l| l.clip == "desk_ask"), "{:?}", seated.iter().map(|l| &l.clip).collect::<Vec<_>>());
    assert!(seated.iter().all(|l| l.behavior != "foot_tap" && l.behavior != "check_watch"), "{s:?}");
}
