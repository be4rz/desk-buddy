//! Edges' window sensor (design `## widgets` "Edges"): other apps' normal windows, front to back,
//! as bounds + owner pid + layer only. Never window titles, never pixels, nothing stored; no
//! Screen Recording prompt. Polled at 2 Hz only while the bear sits on a window's edge (the
//! driver turns it on and off); a drop reads it once.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use mascot_core::replay::SensesPatch;
use mascot_core::WinRect;

use super::CgWin;
use crate::driver::{post, Inbox};

/// While sitting: one read every half second.
pub const SIT_POLL: Duration = Duration::from_millis(500);

static ACTIVE: Mutex<bool> = Mutex::new(false);
static WAKE: Condvar = Condvar::new();

/// Layer-0 windows not owned by `own`, front to back, in whole points.
pub fn rects(wins: &[CgWin], own: i32) -> Vec<WinRect> {
    wins.iter()
        .filter(|w| w.layer == 0 && w.pid != own && w.w >= 1.0 && w.h >= 1.0)
        .map(|w| WinRect { x: w.x.round() as i32, y: w.y.round() as i32, w: w.w.round() as i32, h: w.h.round() as i32, owner_pid: w.pid, layer: w.layer })
        .collect()
}

/// The windows now (any thread); empty when CGWindowList can't be read.
pub fn read() -> Vec<WinRect> {
    super::read_windows().map(|w| rects(&w, std::process::id() as i32)).unwrap_or_default()
}

/// Poll (sitting) or not (anything else).
pub fn set_active(on: bool) {
    let mut a = ACTIVE.lock().unwrap();
    if *a != on {
        *a = on;
        WAKE.notify_all();
    }
}

/// The poll thread: asleep until `set_active(true)`, then a sample every `SIT_POLL`, posted
/// only when the windows changed.
pub fn start() {
    std::thread::Builder::new()
        .name("buddy-windows".into())
        .spawn(|| {
            let mut prev: Option<Vec<WinRect>> = None;
            loop {
                {
                    let mut a = ACTIVE.lock().unwrap();
                    while !*a {
                        prev = None;
                        a = WAKE.wait(a).unwrap();
                    }
                }
                // a failed read is skipped, not posted as "no windows" (that would unseat the bear)
                let Some(now) = super::read_windows().map(|w| rects(&w, std::process::id() as i32)) else {
                    std::thread::sleep(SIT_POLL);
                    continue;
                };
                if prev.as_ref() != Some(&now) {
                    post(Inbox::Senses(SensesPatch { windows: Some(now.clone()), ..SensesPatch::default() }));
                    prev = Some(now);
                }
                std::thread::sleep(SIT_POLL);
            }
        })
        .expect("spawn windows thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(pid: i32, layer: i32, x: f64, y: f64, ww: f64, h: f64) -> CgWin {
        CgWin { owner: "App".into(), pid, layer, x, y, w: ww, h }
    }

    #[test]
    fn only_other_apps_normal_windows_front_to_back() {
        let wins = [w(9, 0, 10.4, 20.6, 800.0, 600.0), w(1, 0, 0.0, 0.0, 284.0, 198.0), w(9, 25, 0.0, 0.0, 1512.0, 24.0), w(7, 0, 5.0, 5.0, 0.0, 0.0), w(7, 0, 100.0, 50.0, 300.0, 200.0)];
        let r = rects(&wins, 1);
        assert_eq!(r, vec![WinRect { x: 10, y: 21, w: 800, h: 600, owner_pid: 9, layer: 0 }, WinRect { x: 100, y: 50, w: 300, h: 200, owner_pid: 7, layer: 0 }]);
    }
}
