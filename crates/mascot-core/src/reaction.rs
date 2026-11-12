//! Reactions (design `## react-brain`, `#pack-reactions`, `#flow-cursor`, `#flow-drag`): the
//! automatic layer under the agent and over the pools. This module keeps the cursor history and
//! the detectors (follow, flinch, pet), the drag measure and the approval-wait timer; `Brain` turns
//! what they find into reaction cues.
//!
//! The cursor is private: it is never logged, never in the sense ring or a prompt, and never
//! leaves the process (the app feeds it straight to the brain; `Senses.cursor` is not serialized).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::pack::Dir;

pub use mascot_protocol::pack::{ContextDef, ReactionDef, CONTEXTS, REACTIONS};

pub const DRAG_HOLD: &str = "drag_hold";
pub const DRAG_LAND: &str = "drag_land";
pub const DIZZY: &str = "dizzy";
pub const FLINCH: &str = "flinch";
pub const PET: &str = "pet";
pub const FOLLOW: &str = "follow";
pub const MORNING: &str = "morning";
pub const APPROVAL_WAIT: &str = "approval_wait";
pub const EDGE_LAND: &str = "edge_land";
/// Not a trigger: the behavior played when the bear leaves an edge it sat on (`#sb-edge`).
pub const EDGE_STAND: &str = "edge_stand";
/// Reactions that belong to the person's drag (`#priority` row 2): DND and quiet hours keep them.
pub const USER_LAYER: [&str; 3] = [DRAG_LAND, EDGE_LAND, EDGE_STAND];

pub const MUSIC: &str = "music";
pub const EDGE: &str = "edge";
pub const EVENING: &str = "evening";
pub const AWAY: &str = "away";
pub const AWAY_LONG: &str = "away_long";

/// Samples kept (`#senses-cursor`).
pub const HISTORY: usize = 24;
/// Follow re-turns at most this often.
pub const RETURN_MS: u64 = 1000;
/// The app sends at most one sample per this many ms. A longer gap means the cursor rested and
/// then moved within one gap, so speed is measured over at most this.
pub const SAMPLE_GAP_MS: u64 = 100;

/// Where the cursor is, in points from the bear's anchor (y up), quantised to 8 by the app.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub dx: i32,
    pub dy: i32,
    /// Over the bear's opaque pixels.
    pub over: bool,
}

impl Cursor {
    pub fn dist(&self) -> f64 {
        (self.dx as f64).hypot(self.dy as f64)
    }

    /// The 8-way sprite dir toward the cursor: the dir whose screen step (the iso camera's table,
    /// Stage A walk `root_px`, y down) points closest to it. `None` on the anchor.
    pub fn dir(&self) -> Option<Dir> {
        if self.dx == 0 && self.dy == 0 {
            return None;
        }
        let (x, y) = (self.dx as f64, -(self.dy as f64));
        let len = x.hypot(y);
        DIR_TABLE
            .iter()
            .map(|&(d, vx, vy)| (d, (x * vx + y * vy) / (len * vx.hypot(vy))))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(d, _)| d)
    }
}

/// Screen step per frame of each dir (Stage A `walk`, y down): the iso 2:1 camera.
const DIR_TABLE: [(Dir, f64, f64); 8] = [
    (Dir::S, 0.0, 1.4142),
    (Dir::SE, 2.0, 1.0),
    (Dir::E, 2.8284, 0.0),
    (Dir::NE, 2.0, -1.0),
    (Dir::N, 0.0, -1.4142),
    (Dir::NW, -2.0, -1.0),
    (Dir::W, -2.8284, 0.0),
    (Dir::SW, -2.0, 1.0),
];

/// The trigger numbers, from the pack's `reactions` block with the brief's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub flinch_near: f64,
    pub flinch_speed: f64,
    pub pet_reversals: u32,
    pub pet_within_ms: u64,
    pub follow_near: f64,
    pub follow_speed: f64,
    pub follow_still_ms: u64,
}

