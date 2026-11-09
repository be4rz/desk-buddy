//! The native mascot window: a non-activating, borderless, transparent NSPanel
//! whose content is a layer-hosting view. A CPU-composited buffer at sprite
//! resolution is handed to the CALayer as a CGImage and scaled up by a whole
//! number with nearest filtering.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Instant;

use block2::RcBlock;
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
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGBitmapContextCreateImage,
    CGBitmapContextGetBytesPerRow, CGBitmapContextGetData, CGColorSpace, CGContext,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{
    ns_string, NSArray, NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSObject,
    NSObjectProtocol, NSOperationQueue, NSRunLoop, NSRunLoopCommonModes, NSTimer,
};
use objc2_io_surface::{
    kIOSurfaceBytesPerElement, kIOSurfaceColorSpace, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth, IOSurfaceLockOptions, IOSurfaceRef,
};
use objc2_quartz_core::{kCAFilterNearest, CALayer, CATransaction};

use crate::assets::Library;
use crate::behaviour::{Behaviour, Size};
use crate::render::Canvas;

/// kCGFloatingWindowLevel = 3; "just above floating".
pub const LEVEL_NORMAL: isize = 3 + 1;
/// kCGStatusWindowLevel = 25; used only while the frontmost app is fullscreen.
pub const LEVEL_FULLSCREEN: isize = 25 + 1;
const POLL_HZ: f64 = 30.0;
/// The 30 Hz cursor poll only runs while the cursor is this close (points).
const POLL_NEAR_PT: f64 = 160.0;

define_class!(
    // SAFETY: NSView has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "S1MascotView"]
    struct MascotView;

    impl MascotView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }

        // Only reached while the cursor is over opaque bear pixels (the poller
        // turns ignoresMouseEvents off there), so any press here is a drag.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(w) = self.window() {
                w.performWindowDragWithEvent(event);
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
    beh: Behaviour,
    canvas: Canvas,
    /// follow camera: world position of the bear (screen px), float
    cam: [f32; 2],
    zoom: u32,
    size: Size,
    present: Present,
    ignoring: bool,
    fullscreen: bool,
    user_visible: bool,
    occluded: bool,
    anim_timer: Option<Retained<NSTimer>>,
    poll_timer: Option<Retained<NSTimer>>,
    /// (frame content id, camera) of what the layer currently shows
    shown: Option<(u64, [i32; 2])>,
    frames: u64,
    skipped: u64,
    polls: u64,
    stats_t0: Instant,
}

