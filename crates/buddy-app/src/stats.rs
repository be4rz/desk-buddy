//! `BUDDY_STATS=1`: every 10 s, `[buddy] fps <uploaded/s> cpu <x.x>% frames <n> (10 s)`,
//! where cpu is this process's user+system time over wall time (getrusage).

use std::time::{Duration, Instant};

pub const PERIOD: Duration = Duration::from_secs(10);

pub struct Stats {
    t0: Instant,
    cpu0: Duration,
    frames: u64,
    skipped: u64,
    wakes: u64,
}

pub fn cpu_time() -> Duration {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage writes into the zeroed struct we own.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
        return Duration::ZERO;
    }
    let tv = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1000);
    tv(ru.ru_utime) + tv(ru.ru_stime)
}

impl Stats {
    pub fn new() -> Stats {
        Stats { t0: Instant::now(), cpu0: cpu_time(), frames: 0, skipped: 0, wakes: 0 }
    }

    pub fn frame(&mut self) {
        self.frames += 1;
    }

    /// A composed frame identical to the one shown (not uploaded).
    pub fn skip(&mut self) {
        self.skipped += 1;
    }

    pub fn wake(&mut self) {
        self.wakes += 1;
    }

    /// The line for the period just ended, and a reset.
    pub fn line(&mut self) -> String {
        let dt = self.t0.elapsed().as_secs_f64().max(1e-3);
        let cpu = cpu_time();
        let pct = (cpu.saturating_sub(self.cpu0)).as_secs_f64() / dt * 100.0;
        let s = format!(
            "[buddy] fps {:.1} cpu {:.1}% frames {} (+{} identical) wakes {} ({:.0} s)",
            self.frames as f64 / dt,
            pct,
            self.frames,
            self.skipped,
            self.wakes,
            dt
        );
        *self = Stats { t0: Instant::now(), cpu0: cpu, frames: 0, skipped: 0, wakes: 0 };
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_has_the_stats_shape_and_resets() {
        let mut s = Stats::new();
        s.frame();
        s.frame();
        s.wake();
        let l = s.line();
        assert!(l.starts_with("[buddy] fps "), "{l}");
        assert!(l.contains(" cpu ") && l.contains("% frames 2 (+0 identical) wakes 1 ("), "{l}");
        assert!(s.line().contains("frames 0"));
    }
}
