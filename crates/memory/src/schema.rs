//! The SQLite schema, its migrations (expand only, one transaction each) and row access.

use crate::Result;
use rusqlite::{Connection, Params, TransactionBehavior};

/// The `user_version` this crate writes. A file with a higher one opens read-only.
pub const VERSION: i64 = 3;

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

// Keyword search over `facts.text` (an external-content index: the rows stay in `facts`).
// `remove_diacritics 2` makes "hom qua" match "hôm qua".
const V2: &str = "
CREATE VIRTUAL TABLE facts_fts USING fts5(
  text, content='facts', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER facts_ai AFTER INSERT ON facts BEGIN
  INSERT INTO facts_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER facts_ad AFTER DELETE ON facts BEGIN
  INSERT INTO facts_fts(facts_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER facts_au AFTER UPDATE OF text ON facts BEGIN
  INSERT INTO facts_fts(facts_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO facts_fts(rowid, text) VALUES (new.id, new.text);
END;
INSERT INTO facts_fts(facts_fts) VALUES ('rebuild');
";

// Vectors (p3). `facts_vec` needs the sqlite-vec module, so [`ensure_vec`] makes it on each writable open where the
// module loaded, and a store without it is keyword only. `vec_rows` is a plain table (no module needed) naming the
// facts that have a vector; its trigger runs on every delete, so a fact erased while the module was missing never
// lends its vector to a later fact that reuses the id: the next open with the module drops every vector it doesn't list.
const V3: &str = "
CREATE TABLE vec_rows (id INTEGER PRIMARY KEY);
CREATE TRIGGER vec_rows_ad AFTER DELETE ON facts BEGIN
  DELETE FROM vec_rows WHERE id = old.id;
END;
";

/// `MIGRATIONS[n]` takes a file from user_version `n` to `n + 1`.
const MIGRATIONS: [&str; 3] = [V1, V2, V3];

/// The embedding size `facts_vec` holds (`nomic-embed-text`).
pub const DIMS: usize = 768;

/// Whether this connection has the sqlite-vec functions (`vec_version()` answers).
pub(crate) fn vec_loaded(conn: &Connection) -> bool {
    conn.query_row("SELECT vec_version()", [], |r| r.get::<_, String>(0)).is_ok()
}

/// The rows recall may bring back (live day moments and keyless notes); the core section is taken out later.
pub(crate) const RECALLABLE: &str = "f.status = 'live' AND (f.kind IN ('episode', 'insight') OR f.predicate IS NULL)";

/// Zeroes, then deletes, the vectors of the facts `ids_sql` (a subquery) names. The zeroing writes over the
/// vector's bytes in its chunk, so an erased fact's embedding is not left in the file even if a vec0 delete only
/// cleared its validity bit (sqlite-vec 0.1.9 with `secure_delete` already leaves none: `tests/hybrid.rs` checks the bytes).
pub(crate) fn drop_vectors_sql(ids_sql: &str) -> String {
    format!(
        "UPDATE facts_vec SET embedding = zeroblob({b}) WHERE rowid IN ({ids_sql});
         DELETE FROM facts_vec WHERE rowid IN ({ids_sql});",
        b = DIMS * 4
    )
}

/// Brings `facts_vec` in line with the module (each writable open, after [`migrate`]). Loaded: the table (one
/// normalised embedding per row, keyed by fact id, cosine distance), its delete trigger, and no vector that
/// `vec_rows` doesn't list or whose fact is no longer live. Not loaded: the trigger goes, so deleting a fact never
/// needs the missing module (`vec_rows` still forgets it; a later loaded open drops the vector). Returns whether
/// vectors are on.
pub(crate) fn ensure_vec(conn: &Connection, loaded: bool) -> Result<bool> {
    if !loaded {
        conn.execute_batch("DROP TRIGGER IF EXISTS facts_vec_ad;")?;
        return Ok(false);
    }
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS facts_vec USING vec0(embedding float[{DIMS}] distance_metric=cosine);
         CREATE TRIGGER IF NOT EXISTS facts_vec_ad AFTER DELETE ON facts BEGIN
           UPDATE facts_vec SET embedding = zeroblob({b}) WHERE rowid = old.id;
           DELETE FROM facts_vec WHERE rowid = old.id;
         END;
         DELETE FROM vec_rows WHERE id NOT IN (SELECT id FROM facts WHERE status = 'live');
         {sweep}",
        b = DIMS * 4,
        sweep = drop_vectors_sql("SELECT rowid FROM facts_vec WHERE rowid NOT IN (SELECT id FROM vec_rows)")
    ))?;
    Ok(true)
}

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

/// One `meta` value.
pub(crate) fn meta_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok()
}

pub(crate) fn meta_i64(conn: &Connection, key: &str) -> i64 {
    meta_get(conn, key).and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub(crate) fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", [key, value])?;
    Ok(())
}

pub(crate) fn meta_del(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM meta WHERE key = ?1", [key])?;
    Ok(())
}

/// The live day moments (episodes and insights): the History, recall by day and the notes files.
pub(crate) const LIVE_HISTORY: &str = "status = 'live' AND kind IN ('episode', 'insight')";
/// The kinds a day moment can be.
pub(crate) const HISTORY_KINDS: [&str; 2] = ["episode", "insight"];

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
    pub(crate) fn read(r: &rusqlite::Row) -> rusqlite::Result<Row> {
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