pub struct Mascot {
    mtm: MainThreadMarker,
    pub panel: Retained<NSPanel>,
    layer: Retained<CALayer>,
    lib: Library,
    st: RefCell<State>,
    observers: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

fn canvas_for(size: Size, lib: &Library) -> Canvas {
    // canvas in sprite px; the view is a soft-edged window onto the world
    let (w, h, tile, clip) = match size {
        Size::S100 => (200usize, 160usize, 32, "idle"),
        Size::S64 => (128, 102, 20, "walk_64"),
    };
    // centre the bear's opaque bbox (frame 0, S) in the canvas
    let c = &lib.clips[clip];
    let (frames, _) = &c.dirs["S"];
    let (mut y0, mut y1) = (c.h, 0);
    for y in 0..c.h {
        for x in 0..c.w {
            if frames[0].px[(y * c.w + x) * 4 + 3] > 0 {
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    let bbox_mid = (y0 + y1) as i32 / 2;
    let anchor_y = h as i32 / 2 + (c.anchor[1] - bbox_mid);
    Canvas::new(w, h, [w as i32 / 2, anchor_y], tile)
}

/// How a finished buffer reaches the CALayer.
enum Present {
    /// Two IOSurfaces, alternated; CA shares them with the render server
    /// without copying (default).
    Surface { s: [CFRetained<IOSurfaceRef>; 2], next: usize },
    /// CGBitmapContext -> CGImage per frame (`S1_PRESENT=cgimage`): CA copies
    /// it into shared memory on every commit.
    Image(CFRetained<CGContext>),
}

fn make_present(w: usize, h: usize) -> Present {
    if std::env::var("S1_PRESENT").as_deref() == Ok("cgimage") {
        return Present::Image(make_ctx(w, h));
    }
    Present::Surface { s: [make_surface(w, h), make_surface(w, h)], next: 0 }
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
        let vals: [Retained<AnyObject>; 4] =
            [n(w).into(), n(h).into(), n(4).into(), bgra.into()];
        let kr: Vec<&objc2_foundation::NSString> = keys.iter().map(|k| &**k).collect();
        let vr: Vec<&AnyObject> = vals.iter().map(|v| &**v).collect();
        let dict = NSDictionary::from_slices(&kr, &vr);
        let cf = &*(Retained::as_ptr(&dict) as *const objc2_core_foundation::CFDictionary);
        let surf = IOSurfaceRef::new(cf).expect("IOSurfaceCreate");
        // our pixels are sRGB; tag the surface so CA colour-matches correctly
        let cs: &objc2_core_foundation::CFType = kCGColorSpaceSRGB;
        surf.set_value(kIOSurfaceColorSpace, cs);
        surf
    }
}

fn make_ctx(w: usize, h: usize) -> CFRetained<CGContext> {
    let cs = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).expect("sRGB");
    // kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little = BGRA in memory
    let info = 2u32 | (2u32 << 12);
    unsafe { CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, 0, Some(&cs), info) }
        .expect("CGBitmapContextCreate")
}

impl Mascot {
    pub fn new(mtm: MainThreadMarker, lib: Library) -> Rc<Self> {
        let size = Size::S100;
        let zoom = std::env::var("S1_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or(1u32).clamp(1, 3);
        let canvas = canvas_for(size, &lib);
        let present = make_present(canvas.w, canvas.h);

        let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(canvas.w as f64, canvas.h as f64));
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
        if std::env::var_os("S1_NO_SRGB_WINDOW").is_none() {
            // our pixels are sRGB: let the window server colour-match on the GPU
            // instead of CoreAnimation converting every frame on the CPU
            panel.setColorSpace(Some(&objc2_app_kit::NSColorSpace::sRGBColorSpace()));
        }

        let view = MascotView::new(mtm, rect);
        let layer = CALayer::new();
        layer.setMagnificationFilter(unsafe { kCAFilterNearest });
        layer.setMinificationFilter(unsafe { kCAFilterNearest });
        // layer-hosting view: set the layer before wantsLayer, AppKit won't redraw it
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        panel.setContentView(Some(&view));

        let beh = Behaviour::new(size);
        let m = Rc::new(Mascot {
            mtm,
            panel,
            layer,
            lib,
            st: RefCell::new(State {
                beh,
                canvas,
                cam: [0.0, 0.0],
                zoom,
                size,
                present,
                ignoring: true,
                fullscreen: false,
                user_visible: true,
                occluded: false,
                anim_timer: None,
                poll_timer: None,
                shown: None,
                frames: 0,
                skipped: 0,
                polls: 0,
                stats_t0: Instant::now(),
            }),
            observers: RefCell::new(vec![]),
        });
        m.place_initially();
        if std::env::var("S1_SIZE").as_deref() == Ok("64") {
            m.set_size(Size::S64);
        }
        m.render();
        m.panel.orderFrontRegardless();
        m.install_observers();
        m.check_fullscreen("startup");
        m.start_timers();
        m.report("startup");
        m.debug_hop();
        m
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

    fn place_initially(&self) {
        let screens = NSScreen::screens(self.mtm);
        let idx = std::env::var("S1_SCREEN").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        let scr = if idx < screens.count() { screens.objectAtIndex(idx) } else { screens.objectAtIndex(0) };
        let vf = scr.visibleFrame();
        let (_, pt) = self.sprite_pt();
        let (w, h) = {
            let st = self.st.borrow();
            (st.canvas.w as f64 * pt, st.canvas.h as f64 * pt)
        };
        let x = (vf.origin.x + vf.size.width - w - 80.0).round();
        let y = (vf.origin.y + 60.0).round();
        self.panel.setFrame_display(CGRect::new(CGPoint::new(x, y), CGSize::new(w, h)), true);
        // re-evaluate at the new screen's backing scale
        self.apply_geometry();
    }

    /// Resize around the current centre and snap the origin to whole points.
    fn apply_geometry(&self) {
        let (_, pt) = self.sprite_pt();
        let (w, h) = {
            let st = self.st.borrow();
            (st.canvas.w as f64 * pt, st.canvas.h as f64 * pt)
        };
        let f = self.panel.frame();
        let cx = f.origin.x + f.size.width / 2.0;
        let cy = f.origin.y + f.size.height / 2.0;
        let r = CGRect::new(CGPoint::new((cx - w / 2.0).round(), (cy - h / 2.0).round()), CGSize::new(w, h));
        if r != f {
            self.panel.setFrame_display(r, true);
        }
        let b = self.panel.contentView().map(|v| v.bounds()).unwrap_or(r);
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

    // ---------- rendering ----------

    fn render(&self) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let clip = &self.lib.clips[&st.beh.clip];
        let (frames, _) = &clip.dirs[st.beh.dir];
        let cam = [
            st.cam[0].round() as i32 - st.canvas.anchor[0],
            st.cam[1].round() as i32 - st.canvas.anchor[1],
        ];
        let frame = &frames[st.beh.frame];
        if st.shown == Some((frame.id, cam)) {
            st.skipped += 1;
            return;
        }
        st.shown = Some((frame.id, cam));
        st.canvas.compose(cam, frame, clip.w, clip.h, clip.anchor);
        let row = st.canvas.w * 4;
        let h = st.canvas.h;
        let copy_rows = |dst: *mut u8, bpr: usize, src: &[u8]| {
            for y in 0..h {
                unsafe { std::ptr::copy_nonoverlapping(src[y * row..].as_ptr(), dst.add(y * bpr), row) };
            }
        };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        match &mut st.present {
            Present::Surface { s, next } => {
                let surf = &s[*next];
                *next ^= 1;
                unsafe {
                    surf.lock(IOSurfaceLockOptions::empty(), std::ptr::null_mut());
                    copy_rows(surf.base_address().as_ptr() as *mut u8, surf.bytes_per_row(), &st.canvas.out);
                    surf.unlock(IOSurfaceLockOptions::empty(), std::ptr::null_mut());
                    let obj: &AnyObject = &*(CFRetained::as_ptr(surf).as_ptr() as *const AnyObject);
                    self.layer.setContents(Some(obj));
                }
            }
            Present::Image(ctx) => {
                let data = CGBitmapContextGetData(Some(ctx)) as *mut u8;
                copy_rows(data, CGBitmapContextGetBytesPerRow(Some(ctx)), &st.canvas.out);
                let img = CGBitmapContextCreateImage(Some(ctx)).expect("CGImage");
                let obj: &AnyObject = unsafe { &*(CFRetained::as_ptr(&img).as_ptr() as *const AnyObject) };
                unsafe { self.layer.setContents(Some(obj)) };
            }
        }
        CATransaction::commit();
        st.frames += 1;
        if let Ok(p) = std::env::var("S1_DUMP") {
            dump_png(&p, st.canvas.w, st.canvas.h, &st.canvas.out);
        }
    }

    /// Show the current frame, then apply its root motion and advance.
    fn anim_tick(self: &Rc<Self>) {
        self.render();
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let clip = &self.lib.clips[&st.beh.clip];
            let d = clip.dirs[st.beh.dir].1[st.beh.frame];
            if std::env::var_os("S1_FREEZE").is_none() {
                st.cam[0] += d[0];
                st.cam[1] += d[1];
                st.beh.advance(&self.lib);
            }
        }
        if NSEvent::pressedMouseButtons() == 0 {
            self.snap_origin();
        }
        if self.st.borrow().poll_timer.is_none() && self.cursor_near() {
            self.start_poll();
        }
        self.maybe_stats();
    }

