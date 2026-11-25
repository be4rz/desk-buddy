//! Pack and art JSON schemas (design §3.3, §3.4): serde structs only, no IO.
//! `mascot-core` loads and validates them; studio and tests share them.
//! Unknown fields are ignored so the art pipeline can add notes freely.

use std::collections::BTreeMap;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

/// The newest `pack_version` this build reads (it reads 1 and 2; every v2 key is optional, `#pack-v2`).
pub const PACK_VERSION: u32 = 2;
/// The oldest `pack_version` this build reads.
pub const PACK_VERSION_MIN: u32 = 1;

/// `mascots/<id>/pack.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PackFile {
    pub pack_version: u32,
    pub id: String,
    pub name: String,
    /// Relative to the pack dir.
    pub art_root: String,
    /// Relative to the pack dir.
    #[serde(default)]
    pub palettes: Vec<String>,
    pub sources: Vec<SourceRef>,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub temperament: Temperament,
    #[serde(default)]
    pub timers: Timers,
    pub behaviors: BTreeMap<String, BehaviorDef>,
    /// Intent name (snake_case) → behavior fallback chain.
    pub intents: BTreeMap<String, Vec<String>>,
    pub idle_pool: Vec<PoolEntry>,
    #[serde(default)]
    pub memory: Memory,
    /// `"persona"`: the characters a person can pick (optional; a pack without it has no persona).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<crate::persona::PersonaFile>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    StageA,
    StageB,
    Face,
    /// v2 (`#face-part`): a stage's face placements, merged into the face table.
    FacePart,
}

/// One art source, relative to `art_root`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SourceRef {
    pub kind: SourceKind,
    pub path: String,
    /// `stage_b` only: its props file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<String>,
    /// An absent optional source is skipped; a present but invalid one is an error.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Capabilities {
    #[serde(default)]
    pub blink: bool,
    #[serde(default)]
    pub lip_sync: bool,
    #[serde(default = "look_at_body")]
    pub look_at: String,
}

fn look_at_body() -> String {
    "body".into()
}

impl Default for Capabilities {
    fn default() -> Self {
        Capabilities { blink: false, lip_sync: false, look_at: look_at_body() }
    }
}

/// Personality, each trait in `0..=1`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Temperament {
    #[serde(default = "half")]
    pub energy: f64,
    #[serde(default = "half")]
    pub curiosity: f64,
    #[serde(default = "half")]
    pub shyness: f64,
    #[serde(default = "half")]
    pub chattiness: f64,
    #[serde(default = "half")]
    pub sleepiness: f64,
}

fn half() -> f64 {
    0.5
}

impl Default for Temperament {
    fn default() -> Self {
        Temperament { energy: 0.5, curiosity: 0.5, shyness: 0.5, chattiness: 0.5, sleepiness: 0.5 }
    }
}

