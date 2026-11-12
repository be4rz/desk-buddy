//! `BUDDY_EVENTS=<file.jsonl>`: one replay `Item` per line (`{"at_ms": 1500, "event":
//! {"type": "thinking"}}`, or `"poke": "click"`, or `"senses": {…}`, or `"face": "NE"`), posted into the
//! inbox at `at_ms` after launch. The "real events" path before the harness (p3) and
//! the QA driver. Blank lines and lines starting with `//` are skipped.

use std::path::Path;
use std::time::{Duration, Instant};

use mascot_core::replay::Item;

use crate::driver::{post, App, Inbox};

/// Parse the whole file up front, so a bad file fails at launch with its line number.
pub fn load(path: &Path) -> Result<Vec<Item>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}:{e}", path.display()))
}

pub fn parse(text: &str) -> Result<Vec<Item>, String> {
    let mut items = vec![];
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let it: Item = serde_json::from_str(line).map_err(|e| format!("{}: {e}", i + 1))?;
        if it.event.is_none() && it.poke.is_none() && it.senses.is_none() && it.face.is_none() {
            return Err(format!("{}: an item needs event, poke, senses or face", i + 1));
        }
        items.push(it);
    }
    items.sort_by_key(|it| it.at_ms);
    Ok(items)
}

/// Post each item at its time, from a background thread.
pub fn replay(items: Vec<Item>, t0: Instant) {
    std::thread::Builder::new()
        .name("buddy-events".into())
        .spawn(move || {
            for it in items {
                let at = t0 + Duration::from_millis(it.at_ms);
                if let Some(d) = at.checked_duration_since(Instant::now()) {
                    std::thread::sleep(d);
                }
                if let Some(s) = it.senses {
                    post(Inbox::Senses(s));
                }
                // the order of mascot_core::replay::run: senses, face, event, poke
                if let Some(d) = it.face {
                    post(Inbox::Call(Box::new(move |a: &mut App| {
                        let now = a.now_ms();
                        a.brain.face(d, now);
                        a.wake();
                    })));
                }
                if let Some(ev) = it.event {
                    post(Inbox::Event(ev));
                }
                if let Some(p) = it.poke {
                    post(Inbox::Poke(p));
                }
            }
        })
        .expect("spawn events thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mascot_protocol::MascotEvent;

    #[test]
    fn the_fixture_parses_in_time_order() {
        let items = load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/events.jsonl")).unwrap();
        assert!(items.len() >= 4);
        assert!(items.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
        assert!(items.iter().any(|i| i.event == Some(MascotEvent::Thinking)));
    }

    #[test]
    fn a_bad_line_names_its_number() {
        let e = parse("{\"at_ms\": 1, \"event\": {\"type\": \"idle\"}}\n\n{\"at_ms\": 2, \"event\": {\"type\": \"nope\"}}").unwrap_err();
        assert!(e.starts_with("3: "), "{e}");
        let e = parse("{\"at_ms\": 5}").unwrap_err();
        assert!(e.contains("needs event"), "{e}");
    }

    /// AC6: `listen.jsonl` (a click from each of the 8 dirs) through the brain's replay on the
    /// listen fixture pack: the `#ex-listen` path, then listen_in → listen → listen_out, idle.
    #[test]
    fn the_listen_fixture_listens_from_every_dir() {
        use mascot_core::{pack, replay, Dir};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let items = load(&dir.join("tests/fixtures/listen.jsonl")).unwrap();
        let faces: Vec<Dir> = items.iter().filter_map(|i| i.face).collect();
        assert_eq!(faces, Dir::ALL);
        let mut p = pack::load(&dir.join("../mascot-core/tests/fixtures/listen_pack")).unwrap();
        p.idle_pool.retain(|e| e.name() == "rest");
        let p = std::sync::Arc::new(p);
        let tl = replay::Timeline { timeline_version: replay::TIMELINE_VERSION, seed: 7, duration_ms: 80_000, start_local: "10:00".into(), weekday: 2, items: items.clone() };
        let log = replay::run(p.clone(), &tl);
        for (k, from) in Dir::ALL.into_iter().enumerate() {
            let (t0, t1) = (k as u64 * 10_000, (k as u64 + 1) * 10_000);
            let seg: Vec<&replay::LogLine> = log.iter().filter(|l| l.t_ms >= t0 + 100 && l.t_ms < t1).collect();
            // one entry per (clip, dir) run: two turn steps of the same clip stay two
            let clips: Vec<&str> = seg.iter().map(|l| (l.clip.as_str(), l.dir)).fold(Vec::new(), |mut v: Vec<(&str, Dir)>, c| {
                if v.last() != Some(&c) {
                    v.push(c);
                }
                v
            }).into_iter().map(|c| c.0).collect();
            let turns = match from {
                Dir::NE | Dir::NW => 1,
                Dir::N => 2,
                _ => 0,
            };
            assert!(clips[..turns].iter().all(|c| c.starts_with("turn_")), "{from:?}: {clips:?}");
            assert_eq!(&clips[turns..turns + 4], ["listen_in", "listen", "listen_out", "idle"], "{from:?}: {clips:?}");
            let listen = seg.iter().find(|l| l.clip == "listen").unwrap();
            assert_eq!(Some(listen.dir), from.listen_target().or(Some(listen.dir)), "{from:?}");
        }
        // the same file on a pack without turn clips: it snaps, and still listens from every dir
        let mut np = (*p).clone();
        np.turning = None;
        let log = replay::run(std::sync::Arc::new(np), &tl);
        assert!(log.iter().all(|l| !l.clip.starts_with("turn_")));
        assert_eq!(log.iter().filter(|l| l.clip == "listen_in").count(), 8);
    }

    /// AC1: the `#ex-read` fixture through the brain's replay: a `do read` asked mid-turn waits for the
    /// reply, sits down after it, holds the read loop for its 5m and stands up at the deadline.
    #[test]
    fn the_activity_fixture_reads_after_the_reply_for_five_minutes() {
        use mascot_core::{pack, replay, Intent};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let items = load(&dir.join("tests/fixtures/activity.jsonl")).unwrap();
        let p = std::sync::Arc::new(pack::load(&dir.join("../../mascots/bear")).unwrap());
        let tl = replay::Timeline { timeline_version: replay::TIMELINE_VERSION, seed: 7, duration_ms: 320_000, start_local: "10:00".into(), weekday: 2, items };
        let log = replay::run(p, &tl);
        let at = |clip: &str| log.iter().find(|l| l.behavior == "read" && l.clip == clip).map(|l| (l.t_ms, l.intent));
        let (sit, intent) = at("sit_down").expect("the bear sits down to read");
        assert_eq!(intent, Intent::Do);
        assert!((7000..9000).contains(&sit), "after the reply (idle at 7000), not before: {sit}");
        assert!(log.iter().all(|l| l.t_ms >= 7000 || l.intent != Intent::Do), "nothing of read during the turn");
        let (read, _) = at("read").expect("then the read loop");
        assert!(read > sit);
        // held: nothing changes between the read loop and the deadline
        assert!(log.iter().filter(|l| l.t_ms > read && l.t_ms < 307_000).all(|l| l.behavior == "read" && l.clip == "read"), "{log:#?}");
        assert_eq!(at("stand_up"), Some((307_000, Intent::Do)), "deadline 7000 + 300000");
        let after = log.iter().find(|l| l.t_ms > 307_000 && l.behavior != "read").expect("then idle again");
        assert_eq!(after.intent, Intent::Idle);
    }

    fn desk_replay(file: &str, pack_dir: &str, seed: u64, until: u64) -> Vec<mascot_core::replay::LogLine> {
        use mascot_core::{pack, replay};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let items = load(&dir.join("tests/fixtures").join(file)).unwrap();
        let p = std::sync::Arc::new(pack::load(&dir.join(pack_dir)).unwrap_or_else(|e| panic!("{e}")));
        let tl = replay::Timeline { timeline_version: replay::TIMELINE_VERSION, seed, duration_ms: until, start_local: "10:00".into(), weekday: 2, items };
        replay::run(p, &tl)
    }

    const DESK_PACK: &str = "../mascot-core/tests/fixtures/desk_pack";
    const BEAR: &str = "../../mascots/bear";

    fn clips<'a>(log: impl IntoIterator<Item = &'a mascot_core::replay::LogLine>) -> Vec<&'a str> {
        let mut v: Vec<&str> = Vec::new();
        for l in log {
            if v.last() != Some(&l.clip.as_str()) {
                v.push(&l.clip);
            }
        }
        v
    }

    /// AC8–AC10: `desk_long` on the desk fixture: seated from desk_sit to desk_stand in one ori, the card
    /// and the answer from the chair (mouth on desk_talk), up only after the answer bubble's Settled at 20 s.
    #[test]
    fn the_desk_long_fixture_sits_answers_from_the_chair_and_stands_on_settled() {
        use mascot_core::{Intent, MouthState};
        for seed in [7, 8] {
            let log = desk_replay("desk_long.jsonl", DESK_PACK, seed, 26_000);
            let c = clips(&log);
            let want = ["desk_sit", "desk_type", "desk_swivel_in", "desk_ask", "desk_swivel_out", "desk_type", "desk_swivel_in", "desk_talk", "desk_stand"];
            let mut it = c.iter();
            assert!(want.iter().all(|w| it.any(|x| x == w)), "seed {seed}: {c:?}");
            let i0 = log.iter().position(|l| l.clip == "desk_sit").unwrap();
            let i1 = log.iter().rposition(|l| l.clip == "desk_stand").unwrap();
            assert!((4000..6000).contains(&log[i0].t_ms), "seed {seed}: sat at {}", log[i0].t_ms);
            let ori = log[i0].ori;
            assert!(ori.is_some() && log[i0..=i1].iter().all(|l| l.ori == ori && l.behavior == "desk" && l.clip != "think"), "seed {seed}: {c:?}");
            assert!(log.iter().any(|l| l.clip == "desk_talk" && l.mouth != MouthState::Closed), "seed {seed}: no mouth");
            assert!(log[i1].t_ms >= 20_000, "seed {seed}: stood before Settled");
            assert_eq!(log.last().map(|l| (l.ori, l.intent)), Some((None, Intent::Idle)));
        }
    }

    /// AC8: `desk_short` never shows set desk; the long job on the bear stays standing until its anim_d exists.
    #[test]
    fn the_desk_short_fixture_and_a_pack_without_desk_art_stay_standing() {
        let log = desk_replay("desk_short.jsonl", DESK_PACK, 7, 10_000);
        assert!(log.iter().all(|l| l.behavior != "desk" && l.ori.is_none()), "{:?}", clips(&log));
        assert!(clips(&log).contains(&"talk"));
        let bear = mascot_core::pack::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join(BEAR)).unwrap();
        let log = desk_replay("desk_long.jsonl", BEAR, 7, 26_000);
        let c = clips(&log);
        if bear.available("desk") {
            assert!(c.contains(&"desk_sit") && c.contains(&"desk_stand"), "{c:?}");
        } else {
            assert!(log.iter().all(|l| l.behavior != "desk"), "{c:?}");
            assert!(c.contains(&"think") && c.contains(&"ask") && c.contains(&"talk"), "{c:?}");
        }
    }

    /// AC5: `desk_click`: the click while typing listens from the chair; closed, it types on; up after Settled.
    #[test]
    fn the_desk_click_fixture_listens_from_the_chair() {
        use mascot_core::Intent;
        let log = desk_replay("desk_click.jsonl", DESK_PACK, 7, 24_000);
        let c = clips(&log);
        let during: Vec<_> = log.iter().filter(|l| (7000..11_000).contains(&l.t_ms)).collect();
        assert!(during.iter().all(|l| l.behavior == "desk" && l.ori.is_some()), "{c:?}");
        assert!(during.iter().any(|l| l.intent == Intent::Listen && l.clip == "desk_talk"), "{c:?}");
        let after = clips(log.iter().filter(|l| (11_000..16_000).contains(&l.t_ms)));
        assert!(after.contains(&"desk_swivel_out") && after.contains(&"desk_type"), "{after:?}");
        let stand = log.iter().find(|l| l.clip == "desk_stand").expect("stands").t_ms;
        assert!(stand >= 19_000, "after Settled: {stand}");
    }

    /// AC18: `idle_long` on the bear, seeds 7 and 8: every read, nap and idle desk visit holds 20–300 s
    /// (a `for_s` / `hold_s` draw, whole loops) and the desk visit sits, types and stands as idle.
    #[test]
    fn the_idle_long_fixture_holds_read_nap_and_desk_twenty_to_three_hundred_seconds() {
        use mascot_core::Intent;
        let mut seen = std::collections::BTreeSet::new();
        for seed in [7, 8] {
            let log = desk_replay("idle_long.jsonl", BEAR, seed, 3_600_000);
            let mut k = 0;
            while k < log.len() {
                let b = log[k].behavior.clone();
                let j = log[k..].iter().position(|l| l.behavior != b).map_or(log.len(), |n| k + n);
                if ["read", "nap", "desk"].contains(&b.as_str()) {
                    let end = log.get(j).map_or(3_600_000, |l| l.t_ms);
                    let s = (end - log[k].t_ms) as f64 / 1000.0;
                    if end < 3_600_000 {
                        assert!((20.0..=300.0).contains(&s), "seed {seed}: {b} held {s} s at {}", log[k].t_ms);
                    }
                    assert!(log[k..j].iter().all(|l| l.intent == Intent::Idle), "seed {seed}: {b} is idle");
                    if b == "desk" {
                        let c: Vec<&str> = clips(&log[k..j]).into_iter().filter(|c| !c.starts_with("turn_")).collect();
                        assert_eq!(c.first(), Some(&"desk_sit"), "seed {seed}: {c:?}");
                        // a spell the hour cuts off is still seated
                        if end < 3_600_000 {
                            assert_eq!(c.last(), Some(&"desk_stand"), "seed {seed}: {c:?}");
                        }
                    }
                    seen.insert(b);
                }
                k = j;
            }
        }
        assert_eq!(seen.len(), 3, "read, nap and desk each come up in two idle hours: {seen:?}");
    }

    /// §1.5 turn-first: `idle_front` on the variety fixture: front-only and back-only fidgets play only in
    /// dirs their clip has, the facing changes only through a turn clip or a walk, and the click still listens.
    #[test]
    fn the_idle_front_fixture_turns_before_one_sided_fidgets() {
        const VARIETY: &str = "../mascot-core/tests/fixtures/variety_pack";
        let p = mascot_core::pack::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join(VARIETY)).unwrap();
        for seed in [7, 8] {
            let log = desk_replay("idle_front.jsonl", VARIETY, seed, 600_000);
            let mut played = 0;
            for w in log.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                if b.clip.starts_with("fid_") {
                    assert!(p.clips[&b.clip].dirs.contains_key(&b.dir), "seed {seed}: {} faces {:?} at {}", b.clip, b.dir, b.t_ms);
                    played += 1;
                }
                let moving = ["turn_ccw", "turn_cw", "walk", "run", "listen_in"].iter().any(|m| b.clip.starts_with(m) || a.clip.starts_with(m));
                assert!(a.dir == b.dir || moving, "seed {seed}: {} {:?} -> {} {:?} at {}", a.clip, a.dir, b.clip, b.dir, b.t_ms);
            }
            assert!(played > 0, "seed {seed}: fidgets play");
            assert!(log.iter().any(|l| l.clip == "fid_back") && log.iter().any(|l| l.clip == "fid_front"), "seed {seed}: both sides");
            assert!(log.iter().any(|l| l.t_ms >= 300_000 && l.intent == mascot_core::Intent::Listen), "seed {seed}: the click listens (the fixture's listen intent plays rest): {:?}", clips(log.iter().filter(|l| (299_000..306_000).contains(&l.t_ms))));
        }
    }
}
