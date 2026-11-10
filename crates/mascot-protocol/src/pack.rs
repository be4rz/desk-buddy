//! Pack and art JSON schemas (design §3.3, §3.4): serde structs only, no IO.
//! `mascot-core` loads and validates them; studio and tests share them.
//! Unknown fields are ignored so the art pipeline can add notes freely.

use std::collections::BTreeMap;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

/// The only `pack_version` this build reads.
pub const PACK_VERSION: u32 = 1;

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
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    StageA,
    StageB,
    Face,
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
/// `"hold"` = until the intent ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Loops {
    #[default]
    Once,
    Range(u32, u32),
    Hold,
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
        }
        match Raw::deserialize(d).map_err(|_| de::Error::custom("loops must be [lo, hi] or \"hold\""))? {
            Raw::Range([lo, hi]) if 1 <= lo && lo <= hi => Ok(Loops::Range(lo, hi)),
            Raw::Range(_) => Err(de::Error::custom("loops [lo, hi] needs 1 <= lo <= hi")),
            Raw::Word(w) if w == "hold" => Ok(Loops::Hold),
            Raw::Word(w) => Err(de::Error::custom(format!("loops must be [lo, hi] or \"hold\", not {w:?}"))),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PoolEntry {
    pub behavior: String,
    pub weight: f64,
    #[serde(default)]
    pub cooldown_s: f64,
    /// Temperament trait → k (effective weight × max(0, 1 + k·(2·trait − 1))).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub traits: BTreeMap<String, f64>,
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
}