impl Temperament {
    /// The trait by its pack name.
    pub fn get(&self, name: &str) -> Option<f64> {
        Some(match name {
            "energy" => self.energy,
            "curiosity" => self.curiosity,
            "shyness" => self.shyness,
            "chattiness" => self.chattiness,
            "sleepiness" => self.sleepiness,
            _ => return None,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Timers {
    #[serde(default)]
    pub blink: BlinkTimer,
    #[serde(default)]
    pub mouth: MouthTimer,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct BlinkTimer {
    pub mean_ms: u32,
    pub jitter: f64,
    pub closed_frames: u32,
    pub double_p: f64,
}

impl Default for BlinkTimer {
    fn default() -> Self {
        BlinkTimer { mean_ms: 4200, jitter: 0.4, closed_frames: 2, double_p: 0.15 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct MouthTimer {
    pub frame_ms: u32,
    pub stall_ms: u32,
}

impl Default for MouthTimer {
    fn default() -> Self {
        MouthTimer { frame_ms: 167, stall_ms: 400 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BehaviorDef {
    /// A Stage B set the behavior plays in (`bed`, `read`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    pub seq: Vec<StepDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<Turn>,
    /// A free behavior whose last step is its way out (like a set's exit clip): an ending
    /// activity jumps to it instead of stopping at once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exit: bool,
}

/// `pack.json` `"body"` (body-tools, p1): what `buddy mascot do|emote` can ask for.
/// Optional; a pack without it gets the bear's block (the loader synthesizes it).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BodyFile {
    #[serde(default = "default_for_s")]
    pub default_for_s: u64,
    #[serde(default = "max_for_s")]
    pub max_for_s: u64,
    /// In pack order (the prompt and `list` keep it).
    #[serde(default, with = "ordered")]
    pub activities: Vec<(String, BodyEntry)>,
    #[serde(default, with = "ordered")]
    pub emotes: Vec<(String, BodyEntry)>,
}

/// A JSON object as `(key, value)` pairs in document order.
mod ordered {
    use serde::de::{Deserializer, MapAccess, Visitor};
    use serde::ser::{SerializeMap, Serializer};

    use super::BodyEntry;

    pub fn serialize<S: Serializer>(v: &[(String, BodyEntry)], s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(v.len()))?;
        for (k, e) in v {
            m.serialize_entry(k, e)?;
        }
        m.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, BodyEntry)>, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vec<(String, BodyEntry)>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object of entries")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = a.next_entry::<String, BodyEntry>()? {
                    out.push((k, v));
                }
                Ok(out)
            }
        }
        d.deserialize_map(V)
    }
}

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        One(String),
        Many(Vec<String>),
    }
    // `"plays": null` stays None, as it did before the one-or-many form
    Ok(Option::<Raw>::deserialize(d).map_err(|_| de::Error::custom("plays must be a name or a list of names"))?.map(|r| match r {
        Raw::One(s) => vec![s],
        Raw::Many(v) => v,
    }))
}

fn default_for_s() -> u64 {
    120
}

fn max_for_s() -> u64 {
    1800
}

/// One activity or emote. Exactly one action key: `plays` (a behavior fallback chain),
/// `intent` (emotes only) or `station` (activities only, p3). The loader checks.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BodyEntry {
    /// A behavior name or a fallback chain (`"read"` or `["read", "rest"]`).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "one_or_many")]
    pub plays: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    /// Activities: `pastime` (default) or `work`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ActivityKind>,
    /// Optional: fidgets, moves, reactions and surprises are listed by name only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    /// v2 (`#pack-body-groups`): activities `pastime | play | posture | work`, emotes
    /// `gesture | fidget | move | reaction | surprise`. Missing = pastime / gesture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    #[default]
    Pastime,
    Work,
}

impl ActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityKind::Pastime => "pastime",
            ActivityKind::Work => "work",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    /// A new facing at the behavior start.
    Random,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StepDef {
    pub clip: String,
    #[serde(default, skip_serializing_if = "Loops::is_once")]
    pub loops: Loops,
}

/// How often a step plays: absent = once, `[lo, hi]` = uniform inclusive,
/// `"hold"` = until the intent ends, v2 `{"for_s": [a, b]}` = whole loops until a seeded
/// draw d in [a, b] seconds has elapsed (`#pack-loops`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Loops {
    #[default]
    Once,
    Range(u32, u32),
    Hold,
    For { for_s: [f64; 2] },
}

impl Loops {
    pub fn is_once(&self) -> bool {
        *self == Loops::Once
    }
}

impl Serialize for Loops {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match *self {
            Loops::Once => [1u32, 1].serialize(s),
            Loops::Range(lo, hi) => [lo, hi].serialize(s),
            Loops::Hold => s.serialize_str("hold"),
            Loops::For { for_s } => {
                use serde::ser::SerializeMap;
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("for_s", &for_s)?;
                m.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Loops {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Range([u32; 2]),
            Word(String),
            For { for_s: [f64; 2] },
        }
        const WANT: &str = "loops must be [lo, hi], \"hold\" or {\"for_s\": [a, b]}";
        match Raw::deserialize(d).map_err(|_| de::Error::custom(WANT))? {
            Raw::For { for_s: [a, b] } if a.is_finite() && b.is_finite() && 0.0 < a && a <= b => Ok(Loops::For { for_s: [a, b] }),
            Raw::For { .. } => Err(de::Error::custom("loops {\"for_s\": [a, b]} needs 0 < a <= b")),
            Raw::Range([lo, hi]) if 1 <= lo && lo <= hi => Ok(Loops::Range(lo, hi)),
            Raw::Range(_) => Err(de::Error::custom("loops [lo, hi] needs 1 <= lo <= hi")),
            Raw::Word(w) if w == "hold" => Ok(Loops::Hold),
            Raw::Word(w) => Err(de::Error::custom(format!("{WANT}, not {w:?}"))),
        }
    }
}

/// One idle pool entry (`#pack-pool`). Exactly one of `behavior` / `station` (the loader checks).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PoolEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<String>,
    /// v2: a `stations` key, entered from idle and held for `hold_s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    /// v2, station only and required with it: the seated hold, seconds `[lo, hi]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_s: Option<[f64; 2]>,
    pub weight: f64,
    #[serde(default)]
    pub cooldown_s: f64,
    /// Temperament trait → k (effective weight × max(0, 1 + k·(2·trait − 1))).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub traits: BTreeMap<String, f64>,
    /// v2: the variety class (`max_moves_in_row` counts `move`).
    #[serde(default, skip_serializing_if = "PoolKind::is_other")]
    pub kind: PoolKind,
    /// v2: the variety key; default = the behavior or station name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// v2: when the entry is eligible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
}

impl PoolEntry {
    /// The behavior or station it names (a station visit is traced as that behavior).
    pub fn name(&self) -> &str {
        self.behavior.as_deref().or(self.station.as_deref()).unwrap_or("")
    }

