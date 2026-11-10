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
}
