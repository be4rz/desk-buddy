//! Senses (design `## senses`, AC4): what the bear notices about the desktop, in memory
//! only. A `buddy-senses` thread polls idle time, the local clock and one CGWindowList
//! snapshot every 2 s (fullscreen front app, screen share); an NSWorkspace activation
//! observer on the main thread reports the frontmost app at once. Each posts a raw
//! `SensesPatch` into the driver's inbox only when a coarse value changes; the driver
//! gates it by settings (`gate`) and hands the brain whole `Senses`.
//!
//! Nothing here touches the disk; no new permission prompts (no window titles, no
//! screen capture, no event tap).
//!
//! `BUDDY_SENSES=0` turns the sensors off (headless replays); `BUDDY_SENSES_LOG=1` prints
//! every sense change to stderr.

pub mod clock;
pub mod cursor;
pub mod frontmost;
pub mod fullscreen;
pub mod idle;
pub mod ring;
pub mod share;
pub mod windows;

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;

use block2::RcBlock;
use mascot_core::replay::SensesPatch;
use mascot_core::{LocalTime, Senses};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification, NSWorkspaceDidActivateApplicationNotification};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{ns_string, NSArray, NSDictionary, NSNotification, NSNumber, NSOperationQueue, NSString};

pub use ring::Ring;

use crate::driver::{post, Inbox};
use crate::settings::Settings;

/// How often the sensor thread samples (the brief caps poll intervals at 2 s).
pub const POLL: Duration = Duration::from_secs(2);

/// pid of the frontmost app, kept by the main-thread observer for the poll thread.
static FRONT_PID: AtomicI32 = AtomicI32::new(-1);

/// One on-screen window from CGWindowList (owner name, never the window title).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct CgWin {
    pub owner: String,
    pub pid: i32,
    pub layer: i32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The on-screen windows, or `None` when CGWindowList returns null (no window server
/// access): callers treat that as "not fullscreen, not sharing". Logged once.
pub fn read_windows() -> Option<Vec<CgWin>> {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    let Some(list) = CGWindowListCopyWindowInfo(CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements, 0) else {
        if first_time(&LOGGED) {
            eprintln!("[buddy] senses: CGWindowList returned null; treating as not fullscreen / not sharing");
        }
        return None;
    };
    // CFArray of CFDictionary is toll-free bridged to NSArray of NSDictionary
    let arr: &NSArray = unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const NSArray) };
    Some(arr.iter().filter_map(|item| item.downcast::<NSDictionary>().ok().and_then(|d| win(&d))).collect())
}

/// True only on the first call for `flag` (log-once).
fn first_time(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::Relaxed)
}

fn num(d: &NSDictionary, k: &NSString) -> Option<f64> {
    let k: &AnyObject = k;
    d.objectForKey(k).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue())
}

fn win(d: &NSDictionary) -> Option<CgWin> {
    let key: &AnyObject = ns_string!("kCGWindowBounds");
    let b = d.objectForKey(key).and_then(|o| o.downcast::<NSDictionary>().ok())?;
    let okey: &AnyObject = ns_string!("kCGWindowOwnerName");
    let owner = d.objectForKey(okey).and_then(|o| o.downcast::<NSString>().ok()).map(|s| s.to_string()).unwrap_or_default();
    Some(CgWin {
        owner,
        pid: num(d, ns_string!("kCGWindowOwnerPID"))? as i32,
        layer: num(d, ns_string!("kCGWindowLayer"))? as i32,
        x: num(&b, ns_string!("X"))?,
        y: num(&b, ns_string!("Y"))?,
        w: num(&b, ns_string!("Width"))?,
        h: num(&b, ns_string!("Height"))?,
    })
}

/// The senses the brain sees: raw senses gated by settings and the session's DND toggle.
/// Quiet hours need a clock sample first (the default local time is 00:00). The sensors
/// never set raw `dnd_toggle` / `quiet_hours`; a `BUDDY_EVENTS` replay may, and wins.
pub fn gate(raw: &Senses, settings: &Settings, dnd_toggle: bool, have_clock: bool) -> Senses {
    let q = &settings.quiet_hours;
    Senses {
        fullscreen: raw.fullscreen && settings.dnd.fullscreen,
        screen_share: raw.screen_share && settings.dnd.screen_share,
        dnd_toggle: dnd_toggle || raw.dnd_toggle,
        quiet_hours: raw.quiet_hours || (have_clock && q.enabled && clock::in_quiet_hours(&q.start, &q.end, raw.local.minutes)),
        ..raw.clone()
    }
}

