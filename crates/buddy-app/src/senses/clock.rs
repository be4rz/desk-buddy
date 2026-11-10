//! Local time of day (each poll; the brain only hears about minute changes) and the
//! quiet-hours window.

use mascot_core::LocalTime;

/// Local wall clock now (`localtime_r`): minutes since midnight, weekday 0 = Monday.
pub fn now() -> LocalTime {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return LocalTime::default();
    }
    LocalTime { minutes: (tm.tm_hour * 60 + tm.tm_min).clamp(0, 1439) as u16, weekday: ((tm.tm_wday + 6) % 7) as u8 }
}

/// `"HH:MM"` → minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u16, u16) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Is `minutes` inside `[start, end)`, wrapping past midnight when `start > end`?
/// An unparsable bound or `start == end` is an empty window (never quiet).
pub fn in_quiet_hours(start: &str, end: &str, minutes: u16) -> bool {
    let (Some(s), Some(e)) = (parse_hhmm(start), parse_hhmm(end)) else { return false };
    if s <= e {
        (s..e).contains(&minutes)
    } else {
        minutes >= s || minutes < e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_across_midnight() {
        let q = |m| in_quiet_hours("22:30", "07:30", m);
        assert!(!q(22 * 60 + 29));
        assert!(q(22 * 60 + 30));
        assert!(q(23 * 60 + 59));
        assert!(q(0));
        assert!(q(7 * 60 + 29));
        assert!(!q(7 * 60 + 30));
        assert!(!q(12 * 60));
    }

    #[test]
    fn quiet_hours_same_day_and_degenerate() {
        assert!(in_quiet_hours("13:00", "14:00", 13 * 60 + 30));
        assert!(!in_quiet_hours("13:00", "14:00", 14 * 60));
        assert!(!in_quiet_hours("13:00", "13:00", 13 * 60), "empty window");
        assert!(!in_quiet_hours("25:00", "07:00", 60), "bad time");
        assert!(!in_quiet_hours("late", "07:00", 60));
    }

    #[test]
    fn now_is_in_range() {
        let t = now();
        assert!(t.minutes < 1440 && t.weekday < 7);
    }
}
