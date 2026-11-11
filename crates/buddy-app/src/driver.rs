//! The main-thread loop (design §2.2). `App` owns the only `Brain`. Other threads push
//! into one `std::sync::mpsc` inbox; a forwarder thread hops each message onto the main
//! thread. Each wake: handle → `brain.tick(now)` → compose + present if the `Show`
//! changed → arm a one-shot timer at `brain.next_wake_ms()`. No timer while the bear is
//! hidden (by the user or the brain) or the panel is occluded: 0 fps.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use block2::RcBlock;
use mascot_core::replay::SensesPatch;
use mascot_core::{Brain, Dir, DndAction, EyeState, Intent, MouthState, Ori, PackMeta, Poke, Senses, Show, Visibility};
use mascot_protocol::MascotEvent;
use mascot_render::{Art, Canvas, View};
use objc2::rc::Retained;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};

use crate::bubble::window::BubbleWin;
use crate::bubble::{Bubble, Mode, Out};
use crate::harness::{self, AgentState};
use crate::panel::Panel;
use crate::senses::{self, Ring};
use crate::settings::{self, DndAct, Settings};
use crate::stats::Stats;
use crate::unprompted::{self, Unprompted};

/// R-midturn: how long a new prompt waits for the turn it replaced to end.
const SUPERSEDE_WAIT: Duration = Duration::from_secs(5);

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
    /// raw senses as the sensors (or a replay) report them; patches merge over them
    raw: Senses,
    /// `raw` gated by settings and the DND toggle: what the brain was handed last
    senses: Senses,
    /// tray "Do not disturb" (this session only, never persisted)
    dnd_toggle: bool,
    /// a local-clock sample arrived (quiet hours need one; the default is 00:00)
    have_clock: bool,
    /// the last 256 raw sense changes (memory only)
    #[allow(dead_code)] // read by p4 (budget) and p6 (studio)
    pub ring: Ring,
    /// `BUDDY_SENSES_LOG=1`: sense changes to stderr
    senses_log: bool,
    /// tray status line ("Hidden: fullscreen app", …) on change
    status: Option<&'static str>,
    pub on_status: Option<Box<dyn Fn(Option<&'static str>)>>,
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
    /// the tray's agent line
    pub on_agent: Option<Box<dyn Fn(&AgentState)>>,
    /// the tray's "Reset web caution" (shown while tainted)
    pub on_taint: Option<Box<dyn Fn(bool)>>,
    /// the tray's Approvals submenu (the policy's mode: ask / allowlist / skip)
    pub on_mode: Option<Box<dyn Fn(&str)>>,
    policy_mode: String,
    /// the tray's checkmarks after `Inbox::Settings`
    pub on_settings: Option<Box<dyn Fn(&Settings)>>,
    /// the speech bubble's state and window
    pub bubble: Bubble,
    pub bubble_win: Option<BubbleWin>,
    /// the page has reported a size once (it is loaded and listening)
    bubble_loaded: bool,
    /// the last `bubble show` label logged (a new line only when mode/streaming change)
    bubble_label: Option<String>,
    /// what the bubble showed after the last `apply` (for the answer's `Settled`)
    bubble_mode: Option<Mode>,
    /// a brain-driven hide, for the `[buddy] hide:` line
    brain_hidden: bool,
    /// `BUDDY_STATS=1`: the mouth while talking
    mouth_log: bool,
    last_mouth: Option<MouthState>,
    /// sense-triggered model turns and their budget (p4)
    unprompted: Unprompted,
    /// the tray's budget line, on change
    budget_line: String,
    pub on_budget: Option<Box<dyn Fn(&str)>>,
    /// the tray's Ask… item: disabled while the brain hides the bear
    pub on_ask: Option<Box<dyn Fn(bool)>>,
    /// R-midturn: a prompt waiting for the turn it replaced to end (the flag is set on that
    /// turn's Idle or Error); a later prompt meanwhile replaces the text
    superseded: Option<(Arc<AtomicBool>, String)>,
    /// The prompt is on screen and the brain was told (`PromptOpen` … `PromptSent`/`PromptClosed`).
    listening: bool,
}

/// The listen poke for the brain when the bubble changed: the prompt came up, or went
/// without being sent (✕/Esc, a card took its place). A sent prompt was told in `submit`.
fn listen_poke(listening: bool, prompt_shown: bool) -> Option<Poke> {
    match (listening, prompt_shown) {
        (false, true) => Some(Poke::PromptOpen),
        (true, false) => Some(Poke::PromptClosed),
        _ => None,
    }
}

