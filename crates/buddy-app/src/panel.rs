//! The native mascot window (moved from spike S1): a non-activating, borderless,
//! transparent NSPanel whose content is a layer-hosting view. A CPU-composited canvas at
//! sprite resolution is handed to the CALayer through an IOSurface and scaled up by a
//! whole number with nearest filtering. Per-pixel click-through, the near-cursor hit
//! poll, occlusion / screen / backing / Space observers and the fullscreen level are
//! S1's code paths; the behavior loop and compositor moved to the brain and
//! `mascot-render`, and the frame timer to `driver.rs`.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Instant;

use block2::RcBlock;
use mascot_core::Poke;
use mascot_render::Canvas;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSPanel, NSResponder, NSScreen, NSView,
    NSWindowCollectionBehavior, NSWindowDidChangeBackingPropertiesNotification,
    NSWindowDidChangeOcclusionStateNotification, NSWindowDidChangeScreenNotification,
    NSWindowOcclusionState, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceActiveSpaceDidChangeNotification, NSWorkspaceDidActivateApplicationNotification,
};
use objc2_core_foundation::{CFRetained, CFUUID, CGPoint, CGRect, CGSize};
use objc2_foundation::{
    ns_string, NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSObject,
    NSObjectProtocol, NSOperationQueue, NSRunLoop, NSRunLoopCommonModes, NSTimer,
};
use objc2_io_surface::{
    kIOSurfaceBytesPerElement, kIOSurfaceColorSpace, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth, IOSurfaceLockOptions, IOSurfaceRef,
};
use objc2_quartz_core::{kCAFilterNearest, CALayer, CATransaction};

use crate::driver::{post, Inbox};
use crate::senses::cursor::{self as cursor_sense, Feed};
use crate::settings::Position;

/// kCGFloatingWindowLevel = 3; "just above floating".
pub const LEVEL_NORMAL: isize = 3 + 1;
/// kCGStatusWindowLevel = 25; used only while the frontmost app is fullscreen.
pub const LEVEL_FULLSCREEN: isize = 25 + 1;
const POLL_HZ: f64 = 30.0;
/// The 30 Hz cursor poll only runs while the cursor is this close (points).
const POLL_NEAR_PT: f64 = 160.0;
/// input watch (drag end, cursor approaching) while shown, whatever the animation does
const WATCH_HZ: f64 = 4.0;
/// A press that moves less than this (points) and lasts less than CLICK_MS is a click.
const CLICK_PT: f64 = 4.0;
const CLICK_MS: u128 = 350;

thread_local! {
    /// mouse-down point and time while a press on the bear is in progress
    static PRESS: Cell<Option<(CGPoint, Instant)>> = const { Cell::new(None) };
    /// a window drag started from the bear and has not ended yet
    static DRAGGING: Cell<bool> = const { Cell::new(false) };
}

define_class!(
    // SAFETY: NSView has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BuddyMascotView"]
    struct MascotView;

    impl MascotView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }

        // Only reached while the cursor is over opaque bear pixels (the poller turns
        // ignoresMouseEvents off there). A press is a click until it moves 4 pt.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            PRESS.set(Some((NSEvent::mouseLocation(), Instant::now())));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let Some((p0, _)) = PRESS.get() else { return };
            if DRAGGING.get() {
                return;
            }
            let p = NSEvent::mouseLocation();
            if (p.x - p0.x).hypot(p.y - p0.y) < CLICK_PT {
                return;
            }
            DRAGGING.set(true);
            post(Inbox::Poke(Poke::DragStart));
            if let Some(w) = self.window() {
                w.performWindowDragWithEvent(event);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let Some((p0, t0)) = PRESS.take() else { return };
            if DRAGGING.get() {
                return; // the poll / input watch ends the drag
            }
            let p = NSEvent::mouseLocation();
            if (p.x - p0.x).hypot(p.y - p0.y) < CLICK_PT && t0.elapsed().as_millis() < CLICK_MS {
                post(Inbox::Poke(Poke::Click));
            }
        }
    }
);

