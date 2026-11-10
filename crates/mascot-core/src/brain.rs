//! The brain (design §4.3): harness events, senses and pokes in; what to show
//! out. Deterministic: same pack + seed + inputs ⇒ same `Show` sequence. No
//! wall clock, no global RNG; time comes in as `now_ms`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use mascot_protocol::pack::{Loops, Temperament, Turn};
use mascot_protocol::{EmoteName, Emotion, MascotEvent};
use serde::{Deserialize, Serialize};

use crate::pack::{Dir, Ori, PackMeta};
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
    /// `event`, `start`, `pick`, `end`, `vis`, `poke`.
    pub what: &'static str,
    pub intent: Intent,
    pub behavior: String,
    pub note: String,
}

const TRACE_N: usize = 256;
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

/// An out-of-turn cue (`say`, `emote`, reminder): over the turn while it lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cue {
    /// Held until `until` or Settled, then `then` once.
    Timed { intent: Intent, until: u64, then: Option<Intent> },
    Once(Intent),
}

impl Cue {
    fn intent(&self) -> Intent {
        match *self {
            Cue::Timed { intent, .. } | Cue::Once(intent) => intent,
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
    cue: Option<Cue>,
    dragging: bool,
    vis: Visibility,
    quiet: bool,
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
            cue: None,
            dragging: false,
            vis: Visibility::Shown,
            quiet: false,
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
                self.cue = Some(Cue::Timed { intent: Intent::Talk, until, then });
                self.talk_until(until);
            }
            MascotEvent::Emote { name } => {
                self.cue = Some(Cue::Once(match name {
                    EmoteName::Wave => Intent::Wave,
                    EmoteName::Nod => Intent::Nod,
                    EmoteName::Shake => Intent::Shake,
                    EmoteName::Jump => Intent::Jump,
                    EmoteName::Spin => Intent::Spin,
                }));
            }
            MascotEvent::Reminder { text, .. } => {
                self.cue = Some(Cue::Timed { intent: Intent::Remind, until: t + bubble_ms(text), then: None });
            }
            MascotEvent::Settled => {
                if matches!(self.turn, Some(TurnState::Held(Intent::Ask | Intent::Talk))) {
                    self.turn = None;
                }
                if let Some(Cue::Timed { then, .. }) = self.cue {
                    self.cue = then.map(Cue::Once);
                }
                self.talking_until = self.talking_until.min(t);
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
        self.quiet = s.quiet_hours;
        let dnd = s.fullscreen || s.screen_share || s.dnd_toggle;
        let vis = match (dnd, self.cfg.dnd_action) {
            (false, _) => Visibility::Shown,
            (true, DndAction::Hide) => Visibility::Hidden,
            (true, DndAction::Still) => Visibility::Still,
        };
        if vis != self.vis {
            let was = self.vis;
            self.vis = vis;
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
        self.settle(t);
    }

    pub fn poke(&mut self, p: Poke, now_ms: u64) {
        self.advance(now_ms);
        let t = self.t;
        self.note(t, "poke", format!("{p:?}"));
        match p {
            Poke::Click => {}
            Poke::DragStart => {
                self.dragging = true;
                if !self.play.drag {
                    self.start("hold", self.desired(), t);
                    self.play.drag = true;
                }
            }
            Poke::DragEnd => self.dragging = false,
        }
        self.dirty = true;
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
            place: None,
        };
        self.cache = Some((now_ms, show.clone()));
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
        if let Some(Cue::Timed { until, .. }) = self.cue {
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
        loop {
            let hidden = self.vis == Visibility::Hidden;
            let frame_end = if hidden { u64::MAX } else { self.frame_end() };
            let cue_end = match self.cue {
                Some(Cue::Timed { until, .. }) => until,
                _ => u64::MAX,
            };
            let blink = if hidden { u64::MAX } else { self.closed_until.unwrap_or(self.blink_at) };
            // Only steps inside the talking window, whatever the tick cadence.
            let mouth = if self.flapping() && self.mouth_at < self.talking_until { self.mouth_at } else { u64::MAX };
            let next = frame_end.min(cue_end).min(blink).min(mouth);
            if next > now {
                break;
            }
            self.t = next;
            self.dirty = true;
            if next == cue_end {
                if let Some(Cue::Timed { then, .. }) = self.cue {
                    self.cue = then.map(Cue::Once);
                }
                self.settle(next);
            } else if next == blink {
                self.blink_step(next);
            } else if next == mouth {
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
            let once_cue = matches!(self.cue, Some(Cue::Once(i)) if i == self.play.intent);
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
            if let Some(Cue::Once(i)) = self.cue {
                if i == was {
                    self.cue = None;
                }
            }
            if self.turn == Some(TurnState::Once(was)) && self.cue.is_none_or(|c| c.intent() != was) {
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
    }

    fn desired(&self) -> Intent {
        if let Some(c) = self.cue {
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
            let mut x = self.rng.unit() * total;
            let mut chosen = weights.last().map(|(b, _)| b.clone()).unwrap_or_else(|| "rest".into());
            for (b, w) in &weights {
                if x < *w {
                    chosen = b.clone();
                    break;
                }
                x -= w;
            }
            chosen
        };
        self.recent.push_front(chosen.clone());
        self.recent.truncate(self.pack.memory.recent);
        self.note(t, "pick", format!("total {total:.3}"));
        chosen
    }

    /// Effective weight of every pool entry now (0 = in cooldown / unavailable).
    fn pool_weights(&self, t: u64) -> Vec<(String, f64)> {
        let pack = &self.pack;
        pack.idle_pool
            .iter()
            .filter(|e| !self.quiet || e.behavior == "nap" || e.behavior == "rest")
            .map(|e| {
                let cooling = e.cooldown_s > 0.0
                    && self.last_start.get(&e.behavior).is_some_and(|s| ((t - s) as f64) < e.cooldown_s * 1000.0);
                if cooling || !pack.available(&e.behavior) {
                    return (e.behavior.clone(), 0.0);
                }
                let mut w = e.weight;
                for (name, k) in &e.traits {
                    let v = self.temperament.get(name).unwrap_or(0.5);
                    w *= (1.0 + k * (2.0 * v - 1.0)).max(0.0);
                }
                for r in &self.recent {
                    if *r == e.behavior {
                        w *= pack.memory.penalty;
                    }
                }
                if self.quiet && e.behavior == "nap" {
                    w *= 4.0;
                }
                (e.behavior.clone(), w)
            })
            .collect()
    }

    // ---- face timers ----

    fn schedule_blink(&mut self, from: u64) {
        let b = self.pack.timers.blink;
        let mut mean = b.mean_ms as f64 * (1.0 + 0.5 * (2.0 * self.temperament.sleepiness - 1.0));
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

#[cfg(test)]
#[path = "brain_tests.rs"]
mod tests;
