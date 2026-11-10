//! Time since the last user input (any device, combined session state). Needs no
//! permission prompt: it is a counter, not an event tap.

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
}

/// kCGEventSourceStateCombinedSessionState
const COMBINED_SESSION: i32 = 0;
/// kCGAnyInputEventType
const ANY_INPUT: u32 = !0;

pub fn idle_ms() -> u64 {
    let s = unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION, ANY_INPUT) };
    if s.is_finite() && s > 0.0 {
        (s * 1000.0) as u64
    } else {
        0
    }
}

/// The grain the brain hears about: 0 while active (< 1 min), else whole minutes away.
pub fn bucket(idle_ms: u64) -> u64 {
    if idle_ms < 60_000 {
        0
    } else {
        idle_ms / 60_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets() {
        assert_eq!(bucket(0), 0);
        assert_eq!(bucket(59_999), 0);
        assert_eq!(bucket(60_000), 1);
        assert_eq!(bucket(359_000), 5);
    }

    #[test]
    fn idle_reads() {
        let _ = idle_ms(); // a number, never a panic, also with no window server
    }
}