/// What the raw sample changed since `prev`, at the coarse grain the brain needs.
fn diff(prev: Option<&Sample>, now: &Sample) -> SensesPatch {
    let mut p = SensesPatch::default();
    let changed = |f: fn(&Sample) -> u64| prev.is_none_or(|o| f(o) != f(now));
    if changed(|s| idle::bucket(s.idle_ms)) || changed(|s| s.local.minutes as u64) {
        p.user_idle_ms = Some(now.idle_ms);
    }
    if prev.is_none_or(|o| o.local != now.local) {
        p.local = Some(now.local);
    }
    if prev.is_none_or(|o| o.month != now.month) {
        p.month = Some(now.month);
    }
    if prev.is_none_or(|o| o.fullscreen != now.fullscreen) {
        p.fullscreen = Some(now.fullscreen);
    }
    if prev.is_none_or(|o| o.share != now.share) {
        p.screen_share = Some(now.share);
    }
    p
}

#[derive(Clone, Debug, PartialEq)]
struct Sample {
    idle_ms: u64,
    local: LocalTime,
    /// 1–12 (pack `when.months`)
    month: u8,
    fullscreen: bool,
    share: bool,
}

/// (fullscreen, sharing) for one snapshot; `None` (CGWindowList null) is (false, false).
fn judge(wins: Option<&[CgWin]>, front_pid: i32, screens: &[fullscreen::Rect]) -> (bool, bool) {
    let Some(w) = wins else { return (false, false) };
    let ours = front_pid == std::process::id() as i32;
    (front_pid > 0 && !ours && fullscreen::is_fullscreen(w, front_pid, screens), share::is_sharing(w, screens))
}

fn sample() -> Sample {
    let wins = read_windows();
    let screens = if wins.is_some() { fullscreen::displays() } else { Vec::new() };
    let (fullscreen, share) = judge(wins.as_deref(), FRONT_PID.load(Ordering::Relaxed), &screens);
    Sample { idle_ms: idle::idle_ms(), local: clock::now(), month: month_now(), fullscreen, share }
}

/// The local month now, 1–12 (`localtime_r`); 0 = unknown (no `when.months` entry plays).
fn month_now() -> u8 {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    (tm.tm_mon + 1).clamp(1, 12) as u8
}

/// The sensors run (`start`): the panel feeds the cursor sense only then, so `BUDDY_SENSES=0` and a
/// `BUDDY_EVENTS` replay see no real cursor.
static SENSORS_ON: AtomicBool = AtomicBool::new(false);

pub fn cursor_on() -> bool {
    SENSORS_ON.load(Ordering::Relaxed)
}

