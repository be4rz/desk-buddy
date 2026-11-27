//! The SQLite schema, its migrations (expand only, one transaction each) and row access.

use crate::Result;
use rusqlite::{Connection, Params, TransactionBehavior};

/// The `user_version` this crate writes. A file with a higher one opens read-only.
pub const VERSION: i64 = 1;

const V1: &str = "
CREATE TABLE facts (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('profile','preference','episode','promise','insight')),
  subject       TEXT NOT NULL DEFAULT 'user',   -- 'user', 'user.cat', 'bear'
  predicate     TEXT,                           -- from the registry; NULL = keyless note
  qualifier     TEXT,                           -- e.g. 'sister' for user.people
  cardinality   TEXT NOT NULL DEFAULT 'many' CHECK (cardinality IN ('one','many')),
  value         TEXT,                           -- 'Hanoi'; NULL for keyless notes
  text          TEXT NOT NULL,                  -- 'Lives in Hanoi' (what prompts and the window show)
  importance    INTEGER CHECK (importance BETWEEN 1 AND 10),
  authority     INTEGER NOT NULL,               -- 4 owner, 3 user, 2 bear, 1 extractor
  status        TEXT NOT NULL DEFAULT 'live'
                CHECK (status IN ('live','superseded','disputed','forgotten')),
  valid_from    TEXT,                           -- ISO date: true in the world from
  valid_to      TEXT,                           -- ISO date: until; NULL = still true
  observed_at   TEXT NOT NULL,                  -- ISO timestamp: when the bear learned it
  superseded_by INTEGER REFERENCES facts(id),
  source_turn   TEXT,                           -- request id ('u:<uuid>') it came from
  quote         TEXT,                           -- the person's own words, when authority = 3
  last_recall   TEXT,
  recall_n      INTEGER NOT NULL DEFAULT 0
);

-- At most one live value per single-valued slot.
CREATE UNIQUE INDEX facts_one_live ON facts(subject, predicate, COALESCE(qualifier,''))
  WHERE status = 'live' AND cardinality = 'one';
CREATE INDEX facts_live ON facts(status, kind);
CREATE INDEX facts_day  ON facts(substr(observed_at, 1, 10)) WHERE kind IN ('episode','insight');

-- Chat turns waiting for the extractor. Ids only: the text stays in agent.sqlite.
CREATE TABLE pending_turns (
  request_id TEXT PRIMARY KEY,                  -- 'u:<uuid>' chat turns only
  ended_at   TEXT NOT NULL,
  tries      INTEGER NOT NULL DEFAULT 0
);

-- Reflection runs, keyed by a hash of the episode ids they cover (phase 2).
CREATE TABLE reflections (
  batch_id    TEXT PRIMARY KEY,
  status      TEXT NOT NULL CHECK (status IN ('running','done','failed')),
  started_at  TEXT NOT NULL,
  finished_at TEXT
);

-- Which episodes an insight rests on (phase 2).
CREATE TABLE insight_sources (
  insight_id INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
  episode_id INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
  PRIMARY KEY (insight_id, episode_id)
);

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

/// `MIGRATIONS[n]` takes a file from user_version `n` to `n + 1`.
const MIGRATIONS: [&str; 1] = [V1];

pub fn user_version(conn: &Connection) -> Result<i64> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Brings a file at or below `VERSION` up to it. Never called on a newer file.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let from = user_version(conn)?.max(0) as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(from) {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

pub(crate) const COLS: &str = "id, kind, subject, predicate, qualifier, value, text, importance, \
     authority, status, valid_from, observed_at, superseded_by";

/// One `facts` row, the columns every reader needs.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub id: i64,
    pub kind: String,
    pub subject: String,
    pub predicate: Option<String>,
    pub qualifier: Option<String>,
    pub value: Option<String>,
    pub text: String,
    pub importance: Option<i64>,
    pub authority: i64,
    pub status: String,
    pub valid_from: Option<String>,
    pub observed_at: String,
    pub superseded_by: Option<i64>,
}

impl Row {
    fn read(r: &rusqlite::Row) -> rusqlite::Result<Row> {
        Ok(Row {
            id: r.get(0)?,
            kind: r.get(1)?,
            subject: r.get(2)?,
            predicate: r.get(3)?,
            qualifier: r.get(4)?,
            value: r.get(5)?,
            text: r.get(6)?,
            importance: r.get(7)?,
            authority: r.get(8)?,
            status: r.get(9)?,
            valid_from: r.get(10)?,
            observed_at: r.get(11)?,
            superseded_by: r.get(12)?,
        })
    }

    /// `pred` or `pred:qual`; None for a keyless note.
    pub fn key(&self) -> Option<String> {
        let p = self.predicate.as_deref()?;
        Some(match self.qualifier.as_deref() {
            Some(q) => format!("{p}:{q}"),
            None => p.to_string(),
        })
    }
}

/// `SELECT <cols> FROM facts <tail>`.
pub(crate) fn query(conn: &Connection, tail: &str, params: impl Params) -> Result<Vec<Row>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM facts {tail}"))?;
    let rows = st.query_map(params, Row::read)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub(crate) fn get(conn: &Connection, id: i64) -> Result<Option<Row>> {
    Ok(query(conn, "WHERE id = ?1", [id])?.into_iter().next())
}

/// The live row of a single-valued slot.
pub(crate) fn live_slot(conn: &Connection, predicate: &str, qualifier: Option<&str>) -> Result<Option<Row>> {
    Ok(query(
        conn,
        "WHERE status = 'live' AND subject = 'user' AND predicate = ?1 \
         AND COALESCE(qualifier, '') = ?2 AND cardinality = 'one'",
        rusqlite::params![predicate, qualifier.unwrap_or("")],
    )?
    .into_iter()
    .next())
}

/// Order of every list: importance, then newest first.
pub(crate) const BEST_FIRST: &str = "ORDER BY COALESCE(importance, 5) DESC, observed_at DESC, id DESC";