    /// The variety key.
    pub fn group_key(&self) -> &str {
        self.group.as_deref().unwrap_or_else(|| self.name())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum PoolKind {
    Stand,
    Fidget,
    Posture,
    Move,
    Long,
    Rare,
    React,
    #[default]
    Other,
}

impl PoolKind {
    pub const ALL: [PoolKind; 8] =
        [PoolKind::Stand, PoolKind::Fidget, PoolKind::Posture, PoolKind::Move, PoolKind::Long, PoolKind::Rare, PoolKind::React, PoolKind::Other];

    pub fn is_other(&self) -> bool {
        *self == PoolKind::Other
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PoolKind::Stand => "stand",
            PoolKind::Fidget => "fidget",
            PoolKind::Posture => "posture",
            PoolKind::Move => "move",
            PoolKind::Long => "long",
            PoolKind::Rare => "rare",
            PoolKind::React => "react",
            PoolKind::Other => "other",
        }
    }
}

/// `"when"` of a pool entry: every given condition must hold.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct When {
    /// `[from, to)` local hours, wrapping past midnight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<[u8; 2]>,
    /// 1..=12.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub months: Option<Vec<u8>>,
    /// A `#pack-contexts` name (evaluated from p4; until then such an entry is never eligible).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Context names `#pack-contexts` fixes in code.
pub const CONTEXTS: [&str; 5] = ["music", "edge", "evening", "away", "away_long"];
/// Reaction trigger names `#pack-reactions` fixes in code.
pub const REACTIONS: [&str; 9] =
    ["drag_hold", "drag_land", "dizzy", "flinch", "pet", "follow", "morning", "approval_wait", "edge_land"];

/// One `pack.json` `"contexts"` entry (`#pack-contexts`, p4). The name is a key of `CONTEXTS`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ContextDef {
    /// While it holds, only its own `when.context` entries are eligible.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub only: bool,
    /// `[from, to)` local hours (evening).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<[u8; 2]>,
    /// Minutes of user idle before it holds (away, away_long).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_min: Option<f64>,
    /// A behavior whose middle step loops until input returns, then its exit steps play (away_long).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<String>,
    /// Pool kinds removed while it holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_kinds: Vec<PoolKind>,
}

/// One `pack.json` `"reactions"` entry (`#pack-reactions`, p4). The name is a key of `REACTIONS`; the
/// fields a trigger reads are fixed in code (README § reactions); a field its trigger does not read
/// is accepted and has no effect.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ReactionDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plays: Option<String>,
    /// Alternatives with no repeat (approval_wait) or played in order (morning).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_pt: Option<f64>,
    /// pt/s: flinch at or above, follow below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reversals: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub still_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_s: Option<[f64; 2]>,
}

