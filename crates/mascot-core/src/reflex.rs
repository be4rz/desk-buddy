//! Reflexes (design `## senses`): small rules that turn sense samples into a
//! one-shot intent. The rule names are frozen: `app_switch`, `back_from_idle`,
//! `long_work`, `morning`, `late`. All are off in DND and quiet hours.
//!
//! Fed whole `Senses` samples (the app posts one on every coarse change, at
//! least once a minute from the clock), so time resolution is one sample.
//! State keeps tracking while the rules are off, so DND ending never fires a
//! stale edge.

use crate::brain::{AppCategory, Intent, LocalTime, Placement, Senses, WinRect};

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
            // the day's first return in the morning is the morning (p4: it plays the `morning` reaction)
            return Some(Fired { rule: if morning { MORNING } else { BACK_FROM_IDLE }, intent: Intent::Greet });
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

// ---- edges (p5, design `## widgets` "Edges"): the bear sits on window top edges ----
//
// Rects are global points, top-left origin (`WinRect`); `windows` are layer-0 windows that aren't
// ours, front to back. `feet` is the bear's floor anchor inside the panel, in points.

/// Dropped with its feet this close to the front window's top edge, the bear sits on it.
pub const EDGE_SNAP_PT: i32 = 24;
/// It never sits within this of the front window's title-bar centre (it dodges sideways).
pub const TITLE_BAND_PT: i32 = 60;

/// The bear sitting on a window's top edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sit {
    pub pid: i32,
    /// The window as last seen.
    pub win: WinRect,
    /// Where the panel sits now.
    pub at: Placement,
    /// Where the panel was before the drag that sat it, to go back to.
    pub back: Option<Placement>,
}

/// What a sample means for a sitting bear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    Stay,
    /// The window moved or resized: sit here now.
    Move(Sit),
    /// The window closed, minimized or went fullscreen: go back (`None` = stay where it is).
    Leave(Option<Placement>),
}

/// Feet x on `win`'s top edge near `x`: inside the window, out of the title-bar centre band.
fn edge_x(x: i32, win: &WinRect) -> Option<i32> {
    let (l, r) = (win.x, win.x + win.w);
    let x = x.clamp(l, r);
    let cx = win.x + win.w / 2;
    let x = if (x - cx).abs() < TITLE_BAND_PT { if x < cx { cx - TITLE_BAND_PT } else { cx + TITLE_BAND_PT } } else { x };
    (l..=r).contains(&x).then_some(x)
}

/// Where the panel sits on `win` with its feet near `feet_x`.
fn sit_on(win: &WinRect, feet_x: i32, feet: [i32; 2]) -> Option<Placement> {
    let x = edge_x(feet_x, win)?;
    Some(Placement { x: x - feet[0], y: win.y - feet[1], follow_pid: Some(win.owner_pid) })
}

/// The bear was dropped with its panel at `panel`: does it sit on the front window's top edge?
pub fn edge_drop(panel: &WinRect, feet: [i32; 2], windows: &[WinRect]) -> Option<Sit> {
    let front = windows.iter().find(|w| w.layer == 0)?;
    let (fx, fy) = (panel.x + feet[0], panel.y + feet[1]);
    let near = (fy - front.y).abs() <= EDGE_SNAP_PT && (front.x..=front.x + front.w).contains(&fx);
    if !near {
        return None;
    }
    let at = sit_on(front, fx, feet)?;
    Some(Sit { pid: front.owner_pid, win: *front, at, back: None })
}

