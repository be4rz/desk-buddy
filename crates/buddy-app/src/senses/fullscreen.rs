//! S1's fullscreen rule (moved from `panel.rs`): the frontmost app shows a normal-layer
//! window whose bounds equal a whole display (menu-bar strip included). That only happens
//! in a fullscreen Space or for borderless-fullscreen games, never for a zoomed window.
//! All rects are CG global coordinates (top-left origin), like CGWindowList bounds.

use super::CgWin;

/// A display in CG global coordinates: x, y, width, height.
pub type Rect = [f64; 4];

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRectC {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRectC;
}

/// Active displays' bounds (any thread). Empty on error.
pub fn displays() -> Vec<Rect> {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) } != 0 {
        return Vec::new();
    }
    ids[..(n as usize).min(ids.len())]
        .iter()
        .map(|&d| {
            let r = unsafe { CGDisplayBounds(d) };
            [r.x, r.y, r.w, r.h]
        })
        .collect()
}

/// Does `w` cover display `d` exactly (within a point)?
pub fn covers(w: &CgWin, d: &Rect) -> bool {
    (w.x - d[0]).abs() < 1.0 && (w.y - d[1]).abs() < 1.0 && (w.w - d[2]).abs() < 1.0 && (w.h - d[3]).abs() < 1.0
}

/// Is app `pid` showing a layer-0 window that covers a whole display?
pub fn is_fullscreen(wins: &[CgWin], pid: i32, displays: &[Rect]) -> bool {
    wins.iter().filter(|w| w.pid == pid && w.layer == 0).any(|w| displays.iter().any(|d| covers(w, d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(pid: i32, layer: i32, x: f64, y: f64, ww: f64, h: f64) -> CgWin {
        CgWin { owner: "App".into(), pid, layer, x, y, w: ww, h }
    }

    const SCREENS: [Rect; 2] = [[0.0, 0.0, 1512.0, 982.0], [1512.0, -200.0, 1920.0, 1080.0]];

    #[test]
    fn covering_a_display_is_fullscreen() {
        assert!(is_fullscreen(&[w(7, 0, 0.0, 0.0, 1512.0, 982.0)], 7, &SCREENS));
        assert!(is_fullscreen(&[w(7, 0, 1512.0, -200.0, 1920.0, 1080.0)], 7, &SCREENS), "second display");
    }

    #[test]
    fn zoomed_other_app_or_overlay_is_not() {
        // zoomed: below the menu bar
        assert!(!is_fullscreen(&[w(7, 0, 0.0, 33.0, 1512.0, 949.0)], 7, &SCREENS));
        // another app's fullscreen window
        assert!(!is_fullscreen(&[w(8, 0, 0.0, 0.0, 1512.0, 982.0)], 7, &SCREENS));
        // a screen-sized overlay at a higher layer (e.g. a share border)
        assert!(!is_fullscreen(&[w(7, 25, 0.0, 0.0, 1512.0, 982.0)], 7, &SCREENS));
        // no displays known
        assert!(!is_fullscreen(&[w(7, 0, 0.0, 0.0, 1512.0, 982.0)], 7, &[]));
        assert!(!is_fullscreen(&[], 7, &SCREENS));
    }

    #[test]
    fn displays_reads() {
        let _ = displays(); // never panics, also with no window server
    }
}
