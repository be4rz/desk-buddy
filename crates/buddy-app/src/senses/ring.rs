//! `senses::Ring`: the last 256 sense changes, in memory only (AC4: sense data is never
//! written). Studio (p6) and the attention budget (p4) may read it.

use std::collections::VecDeque;

use mascot_core::replay::SensesPatch;

pub const CAPACITY: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// app ms (driver clock)
    pub t_ms: u64,
    pub patch: SensesPatch,
}

#[derive(Debug, Default)]
pub struct Ring {
    buf: VecDeque<Entry>,
}

#[allow(dead_code)] // readers arrive with p4 (budget) and p6 (studio)
impl Ring {
    pub fn new() -> Ring {
        Ring { buf: VecDeque::with_capacity(CAPACITY) }
    }

    /// The driver takes a patch's `front_name` out first; the ring drops it too, so no path can
    /// leave an app's display name here.
    pub fn push(&mut self, t_ms: u64, mut patch: SensesPatch) {
        patch.front_name = None;
        if self.buf.len() == CAPACITY {
            self.buf.pop_front();
        }
        self.buf.push_back(Entry { t_ms, patch });
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.buf.iter()
    }

    pub fn last(&self) -> Option<&Entry> {
        self.buf.back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_256() {
        let mut r = Ring::new();
        for t in 0..300u64 {
            r.push(t, SensesPatch { user_idle_ms: Some(t), ..SensesPatch::default() });
        }
        assert_eq!(r.len(), CAPACITY);
        assert_eq!(r.iter().next().unwrap().t_ms, 300 - CAPACITY as u64);
        assert_eq!(r.last().unwrap().t_ms, 299);
    }

    #[test]
    fn never_holds_an_app_name() {
        let mut r = Ring::new();
        r.push(0, serde_json::from_str(r#"{"front_name": "Secret Notes", "user_idle_ms": 3}"#).unwrap());
        let e = r.last().unwrap();
        assert_eq!((e.patch.front_name.clone(), e.patch.user_idle_ms), (None, Some(3)));
        assert!(!format!("{:?}", r).contains("Secret"));
    }
}