impl ReactionDef {
    /// Behaviors it names, `plays` first.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.plays.iter().map(String::as_str).chain(self.chain.iter().map(String::as_str))
    }
}

/// `pack.json` `"variety"` (`#pack-variety`): the hard idle rules, applied to each entry's group.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Variety {
    #[serde(default)]
    pub no_repeat: bool,
    #[serde(default)]
    pub window: u32,
    /// 0 = off.
    #[serde(default)]
    pub max_in_window: u32,
    /// 0 = off.
    #[serde(default)]
    pub max_moves_in_row: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Targets>,
}

/// What `buddy studio audit` checks a run of at least `minutes` against.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    pub minutes: f64,
    pub stand_max: f64,
    pub stand_move_max: f64,
    pub distinct_min: u32,
    pub share_max: f64,
    pub long_len_s: [f64; 2],
}

/// Recent-pick memory of the idle scheduler.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Memory {
    pub recent: usize,
    pub penalty: f64,
}

impl Default for Memory {
    fn default() -> Self {
        Memory { recent: 3, penalty: 0.25 }
    }
}

// ---- Art JSON (pipeline output, read in place) ----

/// `anim/anim.json` and `anim_c/anim_c.json` (Stage A).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageAFile {
    pub fps: u32,
    pub ppu: f64,
    #[serde(default)]
    pub directions: BTreeMap<String, serde_json::Value>,
    pub anims: BTreeMap<String, StageAClip>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageAClip {
    #[serde(rename = "loop")]
    pub looping: bool,
    pub fps: u32,
    pub frame_w: u32,
    pub frame_h: u32,
    pub anchor: [i32; 2],
    pub frames_n: u32,
    /// Direction (`S`, `SE`, …) → strip PNG, relative to the JSON's dir.
    pub files: BTreeMap<String, String>,
    pub frames: Vec<StageAFrame>,
    /// A turn clip (`#anim-l`): +1 = one 45° step counter-clockwise in `Dir` order, −1 = clockwise.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub turn_steps: i8,
    /// `#anim-stage` (optional, informational): `layers | baked | none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// `#anim-stage` (optional): per-dir per-frame `[dx, dy]` panel motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_px: Option<serde_json::Value>,
    /// `#anim-stage` (optional, informational): `{"start": "idle0", "end": "idle0"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<serde_json::Value>,
}

fn is_zero(v: &i8) -> bool {
    *v == 0
}

/// `pack.json` `"turning": {"ccw": "turn_ccw", "cw": "turn_cw"}` (p2): the clips the brain turns with.
/// Optional; without it (or its clips) facing changes snap.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TurningFile {
    pub ccw: String,
    pub cw: String,
}

/// `pack.json` `"stations"` (p3, `#pack-stations`): a set with role clips the brain plays while the
/// bear works there. Optional; clips not rendered yet leave the station unplayable (the bear stands).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StationFile {
    pub set: String,
    /// Walks in from the hand-off spot and sits (its `handoff` is `at: start`).
    pub enter: String,
    /// The work loop (typing).
    #[serde(rename = "loop")]
    pub work_loop: String,
    /// Swivels to face the person.
    pub to_user: String,
    pub talk: String,
    pub ask: String,
    /// The listen pose; absent = `talk`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    /// Swivels back to the screen.
    pub to_screen: String,
    /// Stands up and steps back to the hand-off spot (its `handoff` is `at: end`).
    pub exit: String,
}

/// `pack.json` `"work"` (p3): when a turn goes to a station.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkFile {
    pub station: String,
    #[serde(default = "after_ms")]
    pub after_ms: u64,
    #[serde(default = "after_tools")]
    pub after_tools: u32,
}