impl MascotView {
    fn new(mtm: MainThreadMarker, frame: CGRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

struct State {
    /// canvas size in sprite px
    w: usize,
    h: usize,
    zoom: u32,
    surfaces: [CFRetained<IOSurfaceRef>; 2],
    next: usize,
    /// opaque art pixels of what is shown: the hit-test mask
    hit: Vec<bool>,
    ignoring: bool,
    fullscreen: bool,
    shown: bool,
    poll_timer: Option<Retained<NSTimer>>,
    /// WATCH_HZ input watch while shown: independent of the animation's wakes
    watch_timer: Option<Retained<NSTimer>>,
    polls: u64,
    /// the bear's floor anchor in the canvas (sprite px): the cursor sense's origin
    anchor: Option<[i32; 2]>,
    /// the cursor sense (`senses::cursor`): relative samples, ≤ 10 Hz, to the brain only
    cursor: Feed,
    /// last `Inbox::DragMoved` (ms since `born`)
    drag_posted: Option<u64>,
    born: Instant,
}

pub struct Panel {
    mtm: MainThreadMarker,
    pub panel: Retained<NSPanel>,
    layer: Retained<CALayer>,
    st: RefCell<State>,
    observers: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    verbose: bool,
}

fn make_surface(w: usize, h: usize) -> CFRetained<IOSurfaceRef> {
    let n = |v: usize| NSNumber::new_usize(v);
    let bgra = NSNumber::new_u32(u32::from_be_bytes(*b"BGRA"));
    let key = |k: &objc2_core_foundation::CFString| -> Retained<objc2_foundation::NSString> {
        // CFString is toll-free bridged to NSString
        unsafe { Retained::retain(k as *const _ as *mut objc2_foundation::NSString) }.unwrap()
    };
    unsafe {
        let keys = [
            key(kIOSurfaceWidth),
            key(kIOSurfaceHeight),
            key(kIOSurfaceBytesPerElement),
            key(kIOSurfacePixelFormat),
        ];
        let vals: [Retained<AnyObject>; 4] = [n(w).into(), n(h).into(), n(4).into(), bgra.into()];
        let kr: Vec<&objc2_foundation::NSString> = keys.iter().map(|k| &**k).collect();
        let vr: Vec<&AnyObject> = vals.iter().map(|v| &**v).collect();
        let dict = NSDictionary::from_slices(&kr, &vr);
        let cf = &*(Retained::as_ptr(&dict) as *const objc2_core_foundation::CFDictionary);
        let surf = IOSurfaceRef::new(cf).expect("IOSurfaceCreate");
        // our pixels are sRGB; tag the surface so CA colour-matches correctly
        let cs: &objc2_core_foundation::CFType = objc2_core_graphics::kCGColorSpaceSRGB;
        surf.set_value(kIOSurfaceColorSpace, cs);
        surf
    }
}

#[link(name = "ColorSync", kind = "framework")]
extern "C" {
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> Option<NonNull<CFUUID>>;
}

/// The CGDisplay UUID of a screen, as a string (settings `position.display`).
fn screen_uuid(s: &NSScreen) -> Option<String> {
    let desc = s.deviceDescription();
    let n = desc.objectForKey(ns_string!("NSScreenNumber"))?.downcast::<NSNumber>().ok()?;
    // SAFETY: returns a +1 CFUUID or null.
    let uuid = unsafe { CGDisplayCreateUUIDFromDisplayID(n.unsignedIntValue()) }?;
    let uuid = unsafe { CFRetained::from_raw(uuid) };
    CFUUID::new_string(None, Some(&uuid)).map(|s| s.to_string())
}

impl Panel {
    /// A panel for a `w x h` sprite-px canvas, placed at `pos` when that display is
    /// still attached (else S1's initial placement), shown front.
    pub fn new(mtm: MainThreadMarker, w: usize, h: usize, zoom: u32, pos: Option<&Position>) -> Rc<Self> {
        let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(true);
        panel.setLevel(LEVEL_NORMAL);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setIgnoresMouseEvents(true);
        // our pixels are sRGB: let the window server colour-match on the GPU
        // instead of CoreAnimation converting every frame on the CPU
        panel.setColorSpace(Some(&objc2_app_kit::NSColorSpace::sRGBColorSpace()));

        let view = MascotView::new(mtm, rect);
        let layer = CALayer::new();
        layer.setMagnificationFilter(unsafe { kCAFilterNearest });
        layer.setMinificationFilter(unsafe { kCAFilterNearest });
        // layer-hosting view: set the layer before wantsLayer, AppKit won't redraw it
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        panel.setContentView(Some(&view));

        let p = Rc::new(Panel {
            mtm,
            panel,
            layer,
            st: RefCell::new(State {
                w,
                h,
                zoom: zoom.clamp(1, 3),
                surfaces: [make_surface(w, h), make_surface(w, h)],
                next: 0,
                hit: vec![false; w * h],
                ignoring: true,
                fullscreen: false,
                shown: true,
                poll_timer: None,
                watch_timer: None,
                polls: 0,
                anchor: None,
                cursor: Feed::default(),
                drag_posted: None,
                born: Instant::now(),
            }),
            observers: RefCell::new(vec![]),
            verbose: std::env::var_os("BUDDY_VERBOSE").is_some(),
        });
        if !pos.is_some_and(|pos| p.place_at(pos)) {
            p.place_initially();
        }
        p.panel.orderFrontRegardless();
        p.start_watch();
        p.install_observers();
        p.check_fullscreen("startup");
        p.report("startup");
        p
    }

    // ---------- geometry ----------

    /// device px per sprite px = floor(backing x zoom); window size in points
    /// follows from it, so it is the same on 1x and 2x displays (no jump).
    fn sprite_pt(&self) -> (f64, f64) {
        let backing = self.panel.backingScaleFactor();
        let zoom = self.st.borrow().zoom as f64;
        let k = (backing * zoom).floor().max(1.0);
        (k, k / backing)
    }

    fn size_pt(&self) -> (f64, f64) {
        let (_, pt) = self.sprite_pt();
        let st = self.st.borrow();
        (st.w as f64 * pt, st.h as f64 * pt)
    }

    fn place_initially(&self) {
        let screens = NSScreen::screens(self.mtm);
        let idx = std::env::var("BUDDY_SCREEN").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        let scr = if idx < screens.count() { screens.objectAtIndex(idx) } else { screens.objectAtIndex(0) };
        let vf = scr.visibleFrame();
        let (w, h) = self.size_pt();
        let x = (vf.origin.x + vf.size.width - w - 80.0).round();
        let y = (vf.origin.y + 60.0).round();
        self.panel.setFrame_display(CGRect::new(CGPoint::new(x, y), CGSize::new(w, h)), true);
        // re-evaluate at the new screen's backing scale
        self.apply_geometry();
    }

    /// Restore a saved position if its display is attached and the panel would
    /// overlap that display's visible frame.
    fn place_at(&self, pos: &Position) -> bool {
        let screens = NSScreen::screens(self.mtm);
        let Some(scr) = screens.iter().find(|s| screen_uuid(s).as_deref() == Some(pos.display.as_str())) else {
            return false;
        };
        let vf = scr.visibleFrame();
        let (w, h) = self.size_pt();
        let (x, y) = (pos.x as f64, pos.y as f64);
        let overlaps = x < vf.origin.x + vf.size.width && x + w > vf.origin.x && y < vf.origin.y + vf.size.height && y + h > vf.origin.y;
        if !overlaps {
            return false;
        }
        // position = the window origin (kept fixed, unlike zoom which keeps the centre)
        self.panel.setFrame_display(CGRect::new(CGPoint::new(x, y), CGSize::new(w, h)), true);
        self.set_layer_frame();
        true
    }

    /// Where the panel is now, for settings.
    pub fn position(&self) -> Option<Position> {
        let f = self.panel.frame();
        let display = self.panel.screen().and_then(|s| screen_uuid(&s))?;
        Some(Position { display, x: f.origin.x.round() as i32, y: f.origin.y.round() as i32, extra: Default::default() })
    }

    /// The primary display's height: Cocoa (bottom-left) ↔ CG (top-left) global points.
    fn primary_h(&self) -> Option<f64> {
        NSScreen::screens(self.mtm).iter().next().map(|s| s.frame().size.height)
    }

    /// The panel in CG global points (top-left origin, like CGWindowList bounds), for edges.
    pub fn rect_tl(&self) -> Option<mascot_core::WinRect> {
        let f = self.panel.frame();
        let top = self.primary_h()? - (f.origin.y + f.size.height);
        Some(mascot_core::WinRect { x: f.origin.x.round() as i32, y: top.round() as i32, w: f.size.width.round() as i32, h: f.size.height.round() as i32, owner_pid: std::process::id() as i32, layer: 0 })
    }

    /// Move the panel's top-left corner to `(x, y)` CG global points (edges).
    pub fn move_tl(&self, x: i32, y: i32) {
        let Some(ph) = self.primary_h() else { return };
        let f = self.panel.frame();
        self.panel.setFrameOrigin(CGPoint::new(x as f64, (ph - y as f64 - f.size.height).round()));
    }

    /// The canvas anchor the cursor sense measures from (driver start; the pack's anchor).
    pub fn set_anchor(&self, anchor: [i32; 2]) {
        self.st.borrow_mut().anchor = Some(anchor);
    }

    /// The bear's floor anchor (canvas pixel `anchor`) inside the panel, in points.
    pub fn feet_pt(&self, anchor: [i32; 2]) -> [i32; 2] {
        let (_, pt) = self.sprite_pt();
        [(anchor[0] as f64 * pt).round() as i32, (anchor[1] as f64 * pt).round() as i32]
    }

    /// Resize around the current centre and snap the origin to whole points.
    fn apply_geometry(&self) {
        let (w, h) = self.size_pt();
        let f = self.panel.frame();
        let cx = f.origin.x + f.size.width / 2.0;
        let cy = f.origin.y + f.size.height / 2.0;
        let r = CGRect::new(CGPoint::new((cx - w / 2.0).round(), (cy - h / 2.0).round()), CGSize::new(w, h));
        if r != f {
            self.panel.setFrame_display(r, true);
        }
        self.set_layer_frame();
    }

    fn set_layer_frame(&self) {
        let b = self.panel.contentView().map(|v| v.bounds()).unwrap_or(self.panel.frame());
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.layer.setFrame(b);
        CATransaction::commit();
    }

    fn snap_origin(&self) {
        let f = self.panel.frame();
        let (x, y) = (f.origin.x.round(), f.origin.y.round());
        if x != f.origin.x || y != f.origin.y {
            self.panel.setFrameOrigin(CGPoint::new(x, y));
        }
    }

    // ---------- presenting ----------

    /// Show a composited canvas (premultiplied BGRA at sprite resolution).
    pub fn present(&self, c: &Canvas) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        assert_eq!((c.w, c.h), (st.w, st.h), "canvas size is fixed per pack");
        mascot_render::hit_mask(c, &mut st.hit);
        let row = c.w * 4;
        let surf = &st.surfaces[st.next];
        st.next ^= 1;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe {
            surf.lock(IOSurfaceLockOptions::empty(), std::ptr::null_mut());
            let dst = surf.base_address().as_ptr() as *mut u8;
            let bpr = surf.bytes_per_row();
            for y in 0..c.h {
                std::ptr::copy_nonoverlapping(c.px[y * row..].as_ptr(), dst.add(y * bpr), row);
            }
            surf.unlock(IOSurfaceLockOptions::empty(), std::ptr::null_mut());
            let obj: &AnyObject = &*(CFRetained::as_ptr(surf).as_ptr() as *const AnyObject);
            self.layer.setContents(Some(obj));
        }
        CATransaction::commit();
    }

    /// After each presented frame (driver) and on every input-watch tick (the brain may
    /// sleep for seconds, or for good on a still pose): end a finished drag, snap to
    /// whole points, and start the cursor poll when the cursor comes near.
    pub fn after_frame(self: &Rc<Self>) {
        self.end_drag_if_released();
        if self.st.borrow().poll_timer.is_none() && self.cursor_near() {
            self.start_poll();
        }
    }

    /// `performWindowDragWithEvent` swallows the mouse-up: a drag ends when no button
    /// is down any more.
    fn end_drag_if_released(&self) {
        if NSEvent::pressedMouseButtons() == 0 {
            if DRAGGING.replace(false) {
                PRESS.set(None);
                post(Inbox::Poke(Poke::DragEnd));
            }
            self.snap_origin();
        }
    }

    fn start_watch(self: &Rc<Self>) {
        if self.st.borrow().watch_timer.is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let tick = RcBlock::new(move |_t: NonNull<NSTimer>| {
            if let Some(m) = weak.upgrade() {
                m.after_frame();
            }
        });
        unsafe {
            let t = NSTimer::timerWithTimeInterval_repeats_block(1.0 / WATCH_HZ, true, &tick);
            t.setTolerance(0.05);
            NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes);
            self.st.borrow_mut().watch_timer = Some(t);
        }
    }

