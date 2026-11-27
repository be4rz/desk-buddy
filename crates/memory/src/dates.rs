//! Date words in a message ("yesterday", "hôm qua", "last Friday", "3 days ago", an ISO date) and in
//! the `buddy memory recall` argument, resolved against a caller-given `today`.
//!
//! Matching is on words (runs of letters and digits), lower-cased, with diacritics folded away, so
//! "hôm qua" and "hom qua" are the same words. The first date word in the text wins.

use jiff::civil::{Date, Weekday};
use jiff::ToSpan;
use unicode_normalization::UnicodeNormalization;

/// The days a date word points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Day(Date),
    /// Inclusive, oldest first.
    Range(Date, Date),
}

impl Span {
    /// The first and last day (the same for one day).
    pub fn bounds(&self) -> (Date, Date) {
        match *self {
            Span::Day(d) => (d, d),
            Span::Range(a, b) => (a, b),
        }
    }
}

/// The furthest back `N days ago` goes.
const MAX_AGO: i64 = 365;

struct Word {
    start: usize,
    end: usize,
    /// Lower case, diacritics folded (`đ` → `d`).
    plain: String,
}

/// Letters and digits runs of the NFC lower-cased text.
fn words(lower: &str) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut open: Option<usize> = None;
    for (i, c) in lower.char_indices().chain(std::iter::once((lower.len(), ' '))) {
        match (c.is_alphanumeric(), open) {
            (true, None) => open = Some(i),
            (false, Some(start)) => {
                out.push(Word { start, end: i, plain: plain(&lower[start..i]) });
                open = None;
            }
            _ => {}
        }
    }
    out
}

/// Lower case without accents: "Hôm" → "hom", "đã" → "da".
pub(crate) fn plain(s: &str) -> String {
    s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).map(|c| if c == 'đ' { 'd' } else { c }).collect()
}

fn days_ago(today: Date, n: i64) -> Option<Date> {
    today.checked_sub(n.days()).ok()
}

fn number(w: &str) -> Option<i64> {
    w.bytes().all(|b| b.is_ascii_digit()).then(|| w.parse().ok()).flatten()
}

/// English weekday names, full.
fn en_weekday(w: &str) -> Option<Weekday> {
    Some(match w {
        "monday" => Weekday::Monday,
        "tuesday" => Weekday::Tuesday,
        "wednesday" => Weekday::Wednesday,
        "thursday" => Weekday::Thursday,
        "friday" => Weekday::Friday,
        "saturday" => Weekday::Saturday,
        "sunday" => Weekday::Sunday,
        _ => return None,
    })
}

/// Vietnamese `thứ <n>` (2 = Monday … 7 = Saturday); `n` is a digit or a word (hai, ba, tư, năm, sáu, bảy).
fn vi_weekday(w: &str) -> Option<Weekday> {
    Some(match w {
        "2" | "hai" => Weekday::Monday,
        "3" | "ba" => Weekday::Tuesday,
        "4" | "tu" => Weekday::Wednesday,
        "5" | "nam" => Weekday::Thursday,
        "6" | "sau" => Weekday::Friday,
        "7" | "bay" => Weekday::Saturday,
        _ => return None,
    })
}

/// The most recent `wd` strictly before `today` (a week back when today is `wd`).
fn last_weekday(today: Date, wd: Weekday) -> Option<Date> {
    today.nth_weekday(-1, wd).ok()
}

/// The most recent `wd` on or before `today`.
fn on_weekday(today: Date, wd: Weekday) -> Option<Date> {
    today.tomorrow().ok()?.nth_weekday(-1, wd).ok()
}

/// `wd` in the previous Monday-to-Sunday week.
fn previous_week_weekday(today: Date, wd: Weekday) -> Option<Date> {
    let monday = days_ago(today, today.weekday().to_monday_zero_offset() as i64 + 7)?;
    monday.checked_add((wd.to_monday_zero_offset() as i64).days()).ok()
}

fn last_week(today: Date) -> Option<Span> {
    Some(Span::Range(days_ago(today, 7)?, days_ago(today, 1)?))
}

