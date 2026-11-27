//! The window's `memory_state` (I6), minus `enabled` and `error`, which the app adds.

use crate::predicates;
use crate::render::{disputes, live_core};
use crate::schema::{self, Row, LIVE_HISTORY};
use crate::{Fact, Result};
use jiff::civil::Date;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// `window | chat | bear | guess`, from the authority rank.
pub(crate) fn source(authority: i64) -> &'static str {
    match authority {
        4 => "window",
        3 => "chat",
        2 => "bear",
        _ => "guess",
    }
}

/// "2026-10" from a valid_from, or null.
fn since(r: &Row) -> Option<&str> {
    r.valid_from.as_deref().and_then(|d| d.get(..7))
}

impl From<&Row> for Fact {
    fn from(r: &Row) -> Fact {
        Fact {
            id: r.id,
            kind: r.kind.clone(),
            text: r.text.clone(),
            key: r.key(),
            value: r.value.clone(),
            since: since(r).map(str::to_string),
            source: source(r.authority),
            day: None,
        }
    }
}

fn label(r: &Row) -> Option<&'static str> {
    r.predicate.as_deref().and_then(predicates::label)
}

pub(crate) fn view(conn: &Connection, today: Date) -> Result<Value> {
    let disputes = disputes(conn)?;
    let disputed: HashSet<i64> = disputes.iter().map(|(_, live)| live.id).collect();
    let core = live_core(conn)?;
    let mut groups = Map::new();
    for (name, kind) in [("about", "profile"), ("help", "preference"), ("promises", "promise")] {
        let rows: Vec<Value> = core
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| {
                let mut v = json!(Fact::from(r));
                v["label"] = json!(label(r));
                v["disputed"] = json!(disputed.contains(&r.id));
                v
            })
            .collect();
        groups.insert(name.into(), Value::Array(rows));
    }
    let asks: Vec<Value> = disputes
        .iter()
        .map(|(d, live)| {
            json!({
                "id": d.id, "key": d.key(), "label": label(d), "value": d.value, "text": d.text,
                "live": {"id": live.id, "value": live.value, "source": source(live.authority)},
            })
        })
        .collect();
    let (history, more) = history(conn, None)?;
    Ok(json!({
        "v": 1,
        "today": today.to_string(),
        "phrase": "forget everything",
        "groups": groups,
        "disputes": asks,
        "history": history,
        "historyMore": more,
        "counts": {
            "live": core.len(), "disputes": asks.len(), "hidden": hidden(conn, core.len() + asks.len())?,
            "days": conn.query_row(&format!("SELECT count(DISTINCT substr(observed_at, 1, 10)) FROM facts WHERE {LIVE_HISTORY}"), [], |r| r.get::<_, i64>(0))?,
        },
    }))
}

/// Days shown at once.
const DAYS: usize = 14;

/// One History row: a group row's shape plus the kind and, for an insight, how many episodes back it.
fn item(conn: &Connection, r: &Row) -> Result<Value> {
    let mut v = json!(Fact::from(r));
    v["label"] = Value::Null;
    v["disputed"] = json!(false);
    if r.kind == "insight" {
        let n: i64 = conn.query_row("SELECT count(*) FROM insight_sources WHERE insight_id = ?1", [r.id], |r| r.get(0))?;
        v["sources"] = json!(n);
    }
    Ok(v)
}

/// The 14 newest days with live episodes or insights (strictly before `before`, when given), newest
/// first, each `{"day": "2026-11-26", "items": [...]}` with its items in observed order; and whether
/// older days remain.
pub(crate) fn history(conn: &Connection, before: Option<Date>) -> Result<(Vec<Value>, bool)> {
    let bound = before.map_or_else(|| "9999-12-31".to_string(), |d| d.to_string());
    let mut days: Vec<String> = conn
        .prepare(&format!(
            "SELECT DISTINCT substr(observed_at, 1, 10) AS d FROM facts WHERE {LIVE_HISTORY} AND d < ?1 ORDER BY d DESC LIMIT ?2"
        ))?
        .query_map(rusqlite::params![bound, DAYS + 1], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let more = days.len() > DAYS;
    days.truncate(DAYS);
    let mut out = Vec::new();
    for day in days {
        let rows = schema::query(conn, &format!("WHERE {LIVE_HISTORY} AND substr(observed_at, 1, 10) = ?1 ORDER BY observed_at, id"), [&day])?;
        let items = rows.iter().map(|r| item(conn, r)).collect::<Result<Vec<_>>>()?;
        out.push(json!({"day": day, "items": items}));
    }
    Ok((out, more))
}

/// The state with nothing in it, for a store that can't be read.
pub(crate) fn empty(today: Date) -> Value {
    json!({
        "v": 1, "today": today.to_string(), "phrase": "forget everything",
        "groups": {"about": [], "help": [], "promises": []}, "disputes": [],
        "history": null, "historyMore": false, "counts": {"live": 0, "disputes": 0, "hidden": 0, "days": 0},
    })
}

/// What the file still holds that the window doesn't list (forgotten and superseded rows, chats
/// waiting to be learned from): Forget everything stays offered while any of it is there.
fn hidden(conn: &Connection, shown: usize) -> Result<i64> {
    let facts: i64 = conn.query_row("SELECT count(*) FROM facts", [], |r| r.get(0))?;
    let pending: i64 = conn.query_row("SELECT count(*) FROM pending_turns", [], |r| r.get(0))?;
    Ok((facts - shown as i64).max(0) + pending)
}
