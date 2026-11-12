//! Replay (design §4.4): a scripted timeline of events, senses and pokes →
//! the brain's decisions as a deterministic log, one line per change.

use std::sync::Arc;

use mascot_protocol::MascotEvent;
use serde::{Deserialize, Serialize};

use crate::brain::{
    Brain, BrainConfig, DndAction, EyeState, FrontApp, Intent, LocalTime, MouthState, NowPlaying, Poke, Senses, Visibility, WinRect,
};
use crate::pack::{Dir, Ori, PackMeta};

/// The only `timeline_version` this build reads.
pub const TIMELINE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub timeline_version: u32,
    pub seed: u64,
    pub duration_ms: u64,
    /// `"HH:MM"` at t = 0.
    pub start_local: String,
    /// 0 = Monday.
    pub weekday: u8,
    pub items: Vec<Item>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<MascotEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub senses: Option<SensesPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poke: Option<Poke>,
    /// Test/QA hook (p2): the bear faces this way at once (`Brain::face`), e.g. to click from each dir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<Dir>,
}

/// Every `Senses` field optional, merged over the previous senses.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct SensesPatch {
    #[serde(default, deserialize_with = "double", skip_serializing_if = "Option::is_none")]
    pub frontmost: Option<Option<FrontApp>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_idle_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fullscreen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_share: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dnd_toggle: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<WinRect>>,
    #[serde(default, deserialize_with = "double", skip_serializing_if = "Option::is_none")]
    pub panel: Option<Option<WinRect>>,
    #[serde(default, deserialize_with = "double", skip_serializing_if = "Option::is_none")]
    pub now_playing: Option<Option<NowPlaying>>,
    /// 1–12 (pack v2 `when.months`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
    /// Replays only (`#senses-cursor`): `{"dx":-40,"dy":16,"over":false}` or `null`. Never
    /// serialized, so a patch that is logged or kept in the app's sense ring cannot carry it.
    #[serde(default, deserialize_with = "double", skip_serializing)]
    pub cursor: Option<Option<crate::reaction::Cursor>>,
}

/// `null` = clear (`Some(None)`); an absent key stays `None` via `default`.
fn double<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

impl SensesPatch {
    pub fn apply(&self, s: &mut Senses) {
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(v) = &self.$f { s.$f = v.clone(); } )* };
        }
        set!(frontmost, user_idle_ms, local, fullscreen, screen_share, dnd_toggle, quiet_hours, windows, panel, now_playing, month, cursor);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LogLine {
    pub t_ms: u64,
    pub intent: Intent,
    pub behavior: String,
    pub clip: String,
    pub dir: Dir,
    pub ori: Option<Ori>,
    pub eyes: EyeState,
    pub mouth: MouthState,
    pub vis: Visibility,
}

impl LogLine {
    fn same(&self, o: &LogLine) -> bool {
        (self.intent, &self.behavior, &self.clip, self.dir, self.ori, self.eyes, self.mouth, self.vis)
            == (o.intent, &o.behavior, &o.clip, o.dir, o.ori, o.eyes, o.mouth, o.vis)
    }
}

/// Parses and checks a timeline file's text. The error is one line.
pub fn parse(text: &str) -> Result<Timeline, String> {
    let raw: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("bad JSON: {e}"))?;
    match raw.get("timeline_version").and_then(|v| v.as_u64()) {
        Some(v) if v == TIMELINE_VERSION as u64 => {}
        Some(v) => return Err(format!("timeline_version {v} is not supported (this build reads {TIMELINE_VERSION})")),
        None => return Err("timeline_version is missing or not a number".into()),
    }
    let tl: Timeline = serde_json::from_value(raw).map_err(|e| format!("bad timeline: {e}"))?;
    start_minutes(&tl.start_local)?;
    if tl.weekday > 6 {
        return Err(format!("weekday {} is not 0..6", tl.weekday));
    }
    if let Some(w) = tl.items.windows(2).find(|w| w[1].at_ms < w[0].at_ms) {
        return Err(format!("items must be sorted by at_ms ({} after {})", w[1].at_ms, w[0].at_ms));
    }
    if let Some(i) = tl.items.iter().find(|i| i.at_ms > tl.duration_ms) {
        return Err(format!("item at {} ms is past duration_ms {}", i.at_ms, tl.duration_ms));
    }
    Ok(tl)
}

/// `"HH:MM"` (00:00–23:59) → minutes after midnight.
pub fn start_minutes(s: &str) -> Result<u16, String> {
    let bad = || format!("start_local {s:?} is not HH:MM");
    let (h, m) = s.split_once(':').ok_or_else(bad)?;
    let (h, m): (u16, u16) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if h > 23 || m > 59 || s.len() != 5 {
        return Err(bad());
    }
    Ok(h * 60 + m)
}

/// Runs the timeline (its own `seed`) and returns one line per change of any
/// field but the frame. `tl` must have passed `parse`.
pub fn run(pack: Arc<PackMeta>, tl: &Timeline) -> Vec<LogLine> {
    run_with(pack, tl, |_| {})
}

/// `run`, with `setup` applied to the new brain first (app settings a timeline cannot carry,
/// e.g. `set_edges` for an edge replay).
pub fn run_with(pack: Arc<PackMeta>, tl: &Timeline, setup: impl FnOnce(&mut Brain)) -> Vec<LogLine> {
    let cfg = BrainConfig { seed: tl.seed, temperament: None, dnd_action: DndAction::Hide };
    let mut brain = Brain::new(pack, cfg, 0);
    setup(&mut brain);
    let mut senses = Senses { local: LocalTime { minutes: start_minutes(&tl.start_local).unwrap_or(0), weekday: tl.weekday }, ..Senses::default() };
    brain.senses(&senses, 0);
    let mut out: Vec<LogLine> = Vec::new();
    let mut i = 0;
    let mut t = 0u64;
    loop {
        while i < tl.items.len() && tl.items[i].at_ms <= t {
            let it = &tl.items[i];
            if let Some(p) = &it.senses {
                p.apply(&mut senses);
                brain.senses(&senses, t);
            }
            if let Some(d) = it.face {
                brain.face(d, t);
            }
            if let Some(e) = &it.event {
                brain.event(e, t);
            }
            if let Some(p) = it.poke {
                brain.poke(p, t);
            }
            i += 1;
        }
        let s = brain.tick(t);
        let line = LogLine {
            t_ms: t,
            intent: s.intent,
            behavior: s.behavior,
            clip: s.clip,
            dir: s.dir,
            ori: s.ori,
            eyes: s.eyes,
            mouth: s.mouth,
            vis: s.visibility,
        };
        if out.last().is_none_or(|l| !l.same(&line)) {
            out.push(line);
        }
        let next_item = tl.items.get(i).map_or(u64::MAX, |it| it.at_ms);
        let next = next_item.min(brain.next_wake_ms()).max(t + 1);
        if next > tl.duration_ms {
            break;
        }
        t = next;
    }
    out
}

/// The log as JSONL (one `LogLine` per line, trailing newline).
pub fn to_jsonl(lines: &[LogLine]) -> String {
    let mut s = String::new();
    for l in lines {
        s.push_str(&serde_json::to_string(l).expect("LogLine serializes"));
        s.push('\n');
    }
    s
}
