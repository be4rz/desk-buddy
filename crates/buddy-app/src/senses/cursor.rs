//! The cursor sense (design `#senses-cursor`, react-brain p4): where the pointer is relative to
//! the bear, for petting, flinching and following. Read only by the panel's existing near-poll
//! (`NSEvent::mouseLocation` while the cursor is within 160 pt of the panel; no event tap, no
//! new permission).
//!
//! Privacy: a sample is points from the bear's feet, quantised to `QUANT_PT`, and it goes
//! only to the brain (`Inbox::Cursor`): never to the sense ring, `BUDDY_SENSES_LOG`, a prompt,
//! the agent or the disk. The screen position itself never leaves `panel.rs`.

use mascot_core::Cursor;

/// Samples are rounded to this many points.
pub const QUANT_PT: f64 = 8.0;
/// Farther than this from the feet (points) is no cursor at all.
pub const RANGE_PT: f64 = 180.0;
/// At most one sample per this many ms (≤ 10 Hz).
pub const MIN_GAP_MS: u64 = 100;

/// The cursor relative to the bear: `m` is the pointer and `feet` the bear's feet, both in Cocoa
/// global points (y up); `over` = on the bear's opaque pixels. `None` beyond `RANGE_PT`.
pub fn relative(m: (f64, f64), feet: (f64, f64), over: bool) -> Option<Cursor> {
    let q = |v: f64| ((v / QUANT_PT).round() * QUANT_PT) as i32;
    let (dx, dy) = (m.0 - feet.0, m.1 - feet.1);
    if dx.hypot(dy) > RANGE_PT && !over {
        return None;
    }
    Some(Cursor { dx: q(dx), dy: q(dy), over })
}

/// Changes only, at most one per `MIN_GAP_MS`. A held-back change goes out on the next poll
/// tick after the gap; leaving (`None`) is never held back.
#[derive(Debug, Default)]
pub struct Feed {
    sent: Option<Cursor>,
    sent_at: Option<u64>,
}

impl Feed {
    /// The sample to post now, if any (`Some(None)` = the cursor went away).
    pub fn push(&mut self, c: Option<Cursor>, now_ms: u64) -> Option<Option<Cursor>> {
        if c == self.sent {
            return None;
        }
        let soon = self.sent_at.is_some_and(|t| now_ms < t + MIN_GAP_MS);
        if soon && c.is_some() {
            return None;
        }
        self.sent = c;
        self.sent_at = Some(now_ms);
        Some(c)
    }

    /// The poll stopped (the cursor is far): one `None` if a cursor was last sent.
    pub fn leave(&mut self, now_ms: u64) -> Option<Option<Cursor>> {
        self.push(None, now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(dx: i32, dy: i32) -> Option<Cursor> {
        Some(Cursor { dx, dy, over: false })
    }

    #[test]
    fn relative_quantises_and_drops_far_cursors() {
        assert_eq!(relative((103.0, 205.0), (100.0, 200.0), false), c(0, 8));
        assert_eq!(relative((60.0, 197.0), (100.0, 200.0), false), c(-40, 0), "y up, left is negative");
        assert_eq!(relative((300.0, 200.0), (100.0, 200.0), false), None, "200 pt away");
        assert_eq!(relative((100.0, 380.0), (100.0, 200.0), false), c(0, 184), "180 pt is still in range");
        let over = relative((100.0, 260.0), (100.0, 200.0), true).unwrap();
        assert!(over.over && over.dy == 64);
    }

    #[test]
    fn the_feed_sends_changes_only_at_ten_hz_and_one_none_on_leaving() {
        let mut f = Feed::default();
        assert_eq!(f.push(None, 0), None, "nothing to say yet");
        assert_eq!(f.push(c(80, 40), 0), Some(c(80, 40)));
        assert_eq!(f.push(c(80, 40), 33), None, "no change");
        assert_eq!(f.push(c(72, 40), 66), None, "inside the gap");
        assert_eq!(f.push(c(72, 40), 100), Some(c(72, 40)), "held back, sent after the gap");
        // 30 Hz of moving samples for 1 s: at most 10
        let sent = (0..30u64).filter(|i| f.push(c(*i as i32 * 8, 0), 200 + i * 33).is_some()).count();
        assert!((8..=10).contains(&sent), "{sent}");
        // leaving is never held back, and said once
        let t = 200 + 29 * 33 + 1;
        assert_eq!(f.push(None, t), Some(None));
        assert_eq!(f.leave(t + 5), None);
        assert_eq!(f.push(c(8, 8), t + 10), None, "the gap applies after a None too");
        assert_eq!(f.push(c(8, 8), t + 120), Some(c(8, 8)));
        assert_eq!(f.leave(t + 130), Some(None));
    }
}