/// A new windows sample while sitting.
pub fn edge_follow(sit: &Sit, feet: [i32; 2], windows: &[WinRect], fullscreen: bool) -> Follow {
    if fullscreen {
        return Follow::Leave(sit.back);
    }
    let d = |w: &WinRect| (w.x - sit.win.x).abs() + (w.y - sit.win.y).abs() + (w.w - sit.win.w).abs() + (w.h - sit.win.h).abs();
    let Some(win) = windows.iter().filter(|w| w.layer == 0 && w.owner_pid == sit.pid).min_by_key(|w| d(w)) else {
        return Follow::Leave(sit.back);
    };
    if *win == sit.win {
        return Follow::Stay;
    }
    // keep the same spot along the edge
    let feet_x = sit.at.x + feet[0] - sit.win.x + win.x;
    match sit_on(win, feet_x, feet) {
        Some(at) => Follow::Move(Sit { win: *win, at, ..*sit }),
        None => Follow::Leave(sit.back),
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

    // ---- edges ----
    use super::{edge_drop, edge_follow, Follow, EDGE_SNAP_PT, TITLE_BAND_PT};
    use crate::brain::{Placement, WinRect};

    const FEET: [i32; 2] = [140, 150];
    fn win(x: i32, y: i32, w: i32, h: i32, pid: i32) -> WinRect {
        WinRect { x, y, w, h, owner_pid: pid, layer: 0 }
    }
    /// The panel whose feet are at (fx, fy).
    fn panel_at(fx: i32, fy: i32) -> WinRect {
        WinRect { x: fx - FEET[0], y: fy - FEET[1], w: 284, h: 198, owner_pid: 1, layer: 3 }
    }

    #[test]
    fn a_drop_near_the_front_top_edge_sits_on_it_off_the_title_centre() {
        let front = win(100, 200, 800, 600, 42);
        let back = win(0, 100, 1400, 800, 7);
        // feet 10 pt above the edge, left part of the window
        let s = edge_drop(&panel_at(250, 190), FEET, &[front, back]).expect("sits");
        assert_eq!(s.at, Placement { x: 250 - FEET[0], y: 200 - FEET[1], follow_pid: Some(42) });
        assert_eq!((s.pid, s.win), (42, front));
        // just past the snap distance, or beside the window: no
        assert_eq!(edge_drop(&panel_at(250, 200 - EDGE_SNAP_PT - 1), FEET, &[front]), None);
        assert_eq!(edge_drop(&panel_at(250, 200 + EDGE_SNAP_PT + 1), FEET, &[front]), None);
        assert_eq!(edge_drop(&panel_at(950, 200), FEET, &[front]), None);
        // only the front window counts, even when the drop is on another's edge
        assert_eq!(edge_drop(&panel_at(1200, 100), FEET, &[front, back]), None);
        // dropped on the title-bar centre: dodges sideways, to the nearer side
        let cx = 100 + 400;
        let s = edge_drop(&panel_at(cx - 5, 205), FEET, &[front]).unwrap();
        assert_eq!(s.at.x + FEET[0], cx - TITLE_BAND_PT);
        let s = edge_drop(&panel_at(cx + 1, 205), FEET, &[front]).unwrap();
        assert_eq!(s.at.x + FEET[0], cx + TITLE_BAND_PT);
        // a window too narrow to dodge on: no sit
        assert_eq!(edge_drop(&panel_at(150, 200), FEET, &[win(100, 200, 100, 300, 42)]), None);
        // no windows (no sensor, or nothing open)
        assert_eq!(edge_drop(&panel_at(250, 200), FEET, &[]), None);
    }

    #[test]
    fn a_sitting_bear_follows_its_window_and_goes_back_when_it_leaves() {
        let front = win(100, 200, 800, 600, 42);
        let mut s = edge_drop(&panel_at(250, 195), FEET, &[front]).unwrap();
        let back = Placement { x: 10, y: 20, follow_pid: None };
        s.back = Some(back);
        let other = win(0, 0, 300, 300, 9);
        assert_eq!(edge_follow(&s, FEET, &[other, front], false), Follow::Stay);
        // moved by (+50, +30): the bear moves with it, same spot along the edge
        let moved = win(150, 230, 800, 600, 42);
        let Follow::Move(m) = edge_follow(&s, FEET, &[moved], false) else { panic!() };
        assert_eq!(m.at, Placement { x: s.at.x + 50, y: s.at.y + 30, follow_pid: Some(42) });
        assert_eq!((m.win, m.back), (moved, Some(back)));
        // the same app's other window doesn't steal it: the nearest of its windows is ours
        let Follow::Move(m2) = edge_follow(&m, FEET, &[win(900, 900, 200, 200, 42), win(160, 230, 800, 600, 42)], false) else { panic!() };
        assert_eq!(m2.win.x, 160);
        // shrunk under the bear: clamped onto the edge, still off the centre band
        let Follow::Move(m3) = edge_follow(&s, FEET, &[win(100, 200, 300, 600, 42)], false) else { panic!() };
        assert_eq!(m3.at.x + FEET[0], 250 + TITLE_BAND_PT);
        // closed or minimized (not on screen), or fullscreen: back to the pre-sit spot
        assert_eq!(edge_follow(&s, FEET, &[other], false), Follow::Leave(Some(back)));
        assert_eq!(edge_follow(&s, FEET, &[front], true), Follow::Leave(Some(back)));
    }
}