/// Starts the sensors (main thread): the first sample at once (so the brain never sees the
/// default 00:00 or month 0), the frontmost observer, then the poll thread.
pub fn start() {
    SENSORS_ON.store(true, Ordering::Relaxed);
    windows::start();
    let first = sample();
    crate::driver::with_app(|a| a.handle(Inbox::Senses(diff(None, &first))));
    let (nudge, rx) = channel::<()>();
    observe_frontmost(nudge);
    std::thread::Builder::new()
        .name("buddy-senses".into())
        .spawn(move || {
            let mut prev: Option<Sample> = Some(first);
            loop {
                let s = sample();
                let p = diff(prev.as_ref(), &s);
                if p != SensesPatch::default() {
                    post(Inbox::Senses(p));
                }
                prev = Some(s);
                match rx.recv_timeout(POLL) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("spawn senses thread");
}

/// The frontmost app now (main thread), and the pid the poll thread uses.
fn report_frontmost() {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication();
    let front = app.as_ref().map(|a| {
        let id = a.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
        (a.processIdentifier(), frontmost::front_app(&id))
    });
    // our own panel never activates (non-activating), but a menu click can: keep the
    // app the user was in, for the fullscreen check and the reflexes
    if front.as_ref().is_some_and(|(pid, _)| *pid == std::process::id() as i32) {
        return;
    }
    FRONT_PID.store(front.as_ref().map_or(-1, |(pid, _)| *pid), Ordering::Relaxed);
    // with the clock, so no sample (reflexes, `late`) ever sees the default 00:00
    post(Inbox::Senses(SensesPatch { frontmost: Some(front.map(|(_, f)| f)), local: Some(clock::now()), ..SensesPatch::default() }));
}

fn observe_frontmost(nudge: Sender<()>) {
    report_frontmost();
    let wc = NSWorkspace::sharedWorkspace().notificationCenter();
    for name in unsafe { [NSWorkspaceDidActivateApplicationNotification, NSWorkspaceActiveSpaceDidChangeNotification] } {
        let nudge = nudge.clone();
        let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
            report_frontmost();
            // re-sample fullscreen / share now instead of at the next poll
            let _ = nudge.send(());
        });
        let token: Retained<_> = unsafe { wc.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&NSOperationQueue::mainQueue()), &block) };
        std::mem::forget(token); // observers live as long as the app
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(idle_ms: u64, minutes: u16) -> Sample {
        Sample { idle_ms, local: LocalTime { minutes, weekday: 0 }, month: 11, fullscreen: false, share: false }
    }

    #[test]
    fn first_sample_reports_everything() {
        let p = diff(None, &s(0, 600));
        assert_eq!((p.user_idle_ms, p.local.map(|l| l.minutes), p.fullscreen, p.screen_share), (Some(0), Some(600), Some(false), Some(false)));
        assert_eq!(p.month, Some(11), "the month from the first sample (pack when.months)");
    }

    #[test]
    fn the_month_is_posted_when_it_changes() {
        assert_eq!(diff(Some(&s(0, 600)), &s(0, 601)).month, None);
        let mut dec = s(0, 0);
        dec.month = 12;
        assert_eq!(diff(Some(&s(0, 1439)), &dec).month, Some(12));
        assert!((1..=12).contains(&month_now()));
        assert!((1..=12).contains(&sample().month));
    }

    #[test]
    fn steady_activity_posts_nothing_until_the_minute_changes() {
        assert_eq!(diff(Some(&s(0, 600)), &s(1500, 600)), SensesPatch::default());
        assert_eq!(diff(Some(&s(0, 600)), &s(59_000, 600)), SensesPatch::default());
        let p = diff(Some(&s(0, 600)), &s(1000, 601));
        assert_eq!((p.user_idle_ms, p.local.map(|l| l.minutes)), (Some(1000), Some(601)));
    }

    #[test]
    fn going_idle_and_coming_back_are_posted() {
        assert_eq!(diff(Some(&s(59_000, 600)), &s(61_000, 600)).user_idle_ms, Some(61_000));
        assert_eq!(diff(Some(&s(400_000, 600)), &s(300, 600)).user_idle_ms, Some(300));
        let mut fs = s(0, 600);
        fs.fullscreen = true;
        assert_eq!(diff(Some(&s(0, 600)), &fs).fullscreen, Some(true));
    }

    #[test]
    fn a_null_window_list_is_not_fullscreen_nor_sharing_and_logs_once() {
        let screen = [[0.0, 0.0, 1512.0, 982.0]];
        assert_eq!(judge(None, 7, &screen), (false, false));
        let full = [CgWin { owner: "zoom.us".into(), pid: 7, layer: 0, x: 0.0, y: 0.0, w: 1512.0, h: 982.0 }];
        assert_eq!(judge(Some(&full), 7, &screen), (true, false));
        assert_eq!(judge(Some(&full), -1, &screen), (false, false), "no frontmost app");
        assert_eq!(judge(Some(&full), std::process::id() as i32, &screen), (false, false), "our own panel");
        let flag = AtomicBool::new(false);
        assert!(first_time(&flag));
        assert!(!first_time(&flag) && !first_time(&flag));
        // the real call never panics, with or without a window server
        let _ = read_windows();
        let _ = sample();
    }

    #[test]
    fn gate_applies_settings_and_toggle() {
        let mut st = Settings::default();
        let raw = Senses { fullscreen: true, screen_share: true, local: LocalTime { minutes: 23 * 60, weekday: 0 }, ..Senses::default() };
        let g = gate(&raw, &st, false, true);
        assert!(g.fullscreen && g.screen_share && g.quiet_hours && !g.dnd_toggle);
        st.dnd.fullscreen = false;
        st.dnd.screen_share = false;
        st.quiet_hours.enabled = false;
        let g = gate(&raw, &st, true, true);
        assert!(!g.fullscreen && !g.screen_share && !g.quiet_hours && g.dnd_toggle);
        st.quiet_hours.enabled = true;
        assert!(!gate(&raw, &st, false, false).quiet_hours, "no clock sample yet");
        // a replay's own DND / quiet-hours lines reach the brain
        let replayed = Senses { dnd_toggle: true, quiet_hours: true, ..Senses::default() };
        let g = gate(&replayed, &st, false, false);
        assert!(g.dnd_toggle && g.quiet_hours);
    }
}
