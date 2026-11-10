//! Screen-share heuristic over CGWindowList owner name, layer and bounds (never the
//! window title, `kCGWindowName`, which needs the Screen Recording permission).
//!
//! While sharing, meeting apps put up chrome no meeting shows otherwise:
//! - a **share border**: a floating (layer > 0) window owned by a meeting app that
//!   frames a whole display (Zoom's green frame, Teams' red frame);
//! - a **share toolbar**: a floating, wide, short strip owned by a meeting app (Zoom's
//!   "You are screen sharing" bar, Teams' sharing bar), docked at a display's top or
//!   bottom; for a browser (Chrome's "meet.google.com is sharing your screen" bar) only
//!   at the bottom, since a browser's tooltips and bubbles sit just under its toolbar.
//!
//! A meeting that is only open (the meeting window at layer 0, a floating video
//! thumbnail, a mini "active speaker" tile) shows neither, so it reads as not sharing.

use super::fullscreen::{covers, Rect};
use super::CgWin;

/// Meeting apps (CGWindowList owner names).
const MEETING: &[&str] = &["zoom.us", "Microsoft Teams", "Microsoft Teams (work or school)", "Webex", "Slack", "Discord", "FaceTime"];
/// Browsers that host web meetings (Meet, Teams web, Zoom web).
const BROWSER: &[&str] = &["Google Chrome", "Google Chrome Canary", "Microsoft Edge", "Arc", "Brave Browser", "Firefox", "Safari", "Vivaldi"];

/// Chrome menus, tooltips and pickers sit at the pop-up menu level and above.
const POPUP_LAYER: i32 = 101;

fn is_toolbar(w: &CgWin, displays: &[Rect], top_too: bool) -> bool {
    let strip = w.w >= 240.0 && w.h >= 20.0 && w.h <= 90.0 && w.w >= 4.0 * w.h;
    // docked within ~1/6 of a display's top or bottom edge, horizontally inside it
    let docked = displays.iter().any(|d| {
        let inside = w.x >= d[0] - 1.0 && w.x + w.w <= d[0] + d[2] + 1.0 && w.y >= d[1] - 1.0 && w.y + w.h <= d[1] + d[3] + 1.0;
        let band = d[3] / 6.0;
        inside && ((top_too && w.y - d[1] <= band) || (d[1] + d[3]) - (w.y + w.h) <= band)
    });
    strip && docked
}

/// Does this window snapshot look like someone is sharing their screen?
pub fn is_sharing(wins: &[CgWin], displays: &[Rect]) -> bool {
    wins.iter().filter(|w| w.layer > 0 && w.layer < POPUP_LAYER).any(|w| {
        let meeting = MEETING.contains(&w.owner.as_str());
        let browser = BROWSER.contains(&w.owner.as_str());
        (meeting && (displays.iter().any(|d| covers(w, d)) || is_toolbar(w, displays, true))) || (browser && is_toolbar(w, displays, false))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREENS: [Rect; 1] = [[0.0, 0.0, 1512.0, 982.0]];

    fn fixture(name: &str) -> Vec<CgWin> {
        let text = match name {
            "zoom_sharing" => include_str!("../../tests/fixtures/cgwindows/zoom_sharing.json"),
            "zoom_open_not_sharing" => include_str!("../../tests/fixtures/cgwindows/zoom_open_not_sharing.json"),
            "meet_in_chrome_sharing" => include_str!("../../tests/fixtures/cgwindows/meet_in_chrome_sharing.json"),
            "chrome_menus_not_sharing" => include_str!("../../tests/fixtures/cgwindows/chrome_menus_not_sharing.json"),
            "teams_sharing" => include_str!("../../tests/fixtures/cgwindows/teams_sharing.json"),
            _ => unreachable!(),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn sharing_fixtures_read_true() {
        for f in ["zoom_sharing", "meet_in_chrome_sharing", "teams_sharing"] {
            assert!(is_sharing(&fixture(f), &SCREENS), "{f}");
        }
    }

    #[test]
    fn zoom_open_but_not_sharing_reads_false() {
        assert!(!is_sharing(&fixture("zoom_open_not_sharing"), &SCREENS));
    }

    #[test]
    fn browser_popups_and_no_windows_read_false() {
        assert!(!is_sharing(&fixture("chrome_menus_not_sharing"), &SCREENS));
        assert!(!is_sharing(&[], &SCREENS));
        // a toolbar-shaped strip with no display to dock to
        assert!(!is_sharing(&fixture("meet_in_chrome_sharing"), &[]));
    }
}
