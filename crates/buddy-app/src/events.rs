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
        p.idle_pool.retain(|e| e.behavior == "rest");
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
}

