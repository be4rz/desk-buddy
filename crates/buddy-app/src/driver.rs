//! The main-thread loop (design §2.2). `App` owns the only `Brain`. Other threads push
//! into one `std::sync::mpsc` inbox; a forwarder thread hops each message onto the main
//! thread. Each wake: handle → `brain.tick(now)` → compose + present if the `Show`
//! changed → arm a one-shot timer at `brain.next_wake_ms()`. No timer while the bear is
//! hidden (by the user or the brain) or the panel is occluded: 0 fps.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use block2::RcBlock;
use mascot_core::replay::SensesPatch;
use mascot_core::{Brain, Dir, EyeState, Intent, MouthState, Ori, PackMeta, Poke, Senses, Show, Visibility};
use mascot_protocol::MascotEvent;
use mascot_render::{Art, Canvas, View};
use objc2::rc::Retained;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};

use crate::panel::Panel;
use crate::settings::{self, Settings};
use crate::stats::Stats;

/// Everything that reaches the brain or the app from elsewhere (frozen for later phases).
#[allow(dead_code)] // Settings and inbox() are the frozen entry points for p2+
pub enum Inbox {
    Event(MascotEvent),
    Senses(SensesPatch),
    Poke(Poke),
    Settings(Settings),
    Call(Box<dyn FnOnce(&mut App) + Send>),
}

static INBOX: OnceLock<Mutex<Option<Sender<Inbox>>>> = OnceLock::new();

/// A sender into the main-thread inbox (any thread).
#[allow(dead_code)] // used by senses/harness (p2+)
pub fn inbox() -> Sender<Inbox> {
    let slot = INBOX.get_or_init(|| Mutex::new(None));
    slot.lock().unwrap().clone().expect("driver::start has not run")
}

/// Post one message; dropped silently once the app is gone.
pub fn post(m: Inbox) {
    if let Some(Some(tx)) = INBOX.get().map(|s| s.lock().unwrap().clone()) {
        let _ = tx.send(m);
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Run `f` on the app (main thread only). Re-entrant calls are skipped, not nested.
pub fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Ok(mut a) = a.try_borrow_mut() {
            if let Some(a) = a.as_mut() {
                f(a);
            }
        }
    });
}

/// What the panel shows: a new key means compose + present.
#[derive(Clone, PartialEq)]
struct Key {
    /// compose picks set props by behavior
    behavior: String,
    clip: String,
    dir: Dir,
    ori: Option<Ori>,
    /// content id, not the frame number: idle repeats frames
    frame: u64,
    eyes: EyeState,
    mouth: MouthState,
    effect: Option<String>,
    visibility: Visibility,
    cam: [i32; 2],
}

pub struct App {
    pub brain: Brain,
    /// the senses last handed to the brain (patches merge over them)
    senses: Senses,
    #[allow(dead_code)] // for later phases (studio, senses)
    pub pack: Arc<PackMeta>,
    pub settings: Settings,
    pub home: PathBuf,
    pub panel: Rc<Panel>,
    art: Art,
    canvas: Canvas,
    /// pixels of what the panel shows
    last_px: Vec<u8>,
    view: View,
    t0: Instant,
    shown: Option<Key>,
    timer: Option<Retained<NSTimer>>,
    user_hidden: bool,
    occluded: bool,
    /// (intent, behavior) last logged
    last_log: Option<(Intent, String)>,
    /// ms of the last brain tick: a second tick in the same ms returns the cached Show
    /// (same moved_px) or no new motion, so the camera must not add it again
    last_tick_ms: Option<u64>,
    pub stats: Stats,
    dump: Option<(PathBuf, u64)>,
    presented: u64,
    /// called after a user-visible change (tray labels)
    pub on_hidden: Option<Box<dyn Fn(bool)>>,
}

pub struct Init {
    pub pack: Arc<PackMeta>,
    pub art: Art,
    pub settings: Settings,
    pub home: PathBuf,
    pub panel: Rc<Panel>,
    pub brain: Brain,
    pub t0: Instant,
}

/// Install the app on the main thread and start the inbox forwarder.
pub fn start(init: Init, handle: tauri::AppHandle) {
    let (tx, rx) = channel::<Inbox>();
    *INBOX.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(tx);
    let dump = std::env::var_os("BUDDY_DUMP").map(|d| {
        let d = PathBuf::from(d);
        let _ = std::fs::create_dir_all(&d);
        let every = std::env::var("BUDDY_DUMP_EVERY").ok().and_then(|s| s.parse().ok()).unwrap_or(12u64).max(1);
        (d, every)
    });
    let canvas = init.art.canvas();
    let app = App {
        brain: init.brain,
        senses: Senses::default(),
        pack: init.pack,
        settings: init.settings,
        home: init.home,
        panel: init.panel,
        art: init.art,
        canvas,
        last_px: vec![],
        view: View::default(),
        t0: init.t0,
        shown: None,
        timer: None,
        user_hidden: false,
        occluded: false,
        last_log: None,
        last_tick_ms: None,
        stats: Stats::new(),
        dump,
        presented: 0,
        on_hidden: None,
    };
    APP.with(|a| *a.borrow_mut() = Some(app));
    forward(rx, handle);
    with_app(|a| a.wake());
}

fn forward(rx: Receiver<Inbox>, handle: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("buddy-inbox".into())
        .spawn(move || {
            while let Ok(m) = rx.recv() {
                if handle.run_on_main_thread(move || with_app(|a| a.handle(m))).is_err() {
                    break;
                }
            }
        })
        .expect("spawn inbox thread");
}

