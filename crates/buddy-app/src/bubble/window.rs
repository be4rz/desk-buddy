//! The speech bubble's window (§4.8): one hidden Tauri webview window `bubble` (page
//! `web/bubble/index.html`), created at setup. While shown it is an NSWindow child of the bear's
//! panel, so it follows drags; it sits above the panel's top centre, or below it when there is no
//! room (the page gets `data-side`). It is ordered front without activating the app, except the
//! prompt, which takes the keyboard and gives it back to the app the person was in.
//! The page's commands post into the main-thread inbox.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSEvent, NSPanel, NSRunningApplication, NSWindow, NSWindowOrderingMode, NSWorkspace};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::{Emitter, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use super::model::{BubbleModel, Mode};
use crate::driver::{post, with_app, Inbox};

pub const LABEL: &str = "bubble";
/// The bubble overlaps the panel's top edge by this much (the sprite has headroom).
const OVERLAP_PT: f64 = 10.0;

pub struct BubbleWin {
    mtm: MainThreadMarker,
    win: WebviewWindow,
    ns: Retained<NSWindow>,
    /// the page's last reported size, in points
    size: Cell<(f64, f64)>,
    /// on screen (as a child of the panel)
    visible: Cell<bool>,
    /// a model was sent and the page hasn't sized it yet: shown on the next `bubble_size`
    pending: Cell<bool>,
    /// the pending model is the prompt: it takes the keyboard once placed and shown
    wants_key: Cell<bool>,
    /// the bear is hidden: the bubble waits
    bear_hidden: Cell<bool>,
    side: Cell<&'static str>,
    /// the app that had the keyboard before the prompt took it
    prev: RefCell<Option<Retained<NSRunningApplication>>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
}

impl BubbleWin {
    pub fn new(app: &tauri::App, mtm: MainThreadMarker) -> tauri::Result<BubbleWin> {
        let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("bubble/index.html".into()))
            .title("desk-buddy")
            .visible(false)
            .transparent(true)
            .decorations(false)
            .focused(false)
            .skip_taskbar(true)
            .shadow(false)
            .resizable(false)
            .accept_first_mouse(true)
            // the page measures itself while its window is hidden: it must keep running
            .background_throttling(BackgroundThrottlingPolicy::Disabled)
            .inner_size(306.0, 120.0)
            .build()?;
        let ptr = win.ns_window()? as *mut NSWindow;
        // SAFETY: tauri hands out its live NSWindow; retaining keeps it for our lifetime
        let ns = unsafe { Retained::retain(ptr) }.expect("bubble NSWindow");
        ns.setHasShadow(false);
        ns.setHidesOnDeactivate(false);
        Ok(BubbleWin {
            mtm,
            win,
            ns,
            size: Cell::new((306.0, 120.0)),
            visible: Cell::new(false),
            pending: Cell::new(false),
            wants_key: Cell::new(false),
            bear_hidden: Cell::new(false),
            side: Cell::new("above"),
            prev: RefCell::new(None),
            timer: RefCell::new(None),
        })
    }

    /// Send a model to the page; it is shown once the page reports its size.
    pub fn show(&self, m: &BubbleModel, panel: &NSPanel) {
        if let Err(e) = self.win.emit_to(LABEL, "bubble://show", m) {
            eprintln!("[buddy] bubble: {e}");
        }
        self.pending.set(true);
        self.wants_key.set(m.mode == Mode::Prompt);
        if self.visible.get() {
            self.place(panel);
        }
    }

    pub fn hide(&self, panel: &NSPanel) {
        let _ = self.win.emit_to(LABEL, "bubble://hide", ());
        self.pending.set(false);
        self.wants_key.set(false);
        self.order_out(panel);
        self.give_keyboard_back();
    }

    fn order_out(&self, panel: &NSPanel) {
        if self.visible.replace(false) {
            panel.removeChildWindow(&self.ns);
            self.ns.orderOut(None);
        }
    }

    /// The page measured itself (`bubble_size`): resize, place, and show a pending model.
    pub fn sized(&self, w: f64, h: f64, panel: &NSPanel) {
        self.size.set((w.clamp(40.0, 800.0), h.clamp(20.0, 800.0)));
        if self.pending.replace(false) || self.visible.get() {
            self.order_in(panel);
            if self.visible.get() && self.wants_key.replace(false) {
                self.take_keyboard();
            }
        }
    }

    fn order_in(&self, panel: &NSPanel) {
        if self.bear_hidden.get() {
            return;
        }
        self.place(panel);
        if !self.visible.replace(true) {
            // SAFETY: both windows are alive; the child is removed before it is ordered out
            unsafe { panel.addChildWindow_ordered(&self.ns, NSWindowOrderingMode::Above) };
            self.ns.orderFrontRegardless();
            eprintln!("[buddy] bubble on screen ({side})", side = self.side.get());
        }
    }

    /// Above the panel's top centre, kept on the panel's screen; below when there's no room.
    fn place(&self, panel: &NSPanel) {
        let (w, h) = self.size.get();
        let f = panel.frame();
        let vf = panel.screen().map(|s| s.visibleFrame()).unwrap_or(f);
        let (x, y, side) = place(f, vf, w, h);
        if side != self.side.replace(side) || !self.visible.get() {
            let _ = self.win.eval(format!("document.documentElement.dataset.side={side:?}"));
        }
        self.ns.setFrame_display(CGRect::new(CGPoint::new(x, y), CGSize::new(w, h)), true);
    }

    /// The bear was hidden or shown again.
    pub fn set_bear_hidden(&self, hidden: bool, panel: &NSPanel, has_model: bool) {
        if self.bear_hidden.replace(hidden) == hidden {
            return;
        }
        if hidden {
            self.order_out(panel);
        } else if has_model {
            self.order_in(panel);
        }
    }

    /// The prompt needs the keyboard: activate the app and make the bubble key.
    fn take_keyboard(&self) {
        let app = NSApplication::sharedApplication(self.mtm);
        if !app.isActive() {
            let front = NSWorkspace::sharedWorkspace().frontmostApplication();
            *self.prev.borrow_mut() = front;
        }
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        self.ns.makeKeyAndOrderFront(None);
        let _ = self.win.set_focus();
    }

    /// After the prompt: the app the person was in gets the keyboard back.
    pub fn give_keyboard_back(&self) {
        if let Some(prev) = self.prev.borrow_mut().take() {
            prev.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }

    /// The cursor is over the bubble (keeps a finished bubble up).
    pub fn hovered(&self) -> bool {
        if !self.visible.get() {
            return false;
        }
        let p = NSEvent::mouseLocation();
        let f = self.ns.frame();
        p.x >= f.origin.x && p.x <= f.origin.x + f.size.width && p.y >= f.origin.y && p.y <= f.origin.y + f.size.height
    }

    /// Arm (or clear) the hide timer: `secs` from now, the app's `bubble_tick` runs.
    pub fn arm(&self, secs: Option<f64>) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.invalidate();
        }
        let Some(secs) = secs else { return };
        let block = RcBlock::new(|_t: NonNull<NSTimer>| with_app(|a| a.bubble_tick()));
        unsafe {
            let t = NSTimer::timerWithTimeInterval_repeats_block(secs.max(0.01), false, &block);
            NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes);
            *self.timer.borrow_mut() = Some(t);
        }
    }
}

