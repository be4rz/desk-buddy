//! Screen look (D18, level C): the agent reads the text on the display the bear is on, only when the
//! person turned it on (`settings.screen_look.enabled`) and asked in chat, with a visible cue (the bear
//! peeks), never while a deny-listed app is in front, and nothing captured is stored or logged.
//!
//! - `look`: one look, pure: refusal order, caps, reply shape (`look::run`, `look::look`).
//! - `capture`, `ocr`: the traits, the real ScreenCaptureKit and Vision implementations (`RealCapture`,
//!   `RealOcr`) and the fakes; `cg` is their CoreGraphics glue (memory only).
//! - `frame`: the area downscale and the PNG for `image: true`.
//!
//! The host route `POST /v1/screen/look {image}` lands in `host_look`: a snapshot on the main thread
//! (`App::screen_snapshot`), then the checks, the cue (`App::screen_cue`, `Inbox::Call` like
//! `buddy mascot …`) and the capture and recognition on the host's thread. Debug builds only:
//! `BUDDY_SCREEN_FAKE=1` swaps in a scripted capture and recognizer that need no permission.
//! Nothing in this module touches the disk; the log gets sizes and codes, never text or app names.

pub mod capture;
pub mod cg;
pub mod frame;
pub mod look;
pub mod ocr;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use host::Refusal;
use objc2_app_kit::NSWorkspace;
use serde_json::Value;

use crate::driver::{post, App, Inbox};
use capture::{Capture, RealCapture};
use ocr::{Ocr, RealOcr};

/// How long the host waits for the main thread, like `buddy mascot …`.
const MAIN_WAIT: Duration = Duration::from_secs(2);

/// The emotes that cue a look, in the order tried: the pack's own `peek`, else `look_up` (D13: no new art).
pub const CUE_EMOTES: [&str; 2] = ["peek", "look_up"];

/// The capture and the recognizer a look uses.
pub struct Backend {
    pub capture: Box<dyn Capture>,
    pub ocr: Box<dyn Ocr>,
}

/// The one backend: the real one, or (debug builds, `BUDDY_SCREEN_FAKE=1`) the scripted pair.
pub fn backend() -> &'static Backend {
    static BACKEND: OnceLock<Backend> = OnceLock::new();
    BACKEND.get_or_init(|| {
        #[cfg(debug_assertions)]
        if std::env::var("BUDDY_SCREEN_FAKE").ok().as_deref() == Some("1") {
            eprintln!("[buddy] screen look: scripted capture (BUDDY_SCREEN_FAKE)");
            return Backend { capture: Box::new(capture::FakeCapture::new(1600, 1000)), ocr: Box::new(ocr::FakeOcr::lines(["Fake screen", "Line one of the fake screen text", "Line two"])) };
        }
        Backend { capture: Box::new(RealCapture), ocr: Box::new(RealOcr) }
    })
}

/// Whether macOS has not granted Screen Recording (`CGPreflightScreenCaptureAccess`: never prompts).
pub fn permission_missing() -> bool {
    backend().capture.permission() != capture::Permission::Granted
}

/// Ask macOS for Screen Recording (`CGRequestScreenCaptureAccess`, which shows the system prompt the
/// first time) and say whether it is still missing. Called only when the person turns the tray item on;
/// a debug build with `BUDDY_SCREEN_FAKE=1` never prompts. Main thread, returns at once.
pub fn request_access() -> bool {
    #[cfg(debug_assertions)]
    if std::env::var("BUDDY_SCREEN_FAKE").ok().as_deref() == Some("1") {
        return permission_missing();
    }
    !objc2_core_graphics::CGRequestScreenCaptureAccess()
}

/// The bundle ids of the running apps (main thread).
pub fn running_bundles() -> Vec<String> {
    NSWorkspace::sharedWorkspace().runningApplications().iter().filter_map(|a| a.bundleIdentifier().map(|b| b.to_string())).collect()
}

/// Play the cue: the first of `CUE_EMOTES` the bear accepts (`play` returns a `BodyReply` as JSON).
/// `None`: the pack has none of them, or the bear is busy or hidden.
pub fn play_cue(mut play: impl FnMut(&str) -> Value) -> Option<&'static str> {
    CUE_EMOTES.into_iter().find(|name| play(name)["status"] == "played")
}

/// Whether the command came from a check-in turn (`s:<uuid>`), which never looks at the screen.
pub fn is_sense_turn(turn: Option<&str>) -> bool {
    turn.and_then(bridge::protocol::request_source) == Some("sense")
}

/// Run `f` on the main thread and wait up to `MAIN_WAIT` for its answer; `None` when it did not come,
/// and then `f` never runs (the same rule as `buddy mascot …`).
fn on_main<T: Send + 'static>(f: impl FnOnce(&mut App) -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    let gave_up = Arc::new(Mutex::new(false));
    let g2 = gave_up.clone();
    post(Inbox::Call(Box::new(move |a| {
        let g = g2.lock().unwrap_or_else(|e| e.into_inner());
        if *g {
            return;
        }
        let _ = tx.send(f(a));
    })));
    rx.recv_timeout(MAIN_WAIT).or_else(|_| {
        let mut g = gave_up.lock().unwrap_or_else(|e| e.into_inner());
        rx.try_recv().inspect_err(|_| *g = true)
    }).ok()
}

/// `POST /v1/screen/look`: the reply JSON, or the refusal (plan C.15).
pub fn host_look(image: bool) -> host::Reply {
    let be = backend();
    let busy = || Refusal::new(503, "busy", "The bear didn’t answer in time.");
    let snap = on_main(|a| a.screen_snapshot()).ok_or_else(busy)?;
    let cue = || match on_main(|a| a.screen_cue()) {
        Some(c) => eprintln!("[buddy] screen look cue: {c}"),
        None => eprintln!("[buddy] screen look cue: busy"),
    };
    match look::look(&snap, &*be.capture, &*be.ocr, &cue, image) {
        Ok(l) => {
            eprintln!("[buddy] screen look: {}", l.log_line());
            Ok(l.to_json())
        }
        Err(e) => {
            let (status, code, message) = e.refusal();
            eprintln!("[buddy] screen look refused: {code}");
            Err(Refusal::new(status, code, message))
        }
    }
}

#[cfg(test)]
mod tests;
