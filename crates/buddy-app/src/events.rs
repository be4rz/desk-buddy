//! `BUDDY_EVENTS=<file.jsonl>`: one replay `Item` per line (`{"at_ms": 1500, "event":
//! {"type": "thinking"}}`, or `"poke": "click"`, or `"senses": {…}`), posted into the
//! inbox at `at_ms` after launch. The "real events" path before the harness (p3) and
//! the QA driver. Blank lines and lines starting with `//` are skipped.

use std::path::Path;
use std::time::{Duration, Instant};

use mascot_core::replay::Item;

use crate::driver::{post, Inbox};

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
        if it.event.is_none() && it.poke.is_none() && it.senses.is_none() {
            return Err(format!("{}: an item needs event, poke or senses", i + 1));
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
                // the order of mascot_core::replay::run: senses, event, poke
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

