//! Recall on a chat message, `buddy memory recall` and keyword search, over the FTS5 index.
//!
//! Score = BM25 (scaled 0..1 by the batch's best) × 0.5^(age_days / 30) × (0.5 + importance / 10).
//! A date word in the message picks that day's episodes and insights with no BM25 term and no minimum
//! score, ranked first. Stopwords compare accent-free, so an unaccented "co"/"la" drops like "có"/"là" (and so
//! do the rarer "cò", "lá"): typed-without-accents Vietnamese matters more than those few words.

use crate::dates::{self, plain, Span};
use crate::render;
use crate::schema::{self, Row, COLS, LIVE_HISTORY};
use crate::validate::fold;
use crate::{Error, Fact, Now, Result, Store};
use jiff::civil::Date;
use jiff::Timestamp;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Rows scoring below this are not recalled.
pub const MIN_SCORE: f64 = 0.2;
/// At most this many lines.
pub const TOP: usize = 3;
/// At most this many characters in all the lines.
pub const MAX_CHARS: usize = 600;

const MAX_TERMS: usize = 12;
const FTS_LIMIT: u32 = 50;
const SEARCH_MAX: u32 = 20;

/// Words that say nothing about a topic (English and Vietnamese), plus the date words themselves.
const STOPWORDS: &[&str] = &[
    "what", "did", "we", "talk", "about", "the", "a", "an", "and", "or", "to", "of", "in", "on", "for", "is", "are",
    "was", "were", "i", "you", "me", "my", "it", "that", "this", "with", "do", "how", "any", "can", "có", "không",
    "là", "và", "của", "tôi", "bạn", "em", "anh", "chị", "gì", "nào", "nhé", "ạ", "thì", "mà", "đã", "được", "một",
    "những", "các", "này", "rồi", "cũng", "nữa", "nhỉ", "để", "nhưng", "rất", "lắm",
    // words too common to name a topic ("after two days", "a new project" must not pull in a note)
    "after", "before", "again", "also", "all", "am", "as", "at", "be", "been", "but", "by", "could", "does", "from",
    "get", "got", "had", "has", "have", "he", "she", "her", "him", "his", "if", "into", "just", "let", "more", "much",
    "new", "no", "not", "now", "only", "our", "out", "so", "some", "still", "than", "then", "there", "they", "them",
    "too", "up", "us", "very", "want", "when", "where", "which", "who", "why", "will", "would", "your", "good",
    "ve", "ll", "re", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    // date words (dates.rs resolves them; they are not topics)
    "today", "yesterday", "last", "week", "ago", "day", "days", "monday", "tuesday", "wednesday", "thursday",
    "friday", "saturday", "sunday", "hôm", "nay", "qua", "kia", "tuần", "trước", "ngày", "thứ", "chủ", "nhật",
];

fn stop(word: &str) -> bool {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| STOPWORDS.iter().map(|w| plain(w)).collect()).contains(&plain(word))
}

/// Runs of letters and digits of the folded text, at least `min` chars each, distinct, in order.
fn words(text: &str, min: usize, drop_stop: bool, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in fold(text).split(|c: char| !c.is_alphanumeric()) {
        if w.chars().count() >= min && !(drop_stop && stop(w)) && !out.iter().any(|o| o == w) && out.len() < max {
            out.push(w.to_string());
        }
    }
    out
}

/// Spellings of a folded word that the index tells apart: the tokenizer folds accents but not `đ`, so
/// "dan" must also ask for "đan" (and "đàn" asks for "dan"). Every `d`/`đ` becomes both, at most 8 forms.
fn spellings(word: &str) -> Vec<String> {
    let base = word.replace('đ', "d");
    let mut out = vec![String::new()];
    for ch in base.chars() {
        let alts: &[char] = if ch == 'd' && out.len() < 8 { &['d', 'đ'] } else { &[ch] };
        out = out.iter().flat_map(|o| alts.iter().map(move |a| format!("{o}{a}"))).collect();
    }
    out
}

