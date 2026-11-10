//! The brain (design §4.3): harness events, senses and pokes in; what to show
//! out. Deterministic: same pack + seed + inputs ⇒ same `Show` sequence. No
//! wall clock, no global RNG; time comes in as `now_ms`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use mascot_protocol::pack::{Loops, Temperament, Turn};
use mascot_protocol::{EmoteName, Emotion, MascotEvent};
use serde::{Deserialize, Serialize};

use crate::pack::{Dir, Ori, PackMeta};
use crate::reflex::Reflexes;
use crate::rng::Rng;

/// What the mascot means to do; packs map intents to behaviors.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Idle,
    Think,
    Work,
    Talk,
    Ask,
    Oops,
    Happy,
    Celebrate,
    Sad,
    Surprised,
    Sleepy,
    Wave,
    Nod,
    Shake,
    Jump,
    Spin,
    Greet,
    Stretch,
    Remind,
    Dodge,
}

impl Intent {
    pub const ALL: [Intent; 20] = [
        Intent::Idle,
        Intent::Think,
        Intent::Work,
        Intent::Talk,
        Intent::Ask,
        Intent::Oops,
        Intent::Happy,
        Intent::Celebrate,
        Intent::Sad,
        Intent::Surprised,
        Intent::Sleepy,
        Intent::Wave,
        Intent::Nod,
        Intent::Shake,
        Intent::Jump,
        Intent::Spin,
        Intent::Greet,
        Intent::Stretch,
        Intent::Remind,
        Intent::Dodge,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Intent::Idle => "idle",
            Intent::Think => "think",
            Intent::Work => "work",
            Intent::Talk => "talk",
            Intent::Ask => "ask",
            Intent::Oops => "oops",
            Intent::Happy => "happy",
            Intent::Celebrate => "celebrate",
            Intent::Sad => "sad",
            Intent::Surprised => "surprised",
            Intent::Sleepy => "sleepy",
            Intent::Wave => "wave",
            Intent::Nod => "nod",
            Intent::Shake => "shake",
            Intent::Jump => "jump",
            Intent::Spin => "spin",
            Intent::Greet => "greet",
            Intent::Stretch => "stretch",
            Intent::Remind => "remind",
            Intent::Dodge => "dodge",
        }
    }

    pub fn parse(s: &str) -> Option<Intent> {
        Intent::ALL.into_iter().find(|i| i.as_str() == s)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrainConfig {
    pub seed: u64,
    /// Overrides the pack's temperament.
    pub temperament: Option<Temperament>,
    pub dnd_action: DndAction,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DndAction {
    #[default]
    Hide,
    Still,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Poke {
    Click,
    DragStart,
    DragEnd,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Shown,
    Still,
    Hidden,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EyeState {
    Open,
    Closed,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouthState {
    Closed,
    Open,
    Wide,
}

/// What the app observed about the desktop. The app gates `fullscreen` and
/// `screen_share` by settings before passing them (false when disabled).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Senses {
    pub frontmost: Option<FrontApp>,
    pub user_idle_ms: u64,
    pub local: LocalTime,
    pub fullscreen: bool,
    pub screen_share: bool,
    pub dnd_toggle: bool,
    pub quiet_hours: bool,
    pub windows: Vec<WinRect>,
    pub panel: Option<WinRect>,
    pub now_playing: Option<NowPlaying>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FrontApp {
    pub bundle_id: String,
    pub category: AppCategory,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppCategory {
    Game,
    Video,
    Meeting,
    Presentation,
    Code,
    Browser,
    Chat,
    Other,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LocalTime {
    /// 0..1440
    pub minutes: u16,
    /// 0 = Monday
    pub weekday: u8,
}

/// Points, top-left origin.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub owner_pid: i32,
    pub layer: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub source: String,
    pub title: String,
    pub playing: bool,
}

/// One frame's worth of decisions for the renderer.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Show {
    pub intent: Intent,
    pub behavior: String,
    pub clip: String,
    pub dir: Dir,
    pub ori: Option<Ori>,
    pub frame: u16,
    pub eyes: EyeState,
    pub mouth: MouthState,
    pub effect: Option<String>,
    pub visibility: Visibility,
    /// Whole-pixel root motion accumulated since the previous tick (fraction kept inside).
    pub moved_px: [i32; 2],
    /// A set appeared/disappeared or the shot changed this tick.
    pub cut: bool,
    /// Edges (p5): where the panel should sit; `None` = stay.
    pub place: Option<Placement>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub follow_pid: Option<i32>,
}

/// One brain decision, for replay and studio.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TraceLine {
    pub t_ms: u64,
    /// `event`, `start`, `pick`, `end`, `vis`, `poke`, `edge`.
    pub what: &'static str,
    pub intent: Intent,
    pub behavior: String,
    pub note: String,
}

const TRACE_N: usize = 256;
/// Queued out-of-turn cues (`say`, `emote`, reminders, reflexes) at most.
const CUE_CAP: usize = 4;
/// `advance` replays at most this much of a gap (after a long hide or
/// occlusion the bear picks up from `now − CATCHUP_MS`).
const CATCHUP_MS: u64 = 10_000;
/// Open eyes between the two blinks of a double blink.
const DOUBLE_GAP_FRAMES: u64 = 2;

/// How long a `Say` / `Reminder` line holds its intent: the bubble's time.
/// Talk (p3) uses the same number for the bubble.
pub fn bubble_ms(text: &str) -> u64 {
    (1500 + 50 * text.chars().count() as u64).clamp(1500, 8000)
}

/// What the run's state asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TurnState {
    /// Until another harness event (or Idle; Settled for Ask/Talk).
    Held(Intent),
    /// Plays its behavior once.
    Once(Intent),
}

/// An out-of-turn cue (`say`, `emote`, reminder, reflex): over the turn while
/// it lasts. Cues queue; only the front one plays.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cue {
    /// Held for `dur` ms from when it reaches the front (`until`), or (a Say,
    /// intent Talk) until Settled, then `then` once.
    Timed { intent: Intent, dur: u64, until: Option<u64>, then: Option<Intent> },
    Once(Intent),
    /// A reflex's one-shot: it yields to a harness turn and to DND.
    Reflex(Intent),
}

impl Cue {
    fn intent(&self) -> Intent {
        match *self {
            Cue::Timed { intent, .. } | Cue::Once(intent) | Cue::Reflex(intent) => intent,
        }
    }

    fn once(&self) -> Option<Intent> {
        match *self {
            Cue::Once(i) | Cue::Reflex(i) => Some(i),
            Cue::Timed { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
struct Play {
    intent: Intent,
    behavior: String,
    step: usize,
    /// Iterations left of the current step; `None` = hold.
    left: Option<u32>,
    frame: u16,
    frame_start: u64,
    ori: Option<Ori>,
    /// Frame pinned (Still): no frame advance.
    pinned: bool,
    /// The `hold` a drag started: it ends when the drag does.
    drag: bool,
}

pub struct Brain {
    pack: Arc<PackMeta>,
    cfg: BrainConfig,
    temperament: Temperament,
    rng: Rng,
    t: u64,
    play: Play,
    dir: Dir,
    turn: Option<TurnState>,
    cue: VecDeque<Cue>,
    dragging: bool,
    vis: Visibility,
    quiet: bool,
    /// DND from the last senses (fullscreen || screen share || toggle).
    dnd: bool,
    /// `late` reflex: sleepiness + 0.2.
    late: bool,
    reflex: Reflexes,
    /// Edges: the bear's feet in the panel (points); `None` = off (`settings.edges.enabled`).
    edge_feet: Option<[i32; 2]>,
    sit: Option<crate::reflex::Sit>,
    /// Where the panel was when the current drag started.
    drag_from: Option<Placement>,
    /// The latest panel and windows (from `senses`).
    panel: Option<WinRect>,
    windows: Vec<WinRect>,
    /// A placement for the next `tick`.
    place: Option<Placement>,
    /// `Brain::new` time: the warm-up runs from here.
    started: u64,
    // idle scheduler memory
    last_start: BTreeMap<String, u64>,
    recent: VecDeque<String>,
    // timers
    blink_at: u64,
    blink_first: u64,
    closed_until: Option<u64>,
    double_pending: bool,
    talking_until: u64,
    mouth: MouthState,
    mouth_at: u64,
    mouth_step: u32,
    speak_len: Option<usize>,
    // per-tick output
    moved: [f32; 2],
    cut: bool,
    cache: Option<(u64, Show)>,
    dirty: bool,
    trace: VecDeque<TraceLine>,
}

impl Brain {
    pub fn new(pack: Arc<PackMeta>, cfg: BrainConfig, now_ms: u64) -> Brain {
        let temperament = cfg.temperament.unwrap_or(pack.temperament);
        let mut b = Brain {
            rng: Rng::new(cfg.seed),
            pack,
            cfg,
            temperament,
            t: now_ms,
            play: Play { intent: Intent::Idle, behavior: String::new(), step: 0, left: None, frame: 0, frame_start: now_ms, ori: None, pinned: false, drag: false },
            dir: Dir::S,
            turn: None,
            cue: VecDeque::new(),
            dragging: false,
            vis: Visibility::Shown,
            quiet: false,
            dnd: false,
            late: false,
            reflex: Reflexes::new(),
            edge_feet: None,
            sit: None,
            drag_from: None,
            panel: None,
            windows: vec![],
            place: None,
            started: now_ms,
            last_start: BTreeMap::new(),
            recent: VecDeque::new(),
            blink_at: u64::MAX,
            blink_first: now_ms,
            closed_until: None,
            double_pending: false,
            talking_until: 0,
            mouth: MouthState::Closed,
            mouth_at: u64::MAX,
            mouth_step: 0,
            speak_len: None,
            moved: [0.0; 2],
            cut: false,
            cache: None,
            dirty: true,
            trace: VecDeque::new(),
        };
        b.start("rest", Intent::Idle, now_ms);
        b.schedule_blink(now_ms);
        b
    }

    pub fn event(&mut self, ev: &MascotEvent, now_ms: u64) {
        self.advance(now_ms);
        let t = self.t;
        match ev {
            MascotEvent::Idle => {
                if matches!(self.turn, Some(TurnState::Held(_))) {
                    self.turn = None;
                }
                self.speak_len = None;
            }
            MascotEvent::Thinking => self.set_turn(TurnState::Held(Intent::Think)),
            MascotEvent::Working { .. } => self.set_turn(TurnState::Held(Intent::Work)),
            MascotEvent::AwaitingPermission { .. } => self.set_turn(TurnState::Held(Intent::Ask)),
            MascotEvent::Speaking { text } => {
                let len = text.chars().count();
                if self.speak_len.is_none_or(|l| len > l) {
                    self.talk_until(t + self.pack.timers.mouth.stall_ms as u64);
                }
                self.speak_len = Some(len);
                self.set_turn(TurnState::Held(Intent::Talk));
            }
            MascotEvent::Error { .. } => self.set_turn(TurnState::Once(Intent::Oops)),
            MascotEvent::Say { text, emotion } => {
                let until = t + bubble_ms(text);
                let then = emotion.and_then(|e| match e {
                    Emotion::Neutral => None,
                    Emotion::Happy => Some(Intent::Happy),
                    Emotion::Sad => Some(Intent::Sad),
                    Emotion::Surprised => Some(Intent::Surprised),
                    Emotion::Thinking => Some(Intent::Think),
                    Emotion::Sleepy => Some(Intent::Sleepy),
                });
                // the latest line wins over a queued or playing one
                let say = Cue::Timed { intent: Intent::Talk, dur: until - t, until: None, then };
                match self.cue.iter().position(|c| matches!(c, Cue::Timed { intent: Intent::Talk, .. })) {
                    Some(i) => self.cue[i] = say,
                    None => self.push_cue(say),
                }
                self.activate_cue(t);
            }
            // a moment reaction: nothing to react with while hidden or held still
            MascotEvent::Emote { name } if self.vis == Visibility::Shown => {
                self.push_cue(Cue::Once(match name {
                    EmoteName::Wave => Intent::Wave,
                    EmoteName::Nod => Intent::Nod,
                    EmoteName::Shake => Intent::Shake,
                    EmoteName::Jump => Intent::Jump,
                    EmoteName::Spin => Intent::Spin,
                }));
            }
            MascotEvent::Emote { .. } => {}
            MascotEvent::Reminder { text, .. } => {
                self.push_cue(Cue::Timed { intent: Intent::Remind, dur: bubble_ms(text), until: None, then: None });
                self.activate_cue(t);
            }
            MascotEvent::Settled => {
                if matches!(self.turn, Some(TurnState::Held(Intent::Ask | Intent::Talk))) {
                    self.turn = None;
                }
                // cut the mouth first: a Say queued behind starts its own below
                self.talking_until = self.talking_until.min(t);
                // the turn's Say ends with it; a Reminder showing is not the turn's and keeps its time
                if let Some(&Cue::Timed { intent: Intent::Talk, then, .. }) = self.cue.front() {
                    self.finish_timed(then, t);
                }
            }
        }
        if !matches!(ev, MascotEvent::Speaking { .. }) {
            self.speak_len = None;
        }
        let what = match ev {
            MascotEvent::Idle => "Idle",
            MascotEvent::Thinking => "Thinking",
            MascotEvent::Working { .. } => "Working",
            MascotEvent::AwaitingPermission { .. } => "AwaitingPermission",
            MascotEvent::Speaking { .. } => "Speaking",
            MascotEvent::Error { .. } => "Error",
            MascotEvent::Say { .. } => "Say",
            MascotEvent::Emote { .. } => "Emote",
            MascotEvent::Reminder { .. } => "Reminder",
            MascotEvent::Settled => "Settled",
        };
        self.note(t, "event", what.to_string());
        self.settle(t);
    }

    pub fn senses(&mut self, s: &Senses, now_ms: u64) {
        self.advance(now_ms);
        let t = self.t;
        self.panel = s.panel;
        self.windows.clone_from(&s.windows);
        self.follow_edge(t, s.fullscreen);
        self.quiet = s.quiet_hours;
        self.dnd = s.fullscreen || s.screen_share || s.dnd_toggle;
        self.apply_vis(t);
        let off = !self.unprompted_allowed();
        if off {
            self.drop_cues(false); // reflexes are off in DND and quiet hours
        }
        let free = self.cue.is_empty() && self.turn.is_none() && !self.dragging && self.vis == Visibility::Shown;
        let shyness = self.temperament.shyness;
        let rng = &mut self.rng;
        if let Some(f) = self.reflex.sample(s, t, off, free, shyness, &mut |p| rng.chance(p)) {
            self.note(t, "reflex", f.rule.to_string());
            self.push_cue(Cue::Reflex(f.intent));
        }
        let late = self.reflex.late(s, off);
        if late != self.late {
            self.late = late;
            self.note(t, "reflex", format!("{} {}", crate::reflex::LATE, if late { "on" } else { "off" }));
        }
        self.settle(t);
    }

    /// Unprompted turns (sense-triggered, p4's budget) may start: not in DND
    /// and not in quiet hours.
    pub fn unprompted_allowed(&self) -> bool {
        !self.dnd && !self.quiet
    }

    /// Tray "When busy: Hide / Hold still" (additive to §4.3).
    pub fn set_dnd_action(&mut self, a: DndAction, now_ms: u64) {
        self.advance(now_ms);
        let t = self.t;
        self.cfg.dnd_action = a;
        self.apply_vis(t);
        self.settle(t);
    }

    fn apply_vis(&mut self, t: u64) {
        let vis = match (self.dnd, self.cfg.dnd_action) {
            (false, _) => Visibility::Shown,
            (true, DndAction::Hide) => Visibility::Hidden,
            (true, DndAction::Still) => Visibility::Still,
        };
        if vis != self.vis {
            let was = self.vis;
            self.vis = vis;
            if vis != Visibility::Shown {
                // Hidden/Still play no frames, so a one-shot would wait at the front
                // and block the Says and Reminders behind it
                self.drop_cues(true);
            }
            self.note(t, "vis", format!("{was:?} -> {vis:?}"));
            if was == Visibility::Hidden {
                // Nothing played while hidden: pick up from here.
                self.play.frame_start = t;
                self.schedule_blink(t);
                self.mouth_at = t + self.pack.timers.mouth.frame_ms.max(1) as u64;
            }
            if self.play.pinned && vis != Visibility::Still {
                self.play.pinned = false;
                self.end_behavior(t);
            }
            if vis == Visibility::Still {
                self.schedule_blink(t);
            }
        }
    }

    pub fn poke(&mut self, p: Poke, now_ms: u64) {
        self.advance(now_ms);
        let t = self.t;
        self.note(t, "poke", format!("{p:?}"));
        match p {
            Poke::Click => {}
            Poke::DragStart => {
                self.dragging = true;
                self.drag_from = self.panel.map(|p| Placement { x: p.x, y: p.y, follow_pid: None });
                if self.sit.take().is_some() {
                    self.note(t, "edge", "off (picked up)".into());
                }
                if !self.play.drag {
                    self.start("hold", self.desired(), t);
                    self.play.drag = true;
                }
            }
            Poke::DragEnd => {
                self.dragging = false;
                self.drop_on_edge(t);
            }
        }
        self.dirty = true;
    }

    /// Edges on (with the bear's feet in the panel, in points) or off. Off stands the bear up where
    /// it is.
    pub fn set_edges(&mut self, feet: Option<[i32; 2]>) {
        self.edge_feet = feet;
        if feet.is_none() {
            self.sit = None;
        }
    }

    /// Sitting on a window's edge (the app polls window bounds only then).
    pub fn sitting(&self) -> bool {
        self.sit.is_some()
    }

    fn drop_on_edge(&mut self, t: u64) {
        let (Some(feet), Some(panel)) = (self.edge_feet, self.panel) else { return };
        if let Some(mut s) = crate::reflex::edge_drop(&panel, feet, &self.windows) {
            s.back = self.drag_from;
            self.note(t, "edge", format!("sit on pid {} at {},{}", s.pid, s.at.x, s.at.y));
            self.place = Some(s.at);
            self.sit = Some(s);
        }
    }

    fn follow_edge(&mut self, t: u64, fullscreen: bool) {
        let (Some(feet), Some(sit)) = (self.edge_feet, self.sit) else { return };
        match crate::reflex::edge_follow(&sit, feet, &self.windows, fullscreen) {
            crate::reflex::Follow::Stay => {}
            crate::reflex::Follow::Move(s) => {
                self.place = Some(s.at);
                self.sit = Some(s);
                self.dirty = true;
            }
            crate::reflex::Follow::Leave(back) => {
                self.note(t, "edge", "off (window gone)".into());
                self.sit = None;
                self.place = back;
                self.dirty = true;
            }
        }
    }

    pub fn set_temperament(&mut self, t: Temperament) {
        self.temperament = t;
        self.dirty = true;
    }

    /// Advances to `now_ms` and says what to show. Idempotent for the same `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> Show {
        if let Some((t, show)) = &self.cache {
            if *t == now_ms && !self.dirty {
                return show.clone();
            }
        }
        self.advance(now_ms);
        let mx = self.moved[0].trunc();
        let my = self.moved[1].trunc();
        self.moved[0] -= mx;
        self.moved[1] -= my;
        let show = Show {
            intent: self.play.intent,
            behavior: self.play.behavior.clone(),
            clip: self.clip_name().to_string(),
            dir: self.dir,
            ori: self.play.ori,
            frame: self.play.frame,
            eyes: self.eyes(),
            mouth: self.mouth_shown(),
            effect: None,
            visibility: self.vis,
            moved_px: [mx as i32, my as i32],
            cut: std::mem::take(&mut self.cut),
            place: self.place.take(),
        };
        // a repeat in the same ms has nothing new: no cut, no motion (R7)
        self.cache = Some((now_ms, Show { cut: false, moved_px: [0, 0], ..show.clone() }));
        self.dirty = false;
        show
    }

    /// The next time `tick` would show something new; `u64::MAX` = nothing
    /// until an input arrives (hidden, or a still pose without face layers).
    pub fn next_wake_ms(&self) -> u64 {
        if self.vis == Visibility::Hidden {
            return u64::MAX;
        }
        let mut next = self.frame_end();
        if let Some(&Cue::Timed { until: Some(until), .. }) = self.cue.front() {
            next = next.min(until);
        }
        if self.face_here() {
            if self.pack.capabilities.blink {
                next = next.min(self.closed_until.unwrap_or(self.blink_at));
            }
            if self.flapping() {
                next = next.min(self.mouth_at.min(self.talking_until));
            }
        }
        next.max(self.t)
    }

    pub fn trace(&self) -> &[TraceLine] {
        self.trace.as_slices().0
    }

    // ---- time ----

    fn advance(&mut self, now: u64) {
        if now < self.t {
            return;
        }
        if now - self.t > CATCHUP_MS {
            // R4: skip a long gap instead of replaying every frame of it; cue
            // and talk deadlines stay absolute and simply expire below
            let skip = now - CATCHUP_MS - self.t;
            let shift = |v: &mut u64| {
                if *v != u64::MAX {
                    *v = v.saturating_add(skip);
                }
            };
            shift(&mut self.play.frame_start);
            shift(&mut self.blink_at);
            shift(&mut self.blink_first);
            shift(&mut self.mouth_at);
            if let Some(c) = &mut self.closed_until {
                *c = c.saturating_add(skip);
            }
            self.t += skip;
        }
        loop {
            let hidden = self.vis == Visibility::Hidden;
            let frame_end = if hidden { u64::MAX } else { self.frame_end() };
            let cue_end = match self.cue.front() {
                Some(&Cue::Timed { until: Some(until), .. }) => until,
                _ => u64::MAX,
            };
            let blink = if hidden { u64::MAX } else { self.closed_until.unwrap_or(self.blink_at) };
            // Only steps inside the talking window, whatever the tick cadence.
            let mouth = if self.flapping() && self.mouth_at < self.talking_until { self.mouth_at } else { u64::MAX };
            let next = frame_end.min(cue_end).min(blink).min(mouth);
            if next > now {
                break;
            }
            let due = next;
            // a deadline that passed during a skipped gap lands now, not in the past
            let next = next.max(self.t);
            self.t = next;
            self.dirty = true;
            if due == cue_end {
                if let Some(&Cue::Timed { then, .. }) = self.cue.front() {
                    self.finish_timed(then, next);
                }
                self.settle(next);
            } else if due == blink {
                self.blink_step(next);
            } else if due == mouth {
                self.mouth_step(next);
            } else {
                self.frame_done(next);
            }
        }
        if now > self.t {
            self.t = now;
        }
    }

    fn frame_end(&self) -> u64 {
        if self.play.pinned {
            return u64::MAX;
        }
        let dur = self.clip().map_or(83, |c| c.durs_ms.get(self.play.frame as usize).copied().unwrap_or(83).max(1));
        self.play.frame_start.saturating_add(dur as u64)
    }

    fn frame_done(&mut self, t: u64) {
        let px = self.clip().and_then(|c| c.dirs.get(&self.dir)).and_then(|s| s.root_px.get(self.play.frame as usize)).copied();
        if let Some(px) = px {
            self.moved[0] += px[0];
            self.moved[1] += px[1];
        }
        let frames_n = self.clip().map_or(1, |c| c.frames_n());
        self.play.frame_start = t;
        self.play.frame += 1;
        if (self.play.frame as usize) >= frames_n {
            self.play.frame = 0;
            // A one-shot cue (e.g. Say's `thinking` follow-up) that resolves to a
            // held loop plays it once, then end_behavior consumes the cue.
            let once_cue = self.cue.front().and_then(Cue::once) == Some(self.play.intent);
            let release = self.desired() != self.play.intent || (self.play.drag && !self.dragging) || once_cue;
            let step_done = match &mut self.play.left {
                None => release,
                Some(n) => {
                    *n = n.saturating_sub(1);
                    *n == 0
                }
            };
            if step_done {
                self.leave_step();
                self.next_step(t);
                return;
            }
        }
        self.boundary(t);
    }

    /// Interrupts that happen at a frame boundary.
    fn boundary(&mut self, t: u64) {
        let want = self.desired();
        if want == self.play.intent || self.dragging {
            return;
        }
        let in_hold = self.play.left.is_none();
        let wanted_behavior = self.pack.resolve(want);
        if self.play.intent != Intent::Idle && wanted_behavior == self.play.behavior {
            // Same behavior (e.g. ask → work both `hold`): just relabel. A
            // one-shot turn being relabelled away counts as played, as it
            // would if interrupt() had ended it.
            if self.turn == Some(TurnState::Once(self.play.intent)) {
                self.turn = None;
            }
            self.play.intent = want;
            return;
        }
        if self.play.intent == Intent::Idle || want != Intent::Idle {
            if in_hold && self.play.intent != Intent::Idle {
                return; // a held loop finishes its iteration first
            }
            self.interrupt(t);
        }
    }

    /// Leaves the current behavior now: a set behavior first plays its exit clip.
    fn interrupt(&mut self, t: u64) {
        let Some(def) = self.pack.behaviors.get(&self.play.behavior) else { return };
        let last = def.seq.len() - 1;
        if def.set.is_some() && self.play.step < last {
            self.leave_step();
            self.enter_step(last, t);
            self.note(t, "end", "exit set".into());
        } else if def.set.is_none() {
            self.end_behavior(t);
        }
        // In the exit clip already: let it finish.
    }

    fn leave_step(&mut self) {
        // Leaving a set clip: the free bear stands at the hand-off, facing its way.
        if let (Some(c), Some(ori)) = (self.clip(), self.play.ori) {
            if let Some(h) = c.handoff.get(&ori) {
                if !h.at_start {
                    self.dir = h.facing;
                }
            }
        }
    }

    fn next_step(&mut self, t: u64) {
        let n = self.pack.behaviors.get(&self.play.behavior).map_or(0, |d| d.seq.len());
        if self.play.step + 1 < n {
            self.enter_step(self.play.step + 1, t);
            self.boundary(t);
        } else {
            self.end_behavior(t);
        }
    }

    fn enter_step(&mut self, step: usize, t: u64) {
        let loops = self.pack.behaviors[&self.play.behavior].seq[step].loops;
        self.play.step = step;
        self.play.frame = 0;
        self.play.frame_start = t;
        self.play.left = match loops {
            Loops::Once => Some(1),
            Loops::Range(lo, hi) => Some(self.rng.range(lo, hi)),
            Loops::Hold => None,
        };
    }

    fn end_behavior(&mut self, t: u64) {
        let was = self.play.intent;
        self.note(t, "end", String::new());
        if self.play.intent != Intent::Idle {
            if let Some(i) = self.cue.front().and_then(Cue::once) {
                if i == was {
                    self.cue.pop_front();
                    self.activate_cue(t);
                }
            }
            if self.turn == Some(TurnState::Once(was)) && self.cue.front().is_none_or(|c| c.intent() != was) {
                self.turn = None;
            }
        }
        let want = self.desired();
        if self.dragging {
            self.start("hold", want, t);
            self.play.drag = true;
        } else if want == Intent::Idle {
            if self.vis == Visibility::Still {
                self.start("hold", Intent::Idle, t);
                self.pin(t);
            } else {
                let b = self.pick(t);
                self.start(&b, Intent::Idle, t);
            }
        } else {
            let b = self.pack.resolve(want).to_string();
            self.start(&b, want, t);
        }
    }

    fn start(&mut self, behavior: &str, intent: Intent, t: u64) {
        let pack = self.pack.clone();
        let Some(def) = pack.behaviors.get(behavior) else { return };
        let was_set = self.play.ori.is_some();
        self.play = Play { intent, behavior: behavior.to_string(), step: 0, left: Some(1), frame: 0, frame_start: t, ori: None, pinned: false, drag: false };
        let first = pack.clips.get(&def.seq[0].clip);
        if def.set.is_some() {
            let ori = if self.rng.below(2) == 0 { Ori::A } else { Ori::B };
            self.play.ori = Some(ori);
            if let Some(h) = first.and_then(|c| c.handoff.get(&ori)) {
                self.dir = h.facing;
            }
        } else if def.turn == Some(Turn::Random) {
            if let Some(c) = first {
                let dirs: Vec<Dir> = c.dirs.keys().copied().filter(|d| *d != self.dir).collect();
                if !dirs.is_empty() {
                    self.dir = dirs[self.rng.below(dirs.len() as u64) as usize];
                }
            }
        }
        if was_set != self.play.ori.is_some() {
            self.cut = true;
        }
        self.enter_step(0, t);
        self.last_start.insert(behavior.to_string(), t);
        self.note(t, "start", String::new());
    }

    fn pin(&mut self, t: u64) {
        self.play.pinned = true;
        self.play.frame = 0;
        self.play.frame_start = t;
    }

    /// Applies an input's consequences that don't wait for a frame boundary.
    fn settle(&mut self, t: u64) {
        self.dirty = true;
        let want = self.desired();
        if self.play.pinned && want != Intent::Idle {
            self.play.pinned = false;
            let b = self.pack.resolve(want).to_string();
            self.start(&b, want, t);
        } else if !self.play.pinned && self.vis == Visibility::Still && want == Intent::Idle && !self.dragging {
            // Still: idle frame 0, current facing, right away.
            if self.play.ori.is_some() {
                self.cut = true;
                self.leave_set_now();
            }
            self.start("hold", Intent::Idle, t);
            self.pin(t);
        }
    }

    fn leave_set_now(&mut self) {
        if let Some(ori) = self.play.ori {
            let exit = self.pack.behaviors.get(&self.play.behavior).and_then(|d| d.seq.last()).and_then(|s| self.pack.clips.get(&s.clip));
            if let Some(h) = exit.and_then(|c| c.handoff.get(&ori)) {
                self.dir = h.facing;
            }
        }
    }

    fn set_turn(&mut self, turn: TurnState) {
        self.turn = Some(turn);
        self.drop_cues(false);
    }

    /// A harness turn, DND or quiet hours outrank a pending reflex (it would be
    /// stale later); `once` also drops queued one-shot emotes (hidden/still).
    fn drop_cues(&mut self, once: bool) {
        let front = self.cue.front().copied();
        self.cue.retain(|c| match c {
            Cue::Reflex(_) => false,
            Cue::Once(_) => !once,
            Cue::Timed { .. } => true,
        });
        if self.cue.front().copied() != front {
            let t = self.t;
            self.activate_cue(t);
        }
    }

    /// Queues a cue (R2: a cue no longer overwrites another's follow-up). A
    /// full queue drops its oldest waiting one-shot, never the playing one: a
    /// Say or Reminder outlives an emote. With only Timed cues waiting, an
    /// incoming one-shot is dropped instead, and an incoming Timed cue evicts
    /// the oldest waiting one.
    fn push_cue(&mut self, c: Cue) {
        if self.cue.len() >= CUE_CAP {
            match (1..self.cue.len()).find(|&i| !matches!(self.cue[i], Cue::Timed { .. })) {
                Some(i) => {
                    self.cue.remove(i);
                }
                None if !matches!(c, Cue::Timed { .. }) => return,
                None => {
                    self.cue.remove(1);
                }
            }
        }
        self.cue.push_back(c);
    }

    /// The front cue starts its clock (and a Say its mouth) when it gets there.
    fn activate_cue(&mut self, t: u64) {
        if let Some(Cue::Timed { intent, dur, until, .. }) = self.cue.front_mut() {
            if until.is_none() {
                *until = Some(t + *dur);
                let (intent, end) = (*intent, t + *dur);
                if intent == Intent::Talk {
                    self.talk_until(end);
                }
            }
        }
    }

    /// The front Timed cue ended (time or Settled): its follow-up takes its place.
    fn finish_timed(&mut self, then: Option<Intent>, t: u64) {
        match then {
            Some(i) if self.vis == Visibility::Shown => self.cue[0] = Cue::Once(i),
            _ => {
                self.cue.pop_front();
                self.activate_cue(t);
            }
        }
    }

    fn desired(&self) -> Intent {
        if let Some(c) = self.cue.front() {
            return c.intent();
        }
        match self.turn {
            Some(TurnState::Held(i) | TurnState::Once(i)) => i,
            None => Intent::Idle,
        }
    }

    // ---- idle scheduler ----

    fn pick(&mut self, t: u64) -> String {
        let weights = self.pool_weights(t);
        let total: f64 = weights.iter().map(|(_, w)| w).sum();
        let chosen = if total <= 0.0 {
            "rest".to_string()
        } else {
            weighted(&weights, self.rng.unit() * total)
        };
        self.recent.push_front(chosen.clone());
        self.recent.truncate(self.pack.memory.recent);
        self.note(t, "pick", format!("total {total:.3}"));
        chosen
    }

    /// Effective weight of every pool entry now (0 = in cooldown / unavailable).
    fn pool_weights(&self, t: u64) -> Vec<(String, f64)> {
        let pack = &self.pack;
        let quiet = &pack.quiet_hours;
        let warming = !self.quiet && ((t.saturating_sub(self.started)) as f64) < pack.warmup.duration_s * 1000.0;
        let temper = self.temper();
        pack.idle_pool
            .iter()
            .filter(|e| !self.quiet || quiet.pool.contains(&e.behavior))
            .map(|e| {
                let cooling = e.cooldown_s > 0.0
                    && self.last_start.get(&e.behavior).is_some_and(|s| (t.saturating_sub(*s) as f64) < e.cooldown_s * 1000.0);
                let skipped = warming && pack.warmup.exclude.contains(&e.behavior);
                if cooling || skipped || !pack.available(&e.behavior) {
                    return (e.behavior.clone(), 0.0);
                }
                let mut w = e.weight;
                for (name, k) in &e.traits {
                    let v = temper.get(name).unwrap_or(0.5);
                    w *= (1.0 + k * (2.0 * v - 1.0)).max(0.0);
                }
                for r in &self.recent {
                    if *r == e.behavior {
                        w *= pack.memory.penalty;
                    }
                }
                if self.quiet {
                    w *= quiet.boost.get(&e.behavior).copied().unwrap_or(1.0);
                }
                (e.behavior.clone(), w)
            })
            .collect()
    }

    // ---- face timers ----

    fn schedule_blink(&mut self, from: u64) {
        let b = self.pack.timers.blink;
        let mut mean = b.mean_ms as f64 * (1.0 + 0.5 * (2.0 * self.temper().sleepiness - 1.0));
        if self.vis == Visibility::Still {
            mean *= 2.0;
        }
        let lo = mean * (1.0 - b.jitter);
        let hi = mean * (1.0 + b.jitter);
        self.blink_first = from;
        self.blink_at = from + (lo + self.rng.unit() * (hi - lo)).round().max(1.0) as u64;
        self.closed_until = None;
        self.double_pending = false;
    }

    fn blink_step(&mut self, t: u64) {
        let b = self.pack.timers.blink;
        let frame = self.frame_ms();
        match self.closed_until {
            None => {
                if !self.double_pending {
                    self.blink_first = t;
                }
                self.closed_until = Some(t + b.closed_frames.max(1) as u64 * frame);
            }
            Some(_) => {
                self.closed_until = None;
                if !self.double_pending && self.rng.chance(b.double_p) {
                    self.double_pending = true;
                    self.blink_at = t + DOUBLE_GAP_FRAMES * frame;
                } else {
                    let first = self.blink_first;
                    self.schedule_blink(first);
                    self.blink_at = self.blink_at.max(t + 1);
                }
            }
        }
    }

    fn frame_ms(&self) -> u64 {
        self.clip().map_or(83, |c| 1000 / c.fps.max(1) as u64)
    }

    fn talk_until(&mut self, until: u64) {
        let start = !self.flapping();
        self.talking_until = self.talking_until.max(until);
        if start {
            self.mouth_step = 0;
            self.mouth_at = self.t + self.pack.timers.mouth.frame_ms.max(1) as u64;
            self.mouth = MouthState::Open;
        }
    }

    fn flapping(&self) -> bool {
        self.talking_until > self.t && self.pack.capabilities.lip_sync && self.vis != Visibility::Hidden
    }

    fn mouth_step(&mut self, t: u64) {
        self.mouth_step += 1;
        self.mouth = if self.mouth_step % 3 == 0 {
            MouthState::Closed
        } else {
            let options: Vec<MouthState> = [MouthState::Open, MouthState::Wide].into_iter().filter(|m| *m != self.mouth).collect();
            options[self.rng.below(options.len() as u64) as usize]
        };
        self.mouth_at = t + self.pack.timers.mouth.frame_ms.max(1) as u64;
    }

    fn face_here(&self) -> bool {
        self.play.ori.is_none() && self.pack.face.as_ref().is_some_and(|f| f.covers(self.clip_name(), self.dir))
    }

    fn eyes(&self) -> EyeState {
        if self.face_here() && self.pack.capabilities.blink && self.closed_until.is_some_and(|c| c > self.t) {
            EyeState::Closed
        } else {
            EyeState::Open
        }
    }

    fn mouth_shown(&self) -> MouthState {
        if self.face_here() && self.flapping() {
            self.mouth
        } else {
            MouthState::Closed
        }
    }

    // ---- helpers ----

    /// The temperament in effect: `late` adds sleepiness.
    fn temper(&self) -> Temperament {
        let mut t = self.temperament;
        if self.late {
            t.sleepiness = (t.sleepiness + crate::reflex::LATE_SLEEPINESS).min(1.0);
        }
        t
    }

    fn clip_name(&self) -> &str {
        self.pack
            .behaviors
            .get(&self.play.behavior)
            .and_then(|d| d.seq.get(self.play.step))
            .map_or("idle", |s| s.clip.as_str())
    }

    fn clip(&self) -> Option<&crate::pack::ClipMeta> {
        self.pack.clips.get(self.clip_name())
    }

    fn note(&mut self, t: u64, what: &'static str, note: String) {
        if self.trace.len() == TRACE_N {
            self.trace.pop_front();
        }
        self.trace.push_back(TraceLine { t_ms: t, what, intent: self.play.intent, behavior: self.play.behavior.clone(), note });
        self.trace.make_contiguous();
    }
}

/// The entry `x` (0..total) lands on. R10: rounding can run `x` past the end;
/// the fallback is then the last entry that can play, never a weight-0 one.
fn weighted(weights: &[(String, f64)], mut x: f64) -> String {
    for (b, w) in weights {
        if x < *w {
            return b.clone();
        }
        x -= w;
    }
    weights.iter().rev().find(|(_, w)| *w > 0.0).map_or_else(|| "rest".into(), |(b, _)| b.clone())
}

#[cfg(test)]
#[path = "brain_tests.rs"]
mod tests;