impl Tuning {
    pub fn from(get: impl Fn(&str) -> Option<ReactionDef>) -> Tuning {
        let f = get(FLINCH).unwrap_or_default();
        let p = get(PET).unwrap_or_default();
        let w = get(FOLLOW).unwrap_or_default();
        Tuning {
            flinch_near: f.near_pt.unwrap_or(60.0),
            flinch_speed: f.speed.unwrap_or(1500.0),
            pet_reversals: p.reversals.unwrap_or(3),
            pet_within_ms: p.within_ms.unwrap_or(2000.0) as u64,
            follow_near: w.near_pt.unwrap_or(120.0),
            follow_speed: w.speed.unwrap_or(200.0),
            follow_still_ms: w.still_ms.unwrap_or(1500.0) as u64,
        }
    }
}

/// The last cursor samples and what they add up to.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    samples: VecDeque<(u64, Cursor)>,
    /// The latest input (`None` = out of range).
    last: Option<Cursor>,
    /// Since when the cursor has stayed in range without a fast sample (follow's "still").
    still_since: Option<u64>,
}

/// What one new sample shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Seen {
    pub pet: bool,
    pub flinch: bool,
}

impl Tracker {
    pub fn last(&self) -> Option<Cursor> {
        self.last
    }

    pub fn still_since(&self) -> Option<u64> {
        self.still_since
    }

    /// A new input at `t` (the app sends only changes). Returns what fired on this sample.
    pub fn push(&mut self, c: Option<Cursor>, t: u64, k: &Tuning, dragging: bool) -> Seen {
        if c == self.last {
            return Seen::default();
        }
        self.last = c;
        let Some(c) = c else {
            self.samples.clear();
            self.still_since = None;
            return Seen::default();
        };
        if dragging {
            // the bear moves with the cursor: no speed, pet or flinch from these, nor after the drop
            self.samples.clear();
            self.still_since = Some(t);
            return Seen::default();
        }
        let prev = self.samples.back().copied();
        self.samples.push_back((t, c));
        while self.samples.len() > HISTORY {
            self.samples.pop_front();
        }
        let speed = prev.map(|(pt, p)| {
            let d = ((c.dx - p.dx) as f64).hypot((c.dy - p.dy) as f64);
            d * 1000.0 / (t.saturating_sub(pt).clamp(1, SAMPLE_GAP_MS) as f64)
        });
        // a first sample (just entered the range) counts as moving
        if speed.is_none_or(|s| s >= k.follow_speed) || self.still_since.is_none() {
            self.still_since = Some(t);
        }
        let flinch = prev.is_some_and(|(_, p)| p.dist() >= k.flinch_near) && c.dist() < k.flinch_near && speed.is_some_and(|s| s >= k.flinch_speed);
        let pet = c.over && self.reversals(t, k.pet_within_ms) >= k.pet_reversals;
        Seen { pet, flinch }
    }

    /// Horizontal direction changes among the samples over the bear within the last `within_ms`.
    pub fn reversals(&self, t: u64, within_ms: u64) -> u32 {
        let xs: Vec<i32> = self.samples.iter().filter(|(st, c)| t.saturating_sub(*st) <= within_ms && c.over).map(|(_, c)| c.dx).collect();
        let mut n = 0;
        let mut sign = 0;
        for w in xs.windows(2) {
            let s = (w[1] - w[0]).signum();
            if s != 0 {
                if sign != 0 && s != sign {
                    n += 1;
                }
                sign = s;
            }
        }
        n
    }

    /// When follow's conditions hold from (near, not over, still long enough); `None` = not now.
    pub fn follow_from(&self, k: &Tuning) -> Option<u64> {
        let c = self.last?;
        if c.over || c.dist() > k.follow_near {
            return None;
        }
        Some(self.still_since? + k.follow_still_ms)
    }
}