/// Each word quoted (a `"` doubled) in all its [`spellings`], joined with OR. The tokenizer folds the
/// other accents on both sides. `prefix` words of 3+ chars also match longer words ("bisc" → "biscuit").
fn match_query(terms: &[String], prefix: bool) -> String {
    terms
        .iter()
        .flat_map(|t| {
            let star = if prefix && t.chars().count() >= 3 { "*" } else { "" };
            spellings(t).into_iter().map(move |v| format!("\"{}\"{star}", v.replace('"', "\"\"")))
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

struct Cand {
    id: i64,
    text: String,
    importance: Option<i64>,
    observed_at: String,
    last_recall: Option<String>,
    /// BM25 turned positive (bigger is better); 1 for a date match.
    rel: f64,
    /// Picked by a date word in the message: ranked before every keyword match, never cut by the minimum.
    dated: bool,
}

const CAND_COLS: &str = "f.id, f.text, f.importance, f.observed_at, f.last_recall";

fn cand(r: &rusqlite::Row, rel: f64, dated: bool) -> rusqlite::Result<Cand> {
    Ok(Cand {
        id: r.get(0)?,
        text: r.get(1)?,
        importance: r.get(2)?,
        observed_at: r.get(3)?,
        last_recall: r.get(4)?,
        rel,
        dated,
    })
}

fn day_of(observed_at: &str) -> &str {
    observed_at.get(..10).unwrap_or(observed_at)
}

/// Days since the row was last seen (observed or recalled, whichever is newer) at `now`; never negative.
fn age_days(c: &Cand, now: &Now) -> f64 {
    let ts = |s: &str| s.parse::<Timestamp>().ok();
    let seen = match c.last_recall.as_deref() {
        Some(l) if l > c.observed_at.as_str() => l,
        _ => c.observed_at.as_str(),
    };
    if let (Some(a), Some(b)) = (ts(seen), ts(&now.at)) {
        return ((b.as_second() - a.as_second()) as f64 / 86_400.0).max(0.0);
    }
    day_of(seen).parse::<Date>().ok().and_then(|d| d.until(now.today).ok()).map_or(0.0, |s| f64::from(s.get_days()).max(0.0))
}

fn score(c: &Cand, rel: f64, now: &Now) -> f64 {
    rel * 0.5f64.powf(age_days(c, now) / 30.0) * (0.5 + c.importance.unwrap_or(5) as f64 / 10.0)
}

/// Episodes and insights observed on `from..=to`.
fn on_days(conn: &Connection, from: Date, to: Date) -> Result<Vec<Cand>> {
    let mut st = conn.prepare(&format!(
        "SELECT {CAND_COLS} FROM facts f WHERE {LIVE_HISTORY} AND substr(f.observed_at, 1, 10) BETWEEN ?1 AND ?2"
    ))?;
    let rows = st.query_map([from.to_string(), to.to_string()], |r| cand(r, 1.0, true))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Live episode, insight and keyless rows matching any term, outside the core section, with positive BM25 (unscaled).
fn matching(conn: &Connection, terms: &[String]) -> Result<Vec<Cand>> {
    let core = render::core_ids(conn)?;
    let mut st = conn.prepare(&format!(
        "SELECT {CAND_COLS}, -bm25(facts_fts) AS rel FROM facts_fts JOIN facts f ON f.id = facts_fts.rowid
         WHERE facts_fts MATCH ?1 AND f.status = 'live' AND (f.kind IN ('episode', 'insight') OR f.predicate IS NULL)
         ORDER BY bm25(facts_fts) LIMIT ?2"
    ))?;
    let rows = st
        .query_map(params![match_query(terms, false), FTS_LIMIT], |r| {
            let rel: f64 = r.get("rel")?;
            cand(r, rel, false)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().filter(|c| !core.contains(&c.id)).collect())
}

impl Store {
    /// Up to [`TOP`] lines `"YYYY-MM-DD: text"` of past episodes, insights and notes that may matter for
    /// `message`, at most [`MAX_CHARS`] characters in all. Date words always win: they pick that day's
    /// episodes and insights (no BM25 term and no minimum score, so an old day is still found), ranked
    /// before any keyword match; the keywords pick the rest through FTS5. Core rows never appear. The rows sent
    /// get `last_recall = now.at` and `recall_n + 1`. A read-only store gives none.
    pub fn recall(&self, message: &str, now: &Now) -> Result<Vec<String>> {
        if self.is_newer() {
            return Ok(Vec::new());
        }
        let span = dates::in_message(message, now.today);
        let terms = words(message, 2, true, MAX_TERMS);
        let mut by_id: HashMap<i64, (f64, Cand)> = HashMap::new();
        {
            let c = self.lock();
            if !terms.is_empty() {
                let found = matching(&c, &terms)?;
                let best = found.iter().map(|f| f.rel).fold(0.0, f64::max);
                for f in found {
                    let rel = if best > 0.0 { f.rel / best } else { 1.0 };
                    let s = score(&f, rel, now);
                    if s >= MIN_SCORE {
                        by_id.insert(f.id, (s, f));
                    }
                }
            }
            if let Some((from, to)) = span.map(|s| s.bounds()) {
                for f in on_days(&c, from, to)? {
                    by_id.insert(f.id, (score(&f, 1.0, now), f));
                }
            }
        }
        let mut scored: Vec<(f64, Cand)> = by_id.into_values().collect();
        scored.sort_by(|(a, x), (b, y)| {
            y.dated
                .cmp(&x.dated)
                .then_with(|| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| (&y.observed_at, y.id).cmp(&(&x.observed_at, x.id)))
        });
        let (mut lines, mut ids, mut total) = (Vec::new(), Vec::new(), 0);
        for (_, c) in &scored {
            if lines.len() == TOP {
                break;
            }
            let line = format!("{}: {}", day_of(&c.observed_at), c.text);
            let n = line.chars().count();
            if total + n <= MAX_CHARS {
                total += n;
                lines.push(line);
                ids.push(c.id);
            }
        }
        if !ids.is_empty() {
            // Bookkeeping only: a failure here must not lose the lines.
            let _ = self.write_quiet(|c| {
                let tx = c.transaction()?;
                for id in &ids {
                    tx.execute("UPDATE facts SET last_recall = ?1, recall_n = recall_n + 1 WHERE id = ?2", params![now.at, id])?;
                }
                tx.commit()?;
                Ok(())
            });
        }
        Ok(lines)
    }

    /// The live episodes and insights observed in `span`, oldest first, each with its `day`
    /// (`buddy memory recall <date>`). Reading them does not count as a recall.
    pub fn recall_span(&self, span: &Span) -> Result<Vec<Fact>> {
        let (from, to) = span.bounds();
        let rows = schema::query(
            &self.lock(),
            &format!("WHERE {LIVE_HISTORY} AND substr(observed_at, 1, 10) BETWEEN ?1 AND ?2 ORDER BY observed_at, id"),
            [from.to_string(), to.to_string()],
        )?;
        Ok(rows
            .iter()
            .map(|r| {
                let mut f = Fact::from(r);
                f.day = Some(day_of(&r.observed_at).to_string());
                f
            })
            .collect())
    }

    /// Live rows whose text has any of the words (FTS5, accents, case and `đ`/`d` ignored; a word of 3+
    /// chars also matches as a prefix) or whose value or key contains one. Best BM25 first, then importance, then newest. `limit` is 1..=20.
    pub fn search(&self, q: &str, limit: u32) -> Result<Vec<Fact>> {
        let terms = words(q, 1, false, usize::MAX);
        // the keyed path ignores accents and đ/d like FTS does ("ha noi" finds "Hà Nội")
        let folded = plain(&fold(q));
        let needles: Vec<&str> = folded.split(' ').filter(|w| !w.is_empty()).collect();
        if needles.is_empty() {
            return Err(Error::Usage("an empty query".into()));
        }
        let c = self.lock();
        let mut hits: Vec<(f64, Row)> = Vec::new();
        if !terms.is_empty() {
            let mut st = c.prepare(&format!(
                "SELECT {}, -bm25(facts_fts) AS rel FROM facts_fts JOIN facts ON facts.id = facts_fts.rowid
                 WHERE facts_fts MATCH ?1 AND facts.status = 'live'",
                COLS.split(", ").map(|c| format!("facts.{c}")).collect::<Vec<_>>().join(", ")
            ))?;
            let rows = st.query_map([match_query(&terms, true)], |r| Ok((r.get::<_, f64>("rel")?, Row::read(r)?)))?;
            hits = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        }
        // Keyed rows also match on their value and key ("home_city"), which the text may not carry.
        let seen: HashSet<i64> = hits.iter().map(|(_, r)| r.id).collect();
        for r in schema::query(&c, "WHERE status = 'live' AND predicate IS NOT NULL", [])? {
            let hay = plain(&fold(&format!("{} {}", r.value.as_deref().unwrap_or(""), r.key().unwrap_or_default())));
            if !seen.contains(&r.id) && needles.iter().any(|w| hay.contains(w)) {
                hits.push((0.0, r));
            }
        }
        hits.sort_by(|(a, x), (b, y)| {
            b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal).then_with(|| {
                (y.importance.unwrap_or(5), &y.observed_at, y.id).cmp(&(x.importance.unwrap_or(5), &x.observed_at, x.id))
            })
        });
        Ok(hits.iter().take(limit.clamp(1, SEARCH_MAX) as usize).map(|(_, r)| Fact::from(r)).collect())
    }
}
