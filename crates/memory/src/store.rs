//! The store: one SQLite connection behind a mutex, and every operation the app and host call.

use crate::files::DB;
use crate::reconcile::{delete_disputes, insert, reconcile};
use crate::schema::{self, Row, BEST_FIRST, VERSION};
use crate::secret::looks_secret;
use crate::validate::{self, clean, fold, quote_ok, Item, Raw, TEXT_MAX, VALUE_MAX};
use crate::{predicates, render, view, Applied, Error, Fact, Now, Outcome, RememberIn, Result};
use jiff::civil::Date;
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// A run applies at most this many items.
const MAX_ITEMS: usize = 20;
/// A pending turn the extractor keeps failing on is dropped after this many tries or days (C5).
const MAX_TRIES: i64 = 3;
const MAX_PENDING_DAYS: i32 = 7;
const SEARCH_MAX: u32 = 20;

/// The memory store. `Send + Sync`: share it as `Arc<Store>`.
pub struct Store {
    conn: Mutex<Connection>,
    dir: PathBuf,
    newer: bool,
    /// The core-section revision and the text it was last seen as.
    rev: Mutex<(u64, String)>,
}

impl Store {
    /// Creates `dir`, opens `memory.sqlite` in WAL mode and migrates it to this crate's user_version.
    /// A file with a newer user_version opens **read-only** (see [`Store::is_newer`]); a file that
    /// isn't a database fails with `Error::Store`.
    pub fn open(dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(dir)?;
        let mut conn = Connection::open(dir.join(DB))?;
        conn.busy_timeout(Duration::from_secs(2))?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // An erased row's bytes are zeroed in the file, not left in a free page.
        conn.pragma_update(None, "secure_delete", true)?;
        let newer = schema::user_version(&conn)? > VERSION;
        if newer {
            conn.pragma_update(None, "query_only", true)?;
        } else {
            schema::migrate(&mut conn)?;
        }
        let first = if newer { String::new() } else { render::section(&conn)? };
        Ok(Store { conn: Mutex::new(conn), dir: dir.to_path_buf(), newer, rev: Mutex::new((0, first)) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// True when the file was written by a newer app: nothing is written, extracted or rendered.
    /// The app shows `error.code = "newer"`; Forget everything still recovers it.
    pub fn is_newer(&self) -> bool {
        self.newer
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs a write. Afterwards the core section is compared with the last one, so `core_rev` moves
    /// exactly when what the prompt carries changed (a profile, preference or promise row, or a dispute).
    fn write<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let out = self.write_quiet(f);
        if let Ok(now) = render::section(&self.lock()) {
            let mut rev = self.rev.lock().unwrap_or_else(|e| e.into_inner());
            if rev.1 != now {
                *rev = (rev.0 + 1, now);
            }
        }
        out
    }

    /// A write that can't change the section (the pending queue, meta).
    fn write_quiet<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        if self.newer {
            return Err(Error::Newer);
        }
        f(&mut self.lock())
    }

    pub(crate) fn vacuum(&self) -> Result<()> {
        Ok(self.lock().execute_batch("VACUUM")?)
    }

    // ---- extraction ----

    /// Remembers that chat turn `request_id` ended and waits for the extractor. Only `u:` ids count;
    /// sense turns and anything else are ignored.
    pub fn add_pending(&self, request_id: &str, ended_at: &str) -> Result<()> {
        if !request_id.starts_with("u:") {
            return Ok(());
        }
        self.write_quiet(|c| {
            c.execute("INSERT OR IGNORE INTO pending_turns (request_id, ended_at) VALUES (?1, ?2)", [request_id, ended_at])?;
            Ok(())
        })
    }

    /// Drops every waiting turn (memory switched off).
    pub fn clear_pending(&self) -> Result<()> {
        self.write_quiet(|c| {
            c.execute("DELETE FROM pending_turns", [])?;
            Ok(())
        })
    }

    /// The memory.extract request body (I1): up to `max` (≤ 10) oldest pending turns, the live keyed
    /// rows as `known`, the registry and `today`. None when nothing is pending or the store is read-only.
    pub fn extract_args(&self, max: usize, today: Date) -> Option<Value> {
        let max = max.min(10);
        if self.newer || max == 0 {
            return None;
        }
        let c = self.lock();
        let turns = c
            .prepare("SELECT request_id, ended_at FROM pending_turns ORDER BY ended_at, rowid LIMIT ?1")
            .and_then(|mut st| {
                st.query_map([max as i64], |r| Ok(json!({"requestId": r.get::<_, String>(0)?, "endedAt": r.get::<_, String>(1)?})))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .ok()?;
        if turns.is_empty() {
            return None;
        }
        let known: Vec<Value> = schema::query(&c, "WHERE status = 'live' AND predicate IS NOT NULL ORDER BY id", [])
            .ok()?
            .iter()
            .map(|r| json!({"id": r.id, "subject": r.subject, "predicate": r.predicate, "qualifier": r.qualifier, "value": r.value}))
            .collect();
        Some(json!({
            "turns": turns, "known": known, "registry": predicates::registry_json(),
            "today": today.to_string(), "episodes": false,
        }))
    }

    /// Validates and reconciles a memory.extract reply, one transaction per item, then clears the
    /// pending turns of `args` and the reply's `missing` ones. Quotes are checked against
    /// `reply.turns[].said` (authority 3 if the person really said it, else 1). Episodes are skipped (C6).
    pub fn apply_extract(&self, args: &Value, reply: &Value, now: &Now) -> Result<Applied> {
        let items = reply.get("items").and_then(Value::as_array).ok_or_else(|| Error::Usage("reply has no items".into()))?;
        let asked: Vec<&str> = strings(args.get("turns"), "requestId");
        let said: HashMap<&str, &str> = reply
            .get("turns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t.get("requestId")?.as_str()?, t.get("said")?.as_str()?)))
            .collect();
        let mut clear: Vec<&str> = asked.clone();
        clear.extend(reply.get("missing").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str));

        self.write(|c| {
            let mut out = Applied::default();
            for (n, v) in items.iter().enumerate() {
                let turn = v.get("turn").and_then(Value::as_str).filter(|t| asked.contains(t));
                let outcome = if n >= MAX_ITEMS {
                    Outcome::Skipped("limit")
                } else {
                    match item(v, turn, turn.and_then(|t| said.get(t).copied()), now.today) {
                        Ok(it) => in_tx(c, |tx| reconcile(tx, &it, now))?,
                        Err(why) => Outcome::Skipped(why),
                    }
                };
                out.push(turn.map(str::to_string), outcome);
            }
            for id in clear {
                out.cleared += c.execute("DELETE FROM pending_turns WHERE request_id = ?1", [id])?;
            }
            c.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('last_extract_at', ?1)", [&now.at])?;
            Ok(out)
        })
    }

    /// The extractor failed for `args`: each of its turns has `tries += 1`. A turn is dropped after 3
    /// tries or when it has waited 7 days (counted to `args.today`). Returns the dropped request ids
    /// so the caller can log `[buddy] memory drop <id>`.
    pub fn extract_failed(&self, args: &Value) -> Result<Vec<String>> {
        let today = args.get("today").and_then(Value::as_str).and_then(|d| d.parse::<Date>().ok());
        self.write_quiet(|c| {
            let mut dropped = Vec::new();
            for id in strings(args.get("turns"), "requestId") {
                c.execute("UPDATE pending_turns SET tries = tries + 1 WHERE request_id = ?1", [id])?;
                let row = c.query_row("SELECT tries, ended_at FROM pending_turns WHERE request_id = ?1", [id], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                });
                let Ok((tries, ended_at)) = row else { continue };
                let stale = today
                    .zip(ended_at.get(..10).and_then(|d| d.parse::<Date>().ok()))
                    .and_then(|(today, d)| d.until(today).ok())
                    .is_some_and(|span| span.get_days() >= MAX_PENDING_DAYS);
                if tries >= MAX_TRIES || stale {
                    c.execute("DELETE FROM pending_turns WHERE request_id = ?1", [id])?;
                    dropped.push(id.to_string());
                }
            }
            Ok(dropped)
        })
    }

    // ---- the bear's commands ----

    /// `buddy memory remember`: authority 2, then reconcile. A secret is `Error::Secret`; a bad shape
    /// is `Error::Usage`; an episode is `Outcome::Skipped("episode")`.
    pub fn remember(&self, op: RememberIn, now: &Now) -> Result<Outcome> {
        let usage = |m: &str| Error::Usage(m.to_string());
        let text = clean(&op.text);
        if text.is_empty() || text.chars().count() > TEXT_MAX {
            return Err(usage("text must be 1-200 characters"));
        }
        let (pred, qual) = op.key.as_deref().map(predicates::split_key).unzip();
        let pred = pred.filter(|p| !p.is_empty());
        let known = pred.and_then(predicates::get);
        let value = op.value.as_deref().map(clean).filter(|v| !v.is_empty());
        if known.is_some() && value.is_none() {
            return Err(usage("a key needs a value"));
        }
        let kind = op.kind.as_deref().or(known.map(|p| p.kind)).unwrap_or("profile");
        let raw = Raw {
            kind,
            predicate: pred,
            qualifier: qual.flatten(),
            value: value.as_deref(),
            text: &text,
            importance: op.importance.map_or(5, i64::from),
            valid_from: op.from.as_deref(),
        };
        let mut it = match validate::check(&raw, now.today) {
            Ok(it) => it,
            Err("episode") => return Ok(Outcome::Skipped("episode")),
            Err("secret") => return Err(Error::Secret),
            Err(why) => return Err(Error::Usage(format!("bad {why}"))),
        };
        it.authority = 2;
        self.write(|c| in_tx(c, |tx| reconcile(tx, &it, now)))
    }

    /// Soft forget (status `forgotten`) by id, or every live row of a key (`pred` or `pred:qual`,
    /// or `key` with `qualifier`). Returns the ids; none found is `Error::NotFound`.
    pub fn forget(&self, id: Option<i64>, key: Option<&str>, qualifier: Option<&str>) -> Result<Vec<i64>> {
        self.write(|c| {
            in_tx(c, |tx| {
                let ids: Vec<i64> = match (id, key) {
                    (Some(id), _) => schema::get(tx, id)?.filter(|r| r.status == "live" || r.status == "disputed").map(|r| vec![r.id]).unwrap_or_default(),
                    (None, Some(key)) => {
                        let (pred, q) = predicates::split_key(key);
                        let q = qualifier.map(fold).or_else(|| q.map(fold)).filter(|q| !q.is_empty());
                        schema::query(
                            tx,
                            "WHERE status IN ('live', 'disputed') AND subject = 'user' AND predicate = ?1 AND (?2 IS NULL OR qualifier = ?2)",
                            params![pred, q],
                        )?
                        .iter()
                        .map(|r| r.id)
                        .collect()
                    }
                    (None, None) => return Err(Error::Usage("an id or a key".into())),
                };
                if ids.is_empty() {
                    return Err(Error::NotFound);
                }
                for &id in &ids {
                    soft_forget_row(tx, id)?;
                }
                Ok(ids)
            })
        })
    }

    /// Live rows, best first (importance, then newest), optionally of one kind.
    pub fn list(&self, kind: Option<&str>) -> Result<Vec<Fact>> {
        if let Some(k) = kind {
            if !validate::KINDS.contains(&k) {
                return Err(Error::Usage("unknown kind".into()));
            }
        }
        let rows = schema::query(&self.lock(), &format!("WHERE status = 'live' AND (?1 IS NULL OR kind = ?1) {BEST_FIRST}"), [kind])?;
        Ok(rows.iter().map(Fact::from).collect())
    }

    /// Live rows whose text, value or key contain the words (case-insensitive, accents kept).
    /// Best first: most words matched, then importance, then newest. `limit` is 1..=20.
    pub fn search(&self, q: &str, limit: u32) -> Result<Vec<Fact>> {
        let words: Vec<String> = fold(q).split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
        if words.is_empty() {
            return Err(Error::Usage("an empty query".into()));
        }
        let rows = schema::query(&self.lock(), &format!("WHERE status = 'live' {BEST_FIRST}"), [])?;
        let mut hits: Vec<(usize, &Row)> = rows
            .iter()
            .map(|r| {
                let hay = fold(&format!("{} {} {}", r.text, r.value.as_deref().unwrap_or(""), r.key().unwrap_or_default()));
                (words.iter().filter(|w| hay.contains(w.as_str())).count(), r)
            })
            .filter(|(n, _)| *n > 0)
            .collect();
        hits.sort_by_key(|(n, _)| std::cmp::Reverse(*n)); // stable: keeps best-first inside a tie
        Ok(hits.into_iter().take(limit.clamp(1, SEARCH_MAX) as usize).map(|(_, r)| Fact::from(r)).collect())
    }

    // ---- reading for the prompt, the file and the window ----

    /// The prompt's "## What I know about you" section, "" when nothing is live (or the store is newer).
    pub fn section(&self) -> String {
        if self.newer {
            return String::new();
        }
        render::section(&self.lock()).unwrap_or_default()
    }

    /// Moves whenever the text of [`Store::section`] changes: a profile, preference or promise row
    /// changed (text, importance order, forgotten, superseded) or a dispute opened, closed or changed.
    /// The app calls `Agent::set_memory` when it moves. Counted in memory: it restarts at 0 on open.
    pub fn core_rev(&self) -> u64 {
        self.rev.lock().unwrap_or_else(|e| e.into_inner()).0
    }

    /// The window's `memory_state` (I6) without `enabled` and `error`.
    pub fn view(&self, today: Date) -> Value {
        view::view(&self.lock(), today).unwrap_or_else(|_| view::empty(today))
    }

    // ---- the window (authority 4) ----

    /// A new row the person typed: profile, preference or promise, keyless.
    pub fn add(&self, kind: &str, text: &str, now: &Now) -> Result<i64> {
        if !["profile", "preference", "promise"].contains(&kind) {
            return Err(Error::Usage("kind must be profile, preference or promise".into()));
        }
        let text = clean(text);
        check_len(&text, TEXT_MAX)?;
        let it = Item {
            kind: kind.into(),
            predicate: None,
            qualifier: None,
            value: None,
            text,
            importance: 5,
            valid_from: None,
            authority: 4,
            quote: None,
            source_turn: None,
        };
        let outcome = self.write(|c| in_tx(c, |tx| reconcile(tx, &it, now)))?;
        outcome.id().ok_or(Error::NotFound)
    }

    /// The person's correction of live row `id`: a new authority-4 row with the same kind, predicate and
    /// qualifier replaces it, and the old row stays as `superseded` history. `value` changes a keyed
    /// row's value (None keeps it). `since` carries over unless the value changed.
    pub fn edit(&self, id: i64, text: &str, value: Option<&str>, now: &Now) -> Result<i64> {
        let text = clean(text);
        check_len(&text, TEXT_MAX)?;
        let value = value.map(clean);
        if let Some(v) = &value {
            check_len(v, VALUE_MAX)?;
        }
        self.write(|c| {
            in_tx(c, |tx| {
                let old = schema::get(tx, id)?.filter(|r| r.status == "live").ok_or(Error::NotFound)?;
                let pred = old.predicate.as_deref().and_then(predicates::get);
                let new_value = if pred.is_some() { value.or_else(|| old.value.clone()) } else { None };
                let same = new_value.as_deref().map(fold) == old.value.as_deref().map(fold);
                // A new value with the old sentence would keep saying the old fact (the prompt and
                // MEMORY.md show the text): put the new value in the sentence, or ask for a new one.
                let text = match (&old.value, &new_value) {
                    (Some(was), Some(now_v)) if !same && fold(&text) == fold(&old.text) => {
                        swap_value(&text, was, now_v).ok_or_else(|| Error::Usage("The sentence still says the old value: change it too.".into()))?
                    }
                    _ => text.clone(),
                };
                let it = Item {
                    kind: old.kind.clone(),
                    predicate: pred,
                    qualifier: old.qualifier.clone(),
                    value: new_value,
                    text,
                    importance: old.importance.unwrap_or(5),
                    valid_from: old.valid_from.clone().filter(|_| same),
                    authority: 4,
                    quote: None,
                    source_turn: None,
                };
                // The unique index allows one live row per slot: retire the old one first.
                tx.execute("UPDATE facts SET status = 'superseded', valid_to = ?1 WHERE id = ?2", params![now.today.to_string(), id])?;
                let new = insert(tx, &it, "live", now)?;
                tx.execute("UPDATE facts SET superseded_by = ?1 WHERE id = ?2", params![new, id])?;
                if let Some(p) = pred.filter(|p| p.one) {
                    delete_disputes(tx, p.key, it.qualifier.as_deref(), it.value.as_deref())?;
                }
                Ok(new)
            })
        })
    }

    /// Soft forget one live or disputed row.
    pub fn soft_forget(&self, id: i64) -> Result<()> {
        self.write(|c| {
            in_tx(c, |tx| {
                schema::get(tx, id)?.filter(|r| r.status == "live" || r.status == "disputed").ok_or(Error::NotFound)?;
                soft_forget_row(tx, id)
            })
        })
    }

    /// Hard erase: the row and its whole `superseded_by` chain, both directions (every earlier value
    /// that led to it and every later one that replaced it), whatever their status. If that leaves a
    /// single-valued slot with no live row, that slot's disputed row goes too (it only asked about the
    /// row that is gone). The file is written with `secure_delete`, so the bytes are zeroed.
    pub fn erase(&self, id: i64) -> Result<()> {
        self.write(|c| {
            in_tx(c, |tx| {
                schema::get(tx, id)?.ok_or(Error::NotFound)?;
                let mut chain = BTreeSet::new();
                let mut todo = vec![id];
                while let Some(x) = todo.pop() {
                    if chain.insert(x) {
                        for r in schema::query(tx, "WHERE id = ?1 OR superseded_by = ?1", [x])? {
                            todo.push(r.id);
                            todo.extend(r.superseded_by);
                        }
                    }
                }
                let mut slots = BTreeSet::new();
                for &x in &chain {
                    if let Some(r) = schema::get(tx, x)? {
                        slots.extend(r.predicate.map(|p| (p, r.qualifier)));
                    }
                    tx.execute("UPDATE facts SET superseded_by = NULL WHERE id = ?1", [x])?;
                }
                for &x in &chain {
                    tx.execute("DELETE FROM facts WHERE id = ?1", [x])?;
                }
                for (p, q) in slots {
                    drop_orphan_disputes(tx, &p, q.as_deref(), true)?;
                }
                Ok(())
            })
        })?;
        // the erased rows' pages are zeroed in the WAL; move them into the main file and empty the WAL,
        // so no copy of the text stays on disk after the window says it's gone
        self.lock().query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// The person's answer to a dispute (`id` is the disputed row). Keep the new value: it becomes live
    /// at authority 4 and the old live row is superseded. Keep the old one: the disputed row is forgotten.
    pub fn resolve(&self, id: i64, keep_new: bool, now: &Now) -> Result<()> {
        self.write(|c| {
            in_tx(c, |tx| {
                let d = schema::get(tx, id)?.filter(|r| r.status == "disputed").ok_or(Error::NotFound)?;
                if !keep_new {
                    tx.execute("UPDATE facts SET status = 'forgotten' WHERE id = ?1", [id])?;
                    return Ok(());
                }
                if let Some(live) = d.predicate.as_deref().map(|p| schema::live_slot(tx, p, d.qualifier.as_deref())).transpose()?.flatten() {
                    let to = d.valid_from.clone().unwrap_or_else(|| now.today.to_string());
                    tx.execute(
                        "UPDATE facts SET status = 'superseded', valid_to = ?1, superseded_by = ?2 WHERE id = ?3",
                        params![to, id, live.id],
                    )?;
                }
                tx.execute(
                    "UPDATE facts SET status = 'live', authority = 4, quote = NULL, valid_to = NULL, observed_at = ?1 WHERE id = ?2",
                    params![now.at, id],
                )?;
                Ok(())
            })
        })
    }
}

fn check_len(s: &str, max: usize) -> Result<()> {
    if s.is_empty() {
        Err(Error::Empty)
    } else if s.chars().count() > max {
        Err(Error::TooLong)
    } else if looks_secret(s) {
        Err(Error::Secret)
    } else {
        Ok(())
    }
}

fn in_tx<T>(c: &mut Connection, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

fn strings<'a>(v: Option<&'a Value>, field: &str) -> Vec<&'a str> {
    v.and_then(Value::as_array).into_iter().flatten().filter_map(|t| t.get(field)?.as_str()).collect()
}

/// One `items[]` entry → a validated item with its authority (3 only for a quote the person really said).
fn item(v: &Value, turn: Option<&str>, said: Option<&str>, today: Date) -> std::result::Result<Item, &'static str> {
    let mut it = validate::from_extract(v, today)?;
    let quote = v.get("quote").and_then(Value::as_str).map(clean).filter(|q| !q.is_empty());
    if quote.as_deref().is_some_and(looks_secret) {
        return Err("secret");
    }
    it.source_turn = turn.map(str::to_string);
    if let Some(q) = quote.filter(|q| said.is_some_and(|s| quote_ok(q, s))) {
        it.authority = 3;
        it.quote = Some(q);
    }
    Ok(it)
}

fn soft_forget_row(tx: &Transaction, id: i64) -> Result<()> {
    let r = schema::get(tx, id)?.ok_or(Error::NotFound)?;
    tx.execute("UPDATE facts SET status = 'forgotten' WHERE id = ?1", [id])?;
    if let Some(p) = r.predicate {
        drop_orphan_disputes(tx, &p, r.qualifier.as_deref(), false)?;
    }
    Ok(())
}

/// A disputed row asks "which is right?" about a live one. With no live row left in a single-valued
/// slot there is nothing to ask about, so its disputed row is forgotten (or deleted, when `hard`).
fn drop_orphan_disputes(tx: &Transaction, key: &str, qualifier: Option<&str>, hard: bool) -> Result<()> {
    if !predicates::get(key).is_some_and(|p| p.one) || schema::live_slot(tx, key, qualifier)?.is_some() {
        return Ok(());
    }
    let q = qualifier.unwrap_or("");
    let sql = if hard {
        "DELETE FROM facts WHERE status = 'disputed' AND subject = 'user' AND predicate = ?1 AND COALESCE(qualifier, '') = ?2"
    } else {
        "UPDATE facts SET status = 'forgotten' WHERE status = 'disputed' AND subject = 'user' AND predicate = ?1 AND COALESCE(qualifier, '') = ?2"
    };
    tx.execute(sql, params![key, q])?;
    Ok(())
}

/// `text` with the first case-insensitive match of `was` replaced by `now` (None when it isn't there).
fn swap_value(text: &str, was: &str, now: &str) -> Option<String> {
    let (lt, lw) = (text.to_lowercase(), was.to_lowercase());
    // only when lowercasing keeps byte offsets (true for the ASCII and Vietnamese letters people use)
    if lt.len() != text.len() || lw.is_empty() {
        return text.find(was).map(|i| format!("{}{now}{}", &text[..i], &text[i + was.len()..]));
    }
    let i = lt.find(&lw)?;
    Some(format!("{}{now}{}", &text[..i], &text[i + lw.len()..]))
}