/// The bubble's origin (Cocoa coordinates, points) for panel frame `f` on visible frame `vf`, and
/// its side. Above the panel's top centre; below the panel when it doesn't fit; x kept on screen.
pub fn place(f: CGRect, vf: CGRect, w: f64, h: f64) -> (f64, f64, &'static str) {
    let mid = f.origin.x + f.size.width / 2.0;
    let x = (mid - w / 2.0).clamp(vf.origin.x, (vf.origin.x + vf.size.width - w).max(vf.origin.x));
    let top = f.origin.y + f.size.height;
    let above = top - OVERLAP_PT;
    if above + h <= vf.origin.y + vf.size.height {
        (x, above, "above")
    } else {
        (x, (f.origin.y - h + OVERLAP_PT).max(vf.origin.y), "below")
    }
}

// ---------- the page's commands (§4.8): each posts into the inbox ----------

/// The bubble window only (`caller`): the Sign in… page can't submit prompts or answer cards.
fn mine(win: &WebviewWindow, cmd: &str) -> bool {
    crate::caller::is(win.label(), LABEL, cmd)
}

#[tauri::command]
pub fn prompt_submit(win: WebviewWindow, text: String) {
    if !mine(&win, "prompt_submit") || text.trim().is_empty() {
        return;
    }
    post(Inbox::Call(Box::new(move |a| a.submit(text))));
}

#[tauri::command]
pub fn card_answer(win: WebviewWindow, call_id: String, answer: String) -> Result<(), String> {
    if !mine(&win, "card_answer") {
        return Err("not from the bubble".into());
    }
    if !matches!(answer.as_str(), "allow" | "allow_always" | "deny") {
        return Err(format!("unknown answer {answer:?}"));
    }
    post(Inbox::Call(Box::new(move |a| a.card_answer(call_id, answer))));
    Ok(())
}

#[tauri::command]
pub fn bubble_dismiss(win: WebviewWindow) {
    if mine(&win, "bubble_dismiss") {
        post(Inbox::Call(Box::new(|a| a.bubble_dismiss())));
    }
}

#[tauri::command]
pub fn bubble_size(win: WebviewWindow, w: f64, h: f64) {
    if mine(&win, "bubble_size") {
        post(Inbox::Call(Box::new(move |a| a.bubble_sized(w, h))));
    }
}

#[tauri::command]
pub fn bubble_retry(win: WebviewWindow) {
    if mine(&win, "bubble_retry") {
        post(Inbox::Call(Box::new(|a| a.bubble_retry())));
    }
}

#[tauri::command]
pub fn reminder_snooze(win: WebviewWindow, id: String, minutes: u32) {
    if mine(&win, "reminder_snooze") {
        post(Inbox::Call(Box::new(move |a| a.reminder_snooze(id, minutes.clamp(1, 24 * 60)))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
    }

    #[test]
    fn the_bubble_sits_above_the_bear_or_below_it_near_the_top() {
        let vf = r(0.0, 0.0, 1440.0, 875.0);
        // the panel mid-screen: above, centred on it
        assert_eq!(place(r(500.0, 200.0, 168.0, 246.0), vf, 306.0, 120.0), (584.0 - 153.0, 436.0, "above"));
        // near the top edge: below
        let (_, y, side) = place(r(500.0, 700.0, 168.0, 246.0), vf, 306.0, 120.0);
        assert_eq!((y, side), (590.0, "below"));
        // at the left edge: kept on screen
        assert_eq!(place(r(-40.0, 200.0, 168.0, 246.0), vf, 306.0, 120.0).0, 0.0);
        assert_eq!(place(r(1400.0, 200.0, 168.0, 246.0), vf, 306.0, 120.0).0, 1440.0 - 306.0);
    }
}
