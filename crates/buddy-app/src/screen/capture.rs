//! The capture side of screen look: a `Capture` trait, its error type and the fakes the tests (and
//! debug builds' `BUDDY_SCREEN_FAKE=1`) use. Nothing here touches the disk or the log.
//!
//! `RealCapture` is the ScreenCaptureKit implementation (macOS 14+, `SCScreenshotManager`); its tests
//! never call `grab` (that would capture the person's real screen).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use objc2_foundation::NSError;
use objc2_screen_capture_kit::SCShareableContent;

use crate::senses::fullscreen::Rect;

/// Whether desk-buddy may record the screen (macOS Screen Recording).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Granted,
    Missing,
}

/// What to grab.
#[derive(Clone, Debug, PartialEq)]
pub struct GrabReq {
    /// The display the bear is on (CG global rect); `None`: the main display.
    pub display: Option<Rect>,
    /// Bundle ids of the running apps on the deny list: their windows are left out of the picture
    /// (the implementation adds desk-buddy itself). The front app is never among them (it would
    /// have been refused first).
    pub exclude_bundles: Vec<String>,
    /// The longest edge of the frame, in pixels (`look::OCR_MAX_EDGE`).
    pub max_edge: u32,
}

/// One captured picture: BGRA, row by row, `width * height * 4` bytes, no padding.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CaptureError {
    /// The bear's display is gone and no other can be found.
    NoDisplay,
    /// Anything else, in a short sentence (never a path).
    Failed(String),
}

pub trait Capture: Send + Sync {
    /// A cheap check; never asks the person (`buddy screen look` never prompts).
    fn permission(&self) -> Permission;
    /// One picture of the display. Called on the host thread, once per look.
    fn grab(&self, req: &GrabReq) -> Result<Frame, CaptureError>;
}

/// The real capture: ScreenCaptureKit's one-shot screenshot of the bear's display, with the deny-listed
/// apps and desk-buddy itself left out of the picture. Pixels live in memory only.
pub struct RealCapture;

/// How long `grab` waits for ScreenCaptureKit's two callbacks (the display list, then the picture).
const GRAB_TIMEOUT: Duration = Duration::from_secs(5);

impl Capture for RealCapture {
    /// `CGPreflightScreenCaptureAccess`: never prompts.
    fn permission(&self) -> Permission {
        if objc2_core_graphics::CGPreflightScreenCaptureAccess() {
            Permission::Granted
        } else {
            Permission::Missing
        }
    }

    /// Runs on the host's thread. ScreenCaptureKit answers on its own queues; both answers are bridged
    /// to this thread with a channel and a timeout, so nothing here can block the main thread.
    fn grab(&self, req: &GrabReq) -> Result<Frame, CaptureError> {
        // `SCScreenshotManager` is macOS 14+: look the class up at run time so older systems get a sentence.
        if objc2::runtime::AnyClass::get(c"SCScreenshotManager").is_none() {
            return Err(CaptureError::Failed("Screen look needs macOS 14 or later.".into()));
        }
        let (tx, rx) = mpsc::channel::<Result<Frame, CaptureError>>();
        let req = req.clone();
        let on_content = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
            // SAFETY: the framework passes a live object or null for the duration of the call.
            match unsafe { content.as_ref() } {
                Some(content) => sck::start_screenshot(content, &req, &tx),
                None => {
                    let _ = tx.send(Err(CaptureError::Failed(sck::error_text(err, "Couldn’t list the displays."))));
                }
            }
        });
        // SAFETY: the block signature is the documented one; the framework copies the block.
        unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&on_content) };
        match rx.recv_timeout(GRAB_TIMEOUT) {
            Ok(r) => r,
            Err(_) => Err(CaptureError::Failed("The screen capture took too long.".into())),
        }
    }
}

/// The ScreenCaptureKit calls, run inside its completion handlers.
mod sck {
    use std::sync::mpsc::Sender;

    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2_core_graphics::{CGDisplayCopyDisplayMode, CGDisplayMode, CGImage, CGMainDisplayID};
    use objc2_foundation::{NSArray, NSError};
    use objc2_screen_capture_kit::{SCContentFilter, SCDisplay, SCRunningApplication, SCScreenshotManager, SCStreamConfiguration, SCWindow};

    use super::*;
    use crate::screen::cg;

    /// What ScreenCaptureKit's error code means to the person; never the system's text, which can
    /// carry paths or app names.
    pub fn error_text(err: *mut NSError, what: &str) -> String {
        // SAFETY: a live NSError or null.
        match unsafe { err.as_ref() }.map(|e| e.code()) {
            Some(-3801) => "desk-buddy doesn’t have Screen Recording permission. Open System Settings ▸ Privacy & Security ▸ Screen Recording, allow desk-buddy, then restart it.".into(),
            Some(code) => format!("{what} (error {code})"),
            None => what.to_string(),
        }
    }

