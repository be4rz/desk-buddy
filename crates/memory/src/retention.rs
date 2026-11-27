//! Retention, run inside a reflection run: old, minor, never-recalled episodes are forgotten.
//! Nothing else is ever dropped automatically.

use crate::store::soft_forget_row;
use crate::Result;
use jiff::civil::Date;
use jiff::ToSpan;
use rusqlite::Transaction;

/// Episodes older than this many days are candidates.
const DAYS: i64 = 90;
/// An importance up to this is minor (a missing importance counts as 5, so it is kept).
const MINOR: i64 = 4;

/// Forgets live episodes observed more than 90 days before `today` with importance ≤ 4 and
/// `recall_n = 0`. Returns the days (`YYYY-MM-DD`) of the rows forgotten, one per row.
pub(crate) fn run(tx: &Transaction, today: Date) -> Result<Vec<String>> {
    let Ok(cutoff) = today.checked_sub(DAYS.days()) else { return Ok(Vec::new()) };
    let old: Vec<(i64, String)> = tx
        .prepare(
            "SELECT id, substr(observed_at, 1, 10) FROM facts
             WHERE kind = 'episode' AND status = 'live' AND COALESCE(importance, 5) <= ?1
               AND recall_n = 0 AND substr(observed_at, 1, 10) < ?2",
        )?
        .query_map(rusqlite::params![MINOR, cutoff.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, _) in &old {
        soft_forget_row(tx, *id)?;
    }
    Ok(old.into_iter().map(|(_, day)| day).collect())
}
