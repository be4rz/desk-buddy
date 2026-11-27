//! Date words in both languages, at a fixed Friday: 2026-11-27.
mod common;
use common::date;
use memory::dates::{in_message, parse_arg};
use memory::Span;

fn today() -> jiff::civil::Date {
    date(2026, 11, 27)
}

fn day(m: i8, d: i8) -> Option<Span> {
    Some(Span::Day(date(2026, m, d)))
}

fn found(text: &str) -> Option<Span> {
    in_message(text, today())
}

#[test]
fn today_is_a_friday() {
    assert_eq!(today().weekday(), jiff::civil::Weekday::Friday);
}

#[test]
fn english_day_words() {
    assert_eq!(found("what did we do today?"), day(11, 27));
    assert_eq!(found("What did we talk about YESTERDAY"), day(11, 26));
    assert_eq!(found("5 days ago I was tired"), day(11, 22));
    assert_eq!(found("1 day ago"), day(11, 26));
    assert_eq!(found("365 days ago"), Some(Span::Day(date(2025, 11, 27))));
    assert_eq!(found("0 days ago"), None);
    assert_eq!(found("366 days ago"), None);
}

#[test]
fn english_weekdays() {
    // last <weekday>: strictly before today; a week back when today is that day
    assert_eq!(found("last Friday"), day(11, 20));
    assert_eq!(found("last Thursday"), day(11, 26));
    assert_eq!(found("last monday"), day(11, 23));
    assert_eq!(found("last Saturday"), day(11, 21));
    assert_eq!(found("last sunday"), day(11, 22));
    // on <weekday>: the most recent one, today included
    assert_eq!(found("on Friday"), day(11, 27));
    assert_eq!(found("what did I say on Monday?"), day(11, 23));
    assert_eq!(found("on Saturday"), day(11, 21));
    assert_eq!(found("last blursday"), None);
}

#[test]
fn english_last_week_is_the_seven_days_before_today() {
    assert_eq!(found("last week"), Some(Span::Range(date(2026, 11, 20), date(2026, 11, 26))));
    assert_eq!(found("anything from Last Week?"), Some(Span::Range(date(2026, 11, 20), date(2026, 11, 26))));
}

#[test]
fn iso_dates() {
    assert_eq!(found("on 2026-11-05 we met"), day(11, 5));
    assert_eq!(found("2026-02-28"), day(2, 28));
    assert_eq!(found("2026-02-30"), None, "not a day");
    assert_eq!(found("2026-13-01"), None);
    assert_eq!(found("2026-1-5"), None);
    assert_eq!(found("20261105"), None);
}

#[test]
fn vietnamese_day_words_with_and_without_diacritics() {
    for (text, want) in [
        ("hôm nay mình làm gì", day(11, 27)),
        ("hom nay", day(11, 27)),
        ("Hôm qua mình nói gì nhỉ?", day(11, 26)),
        ("hom qua", day(11, 26)),
        ("HÔM QUA", day(11, 26)),
        ("hôm kia", day(11, 25)),
        ("hom kia", day(11, 25)),
        ("3 ngày trước", day(11, 24)),
        ("3 ngay truoc", day(11, 24)),
        ("1 ngày trước", day(11, 26)),
    ] {
        assert_eq!(found(text), want, "{text}");
    }
    assert_eq!(found("0 ngày trước"), None);
    assert_eq!(found("tuần trước"), Some(Span::Range(date(2026, 11, 20), date(2026, 11, 26))));
    assert_eq!(found("tuan truoc"), Some(Span::Range(date(2026, 11, 20), date(2026, 11, 26))));
}

#[test]
fn vietnamese_weekdays() {
    // thứ 2 = Monday … thứ 7 = Saturday, chủ nhật = Sunday: the most recent one before today
    for (text, want) in [
        ("thứ 2", day(11, 23)),
        ("thứ hai", day(11, 23)),
        ("thứ 3", day(11, 24)),
        ("thứ ba", day(11, 24)),
        ("thứ 4", day(11, 25)),
        ("thứ tư", day(11, 25)),
        ("thứ 5", day(11, 26)),
        ("thứ năm", day(11, 26)),
        ("thứ 6", day(11, 20)), // today is that day: a week back
        ("thứ sáu", day(11, 20)),
        ("thứ 7", day(11, 21)),
        ("thứ bảy", day(11, 21)),
        ("thu bay", day(11, 21)),
        ("thu 7", day(11, 21)),
        ("chủ nhật", day(11, 22)),
        ("chu nhat", day(11, 22)),
        ("Chủ Nhật", day(11, 22)),
    ] {
        assert_eq!(found(text), want, "{text}");
    }
    assert_eq!(found("thứ 9"), None);
}

#[test]
fn vietnamese_weekday_of_last_week_uses_monday_to_sunday_weeks() {
    // this week is Mon 23 Nov .. Sun 29 Nov; last week is Mon 16 .. Sun 22
    for (text, want) in [
        ("thứ 2 tuần trước", day(11, 16)),
        ("thứ hai tuần trước", day(11, 16)),
        ("thứ 6 tuần trước", day(11, 20)),
        ("thu 7 tuan truoc", day(11, 21)),
        ("chủ nhật tuần trước", day(11, 22)),
        ("chu nhat tuan truoc", day(11, 22)),
    ] {
        assert_eq!(found(text), want, "{text}");
    }
}

#[test]
fn the_first_date_word_wins() {
    assert_eq!(found("yesterday and last week"), day(11, 26));
    assert_eq!(found("last week or yesterday"), Some(Span::Range(date(2026, 11, 20), date(2026, 11, 26))));
    assert_eq!(found("hôm qua hay tuần trước"), day(11, 26));
    assert_eq!(found("tuần trước hay hôm qua"), Some(Span::Range(date(2026, 11, 20), date(2026, 11, 26))));
}

#[test]
fn no_date_word_means_none() {
    for text in ["", "I'm nervous about speaking at the meetup again", "todays", "yesterdays news", "what's a good pasta recipe", "mình lo lắng", "last", "thu"] {
        assert_eq!(found(text), None, "{text}");
    }
}

#[test]
fn the_cli_argument_is_exact() {
    let p = |s: &str| parse_arg(s, today());
    assert_eq!(p("2026-11-05"), day(11, 5));
    assert_eq!(p("today"), day(11, 27));
    assert_eq!(p("yesterday"), day(11, 26));
    assert_eq!(p(" Yesterday "), day(11, 26));
    assert_eq!(p("last-week"), Some(Span::Range(date(2026, 11, 20), date(2026, 11, 26))));
    for bad in ["last week", "tomorrow", "2026-13-01", "2026-1-5", "2026-11-05T10:00", "", "hôm qua", "last-friday"] {
        assert_eq!(p(bad), None, "{bad}");
    }
}