    fn cursor_near(&self) -> bool {
        if std::env::var_os("S1_POLL_ALWAYS").is_some() {
            return true; // measuring aid: worst case, cursor parked next to the bear
        }
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
            sx < st.canvas.w && sy < st.canvas.h && st.canvas.hit[sy * st.canvas.w + sx]
        };
        // keep accepting while a drag that started on the bear is in progress
        let dragging = !st.ignoring && NSEvent::pressedMouseButtons() != 0;
        let want_ignore = !hit && !dragging;
        let idle_far = want_ignore && st.ignoring && !inside;
        if want_ignore != st.ignoring {
            st.ignoring = want_ignore;
            drop(st);
            self.panel.setIgnoresMouseEvents(want_ignore);
            if std::env::var_os("S1_VERBOSE").is_some() {
                eprintln!("[s1] ignoresMouseEvents = {want_ignore}");
            }
            return;
        }
        drop(st);
        // cursor wandered off: stop the fast poll, the 12 fps tick restarts it
        if idle_far && !self.cursor_near() {
            if let Some(t) = self.st.borrow_mut().poll_timer.take() {
                t.invalidate();
            }
        }
    }

    // ---------- timers: 12 fps anim + 30 Hz cursor poll, none when hidden ----------

    fn start_timers(self: &Rc<Self>) {
        let mut st = self.st.borrow_mut();
        if st.anim_timer.is_some() || !st.user_visible || st.occluded {
            return;
        }
        let fps = 1000.0 / self.lib.clips["idle"].durs_ms[0] as f64; // 12 fps
        let weak = Rc::downgrade(self);
        let anim = RcBlock::new(move |_t: NonNull<NSTimer>| {
            if let Some(m) = weak.upgrade() {
                m.anim_tick();
            }
        });
        unsafe {
            let a = NSTimer::timerWithTimeInterval_repeats_block(1.0 / fps, true, &anim);
            a.setTolerance(0.004);
            NSRunLoop::mainRunLoop().addTimer_forMode(&a, NSRunLoopCommonModes);
            st.anim_timer = Some(a);
        }
        drop(st);
        if self.cursor_near() {
            self.start_poll();
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

    fn stop_timers(&self) {
        let mut st = self.st.borrow_mut();
        if let Some(t) = st.anim_timer.take() {
            t.invalidate();
        }
        if let Some(t) = st.poll_timer.take() {
            t.invalidate();
        }
        if !st.ignoring {
            st.ignoring = true;
            self.panel.setIgnoresMouseEvents(true);
        }
    }

    fn maybe_stats(&self) {
        if std::env::var_os("S1_STATS").is_none() {
            return;
        }
        let mut st = self.st.borrow_mut();
        let dt = st.stats_t0.elapsed().as_secs_f64();
        if dt >= 10.0 {
            eprintln!(
                "[s1] {:.1} fps uploaded (+{:.1} identical skipped), {:.1} Hz poll, clip {} {}",
                st.frames as f64 / dt,
                st.skipped as f64 / dt,
                st.polls as f64 / dt,
                st.beh.clip,
                st.beh.dir
            );
            st.frames = 0;
            st.skipped = 0;
            st.polls = 0;
            st.stats_t0 = Instant::now();
        }
    }

    // ---------- notifications: occlusion, screen/backing, spaces ----------

    fn observe(
        self: &Rc<Self>,
        center: &NSNotificationCenter,
        name: &objc2_foundation::NSNotificationName,
        obj: Option<&AnyObject>,
        f: impl Fn(&Rc<Mascot>) + 'static,
    ) {
        let weak = Rc::downgrade(self);
        let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
            if let Some(m) = weak.upgrade() {
                f(&m);
            }
        });
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(name),
                obj,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        self.observers.borrow_mut().push(token);
    }

