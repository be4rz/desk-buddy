//! The reflection run: while the person is away, new episodes become a few insights, a few updates
//! and merges. The model call is the sidecar's; this module picks the batch (`reflect_args`), applies
//! the reply in one transaction (`apply_reflect`) and keeps the run's bookkeeping, so a crash or a bad
//! reply leaves nothing half-applied and the next trigger runs the same batch again.

use crate::reconcile::{insert, reconcile};
use crate::schema::{self, meta_get, meta_i64, meta_set};
use crate::store::{in_tx, REFLECT_IMPORTANCE};
use crate::validate::{self, clean, fold, int, Item, TEXT_MAX};
use crate::{predicates, render, retention, secret::looks_secret, Error, Now, Outcome, Result, Store};
use jiff::civil::Date;
use jiff::Timestamp;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// A reply that would change more than this share of the live rows is rejected.
pub const GUARD: f64 = 0.30;
/// A run covers at most this many new episodes.
pub const MAX_EPISODES: usize = 50;
/// A run is due once this much importance has been stored since the last one.
pub const TRIGGER: i64 = 30;
/// A batch is tried at most this many times, then given up on.
pub const MAX_TRIES: u32 = 3;

const MAX_UPDATES: usize = 20;
const MAX_INSIGHTS: usize = 3;
const DAY_SECS: i64 = 86_400;
const LAST_ID: &str = "reflect_last_id";
const LAST_AT: &str = "last_reflection_at";

/// One run to do: the batch id and the `memory.reflect` request body (I1).
#[derive(Debug, Clone)]
pub struct ReflectJob {
    /// Lowercase hex SHA-256 of the batch's sorted episode ids joined by `,`.
    pub batch_id: String,
    pub args: Value,
}

/// What `apply_reflect` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reflected {
    /// Updates that were not skipped.
    pub updates: usize,
    pub insights: usize,
    /// Rows dropped into a kept one.
    pub merges: usize,
    /// Episodes retention forgot.
    pub forgotten: usize,
    /// The share of live rows the reply changed, in percent.
    pub pct: u32,
    /// The days (`YYYY-MM-DD`) whose notes changed: insights added, rows merged or forgotten.
    pub days: Vec<String>,
}

struct Episode {
    id: i64,
    day: String,
    text: String,
    importance: i64,
}

