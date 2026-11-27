//! `reconcile(item)`: the deterministic write rule. One transaction per item, no model call.
//! Authority decides: the person's word beats a guess (4 owner, 3 user, 2 bear, 1 extractor).

use crate::schema;
use crate::validate::{fold, Item};
use crate::{Now, Outcome, Result};
use jiff::civil::Date;
use rusqlite::{params, Transaction};

/// Keyless notes with the same text merge when the older one was seen this recently.
const DEDUPE_DAYS: i64 = 30;

pub(crate) fn reconcile(tx: &Transaction, it: &Item, now: &Now) -> Result<Outcome> {
    match it.predicate {
        None => keyless(tx, it, now),
        Some(p) if !p.one => many(tx, it, now),
        Some(p) => one(tx, p.key, it, now),
    }
}

fn keyless(tx: &Transaction, it: &Item, now: &Now) -> Result<Outcome> {
    let want = fold(&it.text);
    let rows = schema::query(tx, "WHERE status = 'live' AND predicate IS NULL AND kind = ?1", [&it.kind])?;
    match rows.iter().find(|r| fold(&r.text) == want && recent(&r.observed_at, now.today)) {
        Some(r) => {
            extend(tx, r.id, it, now)?;
            Ok(Outcome::Extended(r.id))
        }
        None => Ok(Outcome::Created(insert(tx, it, "live", now)?)),
    }
}

fn many(tx: &Transaction, it: &Item, now: &Now) -> Result<Outcome> {
    let key = it.predicate.map_or("", |p| p.key);
    let rows = schema::query(
        tx,
        "WHERE status = 'live' AND subject = 'user' AND predicate = ?1 AND COALESCE(qualifier, '') = ?2",
        params![key, it.qualifier.as_deref().unwrap_or("")],
    )?;
    let want = it.value.as_deref().map(fold);
    match rows.iter().find(|r| r.value.as_deref().map(fold) == want) {
        Some(r) => {
            extend(tx, r.id, it, now)?;
            Ok(Outcome::Extended(r.id))
        }
        None => Ok(Outcome::Created(insert(tx, it, "live", now)?)),
    }
}

fn one(tx: &Transaction, key: &str, it: &Item, now: &Now) -> Result<Outcome> {
    let live = schema::live_slot(tx, key, it.qualifier.as_deref())?;
    let out = match live {
        None => Outcome::Created(insert(tx, it, "live", now)?),
        Some(l) if l.value.as_deref().map(fold) == it.value.as_deref().map(fold) => {
            extend(tx, l.id, it, now)?;
            if it.authority >= 2 {
                // Said again (not guessed): the person has answered the slot's open question.
                delete_disputes(tx, key, it.qualifier.as_deref(), None)?;
            }
            Outcome::Extended(l.id)
        }
        Some(l) if it.authority >= l.authority => {
            let to = it.valid_from.clone().unwrap_or_else(|| now.today.to_string());
            tx.execute("UPDATE facts SET status = 'superseded', valid_to = ?1 WHERE id = ?2", params![to, l.id])?;
            let new = insert(tx, it, "live", now)?;
            tx.execute("UPDATE facts SET superseded_by = ?1 WHERE id = ?2", params![new, l.id])?;
            Outcome::Superseded { new, old: l.id }
        }
        Some(_) => {
            // Lower authority: the live row stays; this guess waits for the person, one per slot.
            delete_disputes(tx, key, it.qualifier.as_deref(), None)?;
            return Ok(Outcome::Disputed(insert(tx, it, "disputed", now)?));
        }
    };
    // A guess that turned out to be the live value no longer needs asking about.
    delete_disputes(tx, key, it.qualifier.as_deref(), it.value.as_deref())?;
    Ok(out)
}

/// Deletes the slot's disputed rows: all of them, or only those holding `value`.
pub(crate) fn delete_disputes(tx: &Transaction, key: &str, qualifier: Option<&str>, value: Option<&str>) -> Result<()> {
    let rows = schema::query(
        tx,
        "WHERE status = 'disputed' AND subject = 'user' AND predicate = ?1 AND COALESCE(qualifier, '') = ?2",
        params![key, qualifier.unwrap_or("")],
    )?;
    let want = value.map(fold);
    for r in rows.iter().filter(|r| want.is_none() || r.value.as_deref().map(fold) == want) {
        tx.execute("DELETE FROM facts WHERE id = ?1", [r.id])?;
    }
    Ok(())
}

/// A newer sighting of the same fact: fresher `observed_at`, the higher importance and authority win.
fn extend(tx: &Transaction, id: i64, it: &Item, now: &Now) -> Result<()> {
    tx.execute(
        "UPDATE facts SET observed_at = ?1,
           importance = MAX(COALESCE(importance, 0), ?2),
           quote = CASE WHEN ?3 > authority THEN ?4 ELSE quote END,
           source_turn = CASE WHEN ?3 > authority THEN ?5 ELSE source_turn END,
           authority = MAX(authority, ?3)
         WHERE id = ?6",
        params![now.at, it.importance, it.authority, it.quote, it.source_turn, id],
    )?;
    Ok(())
}

pub(crate) fn insert(tx: &Transaction, it: &Item, status: &str, now: &Now) -> Result<i64> {
    tx.execute(
        "INSERT INTO facts (kind, subject, predicate, qualifier, cardinality, value, text, importance,
                            authority, status, valid_from, valid_to, observed_at, source_turn, quote)
         VALUES (?1, 'user', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13)",
        params![
            it.kind,
            it.predicate.map(|p| p.key),
            it.qualifier,
            if it.predicate.is_some_and(|p| p.one) { "one" } else { "many" },
            it.value,
            it.text,
            it.importance,
            it.authority,
            status,
            it.valid_from,
            now.at,
            it.source_turn,
            it.quote,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

fn recent(observed_at: &str, today: Date) -> bool {
    observed_at
        .get(..10)
        .and_then(|d| d.parse::<Date>().ok())
        .and_then(|d| d.until(today).ok())
        .is_some_and(|span| span.get_days() as i64 <= DEDUPE_DAYS)
}