    fn install_observers(self: &Rc<Self>) {
        let dc = NSNotificationCenter::defaultCenter();
        let me: &AnyObject = &self.panel;
        unsafe {
            self.observe(&dc, NSWindowDidChangeOcclusionStateNotification, Some(me), |m| {
                let vis = m.panel.occlusionState().contains(NSWindowOcclusionState::Visible);
                m.st.borrow_mut().occluded = !vis;
                eprintln!("[s1] occlusion: visible={vis}");
                if vis {
                    m.start_timers();
                } else {
                    m.stop_timers();
                }
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
            eprintln!("[s1] fullscreen front app = {fs} ({why}) -> level {}", self.panel.level());
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
            "[s1] {why}: screen '{scr}' backing {b} zoom x{} -> {k} device px per sprite px ({pt} pt); \
             canvas {}x{} sprite px; frame ({}, {}) {}x{} pt; level {}; collectionBehavior {:#x} \
             (canJoinAllSpaces={}, fullScreenAuxiliary={}); windowNumber {}",
            st.zoom,
            st.canvas.w,
            st.canvas.h,
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
        self.st.borrow_mut().zoom = z;
        self.apply_geometry();
        self.report("zoom");
    }

    pub fn set_size(&self, size: Size) {
        {
            let mut st = self.st.borrow_mut();
            if st.size == size {
                return;
            }
            st.size = size;
            st.canvas = canvas_for(size, &self.lib);
            st.present = make_present(st.canvas.w, st.canvas.h);
            st.beh.reset(size);
            st.shown = None;
        }
        self.apply_geometry();
        self.render();
        self.report("size");
    }

    pub fn set_visible(self: &Rc<Self>, v: bool) {
        self.st.borrow_mut().user_visible = v;
        if v {
            self.panel.orderFrontRegardless();
            self.start_timers();
        } else {
            self.stop_timers();
            self.panel.orderOut(None);
        }
        eprintln!("[s1] mascot visible={v}");
    }

    pub fn is_visible(&self) -> bool {
        self.st.borrow().user_visible
    }

    /// Debug aid for the mixed-DPI check: jump to the next display.
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

    /// `S1_HOP_SECS=n`: hop to the next display every n seconds (self-test
    /// for the mixed-DPI check without dragging).
    fn debug_hop(self: &Rc<Self>) {
        if let Some(secs) = std::env::var("S1_HIDE_AFTER").ok().and_then(|s| s.parse::<f64>().ok()) {
            let weak = Rc::downgrade(self);
            let block = RcBlock::new(move |_t: NonNull<NSTimer>| {
                if let Some(m) = weak.upgrade() {
                    m.set_visible(false);
                }
            });
            unsafe {
                let t = NSTimer::timerWithTimeInterval_repeats_block(secs, false, &block);
                NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes);
            }
        }
        let Some(secs) = std::env::var("S1_HOP_SECS").ok().and_then(|s| s.parse::<f64>().ok()) else {
            return;
        };
        let weak = Rc::downgrade(self);
        let block = RcBlock::new(move |_t: NonNull<NSTimer>| {
            if let Some(m) = weak.upgrade() {
                m.next_display();
                m.report("hop");
            }
        });
        unsafe {
            let t = NSTimer::timerWithTimeInterval_repeats_block(secs, true, &block);
            NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes);
        }
    }

    /// Make the bear walk now (from the test webview).
    pub fn poke(&self) {
        let mut st = self.st.borrow_mut();
        if st.size == Size::S100 && st.beh.clip == "idle" {
            st.beh.frame = self.lib.clips["idle"].len() - 1;
            st.beh.force_leave_idle();
        }
    }
}

/// Debug: write the composited (premultiplied BGRA) buffer as straight RGBA PNG.
fn dump_png(path: &str, w: usize, h: usize, bgra: &[u8]) {
    let mut rgba = vec![0u8; w * h * 4];
    for i in 0..w * h {
        let a = bgra[i * 4 + 3] as u32;
        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
        rgba[i * 4] = un(bgra[i * 4 + 2]);
        rgba[i * 4 + 1] = un(bgra[i * 4 + 1]);
        rgba[i * 4 + 2] = un(bgra[i * 4]);
        rgba[i * 4 + 3] = a as u8;
    }
    if let Ok(f) = std::fs::File::create(path) {
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        if let Ok(mut wr) = e.write_header() {
            let _ = wr.write_image_data(&rgba);
        }
    }
}

/// Is the frontmost app showing a window that covers a whole screen
/// (including the menu bar strip)? That only happens in a fullscreen Space
/// (or for borderless-fullscreen games), never for a zoomed window.
fn frontmost_is_fullscreen(mtm: MainThreadMarker) -> bool {
    let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else { return false };
    let pid = app.processIdentifier();
    if pid == std::process::id() as i32 {
        return false;
    }
    let Some(list) = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    ) else {
        return false;
    };
    // CFArray of CFDictionary is toll-free bridged to NSArray of NSDictionary
    let arr: &NSArray = unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const NSArray) };
    let screens = NSScreen::screens(mtm);
    if screens.count() == 0 {
        return false;
    }
    let main_h = screens.objectAtIndex(0).frame().size.height;
    let num = |d: &NSDictionary, k: &objc2_foundation::NSString| -> Option<f64> {
        let k: &AnyObject = k;
        d.objectForKey(k).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue())
    };
    for item in arr.iter() {
        let Ok(d) = item.downcast::<NSDictionary>() else { continue };
        if num(&d, ns_string!("kCGWindowOwnerPID")) != Some(pid as f64) {
            continue;
        }
        if num(&d, ns_string!("kCGWindowLayer")) != Some(0.0) {
            continue;
        }
        let key: &AnyObject = ns_string!("kCGWindowBounds");
        let Some(b) = d.objectForKey(key).and_then(|o| o.downcast::<NSDictionary>().ok()) else { continue };
        let (Some(x), Some(y), Some(w), Some(h)) = (
            num(&b, ns_string!("X")),
            num(&b, ns_string!("Y")),
            num(&b, ns_string!("Width")),
            num(&b, ns_string!("Height")),
        ) else {
            continue;
        };
        for s in screens.iter() {
            let f = s.frame();
            let cg_y = main_h - (f.origin.y + f.size.height);
            if (x - f.origin.x).abs() < 1.0
                && (y - cg_y).abs() < 1.0
                && (w - f.size.width).abs() < 1.0
                && (h - f.size.height).abs() < 1.0
            {
                return true;
            }
        }
    }
    false
}
