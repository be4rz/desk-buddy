//! The core section the bear's prompt carries: "## What I know about you".
//! It holds no date that changes daily, or the prompt cache would break every day.

use crate::schema::{self, Row, BEST_FIRST};
use crate::Result;
use rusqlite::Connection;
use std::collections::HashSet;

const CAP: usize = 2000;
const MAX_PROMISES: usize = 5;
const MAX_DISPUTES: usize = 2;

const HEAD: &str = "## What I know about you
These are notes from earlier chats. They can be out of date: if the person
says otherwise, believe them. Never recite this list unprompted.";
const TAIL: &str = "You remember past chats. If the person asks about the past and it isn't here,
run `buddy memory recall <date>` or `buddy memory search \"<words>\"`.";
const TITLES: [&str; 4] =
    ["About them", "How they like help", "Things you said you'd do", "Not sure yet (ask when it fits, once)"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub(crate) const CORE_KINDS: &str = "kind IN ('profile', 'preference', 'promise')";

struct Entry {
    group: usize,
    line: String,
    importance: i64,
    observed_at: String,
    id: i64,
}

/// Live profile, preference and promise rows, best first.
pub(crate) fn live_core(conn: &Connection) -> Result<Vec<Row>> {
    schema::query(conn, &format!("WHERE status = 'live' AND {CORE_KINDS} {BEST_FIRST}"), [])
}

/// Disputed rows paired with the live row of their slot, best first. A dispute with no live row is left out.
pub(crate) fn disputes(conn: &Connection) -> Result<Vec<(Row, Row)>> {
    let mut out = Vec::new();
    for d in schema::query(conn, &format!("WHERE status = 'disputed' {BEST_FIRST}"), [])? {
        if let Some(live) = d.predicate.as_deref().map(|p| schema::live_slot(conn, p, d.qualifier.as_deref())).transpose()?.flatten() {
            out.push((d, live));
        }
    }
    Ok(out)
}

/// "Oct 2026" from "2026-10-01".
pub(crate) fn month_year(date: &str) -> Option<String> {
    let year = date.get(..4)?;
    let month: usize = date.get(5..7)?.parse().ok()?;
    Some(format!("{} {year}", MONTHS.get(month.checked_sub(1)?)?))
}

/// "Lives in Da Nang" → "lives in Da Nang", but "iPhone" and "NASA" keep their case.
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match (c.next(), c.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && b.is_lowercase() => a.to_lowercase().chain([b]).chain(c).collect(),
        _ => s.to_string(),
    }
}

/// The section text, or "" when nothing live is stored.
pub(crate) fn section(conn: &Connection) -> Result<String> {
    let entries = selected(conn)?;
    Ok(if entries.is_empty() { String::new() } else { compose(&entries) })
}

/// The ids of the rows the section lists (recall never repeats these).
pub(crate) fn core_ids(conn: &Connection) -> Result<HashSet<i64>> {
    Ok(selected(conn)?.iter().map(|e| e.id).collect())
}

/// The entries that make the section, after the cap.
fn selected(conn: &Connection) -> Result<Vec<Entry>> {
    let core = live_core(conn)?;
    if core.is_empty() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let mut promises = 0;
    for r in &core {
        let group = match r.kind.as_str() {
            "profile" => 0,
            "preference" => 1,
            _ => {
                promises += 1;
                if promises > MAX_PROMISES {
                    continue;
                }
                2
            }
        };
        let since = r.valid_from.as_deref().and_then(month_year).map(|m| format!(" (since {m})")).unwrap_or_default();
        entries.push(entry(group, format!("{}{since}", r.text), r));
    }
    for (d, live) in disputes(conn)?.iter().take(MAX_DISPUTES) {
        let on_record = live.value.as_deref().unwrap_or(&live.text);
        let said = d.text.trim_end_matches(['.', '?', '!']);
        entries.push(entry(3, format!("Maybe {}? You have {on_record} on record.", lower_first(said)), d));
    }
    // Lowest importance goes first when over the cap: keep the best n of everything.
    entries.sort_by(|a, b| {
        (b.importance, &b.observed_at, b.id).cmp(&(a.importance, &a.observed_at, a.id))
    });
    for n in (1..=entries.len()).rev() {
        if compose(&entries[..n]).chars().count() <= CAP {
            entries.truncate(n);
            return Ok(entries);
        }
    }
    Ok(Vec::new())
}

fn entry(group: usize, line: String, r: &Row) -> Entry {
    Entry { group, line, importance: r.importance.unwrap_or(5), observed_at: r.observed_at.clone(), id: r.id }
}

fn compose(entries: &[Entry]) -> String {
    let mut out = HEAD.to_string();
    for (group, title) in TITLES.iter().enumerate() {
        let mut lines = entries.iter().filter(|e| e.group == group).peekable();
        if lines.peek().is_none() {
            continue;
        }
        out.push_str("\n\n");
        out.push_str(title);
        for e in lines {
            out.push_str("\n- ");
            out.push_str(&e.line);
        }
    }
    out.push_str("\n\n");
    out.push_str(TAIL);
    out
}
