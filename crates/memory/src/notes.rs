//! `notes/YYYY-MM-DD.md`: one generated file per day that has live episodes or insights.
//! A file for a day with none left is deleted, so a forgotten or erased episode leaves `notes/` too.

use crate::files::HEADER;
use crate::schema::LIVE_HISTORY;
use crate::{Result, Store};
use jiff::civil::Date;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The text of each day's file, keyed by day.
fn days(conn: &Connection) -> Result<BTreeMap<String, String>> {
    let mut st = conn.prepare(&format!(
        "SELECT substr(f.observed_at, 1, 10), f.kind, f.text,
                (SELECT count(*) FROM insight_sources s WHERE s.insight_id = f.id)
         FROM facts f WHERE {LIVE_HISTORY} ORDER BY f.observed_at, f.id"
    ))?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (day, kind, text, sources) in rows {
        let Ok(date) = day.parse::<Date>() else { continue };
        let file = out.entry(day).or_insert_with(|| format!("{HEADER}\n\n# {}\n", date.strftime("%a %-d %b %Y")));
        file.push('\n');
        match (kind.as_str(), sources) {
            ("insight", 0) => file.push_str(&format!("- Pattern: {text}")),
            ("insight", n) => file.push_str(&format!("- Pattern: {text} (from {n} moment{})", if n == 1 { "" } else { "s" })),
            _ => file.push_str(&format!("- {text}")),
        }
    }
    for file in out.values_mut() {
        file.push('\n');
    }
    Ok(out)
}

/// `YYYY-MM-DD.md`, the files this module owns.
fn is_note(name: &str) -> bool {
    name.len() == 13 && name.ends_with(".md") && name[..10].parse::<Date>().is_ok()
}

impl Store {
    /// Writes the notes whose content changed and deletes those of days with no live episode or insight.
    pub(crate) fn write_notes(&self) -> Result<()> {
        let days = days(&self.lock())?;
        let dir = self.dir().join("notes");
        if days.is_empty() && !dir.is_dir() {
            return Ok(());
        }
        fs::create_dir_all(&dir)?;
        for (day, text) in &days {
            let path = dir.join(format!("{day}.md"));
            if fs::read_to_string(&path).ok().as_deref() != Some(text) {
                write_atomic(&path, text)?;
            }
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_note(&name) && !days.contains_key(&name[..10]) {
                fs::remove_file(entry.path())?;
            }
        }
        if days.is_empty() {
            let _ = fs::remove_dir(&dir); // only when nothing else is in it
        }
        Ok(())
    }

    /// After a forget or erase: if notes exist, re-render them so the text leaves them at once.
    pub(crate) fn refresh_notes(&self) {
        if self.dir().join("notes").is_dir() {
            let _ = self.write_notes();
        }
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