/// Live episodes newer than the last run, oldest first.
fn new_episodes(conn: &Connection) -> Result<Vec<Episode>> {
    let mut st = conn.prepare(
        "SELECT id, substr(observed_at, 1, 10), text, COALESCE(importance, 5) FROM facts
         WHERE kind = 'episode' AND status = 'live' AND id > ?1 ORDER BY id LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![meta_i64(conn, LAST_ID), MAX_EPISODES], |r| {
            Ok(Episode { id: r.get(0)?, day: r.get(1)?, text: r.get(2)?, importance: r.get(3)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn batch_hash(ids: &[i64]) -> String {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    let joined = sorted.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    Sha256::digest(joined.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn tries_key(batch: &str) -> String {
    format!("reflect_tries:{batch}")
}

fn status(conn: &Connection, batch: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT status FROM reflections WHERE batch_id = ?1", [batch], |r| r.get(0)).optional()?)
}

/// Writes the run's status (a `done` run stays done).
fn mark(conn: &Connection, batch: &str, status: &str, now: &Now) -> Result<()> {
    let finished = (status != "running").then_some(now.at.as_str());
    conn.execute(
        "INSERT INTO reflections (batch_id, status, started_at, finished_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(batch_id) DO UPDATE SET status = ?2, finished_at = ?4,
           started_at = CASE WHEN ?2 = 'running' THEN ?3 ELSE started_at END
         WHERE status != 'done'",
        params![batch, status, now.at, finished],
    )?;
    Ok(())
}

fn hours_since(then: &str, now: &str) -> Option<i64> {
    let (a, b) = (then.parse::<Timestamp>().ok()?, now.parse::<Timestamp>().ok()?);
    Some((b.as_second() - a.as_second()) / 3600)
}

impl Store {
    /// True when a run should start: the store is writable, no chat turn waits for the extractor, and
    /// either [`TRIGGER`] importance has come in since the last run, or it is 24 hours or more since it
    /// (or there has been none) and there is a new episode. The app checks idleness and the switch.
    pub fn reflect_due(&self, now: &Now) -> bool {
        if self.is_newer() {
            return false;
        }
        let c = self.lock();
        let pending: i64 = c.query_row("SELECT count(*) FROM pending_turns", [], |r| r.get(0)).unwrap_or(1);
        if pending > 0 {
            return false;
        }
        // Nothing new (all the new ones forgotten, or a batch given up on): nothing to reflect on, whatever
        // the importance counter says.
        let has_new = new_episodes(&c).map(|e| !e.is_empty()).unwrap_or(false);
        if !has_new {
            return false;
        }
        if meta_i64(&c, REFLECT_IMPORTANCE) >= TRIGGER {
            return true;
        }
        let day_old = meta_get(&c, LAST_AT).is_none_or(|at| hours_since(&at, &now.at).is_none_or(|h| h >= DAY_SECS / 3600));
        day_old
    }

    /// The next run: the `memory.reflect` body for the new live episodes (at most [`MAX_EPISODES`], oldest
    /// first), the live core rows, the registry and `today`. The batch is marked `running` and the try is
    /// counted. None when there is no new episode, the batch is already `done`, the store is read-only,
    /// or the batch has been tried [`MAX_TRIES`] times: it is then given up (marked `failed`, the next
    /// run starts after it).
    pub fn reflect_args(&self, today: Date, now: &Now) -> Option<ReflectJob> {
        self.write_quiet(|c| in_tx(c, |tx| plan(tx, today, now))).ok().flatten()
    }

    /// Applies a `memory.reflect` reply in one transaction. `updates` go through the extractor's checks and
    /// `reconcile` at authority 1 (episodes and insights refused); `insights` (at most 3) cite episodes of
    /// this batch; `merges` fold keyless rows of one kind into one. A reply that changes more than [`GUARD`]
    /// of the live rows is `Err(Error::Rejected)`: nothing is applied and the run is `failed`. Otherwise
    /// retention runs, the counters reset and the run is `done`; the generated files are then re-rendered.
    /// Applying a batch that is already `done` does nothing. A reply that is not an object marks the run
    /// `failed` and is `Err(Error::Usage)`.
    pub fn apply_reflect(&self, job: &ReflectJob, reply: &Value, now: &Now) -> Result<Reflected> {
        if self.is_newer() {
            return Err(Error::Newer);
        }
        if !reply.is_object() {
            self.reflect_failed(job, now)?;
            return Err(Error::Usage("reply is not an object".into()));
        }
        let batch: Vec<i64> =
            job.args.get("episodes").and_then(Value::as_array).into_iter().flatten().filter_map(|e| e.get("id")?.as_i64()).collect();
        let out = self.write(|c| {
            if status(c, &job.batch_id)?.as_deref() == Some("done") {
                return Ok(Ok(Reflected::default()));
            }
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let live_before: i64 = tx.query_row("SELECT count(*) FROM facts WHERE status = 'live'", [], |r| r.get(0))?;
            let mut done = apply(&tx, &batch, reply, now)?;
            let changed = done.changed;
            let pct = (changed as f64 * 100.0 / live_before.max(1) as f64).round() as u32;
            if changed as f64 / live_before.max(1) as f64 > GUARD {
                tx.rollback()?;
                mark(c, &job.batch_id, "failed", now)?;
                return Ok(Err(pct));
            }
            done.out.pct = pct;
            let forgotten = retention::run(&tx, now.today)?;
            done.out.forgotten = forgotten.len();
            done.out.days.extend(forgotten);
            done.out.days.sort();
            done.out.days.dedup();
            meta_set(&tx, REFLECT_IMPORTANCE, "0")?;
            meta_set(&tx, LAST_AT, &now.at)?;
            if let Some(max) = batch.iter().max() {
                meta_set(&tx, LAST_ID, &(*max).max(meta_i64(&tx, LAST_ID)).to_string())?;
            }
            schema::meta_del(&tx, &tries_key(&job.batch_id))?;
            mark(&tx, &job.batch_id, "done", now)?;
            tx.commit()?;
            Ok(Ok(done.out))
        })?;
        match out {
            Ok(done) => {
                // The data is committed; the files are derived and the app re-renders them on its next change.
                let _ = self.write_files();
                Ok(done)
            }
            Err(pct) => Err(Error::Rejected { pct }),
        }
    }

    /// The model's reply was unusable (bad JSON, an apply error): the run is `failed`, nothing is applied.
    /// The next trigger picks the same batch again, until it has been tried [`MAX_TRIES`] times.
    pub fn reflect_failed(&self, job: &ReflectJob, now: &Now) -> Result<()> {
        self.write_quiet(|c| mark(c, &job.batch_id, "failed", now))
    }

    /// The run never got a reply to judge (no account, a timeout, the sidecar died or restarted, memory
    /// switched off meanwhile): `failed` like [`Store::reflect_failed`], but the try is given back, so a
    /// flaky network or a quit never uses up a batch.
    pub fn reflect_unspent(&self, job: &ReflectJob, now: &Now) -> Result<()> {
        self.write_quiet(|c| {
            in_tx(c, |tx| {
                let key = tries_key(&job.batch_id);
                match meta_i64(tx, &key) {
                    n if n > 1 => meta_set(tx, &key, &(n - 1).to_string())?,
                    _ => schema::meta_del(tx, &key)?,
                }
                mark(tx, &job.batch_id, "failed", now)
            })
        })
    }
}

fn plan(tx: &Transaction, today: Date, now: &Now) -> Result<Option<ReflectJob>> {
    let eps = new_episodes(tx)?;
    let Some(max) = eps.iter().map(|e| e.id).max() else { return Ok(None) };
    let batch_id = batch_hash(&eps.iter().map(|e| e.id).collect::<Vec<_>>());
    if status(tx, &batch_id)?.as_deref() == Some("done") {
        return Ok(None);
    }
    let tries = meta_i64(tx, &tries_key(&batch_id));
    if tries >= i64::from(MAX_TRIES) {
        meta_set(tx, LAST_ID, &max.to_string())?;
        schema::meta_del(tx, &tries_key(&batch_id))?;
        mark(tx, &batch_id, "failed", now)?;
        return Ok(None);
    }
    meta_set(tx, &tries_key(&batch_id), &(tries + 1).to_string())?;
    mark(tx, &batch_id, "running", now)?;
    let known: Vec<Value> = render::live_core(tx)?
        .iter()
        .map(|r| json!({"id": r.id, "kind": r.kind, "predicate": r.predicate, "qualifier": r.qualifier, "value": r.value, "text": r.text}))
        .collect();
    let episodes: Vec<Value> = eps.iter().map(|e| json!({"id": e.id, "day": e.day, "text": e.text, "importance": e.importance})).collect();
    let args = json!({
        "episodes": episodes, "known": known, "registry": predicates::registry_json(), "today": today.to_string(),
    });
    Ok(Some(ReflectJob { batch_id, args }))
}

struct Done {
    out: Reflected,
    /// Live rows the reply touched (the guard's numerator).
    changed: usize,
}

fn array<'a>(reply: &'a Value, key: &str) -> &'a [Value] {
    reply.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Updates, insights and merges of one reply, inside the caller's transaction.
fn apply(tx: &Transaction, batch: &[i64], reply: &Value, now: &Now) -> Result<Done> {
    let mut out = Reflected::default();
    let mut changed = 0;
    let today = now.at.get(..10).unwrap_or_default().to_string();

    for v in array(reply, "updates").iter().take(MAX_UPDATES) {
        let kind = v.get("kind").and_then(Value::as_str);
        if matches!(kind, Some("episode" | "insight")) {
            continue;
        }
        // The extractor's checks (secrets included); no quote, so authority 1.
        let Ok(it) = validate::from_extract(v, now.today) else { continue };
        match reconcile(tx, &it, now)? {
            Outcome::Skipped(_) => {}
            Outcome::Created(_) => out.updates += 1,
            Outcome::Extended(_) | Outcome::Superseded { .. } | Outcome::Disputed(_) => {
                out.updates += 1;
                changed += 1;
            }
        }
    }

    for v in array(reply, "insights").iter().take(MAX_INSIGHTS) {
        let text = clean(v.get("text").and_then(Value::as_str).unwrap_or_default());
        if text.is_empty() || text.chars().count() > TEXT_MAX || looks_secret(&text) {
            continue;
        }
        let importance = v.get("importance").and_then(int).unwrap_or(5).clamp(1, 10);
        let mut sources = BTreeSet::new();
        for s in v.get("sources").and_then(Value::as_array).into_iter().flatten().filter_map(int).filter(|s| batch.contains(s)) {
            let live: i64 = tx.query_row("SELECT count(*) FROM facts WHERE id = ?1 AND kind = 'episode' AND status = 'live'", [s], |r| r.get(0))?;
            if live > 0 {
                sources.insert(s);
            }
        }
        if sources.is_empty() {
            continue;
        }
        let want = fold(&text);
        let same = schema::query(tx, "WHERE status = 'live' AND kind = 'insight'", [])?.iter().any(|r| fold(&r.text) == want);
        if same {
            continue;
        }
        let it = Item {
            kind: "insight".into(),
            predicate: None,
            qualifier: None,
            value: None,
            text,
            importance,
            valid_from: None,
            authority: 1,
            quote: None,
            source_turn: None,
        };
        let id = insert(tx, &it, "live", now)?;
        for s in sources {
            tx.execute("INSERT OR IGNORE INTO insight_sources (insight_id, episode_id) VALUES (?1, ?2)", [id, s])?;
        }
        out.insights += 1;
        changed += 1;
        out.days.push(today.clone());
    }

    for v in array(reply, "merges") {
        let Some(keep) = v.get("keep").and_then(int) else { continue };
        let drops: Vec<i64> = v.get("drop").and_then(Value::as_array).into_iter().flatten().filter_map(int).collect();
        let distinct: BTreeSet<i64> = drops.iter().copied().chain([keep]).collect();
        if drops.is_empty() || distinct.len() != drops.len() + 1 {
            continue;
        }
        let Some(kept) = schema::get(tx, keep)? else { continue };
        let rows: Vec<_> = drops.iter().map(|&d| schema::get(tx, d)).collect::<Result<Vec<_>>>()?;
        let ok = |r: &schema::Row| r.status == "live" && r.predicate.is_none() && r.kind == kept.kind;
        // Reflection never overrides what the person said: only the bear's and the extractor's rows
        // (authority 1-2) can be dropped into another.
        let droppable = |r: &schema::Row| ok(r) && r.authority <= 2;
        if !ok(&kept) || !rows.iter().all(|r| r.as_ref().is_some_and(droppable)) {
            continue;
        }
        for r in rows.into_iter().flatten() {
            tx.execute("UPDATE facts SET status = 'superseded', superseded_by = ?1 WHERE id = ?2", [keep, r.id])?;
            out.merges += 1;
            changed += 1;
            if matches!(r.kind.as_str(), "episode" | "insight") {
                out.days.push(r.observed_at.get(..10).unwrap_or_default().to_string());
            }
        }
    }
    Ok(Done { out, changed })
}