fn after_ms() -> u64 {
    4000
}

fn after_tools() -> u32 {
    2
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageAFrame {
    pub dur: u32,
    #[serde(default)]
    pub root_world: f64,
    /// Direction → screen-pixel move after showing this frame.
    #[serde(default)]
    pub root_px: BTreeMap<String, [f32; 2]>,
}

/// `anim_b/anim_b.json` (Stage B sets).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageBFile {
    pub fps: u32,
    pub ppu: f64,
    /// Set → ori (`A`, `B`) → canvas.
    pub sets: BTreeMap<String, BTreeMap<String, StageBSetOri>>,
    pub anims: BTreeMap<String, StageBClip>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageBSetOri {
    pub frame_w: u32,
    pub frame_h: u32,
    pub anchor: [i32; 2],
    /// Relative to the JSON's dir.
    pub props_frame: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zzz: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageBClip {
    pub set: String,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub fps: u32,
    pub frames_n: u32,
    /// Ori → strip PNG, relative to the JSON's dir.
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub handoff: BTreeMap<String, HandoffDef>,
    #[serde(default)]
    pub frames: Vec<StageBFrame>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HandoffDef {
    /// `start` or `end`.
    pub at: String,
    pub frame: u32,
    /// Stage A anchor offset from the set anchor.
    pub px: [i32; 2],
    pub facing: String,
    #[serde(default)]
    pub pos_world: Vec<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageBFrame {
    pub dur: u32,
}

/// `props/props.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PropsFile {
    pub ppu: f64,
    pub props: BTreeMap<String, PropDef>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PropDef {
    pub set: String,
    #[serde(rename = "A")]
    pub a: PropOri,
    #[serde(rename = "B")]
    pub b: PropOri,
    #[serde(default)]
    pub footprint_world: serde_json::Value,
    #[serde(default)]
    pub origin_world_in_set: Vec<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PropOri {
    pub file: String,
    pub size: [u32; 2],
    pub anchor: [i32; 2],
    pub offset_px_in_set: [i32; 2],
    #[serde(default)]
    pub layer_key: String,
}

/// `face/face.json` (D16 face layers, §3.4).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FaceFile {
    pub face_version: u32,
    pub dirs: Vec<String>,
    pub slots: BTreeMap<String, SlotDef>,
    /// Slot → state → dir → patch.
    pub layers: BTreeMap<String, BTreeMap<String, BTreeMap<String, LayerDef>>>,
    /// Clip → dir → slot → top-left per body frame.
    pub clips: BTreeMap<String, BTreeMap<String, BTreeMap<String, Vec<[i32; 2]>>>>,
    /// v2 (p2): clip → body dir → the dir whose patches it draws (a head turned toward the camera).
    /// A clip or dir without an entry draws its own dir's patches.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub patch_dirs: BTreeMap<String, BTreeMap<String, String>>,
    /// v2 (p3): set clip → ori → the patch dir and the slot positions on the set frame (top-left).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub set_clips: BTreeMap<String, BTreeMap<String, SetFaceDef>>,
}

/// `<stage>/face_<id>.json` (`#face-part`): one stage's face placements, merged into face.json's table.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FacePartFile {
    /// The stage kit writes `face_part` (stagekit.py `write_face_part`); `face_part_version` is the prototype's name.
    #[serde(alias = "face_part")]
    pub face_part_version: u32,
    #[serde(default)]
    pub stage: String,
    /// Clip → dir → `{"patch": "S", "eyes": [...], "mouth": [...]}`; the stage kit leaves `patch` out and
    /// names turned heads in `patch_dirs` instead (a dir without either draws its own patches).
    #[serde(default)]
    pub clips: BTreeMap<String, BTreeMap<String, PartFaceDef>>,
    /// Clip → body dir → patch dir (the stage kit's form of `patch`).
    #[serde(default)]
    pub patch_dirs: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub set_clips: BTreeMap<String, BTreeMap<String, SetFaceDef>>,
    /// Clip → frames that must equal the expected frame exactly (verify_face.py only; the loader ignores it).
    #[serde(default)]
    pub exact: BTreeMap<String, ExactFrames>,
}

/// A face part's `exact` frames for one clip: the prototype's flat list, or the stage kit's
/// dir / ori → list (`{"lean": {"A": [0, 1], "B": [0, 1]}}`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ExactFrames {
    All(Vec<u32>),
    By(BTreeMap<String, Vec<u32>>),
}

/// One dir of a face part's free clip: [`SetFaceDef`] with an optional `patch`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PartFaceDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    /// Slot → top-left per frame.
    #[serde(flatten)]
    pub slots: BTreeMap<String, Vec<[i32; 2]>>,
}

/// One ori of a set clip's face (`#face-v2` `set_clips`): `{"patch": "S", "eyes": [...], "mouth": [...]}`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SetFaceDef {
    pub patch: String,
    /// Slot → top-left per frame, on the set clip's frame.
    #[serde(flatten)]
    pub slots: BTreeMap<String, Vec<[i32; 2]>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SlotDef {
    pub states: Vec<String>,
    pub default: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LayerDef {
    pub file: String,
    pub w: u32,
    pub h: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_takes_one_many_or_null() {
        let e: BodyEntry = serde_json::from_str(r#"{"plays":"read"}"#).unwrap();
        assert_eq!(e.plays, Some(vec!["read".to_string()]));
        let e: BodyEntry = serde_json::from_str(r#"{"plays":["read","rest"]}"#).unwrap();
        assert_eq!(e.plays, Some(vec!["read".to_string(), "rest".to_string()]));
        let e: BodyEntry = serde_json::from_str(r#"{"plays":null,"intent":"happy"}"#).unwrap();
        assert_eq!(e.plays, None);
        assert!(serde_json::from_str::<BodyEntry>(r#"{"plays":3}"#).is_err());
    }

    #[test]
    fn loops_parse_all_forms() {
        let s: StepDef = serde_json::from_str(r#"{"clip":"idle"}"#).unwrap();
        assert_eq!(s.loops, Loops::Once);
        let s: StepDef = serde_json::from_str(r#"{"clip":"idle","loops":[2,4]}"#).unwrap();
        assert_eq!(s.loops, Loops::Range(2, 4));
        let s: StepDef = serde_json::from_str(r#"{"clip":"idle","loops":"hold"}"#).unwrap();
        assert_eq!(s.loops, Loops::Hold);
        assert!(serde_json::from_str::<StepDef>(r#"{"clip":"idle","loops":[4,2]}"#).is_err());
        assert!(serde_json::from_str::<StepDef>(r#"{"clip":"idle","loops":"forever"}"#).is_err());
        let back = serde_json::to_value(StepDef { clip: "x".into(), loops: Loops::Hold }).unwrap();
        assert_eq!(back, serde_json::json!({ "clip": "x", "loops": "hold" }));
    }

    #[test]
    fn stations_work_and_set_faces_parse() {
        let st: StationFile = serde_json::from_value(serde_json::json!({ "set": "desk", "enter": "desk_sit", "loop": "desk_type",
            "to_user": "desk_swivel_in", "talk": "desk_talk", "ask": "desk_ask", "listen": "desk_talk",
            "to_screen": "desk_swivel_out", "exit": "desk_stand" }))
        .unwrap();
        assert_eq!((st.work_loop.as_str(), st.listen.as_deref()), ("desk_type", Some("desk_talk")));
        let w: WorkFile = serde_json::from_value(serde_json::json!({ "station": "desk" })).unwrap();
        assert_eq!((w.after_ms, w.after_tools), (4000, 2));
        assert!(serde_json::from_value::<WorkFile>(serde_json::json!({ "station": "desk", "after": 1 })).is_err());
        let f: SetFaceDef = serde_json::from_value(serde_json::json!({ "patch": "S", "eyes": [[1, 2]], "mouth": [[3, 4]] })).unwrap();
        assert_eq!(f.patch, "S");
        assert_eq!(f.slots["mouth"], vec![[3, 4]]);
    }
}