    fn stop_watch(&self) {
        if let Some(t) = self.st.borrow_mut().watch_timer.take() {
            t.invalidate();
        }
    }

    fn cursor_near(&self) -> bool {
        let m = NSEvent::mouseLocation();
        let f = self.panel.frame();
        m.x >= f.origin.x - POLL_NEAR_PT
            && m.x < f.origin.x + f.size.width + POLL_NEAR_PT
            && m.y >= f.origin.y - POLL_NEAR_PT
            && m.y < f.origin.y + f.size.height + POLL_NEAR_PT
    }

    // ---------- per-pixel click-through ----------

    fn poll_tick(&self) {
        let m = NSEvent::mouseLocation();
        let f = self.panel.frame();
        let (_, pt) = self.sprite_pt();
        let mut st = self.st.borrow_mut();
        st.polls += 1;
        let inside = m.x >= f.origin.x
            && m.x < f.origin.x + f.size.width
            && m.y >= f.origin.y
            && m.y < f.origin.y + f.size.height;
        let hit = inside && {
            let sx = ((m.x - f.origin.x) / pt).floor() as usize;
            let sy = ((f.origin.y + f.size.height - m.y) / pt).floor() as usize;
            sx < st.w && sy < st.h && st.hit[sy * st.w + sx]
        };
        // the cursor sense: relative to the feet, coarse, changes only (never the position itself)
        let now_ms = st.born.elapsed().as_millis() as u64;
        let sample = st.anchor.filter(|_| crate::senses::cursor_on()).and_then(|a| {
            let feet = (f.origin.x + a[0] as f64 * pt, f.origin.y + f.size.height - a[1] as f64 * pt);
            cursor_sense::relative((m.x, m.y), feet, hit)
        });
        let cursor_out = st.cursor.push(sample, now_ms);
        // the drag measure: where the panel is, ≤ 10 Hz while a drag is on
        let drag_out = DRAGGING.get() && st.drag_posted.is_none_or(|t| now_ms >= t + cursor_sense::MIN_GAP_MS);
        if drag_out {
            st.drag_posted = Some(now_ms);
        }
        if let Some(c) = cursor_out {
            post(Inbox::Cursor(c));
        }
        if drag_out {
            post(Inbox::DragMoved);
        }
        // keep accepting while a drag that started on the bear is in progress
        let dragging = !st.ignoring && NSEvent::pressedMouseButtons() != 0;
        let want_ignore = !hit && !dragging;
        let idle_far = want_ignore && st.ignoring && !inside;
        if want_ignore != st.ignoring {
            st.ignoring = want_ignore;
            drop(st);
            self.panel.setIgnoresMouseEvents(want_ignore);
            if self.verbose {
                eprintln!("[buddy] ignoresMouseEvents = {want_ignore}");
            }
            return;
        }
        drop(st);
        if DRAGGING.get() {
            self.end_drag_if_released();
        }
        // cursor wandered off: stop the fast poll, the input watch restarts it
        if idle_far && !self.cursor_near() {
            let mut st = self.st.borrow_mut();
            if let Some(t) = st.poll_timer.take() {
                t.invalidate();
            }
            let now_ms = st.born.elapsed().as_millis() as u64;
            if let Some(c) = st.cursor.leave(now_ms) {
                post(Inbox::Cursor(c));
            }
        }
    }

