//! Reflexes (design `## senses`): small rules that turn sense samples into a
//! one-shot intent. The rule names are frozen: `app_switch`, `back_from_idle`,
//! `long_work`, `morning`, `late`. All are off in DND and quiet hours.
//!
//! Fed whole `Senses` samples (the app posts one on every coarse change, at
//! least once a minute from the clock), so time resolution is one sample.
//! State keeps tracking while the rules are off, so DND ending never fires a
//! stale edge.

use crate::brain::{AppCategory, Intent, LocalTime, Senses};

pub const APP_SWITCH: &str = "app_switch";
pub const BACK_FROM_IDLE: &str = "back_from_idle";
pub const LONG_WORK: &str = "long_work";
pub const MORNING: &str = "morning";
pub const LATE: &str = "late";

/// Away this long, then input → `Greet`.
pub const BACK_IDLE_MS: u64 = 5 * 60_000;
/// A break this long resets the work streak.
pub const BREAK_MS: u64 = 10 * 60_000;
/// Input for this long without a break → `Stretch`, at most once per this long.
pub const LONG_WORK_MS: u64 = 2 * 3_600_000;
/// Idle below this counts as "at the keyboard now".
pub const ACTIVE_MS: u64 = 60_000;
/// `morning` fires from this local minute on…
pub const MORNING_MIN: u16 = 6 * 60;
/// …until this one (a launch in the evening is not a morning).
pub const MORNING_UNTIL_MIN: u16 = 12 * 60;
/// `late` runs from 23:00 to 06:00 local.
pub const LATE_FROM_MIN: u16 = 23 * 60;
/// How much `late` adds to sleepiness.
pub const LATE_SLEEPINESS: f64 = 0.2;

/// Dodge chance on switching to a Game/Video/Presentation app.
pub fn dodge_chance(shyness: f64) -> f64 {
    (0.5 + shyness).clamp(0.0, 1.0)
}

/// 23:00–06:00 local.
pub fn is_late(local: LocalTime) -> bool {
    local.minutes >= LATE_FROM_MIN || local.minutes < MORNING_MIN
}

#[derive(Clone, Debug, Default)]
pub struct Reflexes {
    seen: bool,
    prev_cat: Option<AppCategory>,
    prev_idle: u64,
    weekday: u8,
    /// Days observed (bumps when the weekday changes).
    day: u32,
    morning_day: Option<u32>,
    /// When the current input streak started (`None` = on a break).
    streak_start: Option<u64>,
    last_stretch: Option<u64>,
}

/// What a sample asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fired {
    pub rule: &'static str,
    pub intent: Intent,
}

impl Reflexes {
    pub fn new() -> Reflexes {
        Reflexes::default()
    }

    /// One sense sample at `now_ms`. `off`: DND or quiet hours. `free`: no turn,
    /// cue or drag is playing, so a reflex may show. `chance(p)` draws from the
    /// brain's seeded RNG. Edge rules missed while busy are dropped; condition
    /// rules (`morning`, `long_work`) try again on the next sample.
    pub fn sample(&mut self, s: &Senses, now: u64, off: bool, free: bool, shyness: f64, chance: &mut dyn FnMut(f64) -> bool) -> Option<Fired> {
        let idle = s.user_idle_ms;
        let cat = s.frontmost.as_ref().map(|f| f.category);
        let first = !self.seen;
        if first || s.local.weekday != self.weekday {
            if !first {
                self.day += 1;
            }
            self.weekday = s.local.weekday;
        }
        let switched = !first && cat != self.prev_cat;
        let back = !first && self.prev_idle >= BACK_IDLE_MS && idle < self.prev_idle && idle < ACTIVE_MS;
        if idle >= BREAK_MS {
            self.streak_start = None;
        } else if self.streak_start.is_none() {
            self.streak_start = Some(now.saturating_sub(idle));
        }
        self.seen = true;
        self.prev_cat = cat;
        self.prev_idle = idle;
        if off || !free {
            return None;
        }

        if switched && matches!(cat, Some(AppCategory::Game | AppCategory::Video | AppCategory::Presentation)) && chance(dodge_chance(shyness)) {
            return Some(Fired { rule: APP_SWITCH, intent: Intent::Dodge });
        }
        let morning = (MORNING_MIN..MORNING_UNTIL_MIN).contains(&s.local.minutes) && idle < ACTIVE_MS && self.morning_day != Some(self.day);
        if back || morning {
            if morning {
                self.morning_day = Some(self.day);
            }
            return Some(Fired { rule: if back { BACK_FROM_IDLE } else { MORNING }, intent: Intent::Greet });
        }
        let worked = self.streak_start.is_some_and(|st| now.saturating_sub(st) >= LONG_WORK_MS);
        if worked && self.last_stretch.is_none_or(|l| now.saturating_sub(l) >= LONG_WORK_MS) {
            self.last_stretch = Some(now);
            return Some(Fired { rule: LONG_WORK, intent: Intent::Stretch });
        }
        None
    }