impl App {
    pub fn now_ms(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }

    pub fn handle(&mut self, m: Inbox) {
        let now = self.now_ms();
        match m {
            Inbox::Event(ev) => {
                eprintln!("[buddy] event {}", serde_json::to_string(&ev).unwrap_or_default());
                self.brain.event(&ev, now);
            }
            Inbox::Senses(patch) => {
                // a patch merges over the last senses; the brain always sees them whole
                patch.apply(&mut self.senses);
                self.brain.senses(&self.senses, now);
            }
            Inbox::Poke(p) => {
                if p == Poke::DragEnd {
                    self.save_position();
                }
                eprintln!("[buddy] poke {p:?}");
                self.brain.poke(p, now);
            }
            Inbox::Settings(s) => {
                self.settings = s;
                self.save_settings();
            }
            Inbox::Call(f) => f(self),
        }
        self.wake();
    }

    fn save_position(&mut self) {
        if let Some(p) = self.panel.position() {
            self.settings.position = Some(p);
            self.save_settings();
        }
    }

    pub fn save_settings(&self) {
        if let Err(e) = self.settings.save(&self.home) {
            eprintln!("[buddy] cannot write {}: {e}", settings::path(&self.home).display());
        }
    }

    pub fn set_zoom(&mut self, z: u32) {
        self.settings.zoom = z.clamp(1, 3);
        self.panel.set_zoom(self.settings.zoom);
        // the zoom re-centres the panel: one write with the new zoom and position
        if let Some(p) = self.panel.position() {
            self.settings.position = Some(p);
        }
        self.save_settings();
    }

    /// Tray "Move to next display": the new place is persisted like a drag's.
    pub fn next_display(&mut self) {
        self.panel.next_display();
        self.save_position();
    }

    pub fn set_user_hidden(&mut self, h: bool) {
        self.user_hidden = h;
        eprintln!("[buddy] bear visible={}", !h);
        if let Some(f) = &self.on_hidden {
            f(h);
        }
        self.wake();
    }

    pub fn user_hidden(&self) -> bool {
        self.user_hidden
    }

    pub fn set_occluded(&mut self, o: bool) {
        self.occluded = o;
        self.wake();
    }

    /// One wake: tick the brain, present if changed, arm the next timer.
    pub fn wake(&mut self) {
        if let Some(t) = self.timer.take() {
            t.invalidate();
        }
        self.stats.wake();
        let now = self.now_ms();
        let show = self.brain.tick(now);
        self.log(&show);
        if self.last_tick_ms.replace(now) != Some(now) {
            self.view.cam[0] += show.moved_px[0];
            self.view.cam[1] += show.moved_px[1];
        }
        let hidden = self.user_hidden || show.visibility == Visibility::Hidden;
        self.panel.set_shown(!hidden);
        if hidden || self.occluded {
            return; // 0 fps until something arrives in the inbox
        }
        self.present(&show);
        self.panel.after_frame();
        let next = self.brain.next_wake_ms();
        if next == u64::MAX {
            return; // a still pose: nothing changes until the inbox has something
        }
        // every frame boundary wakes (≤12/s): an identical frame skips compose via Key,
        // and timed cues / blinks land on time (measured idle cpu 0.1–0.3%)
        let next = next.max(now + 1);
        self.arm((next - now) as f64 / 1000.0);
    }

    fn present(&mut self, show: &Show) {
        let key = Key {
            behavior: show.behavior.clone(),
            clip: show.clip.clone(),
            dir: show.dir,
            ori: show.ori,
            frame: self.art.frame_id(show),
            eyes: show.eyes,
            mouth: show.mouth,
            effect: show.effect.clone(),
            visibility: show.visibility,
            cam: self.view.cam,
        };
        if self.shown.as_ref() == Some(&key) {
            return; // S1's identical-frame skip
        }
        self.shown = Some(key);
        mascot_render::compose(&self.art, show, &self.view, &mut self.canvas);
        if self.canvas.px == self.last_px {
            self.stats.skip(); // a held pose (idle repeats frames): nothing to upload
            return;
        }
        self.last_px.clone_from(&self.canvas.px);
        self.panel.present(&self.canvas);
        self.stats.frame();
        self.presented += 1;
        if let Some((dir, every)) = &self.dump {
            if (self.presented - 1) % every == 0 {
                let p = dir.join(format!("frame_{:06}.png", self.presented));
                let rgba = mascot_render::to_rgba(&self.canvas);
                if let Err(e) = mascot_render::write_png(&p, self.canvas.w, self.canvas.h, &rgba) {
                    eprintln!("[buddy] dump {}: {e}", p.display());
                }
            }
        }
    }

    fn log(&mut self, show: &Show) {
        if self.last_log.as_ref().is_some_and(|(i, b)| *i == show.intent && *b == show.behavior) {
            return;
        }
        let intent = show.intent.as_str();
        let ori = show.ori.map(|o| format!(" {o:?}")).unwrap_or_default();
        eprintln!("[buddy] intent {intent} → {} ({} {:?}{ori})", show.behavior, show.clip, show.dir);
        self.last_log = Some((show.intent, show.behavior.clone()));
    }

    fn arm(&mut self, secs: f64) {
        let block = RcBlock::new(|_t: NonNull<NSTimer>| with_app(|a| a.wake()));
        unsafe {
            let t = NSTimer::timerWithTimeInterval_repeats_block(secs, false, &block);
            t.setTolerance(0.004);
            NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes);
            self.timer = Some(t);
        }
    }
}
