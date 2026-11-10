//! The attention budget (design `## accounts`): how many unprompted
//! (sense-triggered) turns the bear may start in a rolling hour. None while
//! `Brain::unprompted_allowed` is false (DND, quiet hours). Turns the person
//! starts never count. In memory only: a restart resets it (§3.1).

use std::collections::VecDeque;

/// The rolling window.
pub const WINDOW_MS: u64 = 60 * 60_000;

#[derive(Clone, Debug)]
pub struct Budget {
    per_hour: u32,
    /// When each turn in the window was taken, oldest first.
    window: VecDeque<u64>,
}

impl Budget {
    pub fn new(per_hour: u32) -> Budget {
        Budget { per_hour, window: VecDeque::new() }
    }

    fn expire(&mut self, now_ms: u64) {
        while self.window.front().is_some_and(|&t| now_ms.saturating_sub(t) >= WINDOW_MS) {
            self.window.pop_front();
        }
    }

    /// Take one turn at `now_ms` if `allowed` and the hour has room.
    pub fn try_take(&mut self, now_ms: u64, allowed: bool) -> bool {
        self.expire(now_ms);
        if !allowed || self.window.len() as u32 >= self.per_hour {
            return false;
        }
        self.window.push_back(now_ms);
        true
    }

    /// Turns taken in the 60 minutes before `now_ms`.
    pub fn used(&self, now_ms: u64) -> u32 {
        self.window.iter().filter(|&&t| now_ms.saturating_sub(t) < WINDOW_MS).count() as u32
    }

    pub fn per_hour(&self) -> u32 {
        self.per_hour
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    #[test]
    fn four_an_hour_then_refused_and_the_fifth_after_sixty_minutes() {
        let mut b = Budget::new(4);
        for i in 0..4 {
            assert!(b.try_take(1_000 + i * 10 * MIN, true), "turn {i}");
        }
        assert_eq!(b.used(31 * MIN), 4);
        assert!(!b.try_take(40 * MIN, true), "a 5th in the hour is refused");
        assert_eq!(b.used(40 * MIN), 4, "a refusal takes nothing");
        // the first turn (t=1s) leaves the window at 60 min + 1 s
        assert!(!b.try_take(60 * MIN, true));
        assert!(b.try_take(60 * MIN + 1_000, true), "allowed again once the oldest is an hour old");
        assert_eq!(b.used(60 * MIN + 1_000), 4);
        assert_eq!(b.per_hour(), 4);
    }

    #[test]
    fn none_while_not_allowed_and_zero_per_hour_means_never() {
        let mut b = Budget::new(4);
        assert!(!b.try_take(0, false), "DND / quiet hours");
        assert_eq!(b.used(0), 0);
        assert!(b.try_take(1, true));
        let mut z = Budget::new(0);
        assert!(!z.try_take(0, true));
    }
}