    /// `late` is active for this sample (and not off).
    pub fn late(&self, s: &Senses, off: bool) -> bool {
        self.seen && !off && is_late(s.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::FrontApp;

    fn at(minutes: u16, idle: u64) -> Senses {
        Senses { local: LocalTime { minutes, weekday: 0 }, user_idle_ms: idle, ..Senses::default() }
    }

    fn yes(_: f64) -> bool {
        true
    }

    #[test]
    fn app_switch_dodges_only_into_game_video_presentation() {
        let mut r = Reflexes::new();
        let mut s = at(600, 0);
        r.sample(&s, 0, false, true, 0.3, &mut yes);
        for (cat, want) in [(AppCategory::Code, None), (AppCategory::Game, Some(Intent::Dodge)), (AppCategory::Browser, None), (AppCategory::Video, Some(Intent::Dodge))] {
            s.frontmost = Some(FrontApp { bundle_id: "x".into(), category: cat });
            assert_eq!(r.sample(&s, 1000, false, true, 0.3, &mut yes).map(|f| f.intent), want, "{cat:?}");
        }
        // the same category again is not a switch
        assert_eq!(r.sample(&s, 2000, false, true, 0.3, &mut yes), None);
    }

    #[test]
    fn dodge_chance_scales_with_shyness() {
        assert_eq!(dodge_chance(0.0), 0.5);
        assert!((dodge_chance(0.3) - 0.8).abs() < 1e-9);
        assert_eq!(dodge_chance(0.9), 1.0);
    }

    #[test]
    fn morning_greets_once_a_day_after_six() {
        let mut r = Reflexes::new();
        assert_eq!(r.sample(&at(300, 0), 0, false, true, 0.3, &mut yes), None, "05:00");
        let f = r.sample(&at(361, 0), 60_000, false, true, 0.3, &mut yes).unwrap();
        assert_eq!((f.rule, f.intent), (MORNING, Intent::Greet));
        assert_eq!(r.sample(&at(400, 0), 120_000, false, true, 0.3, &mut yes), None);
        let mut evening = Reflexes::new();
        evening.sample(&at(21 * 60, 0), 0, false, true, 0.3, &mut yes);
        assert_eq!(evening.sample(&at(21 * 60 + 1, 0), 1, false, true, 0.3, &mut yes), None, "not a morning");
        let mut next_day = at(420, 0);
        next_day.local.weekday = 1;
        assert_eq!(r.sample(&next_day, 999_000, false, true, 0.3, &mut yes).map(|f| f.rule), Some(MORNING));
    }

    #[test]
    fn busy_or_off_retries_morning_later() {
        let mut r = Reflexes::new();
        assert_eq!(r.sample(&at(400, 0), 0, true, true, 0.3, &mut yes), None, "quiet hours");
        assert_eq!(r.sample(&at(401, 0), 1, false, false, 0.3, &mut yes), None, "busy");
        assert_eq!(r.sample(&at(402, 0), 2, false, true, 0.3, &mut yes).map(|f| f.rule), Some(MORNING));
    }

    #[test]
    fn late_is_23_to_6_and_off_in_quiet_hours() {
        let r = {
            let mut r = Reflexes::new();
            r.sample(&at(0, 0), 0, false, false, 0.3, &mut yes);
            r
        };
        assert!(r.late(&at(23 * 60, 0), false));
        assert!(r.late(&at(5 * 60 + 59, 0), false));
        assert!(!r.late(&at(6 * 60, 0), false));
        assert!(!r.late(&at(23 * 60, 0), true));
        assert!(!Reflexes::new().late(&at(0, 0), false), "no sample yet");
    }
}
