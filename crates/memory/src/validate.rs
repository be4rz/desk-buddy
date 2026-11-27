//! Validation before reconcile: shape, limits, the registry, the secret filter and the quote check.

use crate::predicates::{self, Predicate};
use crate::secret::looks_secret;
use jiff::civil::Date;
use jiff::Span;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

pub(crate) const TEXT_MAX: usize = 200;
pub(crate) const VALUE_MAX: usize = 100;
const QUALIFIER_MAX: usize = 40;
pub(crate) const KINDS: [&str; 5] = ["profile", "preference", "episode", "promise", "insight"];

/// A validated memory ready for reconcile. The caller sets `authority`, `quote` and `source_turn`.
pub(crate) struct Item {
    pub kind: String,
    pub predicate: Option<&'static Predicate>,
    pub qualifier: Option<String>,
    pub value: Option<String>,
    pub text: String,
    pub importance: i64,
    pub valid_from: Option<String>,
    pub authority: i64,
    pub quote: Option<String>,
    pub source_turn: Option<String>,
}

/// Everything the extractor and `remember` share, before authority.
pub(crate) struct Raw<'a> {
    pub kind: &'a str,
    pub predicate: Option<&'a str>,
    pub qualifier: Option<&'a str>,
    pub value: Option<&'a str>,
    pub text: &'a str,
    pub importance: i64,
    pub valid_from: Option<&'a str>,
}

/// NFC, case-folded, whitespace collapsed: how texts and quotes are compared.
pub(crate) fn fold(s: &str) -> String {
    let nfc: String = s.nfc().collect();
    nfc.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whitespace (newlines included) collapsed to single spaces, so a text stays one line.
pub(crate) fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The quote is the person's own: ≥ 3 chars and a substring of what they said.
pub(crate) fn quote_ok(quote: &str, said: &str) -> bool {
    let q = fold(quote);
    q.chars().count() >= 3 && fold(said).contains(&q)
}

/// `YYYY-MM-DD` or `YYYY-MM` (stored as the 1st), at most a year past `today`.
pub(crate) fn parse_valid_from(s: &str, today: Date) -> Option<String> {
    let s = s.trim();
    let full = match s.len() {
        10 => s.to_string(),
        7 => format!("{s}-01"),
        _ => return None,
    };
    let shape = full.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() });
    let date: Date = full.parse().ok().filter(|_| shape)?;
    let limit = today.checked_add(Span::new().years(1)).ok()?;
    (date <= limit).then(|| date.to_string())
}

/// Episodes below this importance are not worth keeping.
const EPISODE_MIN: i64 = 3;

/// Checks one memory. `Err` carries the reason, a short static word (never the text).
/// `extracted` is true for the extractor's items, the only source of episodes: the bear's own
/// `remember` never writes one, and an insight comes only from a reflection run.
pub(crate) fn check(raw: &Raw, today: Date, extracted: bool) -> Result<Item, &'static str> {
    if !KINDS.contains(&raw.kind) {
        return Err("kind");
    }
    match raw.kind {
        "episode" if !extracted => return Err("episode"),
        "insight" => return Err("kind"),
        _ => {}
    }
    let episode = raw.kind == "episode";
    if episode && raw.predicate.is_some_and(|p| !p.trim().is_empty()) {
        return Err("predicate");
    }
    let text = clean(raw.text);
    if text.is_empty() || text.chars().count() > TEXT_MAX {
        return Err("text");
    }
    let value = raw.value.map(clean).filter(|v| !v.is_empty());
    if value.as_ref().is_some_and(|v| v.chars().count() > VALUE_MAX) {
        return Err("value");
    }
    if !(1..=10).contains(&raw.importance) {
        return Err("importance");
    }
    if episode && raw.importance < EPISODE_MIN {
        return Err("low");
    }
    let valid_from = match raw.valid_from.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(parse_valid_from(s, today).ok_or("valid_from")?),
        None => None,
    };
    if looks_secret(&text) || value.as_deref().is_some_and(looks_secret) || raw.qualifier.as_deref().is_some_and(looks_secret) {
        return Err("secret");
    }

    // Unknown predicate → a keyless note. A promise is never keyed. A keyed row needs its value.
    let predicate = raw
        .predicate
        .and_then(predicates::get)
        .filter(|_| raw.kind != "promise" && value.is_some());
    let qualifier = match predicate.and_then(|p| p.qualifier) {
        Some(_) => raw.qualifier.map(fold).filter(|q| !q.is_empty()),
        None => None,
    };
    if qualifier.as_ref().is_some_and(|q| q.chars().count() > QUALIFIER_MAX) {
        return Err("qualifier");
    }
    Ok(Item {
        // The registry decides the kind of a keyed row, like its cardinality.
        kind: predicate.map_or(raw.kind, |p| p.kind).to_string(),
        value: predicate.and(value),
        predicate,
        qualifier,
        text,
        importance: raw.importance,
        valid_from,
        authority: 1,
        quote: None,
        source_turn: None,
    })
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// A whole number from a model reply (`7` or `7.0`).
pub(crate) fn int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
}

/// One `items[]` entry of a memory.extract reply.
pub(crate) fn from_extract(v: &Value, today: Date) -> Result<Item, &'static str> {
    let importance = v.get("importance").and_then(int).ok_or("importance")?;
    let raw = Raw {
        kind: str_of(v, "kind").ok_or("kind")?,
        predicate: str_of(v, "predicate"),
        qualifier: str_of(v, "qualifier"),
        value: str_of(v, "value"),
        text: str_of(v, "text").ok_or("text")?,
        importance,
        valid_from: str_of(v, "validFrom").or_else(|| str_of(v, "valid_from")),
    };
    check(&raw, today, true)
}