    fn start_poll(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let poll = RcBlock::new(move |_t: NonNull<NSTimer>| {
            if let Some(m) = weak.upgrade() {
                m.poll_tick();
            }
        });
        unsafe {
            let p = NSTimer::timerWithTimeInterval_repeats_block(1.0 / POLL_HZ, true, &poll);
            p.setTolerance(0.008);
            NSRunLoop::mainRunLoop().addTimer_forMode(&p, NSRunLoopCommonModes);
            self.st.borrow_mut().poll_timer = Some(p);
        }
    }

    fn stop_poll(&self) {
        let mut st = self.st.borrow_mut();
        if let Some(t) = st.poll_timer.take() {
            t.invalidate();
        }
        let now_ms = st.born.elapsed().as_millis() as u64;
        if let Some(c) = st.cursor.leave(now_ms) {
            post(Inbox::Cursor(c));
        }
        if !st.ignoring {
            st.ignoring = true;
            self.panel.setIgnoresMouseEvents(true);
        }
    }

    /// Polls handled since the last call (stats).
    pub fn take_polls(&self) -> u64 {
        std::mem::take(&mut self.st.borrow_mut().polls)
    }

    // ---------- notifications: occlusion, screen/backing, spaces ----------

    fn observe(
        self: &Rc<Self>,
        center: &NSNotificationCenter,
        name: &objc2_foundation::NSNotificationName,
        obj: Option<&AnyObject>,
        f: impl Fn(&Rc<Panel>) + 'static,
    ) {
        let weak = Rc::downgrade(self);
        let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
            if let Some(m) = weak.upgrade() {
                f(&m);
            }
        });
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), obj, Some(&NSOperationQueue::mainQueue()), &block)
        };
        self.observers.borrow_mut().push(token);
    }

    fn install_observers(self: &Rc<Self>) {
        let dc = NSNotificationCenter::defaultCenter();
        let me: &AnyObject = &self.panel;
        unsafe {
            self.observe(&dc, NSWindowDidChangeOcclusionStateNotification, Some(me), |m| {
                let vis = m.panel.occlusionState().contains(NSWindowOcclusionState::Visible);
                eprintln!("[buddy] occlusion: visible={vis}");
                // covered: no cursor poll and no input watch either (the watch would
                // restart the poll against a stale mask); uncovered: the watch resumes
                if !vis {
                    m.stop_poll();
                    m.stop_watch();
                } else if m.st.borrow().shown {
                    m.start_watch();
                }
                post(Inbox::Call(Box::new(move |app| app.set_occluded(!vis))));
            });
            self.observe(&dc, NSWindowDidChangeBackingPropertiesNotification, Some(me), |m| {
                m.apply_geometry();
                m.report("backing changed");
            });
            self.observe(&dc, NSWindowDidChangeScreenNotification, Some(me), |m| {
                m.apply_geometry();
                m.report("screen changed");
            });
            let wc = NSWorkspace::sharedWorkspace().notificationCenter();
            self.observe(&wc, NSWorkspaceActiveSpaceDidChangeNotification, None, |m| {
                m.check_fullscreen("space changed");
                m.check_fullscreen_later();
            });
            self.observe(&wc, NSWorkspaceDidActivateApplicationNotification, None, |m| {
                m.check_fullscreen("app activated");
                m.check_fullscreen_later();
            });
        }
    }

    /// Space switches animate; re-check once the new Space has settled.
    fn check_fullscreen_later(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let block = RcBlock::new(move |_t: NonNull<NSTimer>| {
            if let Some(m) = weak.upgrade() {
                m.check_fullscreen("settled");
            }
        });
        unsafe {
            let t = NSTimer::timerWithTimeInterval_repeats_block(0.8, false, &block);
            NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes);
        }
    }

    fn check_fullscreen(&self, why: &str) {
        let fs = frontmost_is_fullscreen(self.mtm);
        let changed = {
            let mut st = self.st.borrow_mut();
            let c = st.fullscreen != fs;
            st.fullscreen = fs;
            c
        };
        let want = if fs { LEVEL_FULLSCREEN } else { LEVEL_NORMAL };
        if self.panel.level() != want {
            self.panel.setLevel(want);
        }
        if changed || why == "startup" {
            eprintln!("[buddy] fullscreen front app = {fs} ({why}) -> level {}", self.panel.level());
        }
    }

    pub fn report(&self, why: &str) {
        let (k, pt) = self.sprite_pt();
        let st = self.st.borrow();
        let f = self.panel.frame();
        let b = self.panel.backingScaleFactor();
        let cb = self.panel.collectionBehavior();
        let scr = self.panel.screen().map(|s| s.localizedName().to_string()).unwrap_or_default();
        eprintln!(
            "[buddy] {why}: screen '{scr}' backing {b} zoom x{} -> {k} device px per sprite px ({pt} pt); \
             canvas {}x{} sprite px; frame ({}, {}) {}x{} pt; level {}; collectionBehavior {:#x} \
             (canJoinAllSpaces={}, fullScreenAuxiliary={}); windowNumber {}",
            st.zoom,
            st.w,
            st.h,
            f.origin.x,
            f.origin.y,
            f.size.width,
            f.size.height,
            self.panel.level(),
            cb.0,
            cb.contains(NSWindowCollectionBehavior::CanJoinAllSpaces),
            cb.contains(NSWindowCollectionBehavior::FullScreenAuxiliary),
            self.panel.windowNumber()
        );
    }

    // ---------- menu actions ----------

    pub fn set_zoom(&self, z: u32) {
        self.st.borrow_mut().zoom = z.clamp(1, 3);
        self.apply_geometry();
        self.report("zoom");
    }

    /// Order the panel in or out. Hidden → no cursor poll, no input watch, no frame timer.
    pub fn set_shown(self: &Rc<Self>, v: bool) {
        if self.st.borrow().shown == v {
            return;
        }
        self.st.borrow_mut().shown = v;
        if v {
            self.panel.orderFrontRegardless();
            self.start_watch();
        } else {
            self.stop_poll();
            self.stop_watch();
            self.panel.orderOut(None);
        }
    }

    /// Jump to the next display (mixed-DPI check), S1's placement there.
    pub fn next_display(&self) {
        let screens = NSScreen::screens(self.mtm);
        let n = screens.count();
        if n < 2 {
            return;
        }
        let cur = self.panel.screen();
        let mut idx = 0;
        for i in 0..n {
            if cur.as_deref().map(|c| std::ptr::eq(c, &*screens.objectAtIndex(i))).unwrap_or(false) {
                idx = i;
            }
        }
        let vf = screens.objectAtIndex((idx + 1) % n).visibleFrame();
        let f = self.panel.frame();
        let x = (vf.origin.x + vf.size.width - f.size.width - 80.0).round();
        let y = (vf.origin.y + 60.0).round();
        self.panel.setFrameOrigin(CGPoint::new(x, y));
        self.apply_geometry();
    }
}

/// Is the frontmost app fullscreen (`senses::fullscreen`, S1's bounds rule)? A null
/// CGWindowList reads as "not fullscreen" (logged once by `senses::read_windows`).
fn frontmost_is_fullscreen(_mtm: MainThreadMarker) -> bool {
    let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else { return false };
    let pid = app.processIdentifier();
    if pid == std::process::id() as i32 {
        return false;
    }
    let Some(wins) = crate::senses::read_windows() else { return false };
    crate::senses::fullscreen::is_fullscreen(&wins, pid, &crate::senses::fullscreen::displays())
}
