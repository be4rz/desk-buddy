//! Shared helpers. Fixtures use invented people and places only.
#![allow(dead_code)]

use jiff::civil::Date;
use memory::{Applied, Now, Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

/// A scratch directory that removes itself.
pub struct Tmp(pub PathBuf);

impl Tmp {
    pub fn new() -> Tmp {
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("memory-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }

    /// The store's folder, like `BUDDY_HOME/memory`.
    pub fn mem(&self) -> PathBuf {
        self.0.join("memory")
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn date(y: i16, m: i8, d: i8) -> Date {
    Date::new(y, m, d).unwrap()
}

/// 2026-11-27, the day the design was written.
pub fn now() -> Now {
    now_on(date(2026, 11, 27))
}

pub fn now_on(today: Date) -> Now {
    Now::new(today, format!("{today}T09:12:03+07:00"))
}

pub fn store() -> (Tmp, Store) {
    let tmp = Tmp::new();
    let s = Store::open(&tmp.mem()).unwrap();
    (tmp, s)
}

/// A direct read connection, to look at columns the API doesn't show.
pub fn db(dir: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.join("memory.sqlite")).unwrap()
}

/// `SELECT status, valid_to, superseded_by, authority FROM facts WHERE id = ?`
pub fn row(dir: &Path, id: i64) -> (String, Option<String>, Option<i64>, i64) {
    db(dir)
        .query_row("SELECT status, valid_to, superseded_by, authority FROM facts WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
}

pub fn count(dir: &Path, sql: &str) -> i64 {
    db(dir).query_row(sql, [], |r| r.get(0)).unwrap()
}

/// A memory.extract item.
pub fn item(kind: &str, predicate: Option<&str>, value: Option<&str>, text: &str) -> Value {
    json!({"turn": "u:t1", "kind": kind, "predicate": predicate, "qualifier": null, "value": value,
           "text": text, "importance": 6, "validFrom": null, "quote": null})
}

pub fn with(mut v: Value, key: &str, val: Value) -> Value {
    v[key] = val;
    v
}

/// Runs one extraction of turn `u:t1` where the person said `said`.
pub fn extract(s: &Store, said: &str, items: Vec<Value>, now: &Now) -> Applied {
    s.add_pending("u:t1", &now.at).unwrap();
    let args = s.extract_args(10, now.today).unwrap();
    let reply = json!({"items": items, "turns": [{"requestId": "u:t1", "said": said}], "missing": []});
    s.apply_extract(&args, &reply, now).unwrap()
}

/// Stores `home_city = value` at the given authority (1 guess, 2 bear, 3 chat, 4 window).
pub fn city(s: &Store, value: &str, authority: u8, now: &Now) -> i64 {
    let text = format!("Lives in {value}");
    let id = match authority {
        1 => extract(s, "x", vec![item("profile", Some("home_city"), Some(value), &text)], now).items[0].outcome.id(),
        3 => extract(
            s,
            &format!("I live in {value}"),
            vec![with(item("profile", Some("home_city"), Some(value), &text), "quote", json!(format!("I live in {value}")))],
            now,
        )
        .items[0]
            .outcome
            .id(),
        _ => {
            let op = memory::RememberIn { text, key: Some("home_city".into()), value: Some(value.into()), ..Default::default() };
            s.remember(op, now).unwrap().id()
        }
    }
    .unwrap();
    if authority == 4 {
        s.edit(id, &format!("Lives in {value}"), None, now).unwrap()
    } else {
        id
    }
}

/// A live episode observed on `day`, stored through the extractor as in real use.
pub fn episode(s: &Store, day: Date, text: &str, importance: i64) -> i64 {
    let it = with(item("episode", None, None, text), "importance", json!(importance));
    extract(s, "x", vec![it], &now_on(day)).items[0].outcome.id().unwrap()
}

/// `meta.<key>` read straight from the file.
pub fn meta(dir: &Path, key: &str) -> Option<String> {
    db(dir).query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok()
}

/// A `Now` on `day` at `hh:mm:ss` local (+07:00).
pub fn now_at(day: Date, hms: &str) -> Now {
    Now::new(day, format!("{day}T{hms}+07:00"))
}

pub mod ollama;

/// Made-up embeddings for the tests (no model): one axis per topic a text names, plus a small axis of its own,
/// normalised. Two texts on the same topic come out close (cosine ≈ 0.9) whatever their words; unrelated ones ≈ 0.
pub fn concept_vec(text: &str) -> Vec<f32> {
    const TOPICS: &[&[&str]] = &[
        &["speak", "nervous", "calm", "talk", "stress", "present", "anxious", "intro"],
        &["cook", "noodle", "lunch", "pasta", "dinner", "recipe", "food"],
        &["bug", "login", "code", "deploy"],
        &["lake", "walk", "hike"],
        &["cat", "mochi", "vet"],
    ];
    let t = text.to_lowercase();
    let mut v = vec![0.0f32; memory::DIMS];
    for (i, words) in TOPICS.iter().enumerate() {
        if words.iter().any(|w| t.contains(w)) {
            v[i] = 1.0;
        }
    }
    let own = 100 + (t.bytes().fold(7u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))) % 600) as usize;
    v[own] += 0.35;
    memory::embed::normalise(&mut v);
    v
}