    /// With the display list in hand: pick the display, build the filter, ask for the picture. The
    /// picture's own callback reads it into a `Frame` and sends it.
    pub fn start_screenshot(content: &SCShareableContent, req: &GrabReq, tx: &Sender<Result<Frame, CaptureError>>) {
        // SAFETY (this whole fn): ScreenCaptureKit objects are used as documented, on the framework's queue.
        let displays = unsafe { content.displays() };
        let list: Vec<(u32, Rect)> = displays.iter().map(|d| (unsafe { d.displayID() }, rect_of(&d))).collect();
        let rects: Vec<Rect> = list.iter().map(|(_, r)| *r).collect();
        let Some(i) = pick_display(&rects, req.display, list.iter().position(|(id, _)| *id == CGMainDisplayID())) else {
            let _ = tx.send(Err(CaptureError::NoDisplay));
            return;
        };
        let display: Retained<SCDisplay> = displays.objectAtIndex(i);

        let me = unsafe { libc::getpid() };
        let apps: Vec<Retained<SCRunningApplication>> = unsafe { content.applications() }
            .iter()
            .filter(|a| unsafe { a.processID() } == me || req.exclude_bundles.iter().any(|b| *b == unsafe { a.bundleIdentifier() }.to_string()))
            .collect();
        let apps: Retained<NSArray<SCRunningApplication>> = NSArray::from_retained_slice(&apps);
        let windows: Retained<NSArray<SCWindow>> = NSArray::new();
        let filter = unsafe { SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(SCContentFilter::alloc(), &display, &apps, &windows) };

        // the display's native pixels (a Retina display has twice its point size), capped at `max_edge`
        let (pw, ph) = native_pixels(list[i].0, &list[i].1);
        let (w, h) = cg::fit_edge(pw, ph, req.max_edge);
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth(w);
            config.setHeight(h);
            config.setShowsCursor(false);
        }

        let (tx2, max_edge) = (tx.clone(), req.max_edge);
        let done = RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
            // SAFETY: a live CGImage or null, valid for this call; read back before returning.
            let r = match unsafe { image.as_ref() } {
                Some(img) => cg::image_to_frame(img, max_edge).map_err(CaptureError::Failed),
                None => Err(CaptureError::Failed(error_text(err, "The screen capture failed."))),
            };
            let _ = tx2.send(r);
        });
        // SAFETY: documented signature; the framework copies the block.
        unsafe { SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &config, Some(&done)) };
    }

    fn rect_of(d: &SCDisplay) -> Rect {
        let f = unsafe { d.frame() };
        [f.origin.x, f.origin.y, f.size.width, f.size.height]
    }

    fn native_pixels(id: u32, points: &Rect) -> (usize, usize) {
        match CGDisplayCopyDisplayMode(id) {
            Some(m) => match (CGDisplayMode::pixel_width(Some(&m)), CGDisplayMode::pixel_height(Some(&m))) {
                (w, h) if w > 0 && h > 0 => (w, h),
                _ => (points[2] as usize, points[3] as usize),
            },
            None => (points[2] as usize, points[3] as usize),
        }
    }
}

/// Which of `displays` (CG global rects) to capture: the one equal to `want` (within a point), else the
/// main display (`main`: its index), else the first. `None` only when there is no display at all.
pub fn pick_display(displays: &[Rect], want: Option<Rect>, main: Option<usize>) -> Option<usize> {
    let same = |a: &Rect, b: &Rect| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0);
    want.and_then(|w| displays.iter().position(|d| same(d, &w))).or(main).or((!displays.is_empty()).then_some(0))
}

#[cfg(any(test, debug_assertions))]
/// A scripted capture: a gradient of `size`, what it was asked for and in what order (shared with the
/// test's cue and OCR fakes through `log`).
pub struct FakeCapture {
    pub permission: Permission,
    pub size: (u32, u32),
    pub result: Option<CaptureError>,
    pub log: Arc<Mutex<Vec<String>>>,
    pub grabs: Mutex<Vec<GrabReq>>,
}

#[cfg(any(test, debug_assertions))]
impl FakeCapture {
    pub fn new(width: u32, height: u32) -> FakeCapture {
        FakeCapture { permission: Permission::Granted, size: (width, height), result: None, log: Arc::default(), grabs: Mutex::new(vec![]) }
    }
}

#[cfg(any(test, debug_assertions))]
impl Capture for FakeCapture {
    fn permission(&self) -> Permission {
        self.log.lock().unwrap().push("permission".into());
        self.permission
    }

    fn grab(&self, req: &GrabReq) -> Result<Frame, CaptureError> {
        self.log.lock().unwrap().push("grab".into());
        self.grabs.lock().unwrap().push(req.clone());
        if let Some(e) = &self.result {
            return Err(e.clone());
        }
        let (w, h) = self.size;
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                bgra.extend_from_slice(&[(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 128, 255]);
            }
        }
        Ok(Frame { width: w, height: h, bgra })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = [0.0, 0.0, 1440.0, 900.0];
    const B: Rect = [1440.0, 0.0, 1920.0, 1080.0];

    #[test]
    fn the_bears_display_wins_then_main_then_the_first() {
        assert_eq!(pick_display(&[A, B], Some(B), Some(0)), Some(1));
        assert_eq!(pick_display(&[A, B], Some([1440.4, 0.0, 1920.0, 1080.0]), Some(0)), Some(1));
        assert_eq!(pick_display(&[A, B], Some([9.0, 9.0, 9.0, 9.0]), Some(1)), Some(1), "unknown display: the main one");
        assert_eq!(pick_display(&[A, B], None, Some(1)), Some(1));
        assert_eq!(pick_display(&[A, B], None, None), Some(0));
        assert_eq!(pick_display(&[], Some(A), None), None);
    }

    /// The preflight never prompts and never panics; the answer depends on the machine, so only that it
    /// returns. `grab` is never called here: it would capture the real screen.
    #[test]
    fn the_real_permission_check_returns_without_prompting() {
        let p = RealCapture.permission();
        assert!(matches!(p, Permission::Granted | Permission::Missing));
    }
}