/// What the words from `i` on say, and how many words that took.
fn at(ws: &[Word], i: usize, text: &str, today: Date) -> Option<Span> {
    let w = |k: usize| ws.get(i + k).map(|w| w.plain.as_str());
    let day = |d: Option<Date>| d.map(Span::Day);
    let ago = |n: &str| number(n).filter(|n| (1..=MAX_AGO).contains(n)).and_then(|n| days_ago(today, n));
    match (w(0)?, w(1), w(2)) {
        ("today", ..) => day(Some(today)),
        ("yesterday", ..) => day(days_ago(today, 1)),
        ("last", Some("week"), _) => last_week(today),
        ("last", Some(d), _) => day(en_weekday(d).and_then(|d| last_weekday(today, d))),
        ("on", Some(d), _) => day(en_weekday(d).and_then(|d| on_weekday(today, d))),
        (n, Some("day" | "days"), Some("ago")) => day(ago(n)),
        ("hom", Some("nay"), _) => day(Some(today)),
        ("hom", Some("qua"), _) => day(days_ago(today, 1)),
        ("hom", Some("kia"), _) => day(days_ago(today, 2)),
        ("tuan", Some("truoc"), _) => last_week(today),
        (n, Some("ngay"), Some("truoc")) => day(ago(n)),
        (t @ ("thu" | "chu"), Some(d), tail) => {
            let wd = if t == "chu" { (d == "nhat").then_some(Weekday::Sunday)? } else { vi_weekday(d)? };
            // "thứ 2 tuần trước": that weekday of the previous week
            if tail == Some("tuan") && w(3) == Some("truoc") {
                day(previous_week_weekday(today, wd))
            } else {
                day(last_weekday(today, wd))
            }
        }
        (y, Some(_), Some(_)) => iso(ws, i, y, text).map(Span::Day),
        _ => None,
    }
}

/// `YYYY-MM-DD`: three digit words joined by single hyphens.
fn iso(ws: &[Word], i: usize, y: &str, text: &str) -> Option<Date> {
    let (m, d) = (ws.get(i + 1)?, ws.get(i + 2)?);
    let sep = |a: &Word, b: &Word| &text[a.end..b.start] == "-";
    let shape = |w: &str, n: usize| w.len() == n && w.bytes().all(|b| b.is_ascii_digit());
    let ok = shape(y, 4) && shape(&m.plain, 2) && shape(&d.plain, 2) && sep(&ws[i], m) && sep(m, d);
    ok.then(|| format!("{y}-{}-{}", m.plain, d.plain).parse().ok()).flatten()
}

/// The first date word in `text`, resolved against `today`.
///
/// English: `today`, `yesterday`, `last week` (the 7 days before today), `last <weekday>` (the most recent
/// one strictly before today: a week back when today is that day), `on <weekday>` (the most recent one on or
/// before today, so today itself when it is that day), `N days ago` (1 to 365) and `YYYY-MM-DD`.
/// Vietnamese: `hôm nay`, `hôm qua`, `hôm kia`, `tuần trước` (last week), `thứ 2` … `thứ 7` or `thứ hai` …
/// `thứ bảy` and `chủ nhật` (the most recent one strictly before today; with `tuần trước` after it, that day
/// of the previous Monday-to-Sunday week) and `N ngày trước`. Diacritics are optional.
pub fn in_message(text: &str, today: Date) -> Option<Span> {
    let lower: String = text.nfc().collect::<String>().to_lowercase();
    let ws = words(&lower);
    (0..ws.len()).find_map(|i| at(&ws, i, &lower, today))
}

/// The `buddy memory recall` argument: `YYYY-MM-DD`, `today`, `yesterday` or `last-week`.
pub fn parse_arg(arg: &str, today: Date) -> Option<Span> {
    let a = arg.trim().to_lowercase();
    match a.as_str() {
        "today" => Some(Span::Day(today)),
        "yesterday" => days_ago(today, 1).map(Span::Day),
        "last-week" => last_week(today),
        _ => {
            let shape = a.len() == 10 && a.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() });
            shape.then(|| a.parse().ok()).flatten().map(Span::Day)
        }
    }
}