/// The drag being measured (`#flow-drag`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragMeasure {
    pub start: u64,
    pub path_pt: f64,
    pub last: Option<[i32; 2]>,
}

impl DragMeasure {
    pub fn new(t: u64, at: Option<[i32; 2]>) -> DragMeasure {
        DragMeasure { start: t, path_pt: 0.0, last: at }
    }

    /// The panel is at `at` now.
    pub fn moved(&mut self, at: Option<[i32; 2]>) {
        if let (Some(a), Some(b)) = (self.last, at) {
            self.path_pt += ((b[0] - a[0]) as f64).hypot((b[1] - a[1]) as f64);
        }
        if at.is_some() {
            self.last = at;
        }
    }

    /// A drag this long or this far ends dizzy.
    pub fn dizzy(&self, t: u64, d: &ReactionDef) -> bool {
        let ms = t.saturating_sub(self.start) as f64;
        ms >= d.min_ms.unwrap_or(1500.0) || self.path_pt >= d.min_pt.unwrap_or(600.0)
    }
}

/// The follow reaction while it plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Following {
    pub until: u64,
    /// The dir it faces the cursor with.
    pub dir: Option<Dir>,
    pub turned_at: u64,
}

/// Nearest dirs of `drawn` to `to` (by 45° steps either way).
pub fn nearest(to: Dir, drawn: &[Dir]) -> Vec<Dir> {
    let steps = |d: Dir| {
        let n = d.ccw_to(to);
        n.min(8 - n)
    };
    let best = drawn.iter().map(|d| steps(*d)).min();
    drawn.iter().copied().filter(|d| Some(steps(*d)) == best).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k() -> Tuning {
        Tuning::from(|_| None)
    }

    fn c(dx: i32, dy: i32, over: bool) -> Option<Cursor> {
        Some(Cursor { dx, dy, over })
    }

    #[test]
    fn dir_uses_the_iso_table() {
        assert_eq!(c(0, -40, false).unwrap().dir(), Some(Dir::S));
        assert_eq!(c(40, 0, false).unwrap().dir(), Some(Dir::E));
        assert_eq!(c(-40, 0, false).unwrap().dir(), Some(Dir::W));
        // iso: SE is 2 right per 1 down
        assert_eq!(c(48, -24, false).unwrap().dir(), Some(Dir::SE));
        assert_eq!(c(-48, 24, false).unwrap().dir(), Some(Dir::NW));
        assert_eq!(c(0, 0, false).unwrap().dir(), None);
    }

    #[test]
    fn pet_needs_three_reversals_over_the_bear_within_the_window() {
        let mut tr = Tracker::default();
        let k = k();
        let xs = [-32, -24, -40, -24, -40];
        let mut seen = Seen::default();
        for (i, x) in xs.iter().enumerate() {
            seen = tr.push(c(*x, 8, true), 1000 + i as u64 * 250, &k, false);
        }
        assert!(seen.pet, "3 reversals in 1 s");
        // not over: no pet
        let mut tr = Tracker::default();
        for (i, x) in xs.iter().enumerate() {
            seen = tr.push(c(*x, 8, false), 1000 + i as u64 * 250, &k, false);
        }
        assert!(!seen.pet);
        // too slow: the window holds fewer reversals
        let mut tr = Tracker::default();
        for (i, x) in xs.iter().enumerate() {
            seen = tr.push(c(*x, 8, true), 1000 + i as u64 * 900, &k, false);
        }
        assert!(!seen.pet);
        // a button held (dragging): no pet
        let mut tr = Tracker::default();
        for (i, x) in xs.iter().enumerate() {
            seen = tr.push(c(*x, 8, true), 1000 + i as u64 * 250, &k, true);
        }
        assert!(!seen.pet);
    }

    #[test]
    fn flinch_on_a_fast_approach_only() {
        let k = k();
        let mut tr = Tracker::default();
        tr.push(c(168, 0, false), 1000, &k, false);
        assert!(!tr.push(c(24, 0, false), 1100, &k, false).flinch, "144 pt in 100 ms = 1440 pt/s");
        let mut tr = Tracker::default();
        tr.push(c(176, 0, false), 1000, &k, false);
        assert!(tr.push(c(16, 0, false), 1100, &k, false).flinch, "160 pt in 100 ms");
        // slow: the app samples a move every 100 ms, so 400 pt/s is 40 pt a sample
        let mut tr = Tracker::default();
        let slow: Vec<bool> = [176, 136, 96, 56, 16].iter().enumerate().map(|(i, x)| tr.push(c(*x, 0, false), 1000 + i as u64 * 100, &k, false).flinch).collect();
        assert!(!slow.contains(&true), "slow");
        // a rest, then one fast sample: speed over at most one sample gap, so it flinches
        let mut tr = Tracker::default();
        tr.push(c(176, 0, false), 1000, &k, false);
        assert!(tr.push(c(16, 0, false), 4000, &k, false).flinch, "rested 3 s, then 160 pt at once");
    }

    #[test]
    fn a_rest_then_a_flick_is_moving_and_drag_samples_are_forgotten() {
        let k = k();
        let mut tr = Tracker::default();
        tr.push(c(100, 0, false), 1000, &k, false);
        tr.push(c(48, 0, false), 4000, &k, false); // 52 pt within one gap after a 3 s rest
        assert_eq!(tr.follow_from(&k), Some(5500), "the flick restarts the still clock");
        // reversals over the bear during a drag do not pet, then or right after the drop
        let mut tr = Tracker::default();
        for (i, x) in [-32, -24, -40, -24].iter().enumerate() {
            tr.push(c(*x, 8, true), 1000 + i as u64 * 150, &k, true);
        }
        assert!(!tr.push(c(-40, 8, true), 1700, &k, false).pet, "drag-time reversals are not kept");
    }

    #[test]
    fn follow_waits_for_a_still_cursor_near() {
        let k = k();
        let mut tr = Tracker::default();
        tr.push(c(80, 0, false), 1000, &k, false);
        assert_eq!(tr.follow_from(&k), Some(2500));
        tr.push(c(88, 0, false), 1100, &k, false); // 80 pt/s: still
        assert_eq!(tr.follow_from(&k), Some(2500));
        tr.push(c(112, 0, false), 1150, &k, false); // 480 pt/s: moving
        assert_eq!(tr.follow_from(&k), Some(2650));
        tr.push(c(144, 0, false), 2000, &k, false);
        assert_eq!(tr.follow_from(&k), None, "beyond 120");
        tr.push(None, 2100, &k, false);
        assert_eq!(tr.follow_from(&k), None);
        assert_eq!(tr.still_since(), None);
    }

    #[test]
    fn drag_measure_and_dizzy() {
        let d = ReactionDef::default();
        let mut m = DragMeasure::new(0, Some([0, 0]));
        m.moved(Some([300, 400]));
        assert_eq!(m.path_pt, 500.0);
        assert!(!m.dizzy(1000, &d));
        m.moved(Some([300, 520]));
        assert!(m.dizzy(1000, &d), "620 pt");
        assert!(DragMeasure::new(0, None).dizzy(2100, &d), "2.1 s");
        assert!(!DragMeasure::new(0, None).dizzy(200, &d));
    }

    #[test]
    fn nearest_drawn_dir() {
        let f5 = [Dir::S, Dir::SE, Dir::E, Dir::W, Dir::SW];
        assert_eq!(nearest(Dir::E, &f5), vec![Dir::E]);
        assert_eq!(nearest(Dir::NE, &f5), vec![Dir::E]);
        assert_eq!(nearest(Dir::N, &f5), vec![Dir::E, Dir::W]);
    }
}