/// The answer's speech bubble went (hidden, or another bubble took its place): the brain's
/// `Settled`, so a bear answering from a station stands up (p3 desk). Not when a card replaces it
/// (the turn goes on) or the speech is only refreshed.
fn speech_settled(before: Option<Mode>, after: Option<Mode>) -> bool {
    before == Some(Mode::Speech) && !matches!(after, Some(Mode::Speech | Mode::Card))
}

/// The Ask… prompt is what the bubble shows.
fn prompt_shown(b: &Bubble) -> bool {
    b.shown().is_some_and(|m| m.mode == Mode::Prompt)
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
        raw: Senses::default(),
        senses: Senses::default(),
        dnd_toggle: false,
        have_clock: false,
        ring: Ring::new(),
        senses_log: std::env::var_os("BUDDY_SENSES_LOG").is_some_and(|v| v != "0"),
        status: None,
        on_status: None,
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
        on_agent: None,
        on_taint: None,
        on_mode: None,
        policy_mode: "allowlist".into(),
        on_settings: None,
        bubble: Bubble::new(),
        bubble_win: None,
        bubble_loaded: false,
        bubble_label: None,
        bubble_mode: None,
        brain_hidden: false,
        mouth_log: std::env::var_os("BUDDY_STATS").is_some(),
        last_mouth: None,
        unprompted: Unprompted::default(),
        budget_line: Unprompted::default().line(0, None),
        on_budget: None,
        on_ask: None,
        superseded: None,
        listening: false,
    };
    APP.with(|a| *a.borrow_mut() = Some(app));
    forward(rx, handle);
    with_app(|a| {
        a.edges_cfg();
        a.wake()
    });
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
        // the reminder chip's age is worked out against this when a bubble shows (also a deferred one)
        self.bubble.wall_ms = policy::now_ms();
        match m {
            Inbox::Event(ev) => {
                eprintln!("[buddy] event {}", serde_json::to_string(&ev).unwrap_or_default());
                if matches!(ev, MascotEvent::Idle | MascotEvent::Error { .. }) {
                    if let Some((done, _)) = &self.superseded {
                        done.store(true, Ordering::SeqCst);
                    }
                }
                self.brain.event(&ev, now);
                let out = self.bubble.event(&ev, now);
                self.apply(out);
                // the turn ended seated with no answer on screen (stopped, replaced, or none): no bubble
                // hide will come to settle it, so it settles now
                if matches!(ev, MascotEvent::Idle) && self.bubble_mode != Some(Mode::Speech) && self.brain.holds_seat() {
                    eprintln!("[buddy] bubble settled (no answer showing)");
                    self.brain.event(&MascotEvent::Settled, now);
                }
            }
            Inbox::Senses(patch) => {
                // a patch merges over the raw senses; the brain always sees them whole, gated
                if self.senses_log {
                    eprintln!("[buddy] senses {}", serde_json::to_string(&patch).unwrap_or_default());
                }
                self.have_clock |= patch.local.is_some();
                patch.apply(&mut self.raw);
                self.ring.push(now, patch);
                self.regate(now);
                self.sense_turn(now);
            }
            Inbox::Poke(p) => {
                if p == Poke::DragEnd {
                    self.save_position();
                }
                // edges: where the panel is now and, for a drop, the windows under it (one read)
                self.raw.panel = self.panel.rect_tl();
                if p == Poke::DragEnd && self.settings.edges.enabled {
                    self.raw.windows = senses::windows::read();
                }
                if p != Poke::Click {
                    self.regate(now);
                }
                eprintln!("[buddy] poke {p:?}");
                self.brain.poke(p, now);
                if p == Poke::Click {
                    self.open_prompt();
                }
            }
            Inbox::Settings(s) => {
                let action = s.dnd.action;
                self.settings = s;
                self.save_settings();
                self.brain.set_dnd_action(dnd_action(action), now);
                self.edges_cfg();
                self.regate(now);
                if let Some(f) = &self.on_settings {
                    f(&self.settings);
                }
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
        self.edges_cfg();
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

    /// Edges on or off (`settings.edges.enabled`), with the bear's feet at this zoom.
    fn edges_cfg(&mut self) {
        let feet = self.settings.edges.enabled.then(|| self.panel.feet_pt(self.art.anchor()));
        self.brain.set_edges(feet);
        senses::windows::set_active(self.brain.sitting());
    }

    /// Gate the raw senses and hand them to the brain; update the tray status line.
    fn regate(&mut self, now: u64) {
        let g = senses::gate(&self.raw, &self.settings, self.dnd_toggle, self.have_clock);
        let flags = |s: &Senses| (s.fullscreen, s.screen_share, s.dnd_toggle, s.quiet_hours);
        let changed = flags(&g) != flags(&self.senses);
        self.senses = g;
        self.brain.senses(&self.senses, now);
        if self.senses_log && changed {
            let s = &self.senses;
            eprintln!(
                "[buddy] senses gated: fullscreen={} screen_share={} dnd_toggle={} quiet_hours={} -> unprompted_allowed={}",
                s.fullscreen,
                s.screen_share,
                s.dnd_toggle,
                s.quiet_hours,
                self.brain.unprompted_allowed()
            );
        }
        let st = status(&self.senses, self.settings.dnd.action);
        if st != self.status {
            self.status = st;
            if let Some(f) = &self.on_status {
                f(st);
            }
        }
        self.refresh_budget(now);
    }

    /// A sense sample may start an unprompted model turn (budgeted, off the main thread).
    fn sense_turn(&mut self, now: u64) {
        let busy = self.bubble.running() || matches!(self.bubble.shown().map(|m| m.mode), Some(Mode::Prompt | Mode::Card | Mode::Reminder));
        let h = harness::get();
        let ready = unprompted::Ready { busy, can_turn: h.as_ref().is_some_and(|h| h.can_turn()) };
        if let (Some(summary), Some(h)) = (self.unprompted.sample(&self.senses, now, self.brain.unprompted_allowed(), ready), h) {
            std::thread::spawn(move || {
                if let Err(e) = h.prompt_sense(&summary) {
                    eprintln!("[buddy] unprompted turn: {e}");
                }
            });
        }
        self.refresh_budget(now);
    }

    /// The tray's "Check-ins: N of 4 this hour" / "Check-ins paused (…)", on change.
    fn refresh_budget(&mut self, now: u64) {
        let paused = (!self.brain.unprompted_allowed()).then(|| unprompted::paused(&self.senses).unwrap_or("do not disturb"));
        let line = self.unprompted.line(now, paused);
        if line != self.budget_line {
            self.budget_line = line;
            if let Some(f) = &self.on_budget {
                f(&self.budget_line);
            }
        }
    }

    pub fn budget_line(&self) -> &str {
        &self.budget_line
    }

    /// A sign-in picked the model (its provider had no credential before): it's the setting now.
    pub fn model_chosen(&mut self, m: settings::Model) {
        eprintln!("[buddy] model {}/{}", m.provider, m.model);
        self.settings.model = m;
        self.save_settings();
    }

    /// Tray "Do not disturb" (session only).
    pub fn set_dnd_toggle(&mut self, on: bool) {
        self.dnd_toggle = on;
        eprintln!("[buddy] do not disturb {}", if on { "on" } else { "off" });
        let now = self.now_ms();
        self.regate(now);
        self.wake();
    }

    pub fn dnd_toggle(&self) -> bool {
        self.dnd_toggle
    }

    /// Tray "When busy: Hide / Hold still" (persisted as `dnd.action`).
    pub fn set_dnd_action(&mut self, a: DndAct) {
        self.settings.dnd.action = a;
        self.save_settings();
        let now = self.now_ms();
        self.brain.set_dnd_action(dnd_action(a), now);
        self.regate(now);
        self.wake();
    }

    /// Tray "Quiet hours" (persisted as `quiet_hours.enabled`).
    pub fn set_quiet_enabled(&mut self, on: bool) {
        self.settings.quiet_hours.enabled = on;
        self.save_settings();
        let now = self.now_ms();
        self.regate(now);
        self.wake();
    }

    pub fn set_user_hidden(&mut self, h: bool) {
        self.user_hidden = h;
        // the body tools refuse with reason "hidden" while the person has hidden the bear
        let now = self.now_ms();
        self.brain.set_user_hidden(h, now);
        eprintln!("[buddy] bear visible={}", !h);
        if let Some(f) = &self.on_hidden {
            f(h);
        }
        self.wake();
    }

    /// A `buddy mascot …` request (design `#host-api`): the brain's reply as JSON, `playing.untilMs`
    /// moved from the brain's clock to epoch ms.
    pub fn body(&mut self, req: &mascot_core::body::BodyReq) -> serde_json::Value {
        let now = self.now_ms();
        let mut r = self.brain.body(req, now);
        if let Some(Some(p)) = &mut r.playing {
            if let Some(u) = &mut p.until_ms {
                *u = policy::now_ms() + u.saturating_sub(now);
            }
        }
        self.wake();
        serde_json::to_value(r).unwrap_or_default()
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
        if let Some(p) = show.place {
            // edges: sit on a window's top edge, follow it, or go back
            self.panel.move_tl(p.x, p.y);
            self.raw.panel = self.panel.rect_tl();
        }
        senses::windows::set_active(self.brain.sitting());
        if self.last_tick_ms.replace(now) != Some(now) {
            self.view.cam[0] += show.moved_px[0];
            self.view.cam[1] += show.moved_px[1];
        }
        let brain_hidden = show.visibility == Visibility::Hidden;
        if brain_hidden != self.brain_hidden {
            self.brain_hidden = brain_hidden;
            if let Some(f) = &self.on_ask {
                f(brain_hidden);
            }
            match brain_hidden {
                true => eprintln!("[buddy] hide: {}", hide_reason(&self.senses)),
                false => eprintln!("[buddy] hide: none (shown again)"),
            }
        }
        let hidden = self.user_hidden || brain_hidden;
        self.panel.set_shown(!hidden);
        if let Some(w) = &self.bubble_win {
            w.set_bear_hidden(hidden, &self.panel.panel, self.bubble.shown().is_some());
        }
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
        if self.mouth_log {
            let m = (show.intent == Intent::Talk).then_some(show.mouth);
            if m != self.last_mouth {
                if let Some(m) = m {
                    eprintln!("[buddy] mouth {m:?}");
                }
                self.last_mouth = m;
            }
        }
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

/// Why the brain hid the bear (the gated senses that hide it).
fn hide_reason(s: &Senses) -> &'static str {
    match () {
        _ if s.fullscreen => "fullscreen app",
        _ if s.screen_share => "screen sharing",
        _ if s.dnd_toggle => "do not disturb",
        _ if s.quiet_hours => "quiet hours",
        _ => "busy",
    }
}

/// The speech bubble's side of the app: what goes in, what the window does.
impl App {
    /// Show, hide or keep what the bubble state machine decided; re-arm its hide timer.
    pub fn apply(&mut self, out: Out) {
        let mode = self.bubble.shown().map(|m| m.mode);
        if speech_settled(std::mem::replace(&mut self.bubble_mode, mode), mode) {
            eprintln!("[buddy] bubble settled");
            let now = self.now_ms();
            self.brain.event(&MascotEvent::Settled, now);
            self.wake();
        }
        // the brain listens while the prompt is really up (not the setup bubble, not a kept card)
        if let Some(p) = listen_poke(self.listening, prompt_shown(&self.bubble)) {
            self.listening = p == Poke::PromptOpen;
            eprintln!("[buddy] poke {p:?}");
            let now = self.now_ms();
            self.brain.poke(p, now);
            self.wake();
        }
        let Some(w) = &self.bubble_win else { return };
        match out {
            Out::Show(m) => {
                let label = m.label();
                if self.bubble_label.as_ref() != Some(&label) {
                    eprintln!("[buddy] bubble show {label}");
                    self.bubble_label = Some(label);
                }
                w.show(&m, &self.panel.panel);
            }
            Out::Hide => {
                self.bubble_label = None;
                eprintln!("[buddy] bubble hide");
                w.hide(&self.panel.panel);
            }
            Out::Keep => {}
        }
        let now = self.now_ms();
        w.arm(self.bubble.hide_at().map(|t| t.saturating_sub(now) as f64 / 1000.0));
    }

    /// The bubble's hide timer.
    pub fn bubble_tick(&mut self) {
        let hovered = self.bubble_win.as_ref().is_some_and(|w| w.hovered());
        let out = self.bubble.tick(self.now_ms(), hovered);
        self.apply(out);
    }

    /// Click on the bear or tray → Ask… A bear the person hid comes back first (R-ask-hidden):
    /// the bubble hangs off it.
    pub fn open_prompt(&mut self) {
        if self.user_hidden {
            self.set_user_hidden(false);
        }
        let out = self.bubble.open_prompt();
        self.apply(out);
    }

    /// The person sent a prompt: to the agent, off the main thread. Mid-turn (R-midturn), the
    /// running turn is aborted first and the prompt goes once it has ended (≤ 5 s), so none of
    /// its late text lands in the new answer.
    pub fn submit(&mut self, text: String) {
        let midturn = self.bubble.running();
        // sent: the brain goes from listen straight to think (not via PromptClosed)
        self.listening = false;
        eprintln!("[buddy] poke {:?}", Poke::PromptSent);
        let now = self.now_ms();
        self.brain.poke(Poke::PromptSent, now);
        // still waiting for a replaced turn (also after ■ on it): its late text stays quiet
        let out = if midturn || self.superseded.is_some() { self.bubble.supersede(&text) } else { self.bubble.submitted(&text) };
        self.apply(out);
        if let Some(w) = &self.bubble_win {
            w.give_keyboard_back();
        }
        eprintln!("[buddy] prompt {}{}", serde_json::to_string(&text).unwrap_or_default(), if midturn { " (replaces the running turn)" } else { "" });
        // already waiting for a replaced turn to end (it may have ended, its send not yet run):
        // this text goes instead, once
        if let Some((_, pending)) = &mut self.superseded {
            *pending = text;
            return;
        }
        if !midturn {
            return send_prompt(text);
        }
        let done = Arc::new(AtomicBool::new(false));
        self.superseded = Some((done.clone(), text));
        std::thread::spawn(move || {
            if let Some(h) = harness::get() {
                if let Err(e) = h.abort() {
                    eprintln!("[buddy] stop: {e}");
                }
            }
            let t = Instant::now();
            while !done.load(Ordering::SeqCst) && t.elapsed() < SUPERSEDE_WAIT {
                std::thread::sleep(Duration::from_millis(25));
            }
            if !done.load(Ordering::SeqCst) {
                eprintln!("[buddy] the replaced turn didn't end in {} s; sending anyway", SUPERSEDE_WAIT.as_secs());
            }
            // on the main thread (FIFO inbox: the unstop lands before any event of the new
            // turn), so the latest text is the one sent
            post(Inbox::Call(Box::new(|a| {
                let Some((_, text)) = a.superseded.take() else { return };
                let out = a.bubble.unstop();
                a.apply(out);
                send_prompt(text);
            })));
        });
    }

    /// Allow once / Always / Deny: to the policy gate, off the main thread.
    pub fn card_answer(&mut self, call_id: String, answer: String) {
        if answer == "allow_always" && !self.bubble.can_always(&call_id) {
            eprintln!("[buddy] card {call_id}: \"Always\" isn't offered now");
            return;
        }
        eprintln!("[buddy] card {call_id}: {answer}");
        std::thread::spawn(move || {
            if let Some(h) = harness::get() {
                if let Err(e) = h.answer(&call_id, &answer) {
                    eprintln!("[buddy] card answer: {e}");
                }
            }
            post(Inbox::Call(Box::new(move |a| a.bubble_answered(&call_id))));
        });
    }

    /// The card was answered (by the person or `BUDDY_ANSWER`): the brain settles.
    pub fn bubble_answered(&mut self, call_id: &str) {
        let out = self.bubble.answered(call_id, self.now_ms());
        if out != Out::Keep {
            let now = self.now_ms();
            self.brain.event(&MascotEvent::Settled, now);
        }
        self.apply(out);
        self.wake();
    }

    /// ✕ / Esc / Close / Done, and ■ on the streaming answer (which also stops the turn).
    pub fn bubble_dismiss(&mut self) {
        if self.bubble.dismiss_stops() {
            eprintln!("[buddy] stop");
            std::thread::spawn(|| {
                if let Some(h) = harness::get() {
                    if let Err(e) = h.abort() {
                        eprintln!("[buddy] stop: {e}");
                    }
                }
            });
        }
        let out = self.bubble.dismiss(self.now_ms());
        self.apply(out);
        if let Some(w) = &self.bubble_win {
            w.give_keyboard_back();
        }
    }

    /// The page measured itself. Its first report means it loaded: resend what's shown.
    pub fn bubble_sized(&mut self, w: f64, h: f64) {
        let Some(win) = &self.bubble_win else { return };
        if !std::mem::replace(&mut self.bubble_loaded, true) {
            if let Some(m) = self.bubble.shown() {
                win.show(m, &self.panel.panel);
                return;
            }
        }
        win.sized(w, h, &self.panel.panel);
    }

    /// Try again (error bubble): the last prompt once more. With no model: "Sign in…". An
    /// unprompted turn's error has neither (Close only), so nothing is re-sent for it.
    pub fn bubble_retry(&mut self) {
        let action = self.bubble.shown().and_then(|m| m.error.as_ref()).and_then(|e| e.action.clone());
        match (action.as_deref(), self.bubble.last_prompt().map(str::to_string)) {
            (Some("sign_in"), _) => {
                crate::signin::window::open();
                self.bubble_dismiss()
            }
            (Some("retry"), Some(p)) => self.submit(p),
            // the setup bubble's "Set up…"
            _ if self.bubble.is_setup() => {
                crate::signin::window::open();
                self.bubble_dismiss()
            }
            _ => self.bubble_dismiss(),
        }
    }

    /// The prompt's "Last answer": the last finished answer again.
    pub fn bubble_recall(&mut self) {
        let out = self.bubble.recall(self.now_ms());
        self.apply(out);
        if let Some(w) = &self.bubble_win {
            w.give_keyboard_back();
        }
    }

    /// Snooze: the reminder's text again in `minutes`, then the bubble goes.
    pub fn reminder_snooze(&mut self, id: String, minutes: u32) {
        let text = self.bubble.reminder().filter(|(i, _)| *i == id).map(|(_, t)| t.clone());
        if let Some(text) = text {
            eprintln!("[buddy] snooze {id} {minutes} min");
            std::thread::spawn(move || match harness::get().map(|h| h.snooze(&text, minutes)) {
                Some(Ok(_)) => {}
                Some(Err(e)) => eprintln!("[buddy] snooze: {}", e.message),
                None => eprintln!("[buddy] snooze: the agent isn't set up"),
            });
        }
        let out = self.bubble.dismiss(self.now_ms());
        self.apply(out);
    }

    /// A `buddy say` line (from the host).
    pub fn bubble_say(&mut self, text: String, emotion: Option<String>, merge: bool) {
        let out = self.bubble.say(&text, emotion, merge, self.now_ms());
        self.apply(out);
    }

    /// An approval card's details.
    pub fn bubble_approval(&mut self, call_id: &str, data: serde_json::Value) {
        let out = self.bubble.approval(call_id, data);
        self.apply(out);
    }

    /// The prompt didn't go through.
    pub fn bubble_failed(&mut self, e: &bridge::BridgeError) {
        eprintln!("[buddy] prompt failed: {e}");
        let out = self.bubble.failed(e.code, &e.message);
        self.apply(out);
        // no turn will come: the bear stops thinking (PromptSent started it)
        let now = self.now_ms();
        self.brain.event(&MascotEvent::Idle, now);
        self.wake();
    }

    /// The policy's view changed (taint, mode, rules, pending).
    pub fn policy_changed(&mut self, view: &serde_json::Value) {
        let tainted = !view["taint"].is_null();
        if let Some(f) = &self.on_taint {
            f(tainted);
        }
        if let Some(mode) = view["mode"].as_str() {
            self.policy_mode = mode.to_string();
        }
        self.policy_mode_refresh();
        let out = self.bubble.set_tainted(tainted);
        self.apply(out);
    }

    /// The tray's Approvals checkmarks, from the policy's last view (a click toggles the native
    /// checkmark by itself; this puts it back on what the policy really has).
    pub fn policy_mode_refresh(&self) {
        if let Some(f) = &self.on_mode {
            f(&self.policy_mode);
        }
    }

    /// The agent started, stopped, crashed, …
    pub fn agent_changed(&mut self, s: &AgentState) {
        if let Some(f) = &self.on_agent {
            f(s);
        }
        let signed_out = matches!(s, AgentState::SignedOut | AgentState::Keychain(_));
        let ready = matches!(s, AgentState::Running { .. });
        let now = self.now_ms();
        let mut out = self.bubble.agent(signed_out, ready, now);
        // the very first launch: say hello, or offer to connect a model, once (settings.onboarded)
        if (signed_out || ready) && !self.settings.onboarded {
            self.settings.onboarded = true;
            self.save_settings();
            if out == Out::Keep {
                out = if signed_out { self.bubble.setup() } else { self.bubble.say(crate::bubble::model::WELCOME_LINE, Some("happy".into()), false, now) };
            }
        }
        self.apply(out);
    }
}

/// One prompt to the agent, off the main thread; a failure comes back as the error bubble.
fn send_prompt(text: String) {
    std::thread::spawn(move || {
        let r = match harness::get() {
            Some(h) => h.prompt(&text).map(|_| ()),
            None => Err(bridge::BridgeError::new("no_agent", "The agent isn't set up.")),
        };
        if let Err(e) = r {
            post(Inbox::Call(Box::new(move |a| a.bubble_failed(&e))));
        }
    });
}

pub fn dnd_action(a: DndAct) -> DndAction {
    match a {
        DndAct::Hide => DndAction::Hide,
        DndAct::Still => DndAction::Still,
    }
}

/// The tray status line for the gated senses (None = nothing active).
pub fn status(s: &Senses, action: DndAct) -> Option<&'static str> {
    let hide = action == DndAct::Hide;
    Some(match () {
        _ if s.fullscreen => if hide { "Hidden: fullscreen app" } else { "Still: fullscreen app" },
        _ if s.screen_share => if hide { "Hidden: screen sharing" } else { "Still: screen sharing" },
        _ if s.dnd_toggle => if hide { "Hidden: do not disturb" } else { "Still: do not disturb" },
        _ if s.quiet_hours => "Sleepy: quiet hours",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_bubble_settles_when_it_goes_or_the_prompt_replaces_it() {
        assert!(speech_settled(Some(Mode::Speech), None), "dismissed or timed out");
        assert!(speech_settled(Some(Mode::Speech), Some(Mode::Prompt)), "the person clicked for a new prompt");
        assert!(speech_settled(Some(Mode::Speech), Some(Mode::Reminder)), "a reminder took its place");
        assert!(speech_settled(Some(Mode::Speech), Some(Mode::Error)), "an error took its place");
        assert!(!speech_settled(Some(Mode::Speech), Some(Mode::Card)), "a card: the turn goes on");
        assert!(!speech_settled(Some(Mode::Speech), Some(Mode::Speech)), "still talking");
        assert!(!speech_settled(Some(Mode::Card), None), "the card's Settled comes from its answer");
        assert!(!speech_settled(None, None));
        // the bubble's own path: an answer, then its hide timer
        let mut b = Bubble::new();
        b.submitted("hi");
        b.event(&MascotEvent::Speaking { text: "Hello.".into() }, 0);
        b.event(&MascotEvent::Idle, 100);
        let before = b.shown().map(|m| m.mode);
        assert_eq!(before, Some(Mode::Speech));
        let out = b.dismiss(200);
        assert_eq!(out, Out::Hide);
        assert!(speech_settled(before, b.shown().map(|m| m.mode)));
    }

    #[test]
    fn listen_pokes_follow_the_prompt_on_screen() {
        assert_eq!(listen_poke(false, true), Some(Poke::PromptOpen));
        assert_eq!(listen_poke(true, true), None, "still up: once");
        assert_eq!(listen_poke(true, false), Some(Poke::PromptClosed));
        assert_eq!(listen_poke(false, false), None);
        // a click while a card is up keeps the card: no prompt, so no PromptOpen
        let mut b = Bubble::new();
        b.submitted("hi");
        b.event(&MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run?".into() }, 0);
        assert_eq!(b.open_prompt(), Out::Keep);
        assert_eq!(listen_poke(false, prompt_shown(&b)), None);
        // the setup bubble (signed out) is not the prompt either
        let mut b = Bubble::new();
        b.agent(true, false, 0);
        b.open_prompt();
        assert!(b.is_setup());
        assert_eq!(listen_poke(false, prompt_shown(&b)), None);
        let mut b = Bubble::new();
        b.open_prompt();
        assert_eq!(listen_poke(false, prompt_shown(&b)), Some(Poke::PromptOpen));
    }

    #[test]
    fn status_line_wording() {
        let mut s = Senses::default();
        assert_eq!(status(&s, DndAct::Hide), None);
        s.quiet_hours = true;
        assert_eq!(status(&s, DndAct::Hide), Some("Sleepy: quiet hours"));
        s.dnd_toggle = true;
        assert_eq!(status(&s, DndAct::Still), Some("Still: do not disturb"));
        assert_eq!(status(&s, DndAct::Hide), Some("Hidden: do not disturb"));
        s.fullscreen = true;
        assert_eq!(status(&s, DndAct::Hide), Some("Hidden: fullscreen app"));
    }
}
