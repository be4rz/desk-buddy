//! Pack loader (design §3.3, §4.2): `pack.json` + the art JSON it points at →
//! `PackMeta`. Metadata only: strip files are checked to exist, never decoded.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use mascot_protocol::pack::{
    ActivityKind, BehaviorDef, BodyEntry, BodyFile, Capabilities, FaceFile, Loops, Memory, PackFile, PoolEntry, PropsFile, SourceKind, StageAFile, StageBFile,
    StationFile, StepDef, Temperament, Timers, TurningFile, WorkFile, PACK_VERSION,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::brain::Intent;

/// Screen direction of a sprite (S faces the viewer).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dir {
    S,
    SE,
    E,
    NE,
    N,
    NW,
    W,
    SW,
}

impl Dir {
    pub const ALL: [Dir; 8] = [Dir::S, Dir::SE, Dir::E, Dir::NE, Dir::N, Dir::NW, Dir::W, Dir::SW];

    pub fn parse(s: &str) -> Option<Dir> {
        Dir::ALL.into_iter().find(|d| d.as_str() == s)
    }

    /// One 45° step: +1 counter-clockwise (the next in `ALL`), −1 clockwise.
    pub fn step(self, by: i8) -> Dir {
        let i = Dir::ALL.iter().position(|d| *d == self).unwrap_or(0) as i32;
        Dir::ALL[(i + by as i32).rem_euclid(8) as usize]
    }

    /// Steps from `self` to `to`, counter-clockwise (0..8).
    pub fn ccw_to(self, to: Dir) -> u8 {
        let i = |d: Dir| Dir::ALL.iter().position(|x| *x == d).unwrap_or(0) as i32;
        (i(to) - i(self)).rem_euclid(8) as u8
    }

    /// The dir a listening bear faces (`#pack-turning`): the five front dirs stay, NE → E, NW → W;
    /// N has two (E or W, 2 steps each), `None` here.
    pub fn listen_target(self) -> Option<Dir> {
        match self {
            Dir::NE => Some(Dir::E),
            Dir::NW => Some(Dir::W),
            Dir::N => None,
            d => Some(d),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Dir::S => "S",
            Dir::SE => "SE",
            Dir::E => "E",
            Dir::NE => "NE",
            Dir::N => "N",
            Dir::NW => "NW",
            Dir::W => "W",
            Dir::SW => "SW",
        }
    }
}

/// A Stage B set placement: A is the default, B its true 3D mirror.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ori {
    A,
    B,
}

impl Ori {
    pub fn parse(s: &str) -> Option<Ori> {
        match s {
            "A" => Some(Ori::A),
            "B" => Some(Ori::B),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackMeta {
    pub id: String,
    pub dir: PathBuf,
    pub art_root: PathBuf,
    pub palettes: Vec<PathBuf>,
    /// Free + set clips by name.
    pub clips: BTreeMap<String, ClipMeta>,
    /// `bed`, `read`, …
    pub sets: BTreeMap<String, SetMeta>,
    pub face: Option<FaceMeta>,
    pub behaviors: BTreeMap<String, BehaviorDef>,
    pub intents: BTreeMap<Intent, Vec<String>>,
    pub idle_pool: Vec<PoolEntry>,
    pub memory: Memory,
    pub temperament: Temperament,
    pub timers: Timers,
    pub capabilities: Capabilities,
    /// The quiet-hours pool (additive to §3.3; default `nap`/`rest`, nap ×4).
    pub quiet_hours: QuietPool,
    /// Behaviors left out of the pool just after start (additive to §3.3).
    pub warmup: Warmup,
    /// What `buddy mascot do|emote` can ask for (body-tools, p1).
    pub body: BodyMeta,
    /// Problems in `pack.json`'s `body` block; their entries are dropped. `load` refuses a pack
    /// with any, `load_report` keeps them for `buddy studio lint`.
    pub body_issues: Vec<String>,
    /// `pack.json` `"turning"` (p2): the clips facing changes play. `None`, or clips that are not
    /// there (art not rendered yet), and facing changes snap.
    pub turning: Option<TurningFile>,
    /// `pack.json` `"stations"` (p3): by name. Declared stations whose art is not loaded are kept
    /// (names only, like `turning`); `available(name)` says whether one can play.
    pub stations: BTreeMap<String, StationMeta>,
    /// `pack.json` `"work"` (p3): when a long turn goes to a station. `None` = never.
    pub work: Option<WorkFile>,
}

/// A station (`#pack-stations`): its role clips, played as one synthesized behavior whose step
/// index is the role ([`Role`]).
#[derive(Clone, Debug, PartialEq)]
pub struct StationMeta {
    pub file: StationFile,
    /// `set` = the station's set; seq = enter, loop, to_user, talk, ask, listen, to_screen, exit.
    pub behavior: BehaviorDef,
}

/// A station role: the step of its synthesized behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Enter = 0,
    Loop = 1,
    ToUser = 2,
    Talk = 3,
    Ask = 4,
    Listen = 5,
    ToScreen = 6,
    Exit = 7,
}

impl Role {
    pub const ALL: [Role; 8] = [Role::Enter, Role::Loop, Role::ToUser, Role::Talk, Role::Ask, Role::Listen, Role::ToScreen, Role::Exit];

    pub fn step(self) -> usize {
        self as usize
    }

    pub fn from_step(i: usize) -> Option<Role> {
        Role::ALL.get(i).copied()
    }

    /// The loops a role holds (until the brain moves on); the rest play once.
    pub fn holds(self) -> bool {
        matches!(self, Role::Loop | Role::Talk | Role::Ask | Role::Listen)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Enter => "enter",
            Role::Loop => "loop",
            Role::ToUser => "to_user",
            Role::Talk => "talk",
            Role::Ask => "ask",
            Role::Listen => "listen",
            Role::ToScreen => "to_screen",
            Role::Exit => "exit",
        }
    }
}

impl StationMeta {
    pub fn new(file: StationFile) -> StationMeta {
        let clips = [
            &file.enter,
            &file.work_loop,
            &file.to_user,
            &file.talk,
            &file.ask,
            file.listen.as_ref().unwrap_or(&file.talk),
            &file.to_screen,
            &file.exit,
        ];
        let seq = Role::ALL
            .iter()
            .zip(clips)
            .map(|(r, c)| StepDef { clip: c.clone(), loops: if r.holds() { Loops::Hold } else { Loops::Once } })
            .collect();
        let behavior = BehaviorDef { set: Some(file.set.clone()), seq, turn: None, exit: false };
        StationMeta { file, behavior }
    }

    /// The clip a role plays.
    pub fn clip(&self, role: Role) -> &str {
        &self.behavior.seq[role.step()].clip
    }
}

/// The pack's body: activities (`buddy mascot do`) and emotes (`buddy mascot emote`).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyMeta {
    pub default_for_s: u64,
    pub max_for_s: u64,
    /// In pack order.
    pub activities: Vec<(String, ActivityMeta)>,
    pub emotes: Vec<(String, EmoteMeta)>,
}

impl BodyMeta {
    pub fn activity(&self, name: &str) -> Option<&ActivityMeta> {
        self.activities.iter().find(|(n, _)| n == name).map(|(_, a)| a)
    }

    pub fn emote(&self, name: &str) -> Option<&EmoteMeta> {
        self.emotes.iter().find(|(n, _)| n == name).map(|(_, e)| e)
    }

    pub fn activity_names(&self) -> Vec<String> {
        self.activities.iter().map(|(n, _)| n.clone()).collect()
    }

    pub fn emote_names(&self) -> Vec<String> {
        self.emotes.iter().map(|(n, _)| n.clone()).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivityMeta {
    /// Behavior fallback chain: the first available one plays.
    pub plays: Vec<String>,
    pub kind: ActivityKind,
    pub about: String,
    /// A station activity (`computer`): `plays` is `[station]`, its synthesized behavior.
    pub station: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmoteMeta {
    pub act: EmoteAct,
    pub about: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EmoteAct {
    /// A pack intent (its `intents` chain plays).
    Intent(Intent),
    /// A behavior fallback chain.
    Plays(Vec<String>),
}

/// The `body` a pack without one gets (design `#pack-body`), as JSON.
pub const DEFAULT_BODY: &str = r#"{"default_for_s":120,"max_for_s":1800,
 "activities":{"read":{"plays":["read"],"kind":"pastime","about":"sit down and read a book"},
               "nap":{"plays":["nap"],"kind":"pastime","about":"get into bed and sleep"}},
 "emotes":{"wave":{"intent":"wave","about":"a happy little wave"},"nod":{"intent":"nod","about":"a small yes"},
           "shake":{"intent":"shake","about":"a sad little no"},"jump":{"intent":"jump","about":"a bounce of joy"},
           "spin":{"intent":"spin","about":"a twirl"}}}"#;

/// An activity or emote name: `[a-z0-9_-]{1,32}` (the host checks the same shape).
pub fn body_name_ok(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The longest `max_for_s` (30m, the CLI's `--for` limit).
pub const MAX_FOR_S: u64 = 1800;
/// The longest `about` text.
pub const MAX_ABOUT: usize = 120;

/// `pack.json` `"quiet_hours": {"pool": [...], "boost": {behavior: ×}}`: in quiet
/// hours the idle pool is `pool` and each `boost` entry's weight is multiplied.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QuietPool {
    pub pool: Vec<String>,
    #[serde(default)]
    pub boost: BTreeMap<String, f64>,
}

impl Default for QuietPool {
    fn default() -> Self {
        QuietPool { pool: vec!["nap".into(), "rest".into()], boost: BTreeMap::from([("nap".into(), 4.0)]) }
    }
}

/// `pack.json` `"warmup": {"duration_s": 180, "exclude": [...]}`: for the first
/// `duration_s` after the brain starts, `exclude` is out of the idle pool
/// (quiet hours excepted), so a fresh start shows awake variety.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Warmup {
    pub duration_s: f64,
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl Default for Warmup {
    fn default() -> Self {
        Warmup { duration_s: 180.0, exclude: vec!["nap".into()] }
    }
}

/// The optional p2 keys, read from the same text (`PackFile` ignores them).
#[derive(Deserialize, Default)]
struct Extras {
    #[serde(default)]
    quiet_hours: Option<QuietPool>,
    #[serde(default)]
    warmup: Option<Warmup>,
    #[serde(default)]
    body: Option<BodyFile>,
    #[serde(default)]
    turning: Option<TurningFile>,
    #[serde(default)]
    stations: BTreeMap<String, StationFile>,
    #[serde(default)]
    work: Option<WorkFile>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipMeta {
    pub name: String,
    /// The Stage B set it plays in; `None` = a free (Stage A) clip.
    pub set: Option<String>,
    pub looping: bool,
    pub fps: u32,
    /// Set clips: ori A's canvas (per-ori values in `SetMeta`).
    pub frame_w: u32,
    pub frame_h: u32,
    pub anchor: [i32; 2],
    pub durs_ms: Vec<u32>,
    /// A turn clip's step (+1 ccw, −1 cw); 0 for every other clip.
    pub turn_steps: i8,
    /// Free clips.
    pub dirs: BTreeMap<Dir, StripRef>,
    /// Set clips.
    pub oris: BTreeMap<Ori, StripRef>,
    /// Set transitions: where the free bear stands and faces at the hand-off.
    pub handoff: BTreeMap<Ori, Handoff>,
}

impl ClipMeta {
    pub fn frames_n(&self) -> usize {
        self.durs_ms.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripRef {
    pub file: PathBuf,
    /// Per frame: screen-pixel move after showing it (empty for set clips).
    pub root_px: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    /// The hand-off is at the clip's start (entering the set) or end (leaving it).
    pub at_start: bool,
    pub frame: u32,
    /// Stage A anchor offset from the set anchor.
    pub px: [i32; 2],
    pub facing: Dir,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetMeta {
    pub name: String,
    pub oris: BTreeMap<Ori, SetOri>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetOri {
    pub frame_w: u32,
    pub frame_h: u32,
    pub anchor: [i32; 2],
    /// The props-only frame on the clip canvas.
    pub props_frame: PathBuf,
    /// The set's separate prop sprites, by prop name.
    pub props: Vec<PropRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropRef {
    pub name: String,
    pub file: PathBuf,
    pub size: [u32; 2],
    pub anchor: [i32; 2],
    pub offset_px_in_set: [i32; 2],
}

/// D16 face layers (§3.4): opaque patches over the body's baked default face.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceMeta {
    pub dirs: Vec<Dir>,
    /// Slot (`eyes`, `mouth`) → state → dir → patch.
    pub layers: BTreeMap<String, BTreeMap<String, BTreeMap<Dir, FaceLayer>>>,
    /// Clip → dir → slot → top-left per body frame.
    pub clips: BTreeMap<String, BTreeMap<Dir, BTreeMap<String, Vec<[i32; 2]>>>>,
    /// v2: clip → body dir → the dir whose patches it draws (`listen`: the head turned to the camera).
    pub patch_dirs: BTreeMap<String, BTreeMap<Dir, Dir>>,
    /// v2 (p3): set clip → ori → the patch dir and slot positions on the set frame.
    pub set_clips: BTreeMap<String, BTreeMap<Ori, SetFace>>,
}

/// The face of one set clip in one ori (`#face-v2` `set_clips`).
#[derive(Clone, Debug, PartialEq)]
pub struct SetFace {
    /// The dir whose patches it draws (the head turned to the camera: `S`).
    pub patch: Dir,
    /// Slot → top-left per frame, on the set clip's frame.
    pub slots: BTreeMap<String, Vec<[i32; 2]>>,
}

impl FaceMeta {
    /// Whether the face layers cover this clip in this direction.
    pub fn covers(&self, clip: &str, dir: Dir) -> bool {
        self.clips.get(clip).is_some_and(|d| d.contains_key(&dir))
    }

    /// The dir whose patches this clip draws when the body faces `dir` (itself unless `patch_dirs` says).
    pub fn patch_dir(&self, clip: &str, dir: Dir) -> Dir {
        self.patch_dirs.get(clip).and_then(|m| m.get(&dir)).copied().unwrap_or(dir)
    }

    /// The face of a set clip in this ori, if the face layers cover it.
    pub fn set_face(&self, clip: &str, ori: Ori) -> Option<&SetFace> {
        self.set_clips.get(clip).and_then(|m| m.get(&ori))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceLayer {
    pub file: PathBuf,
    pub w: u32,
    pub h: u32,
}

/// Why a pack doesn't load. `dir` is the pack dir; `path` the file as written
/// relative to it. Displays on one line: `pack <dir>: <path>: <what>`.
#[derive(Debug, Clone, PartialEq)]
pub enum PackError {
    Missing { dir: PathBuf, path: PathBuf },
    Parse { dir: PathBuf, path: PathBuf, msg: String },
    Version { dir: PathBuf, path: PathBuf, found: u32, supported: u32 },
    Invalid { dir: PathBuf, path: PathBuf, msg: String },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Missing { dir, path } => write!(f, "pack {}: {}: not found", dir.display(), path.display()),
            PackError::Parse { dir, path, msg } => write!(f, "pack {}: {}: bad JSON: {}", dir.display(), path.display(), one_line(msg)),
            PackError::Version { dir, path, found, supported } => {
                write!(f, "pack {}: {}: pack_version {found} is not supported (this build reads {supported})", dir.display(), path.display())
            }
            PackError::Invalid { dir, path, msg } => write!(f, "pack {}: {}: {}", dir.display(), path.display(), one_line(msg)),
        }
    }
}

impl std::error::Error for PackError {}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl PackMeta {
    /// The behavior an intent plays: the first available one of its chain.
    /// `load` guarantees there is one for every intent.
    pub fn resolve(&self, intent: Intent) -> &str {
        self.intents
            .get(&intent)
            .and_then(|chain| chain.iter().find(|b| self.available(b)))
            .map(String::as_str)
            .unwrap_or("rest")
    }

    /// The pack's turn clip for a step from `from` (`by` +1 ccw, −1 cw), if the art has it.
    pub fn turn_clip(&self, from: Dir, by: i8) -> Option<&str> {
        let t = self.turning.as_ref()?;
        let name = if by > 0 { &t.ccw } else { &t.cw };
        self.clips.get(name).filter(|c| c.set.is_none() && c.dirs.contains_key(&from)).map(|c| c.name.as_str())
    }

    /// A behavior by name: a pack behavior, or a station's synthesized one.
    pub fn behavior(&self, name: &str) -> Option<&BehaviorDef> {
        self.behaviors.get(name).or_else(|| self.stations.get(name).map(|s| &s.behavior))
    }

    /// The station `name`, if it is one.
    pub fn station(&self, name: &str) -> Option<&StationMeta> {
        self.stations.get(name)
    }

    /// The station long turns go to, when its art is there.
    pub fn work_station(&self) -> Option<(&str, &WorkFile)> {
        let w = self.work.as_ref()?;
        self.available(&w.station).then_some((w.station.as_str(), w))
    }

    /// The first available behavior of a fallback chain.
    pub fn first_available<'a>(&self, chain: &'a [String]) -> Option<&'a str> {
        chain.iter().find(|b| self.available(b)).map(String::as_str)
    }

    /// Every clip it names exists (free clips in at least `S`, set clips in
    /// both orientations, in its set) and its set exists.
    pub fn available(&self, behavior: &str) -> bool {
        let Some(b) = self.behavior(behavior) else { return false };
        if let Some(set) = &b.set {
            if !self.sets.contains_key(set) {
                return false;
            }
        }
        !b.seq.is_empty()
            && b.seq.iter().all(|step| match (self.clips.get(&step.clip), &b.set) {
                (Some(c), None) => c.set.is_none() && c.dirs.contains_key(&Dir::S),
                (Some(c), Some(set)) => c.set.as_deref() == Some(set) && c.oris.contains_key(&Ori::A) && c.oris.contains_key(&Ori::B),
                (None, _) => false,
            })
    }
}

/// Loads and validates the pack in `dir` (`dir/pack.json`). A problem in the `body` block
/// is an error too (the first one).
pub fn load(dir: &Path) -> Result<PackMeta, PackError> {
    let meta = load_report(dir)?;
    match meta.body_issues.first() {
        Some(i) => Err(PackError::Invalid { dir: dir.to_path_buf(), path: "pack.json".into(), msg: i.clone() }),
        None => Ok(meta),
    }
}

/// `load`, except that `body` problems are kept in `body_issues` (their entries dropped)
/// instead of failing: for the studio (lint reports them; render, audit and the window still show the art).
pub fn load_report(dir: &Path) -> Result<PackMeta, PackError> {
    Loader { dir: dir.to_path_buf() }.load()
}

struct Loader {
    dir: PathBuf,
}

impl Loader {
    fn abs(&self, rel: &Path) -> PathBuf {
        self.dir.join(rel)
    }

    fn missing(&self, rel: &Path) -> PackError {
        PackError::Missing { dir: self.dir.clone(), path: rel.to_path_buf() }
    }

    fn invalid(&self, rel: &Path, msg: impl Into<String>) -> PackError {
        PackError::Invalid { dir: self.dir.clone(), path: rel.to_path_buf(), msg: msg.into() }
    }

    fn read(&self, rel: &Path) -> Result<String, PackError> {
        let abs = self.abs(rel);
        if !abs.is_file() {
            return Err(self.missing(rel));
        }
        std::fs::read_to_string(&abs).map_err(|e| self.invalid(rel, format!("unreadable: {e}")))
    }

    fn json<T: DeserializeOwned>(&self, rel: &Path, text: &str) -> Result<T, PackError> {
        serde_json::from_str(text).map_err(|e| PackError::Parse { dir: self.dir.clone(), path: rel.to_path_buf(), msg: e.to_string() })
    }

    fn need_file(&self, rel: &Path) -> Result<PathBuf, PackError> {
        let abs = self.abs(rel);
        if abs.is_file() {
            Ok(abs)
        } else {
            Err(self.missing(rel))
        }
    }

    fn load(&self) -> Result<PackMeta, PackError> {
        let rel = Path::new("pack.json");
        let text = self.read(rel)?;
        // Syntax first (line/col), then the version, then the schema.
        let raw: serde_json::Value = self.json(rel, &text)?;
        match raw.get("pack_version").and_then(|v| v.as_u64()) {
            Some(v) if v == PACK_VERSION as u64 => {}
            Some(v) => {
                return Err(PackError::Version {
                    dir: self.dir.clone(),
                    path: rel.into(),
                    found: v.min(u32::MAX as u64) as u32,
                    supported: PACK_VERSION,
                })
            }
            None => return Err(self.invalid(rel, "pack_version is missing or not a number")),
        }
        let file: PackFile = self.json(rel, &text)?;
        let extras: Extras = self.json(rel, &text)?;

        let art_rel = PathBuf::from(&file.art_root);
        if !self.abs(&art_rel).is_dir() {
            return Err(self.missing(&art_rel));
        }
        let palettes = file.palettes.iter().map(|p| self.need_file(Path::new(p))).collect::<Result<Vec<_>, _>>()?;

        let mut clips = BTreeMap::new();
        let mut sets = BTreeMap::new();
        let mut faces = Vec::new();
        let mut skipped_optional = false;
        for src in &file.sources {
            let src_rel = art_rel.join(&src.path);
            if src.optional && !self.abs(&src_rel).exists() {
                skipped_optional = true;
                continue;
            }
            let text = self.read(&src_rel)?;
            let base = src_rel.parent().map(Path::to_path_buf).unwrap_or_default();
            match src.kind {
                SourceKind::StageA => self.stage_a(&src_rel, &base, self.json(&src_rel, &text)?, &mut clips)?,
                SourceKind::StageB => {
                    let Some(props) = &src.props else { return Err(self.invalid(Path::new("pack.json"), format!("stage_b source {} has no props", src.path))) };
                    let props_rel = art_rel.join(props);
                    let props_text = self.read(&props_rel)?;
                    let props: PropsFile = self.json(&props_rel, &props_text)?;
                    self.stage_b(&src_rel, &base, self.json(&src_rel, &text)?, &props_rel, props, &mut clips, &mut sets)?
                }
                SourceKind::Face => faces.push((src_rel.clone(), base, self.json::<FaceFile>(&src_rel, &text)?)),
            }
        }
        let mut face = None;
        for (rel, base, f) in faces {
            if face.is_some() {
                return Err(self.invalid(&rel, "only one face source is allowed"));
            }
            face = Some(self.face(&rel, &base, f, &clips, skipped_optional)?);
        }

        let mut meta = PackMeta {
            id: file.id.clone(),
            dir: self.dir.clone(),
            art_root: self.abs(&art_rel),
            palettes,
            clips,
            sets,
            face,
            behaviors: file.behaviors.clone(),
            intents: BTreeMap::new(),
            idle_pool: file.idle_pool.clone(),
            memory: file.memory,
            temperament: file.temperament,
            timers: file.timers,
            capabilities: file.capabilities.clone(),
            quiet_hours: QuietPool::default(),
            warmup: Warmup::default(),
            body: BodyMeta { default_for_s: 120, max_for_s: 1800, activities: Vec::new(), emotes: Vec::new() },
            body_issues: Vec::new(),
            turning: None,
            stations: BTreeMap::new(),
            work: None,
        };
        meta.turning = self.check_turning(extras.turning, &meta.clips)?;
        self.check_stations(extras.stations, extras.work, &mut meta)?;
        // the built-in defaults name only what this pack defines (a pack without
        // nap still loads); values the pack writes itself are checked strictly
        let defined = |b: &String| meta.behaviors.contains_key(b);
        meta.quiet_hours = extras.quiet_hours.unwrap_or_else(|| {
            let mut q = QuietPool::default();
            q.pool.retain(defined);
            q.boost.retain(|b, _| defined(b));
            q
        });
        meta.warmup = extras.warmup.unwrap_or_else(|| {
            let mut w = Warmup::default();
            w.exclude.retain(defined);
            w
        });
        self.check_behaviors(&file, &mut meta)?;
        match extras.body {
            Some(b) => check_body(b, &mut meta),
            None => {
                // the built-in block, without what this pack can't play (like quiet_hours)
                let b: BodyFile = serde_json::from_str(DEFAULT_BODY).expect("DEFAULT_BODY parses");
                check_body(b, &mut meta);
                meta.body_issues.clear();
            }
        }
        Ok(meta)
    }

    fn stage_a(&self, rel: &Path, base: &Path, f: StageAFile, clips: &mut BTreeMap<String, ClipMeta>) -> Result<(), PackError> {
        for (name, c) in f.anims {
            if c.frames.len() != c.frames_n as usize || c.frames_n == 0 {
                return Err(self.invalid(rel, format!("clip {name}: frames_n {} but {} frames", c.frames_n, c.frames.len())));
            }
            let mut dirs = BTreeMap::new();
            for (d, file) in &c.files {
                let dir = Dir::parse(d).ok_or_else(|| self.invalid(rel, format!("clip {name}: unknown direction {d:?}")))?;
                let root_px = c
                    .frames
                    .iter()
                    .enumerate()
                    .map(|(i, fr)| fr.root_px.get(d).copied().ok_or_else(|| self.invalid(rel, format!("clip {name}: frame {i} has no root_px for {d}"))))
                    .collect::<Result<Vec<_>, _>>()?;
                dirs.insert(dir, StripRef { file: self.need_file(&base.join(file))?, root_px });
            }
            let clip = ClipMeta {
                name: name.clone(),
                set: None,
                looping: c.looping,
                fps: c.fps,
                frame_w: c.frame_w,
                frame_h: c.frame_h,
                anchor: c.anchor,
                durs_ms: c.frames.iter().map(|fr| fr.dur).collect(),
                turn_steps: c.turn_steps,
                dirs,
                oris: BTreeMap::new(),
                handoff: BTreeMap::new(),
            };
            if clips.insert(name.clone(), clip).is_some() {
                return Err(self.invalid(rel, format!("clip {name} is defined by two sources")));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_b(
        &self,
        rel: &Path,
        base: &Path,
        f: StageBFile,
        props_rel: &Path,
        props: PropsFile,
        clips: &mut BTreeMap<String, ClipMeta>,
        sets: &mut BTreeMap<String, SetMeta>,
    ) -> Result<(), PackError> {
        let props_base = props_rel.parent().map(Path::to_path_buf).unwrap_or_default();
        for (set_name, oris) in &f.sets {
            let mut set = SetMeta { name: set_name.clone(), oris: BTreeMap::new() };
            for (o, s) in oris {
                let ori = Ori::parse(o).ok_or_else(|| self.invalid(rel, format!("set {set_name}: unknown ori {o:?}")))?;
                let mut set_props = Vec::new();
                for (pname, p) in props.props.iter().filter(|(_, p)| &p.set == set_name) {
                    let po = if ori == Ori::A { &p.a } else { &p.b };
                    set_props.push(PropRef {
                        name: pname.clone(),
                        file: self.need_file(&props_base.join(&po.file))?,
                        size: po.size,
                        anchor: po.anchor,
                        offset_px_in_set: po.offset_px_in_set,
                    });
                }
                set.oris.insert(
                    ori,
                    SetOri {
                        frame_w: s.frame_w,
                        frame_h: s.frame_h,
                        anchor: s.anchor,
                        props_frame: self.need_file(&base.join(&s.props_frame))?,
                        props: set_props,
                    },
                );
            }
            for ori in [Ori::A, Ori::B] {
                if !set.oris.contains_key(&ori) {
                    return Err(self.invalid(rel, format!("set {set_name}: ori {ori:?} is missing")));
                }
            }
            sets.insert(set_name.clone(), set);
        }
        for (name, c) in f.anims {
            let Some(set) = sets.get(&c.set) else { return Err(self.invalid(rel, format!("clip {name}: unknown set {}", c.set))) };
            if c.frames_n == 0 || (!c.frames.is_empty() && c.frames.len() != c.frames_n as usize) {
                return Err(self.invalid(rel, format!("clip {name}: frames_n {} but {} frames", c.frames_n, c.frames.len())));
            }
            let durs_ms = if c.frames.is_empty() {
                vec![1000 / c.fps.max(1); c.frames_n as usize]
            } else {
                c.frames.iter().map(|fr| fr.dur).collect()
            };
            let mut oris = BTreeMap::new();
            for (o, file) in &c.files {
                let ori = Ori::parse(o).ok_or_else(|| self.invalid(rel, format!("clip {name}: unknown ori {o:?}")))?;
                if !set.oris.contains_key(&ori) {
                    return Err(self.invalid(rel, format!("clip {name}: set {} has no ori {o}", c.set)));
                }
                oris.insert(ori, StripRef { file: self.need_file(&base.join(file))?, root_px: Vec::new() });
            }
            let mut handoff = BTreeMap::new();
            for (o, h) in &c.handoff {
                let ori = Ori::parse(o).ok_or_else(|| self.invalid(rel, format!("clip {name}: unknown ori {o:?}")))?;
                let facing = Dir::parse(&h.facing).ok_or_else(|| self.invalid(rel, format!("clip {name}: unknown facing {:?}", h.facing)))?;
                let at_start = match h.at.as_str() {
                    "start" => true,
                    "end" => false,
                    other => return Err(self.invalid(rel, format!("clip {name}: handoff at {other:?}"))),
                };
                handoff.insert(ori, Handoff { at_start, frame: h.frame, px: h.px, facing });
            }
            let a = set.oris.get(&Ori::A).or_else(|| set.oris.values().next());
            let clip = ClipMeta {
                name: name.clone(),
                set: Some(c.set.clone()),
                looping: c.looping,
                fps: c.fps,
                frame_w: a.map_or(0, |s| s.frame_w),
                frame_h: a.map_or(0, |s| s.frame_h),
                anchor: a.map_or([0, 0], |s| s.anchor),
                durs_ms,
                turn_steps: 0,
                dirs: BTreeMap::new(),
                oris,
                handoff,
            };
            if clips.insert(name.clone(), clip).is_some() {
                return Err(self.invalid(rel, format!("clip {name} is defined by two sources")));
            }
        }
        Ok(())
    }

    /// `skipped_optional`: an optional source is missing, so face positions for clips it would have
    /// brought (e.g. `listen` without `anim_l`) are dropped instead of failing the whole pack.
    fn face(&self, rel: &Path, base: &Path, mut f: FaceFile, clips: &BTreeMap<String, ClipMeta>, skipped_optional: bool) -> Result<FaceMeta, PackError> {
        if skipped_optional {
            f.clips.retain(|c, _| clips.contains_key(c));
            let kept: Vec<String> = f.clips.keys().cloned().collect();
            f.patch_dirs.retain(|c, _| kept.contains(c));
            f.set_clips.retain(|c, _| clips.contains_key(c));
        }
        if !(1..=2).contains(&f.face_version) {
            return Err(self.invalid(rel, format!("face_version {} is not supported (this build reads 1 and 2)", f.face_version)));
        }
        if f.face_version == 1 && !f.patch_dirs.is_empty() {
            return Err(self.invalid(rel, "patch_dirs needs face_version 2"));
        }
        if f.face_version == 1 && !f.set_clips.is_empty() {
            return Err(self.invalid(rel, "set_clips needs face_version 2"));
        }
        let dir_of = |d: &str| Dir::parse(d).ok_or_else(|| self.invalid(rel, format!("unknown direction {d:?}")));
        let dirs = f.dirs.iter().map(|d| dir_of(d)).collect::<Result<Vec<_>, _>>()?;
        // The slots and states the brain shows (README "Face layers").
        const NEEDED: [(&str, &[&str]); 2] = [("eyes", &["open", "closed"]), ("mouth", &["closed", "open", "wide"])];
        for (slot, states) in NEEDED {
            let Some(def) = f.slots.get(slot) else { return Err(self.invalid(rel, format!("slot {slot} is missing"))) };
            for st in states {
                if !def.states.iter().any(|s| s == st) {
                    return Err(self.invalid(rel, format!("slot {slot} has no state {st}")));
                }
            }
        }
        let mut layers = BTreeMap::new();
        for (slot, states) in &f.layers {
            let def = f.slots.get(slot).ok_or_else(|| self.invalid(rel, format!("layers: unknown slot {slot}")))?;
            let mut by_state = BTreeMap::new();
            for (state, by_dir) in states {
                if !def.states.contains(state) {
                    return Err(self.invalid(rel, format!("layers: slot {slot} has no state {state}")));
                }
                let mut m = BTreeMap::new();
                for (d, l) in by_dir {
                    m.insert(dir_of(d)?, FaceLayer { file: self.need_file(&base.join(&l.file))?, w: l.w, h: l.h });
                }
                by_state.insert(state.clone(), m);
            }
            layers.insert(slot.clone(), by_state);
        }
        let mut patch_dirs: BTreeMap<String, BTreeMap<Dir, Dir>> = BTreeMap::new();
        for (clip, m) in &f.patch_dirs {
            if !f.clips.contains_key(clip) {
                return Err(self.invalid(rel, format!("patch_dirs: {clip} has no face positions")));
            }
            for (body, patch) in m {
                let (b, p) = (dir_of(body)?, dir_of(patch)?);
                if !dirs.contains(&p) {
                    return Err(self.invalid(rel, format!("patch_dirs: {clip} {body}: {patch} is not a face dir")));
                }
                patch_dirs.entry(clip.clone()).or_default().insert(b, p);
            }
        }
        let mut fclips = BTreeMap::new();
        for (clip, by_dir) in f.clips {
            let Some(c) = clips.get(&clip) else { return Err(self.invalid(rel, format!("clips: unknown clip {clip}"))) };
            let mut m = BTreeMap::new();
            for (d, slots) in by_dir {
                let dir = dir_of(&d)?;
                // the patches it draws: its own dir's, or the one patch_dirs names
                let pd = patch_dirs.get(&clip).and_then(|m| m.get(&dir)).copied().unwrap_or(dir);
                for (slot, states) in NEEDED {
                    if !slots.contains_key(slot) {
                        return Err(self.invalid(rel, format!("clips: {clip} {d}: no {slot} positions")));
                    }
                    for st in states {
                        if !layers.get(slot).and_then(|m: &BTreeMap<String, BTreeMap<Dir, FaceLayer>>| m.get(*st)).is_some_and(|m| m.contains_key(&pd)) {
                            return Err(self.invalid(rel, format!("clips: {clip} {d}: no {slot} {st} layer for {}", pd.as_str())));
                        }
                    }
                }
                for (slot, pos) in &slots {
                    if !f.slots.contains_key(slot) {
                        return Err(self.invalid(rel, format!("clips: {clip} {d}: unknown slot {slot}")));
                    }
                    if pos.len() != c.frames_n() {
                        return Err(self.invalid(rel, format!("clips: {clip} {d} {slot}: {} positions for {} frames", pos.len(), c.frames_n())));
                    }
                }
                m.insert(dir, slots);
            }
            fclips.insert(clip, m);
        }
        let mut set_clips = BTreeMap::new();
        for (clip, by_ori) in f.set_clips {
            let Some(c) = clips.get(&clip).filter(|c| c.set.is_some()) else { return Err(self.invalid(rel, format!("set_clips: {clip} is not a set clip"))) };
            let mut m = BTreeMap::new();
            for (o, sf) in by_ori {
                let at = format!("set_clips: {clip} {o}");
                let ori = Ori::parse(&o).filter(|ori| c.oris.contains_key(ori)).ok_or_else(|| self.invalid(rel, format!("{at}: the clip has no ori {o}")))?;
                let patch = dir_of(&sf.patch)?;
                for (slot, states) in NEEDED {
                    if !sf.slots.contains_key(slot) {
                        return Err(self.invalid(rel, format!("{at}: no {slot} positions")));
                    }
                    for st in states {
                        if !layers.get(slot).and_then(|m: &BTreeMap<String, BTreeMap<Dir, FaceLayer>>| m.get(*st)).is_some_and(|m| m.contains_key(&patch)) {
                            return Err(self.invalid(rel, format!("{at}: no {slot} {st} layer for {}", patch.as_str())));
                        }
                    }
                }
                for (slot, pos) in &sf.slots {
                    if !f.slots.contains_key(slot) {
                        return Err(self.invalid(rel, format!("{at}: unknown slot {slot}")));
                    }
                    if pos.len() != c.frames_n() {
                        return Err(self.invalid(rel, format!("{at} {slot}: {} positions for {} frames", pos.len(), c.frames_n())));
                    }
                }
                m.insert(ori, SetFace { patch, slots: sf.slots });
            }
            set_clips.insert(clip, m);
        }
        Ok(FaceMeta { dirs, layers, clips: fclips, patch_dirs, set_clips })
    }

    /// The `turning` block: names only (clips not rendered yet are fine: the brain snaps), but a
    /// turn clip that is there must be a free clip stepping the way its role says.
    fn check_turning(&self, t: Option<TurningFile>, clips: &BTreeMap<String, ClipMeta>) -> Result<Option<TurningFile>, PackError> {
        let rel = Path::new("pack.json");
        let Some(t) = t else { return Ok(None) };
        for (role, name, want) in [("ccw", &t.ccw, 1i8), ("cw", &t.cw, -1i8)] {
            let Some(c) = clips.get(name) else { continue };
            if c.set.is_some() {
                return Err(self.invalid(rel, format!("turning: {role} clip {name} is a set clip")));
            }
            if c.turn_steps != want {
                return Err(self.invalid(rel, format!("turning: {role} clip {name} has turn_steps {}, not {want}", c.turn_steps)));
            }
        }
        Ok(Some(t))
    }

    /// `stations` + `work` (p3). A station whose set is not loaded (art not rendered yet) is kept as
    /// a name only: it never plays. One whose set is there must be complete.
    fn check_stations(&self, stations: BTreeMap<String, StationFile>, work: Option<WorkFile>, meta: &mut PackMeta) -> Result<(), PackError> {
        let rel = Path::new("pack.json");
        for (name, f) in stations {
            let at = format!("stations: {name}");
            if meta.behaviors.contains_key(&name) {
                return Err(self.invalid(rel, format!("{at}: a behavior has the same name")));
            }
            let st = StationMeta::new(f);
            if meta.sets.contains_key(&st.file.set) {
                for role in Role::ALL {
                    let clip = st.clip(role);
                    let Some(c) = meta.clips.get(clip) else { return Err(self.invalid(rel, format!("{at}: {} clip {clip} is missing", role.as_str()))) };
                    if c.set.as_deref() != Some(st.file.set.as_str()) {
                        return Err(self.invalid(rel, format!("{at}: {} clip {clip} is not in set {}", role.as_str(), st.file.set)));
                    }
                    for ori in [Ori::A, Ori::B] {
                        if !c.oris.contains_key(&ori) {
                            return Err(self.invalid(rel, format!("{at}: {} clip {clip} has no ori {ori:?}", role.as_str())));
                        }
                        let want = match role {
                            Role::Enter => Some(true),
                            Role::Exit => Some(false),
                            _ => None,
                        };
                        if let Some(at_start) = want {
                            if c.handoff.get(&ori).map(|h| h.at_start) != Some(at_start) {
                                let w = if at_start { "start" } else { "end" };
                                return Err(self.invalid(rel, format!("{at}: {} clip {clip} needs a hand-off at {w} for ori {ori:?}", role.as_str())));
                            }
                        }
                    }
                }
            }
            meta.stations.insert(name, st);
        }
        if let Some(w) = &work {
            if !meta.stations.contains_key(&w.station) {
                return Err(self.invalid(rel, format!("work: unknown station {}", w.station)));
            }
            if w.after_tools == 0 {
                return Err(self.invalid(rel, "work: after_tools must be at least 1"));
            }
        }
        meta.work = work;
        Ok(())
    }

    fn check_behaviors(&self, file: &PackFile, meta: &mut PackMeta) -> Result<(), PackError> {
        let rel = Path::new("pack.json");
        for (name, b) in &file.behaviors {
            if b.seq.is_empty() {
                return Err(self.invalid(rel, format!("behavior {name} has an empty seq")));
            }
        }
        for key in ["rest", "hold"] {
            if !meta.available(key) {
                return Err(self.invalid(rel, format!("behavior {key} must exist and be available")));
            }
        }
        for (key, chain) in &file.intents {
            let intent = Intent::parse(key).ok_or_else(|| self.invalid(rel, format!("intents: unknown intent {key}")))?;
            if let Some(b) = chain.iter().find(|b| !meta.behaviors.contains_key(*b)) {
                return Err(self.invalid(rel, format!("intents: {key} names undefined behavior {b}")));
            }
            if !chain.iter().any(|b| meta.available(b)) {
                return Err(self.invalid(rel, format!("intents: {key} has no available behavior in [{}]", chain.join(", "))));
            }
            meta.intents.insert(intent, chain.clone());
        }
        if let Some(i) = Intent::ALL.iter().find(|i| !meta.intents.contains_key(i)) {
            return Err(self.invalid(rel, format!("intents: {} is missing", i.as_str())));
        }
        // optional intents get their built-in chain, without names this pack doesn't define
        for i in Intent::OPTIONAL {
            if !meta.intents.contains_key(&i) {
                let chain: Vec<String> = i.default_chain().iter().map(|b| b.to_string()).filter(|b| meta.behaviors.contains_key(b)).collect();
                meta.intents.insert(i, chain);
            }
        }
        for e in &file.idle_pool {
            if !meta.behaviors.contains_key(&e.behavior) {
                return Err(self.invalid(rel, format!("idle_pool: undefined behavior {}", e.behavior)));
            }
            if !(e.weight >= 0.0 && e.cooldown_s >= 0.0) {
                return Err(self.invalid(rel, format!("idle_pool: {} needs weight >= 0 and cooldown_s >= 0", e.behavior)));
            }
            if let Some(t) = e.traits.keys().find(|t| file.temperament.get(t).is_none()) {
                return Err(self.invalid(rel, format!("idle_pool: {} has unknown trait {t}", e.behavior)));
            }
        }
        let q = &meta.quiet_hours;
        let named = q.pool.iter().chain(q.boost.keys()).chain(meta.warmup.exclude.iter());
        if let Some(b) = named.into_iter().find(|b| !meta.behaviors.contains_key(*b)) {
            return Err(self.invalid(rel, format!("quiet_hours/warmup: undefined behavior {b}")));
        }
        if let Some((b, _)) = q.boost.iter().find(|(_, w)| !(**w >= 0.0)) {
            return Err(self.invalid(rel, format!("quiet_hours: boost for {b} must be >= 0")));
        }
        if !(meta.warmup.duration_s >= 0.0) {
            return Err(self.invalid(rel, "warmup: duration_s must be >= 0"));
        }
        Ok(())
    }
}

/// Validates `b` into `meta.body`; each bad entry is dropped with an issue.
fn check_body(b: BodyFile, meta: &mut PackMeta) {
    let mut issues = Vec::new();
    let mut body = BodyMeta { default_for_s: b.default_for_s, max_for_s: b.max_for_s, activities: Vec::new(), emotes: Vec::new() };
    if b.default_for_s > b.max_for_s {
        issues.push(format!("body: default_for_s {} is more than max_for_s {}", b.default_for_s, b.max_for_s));
        body.default_for_s = b.max_for_s;
    }
    if b.max_for_s == 0 {
        issues.push("body: max_for_s must be more than 0".into());
    }
    // `buddy mascot do --for` takes at most 30m (design #cli): a longer cap would be advertised but refused
    if b.max_for_s > MAX_FOR_S {
        issues.push(format!("body: max_for_s {} is more than {MAX_FOR_S}", b.max_for_s));
        body.max_for_s = MAX_FOR_S;
        body.default_for_s = body.default_for_s.min(MAX_FOR_S);
    }
    let common = |kind: &str, name: &str, e: &BodyEntry, issues: &mut Vec<String>| -> bool {
        let at = format!("body: {kind}.{name}");
        let list = if kind == "activities" { &b.activities } else { &b.emotes };
        if list.iter().filter(|(n, _)| n == name).count() > 1 {
            issues.push(format!("{at}: defined twice"));
            return false;
        }
        if !body_name_ok(name) {
            issues.push(format!("{at}: a name is 1-32 of a-z, 0-9, _ and -"));
            return false;
        }
        let keys = [e.plays.is_some(), e.intent.is_some(), e.station.is_some()].iter().filter(|k| **k).count();
        if keys != 1 {
            let want = if kind == "activities" { "plays or station" } else { "plays or intent" };
            issues.push(format!("{at}: needs exactly one of {want}"));
            return false;
        }
        let about = e.about.trim();
        if about.is_empty() {
            issues.push(format!("{at}: about is empty"));
            return false;
        }
        if about.chars().count() > MAX_ABOUT {
            issues.push(format!("{at}: about is longer than {MAX_ABOUT} characters"));
            return false;
        }
        if about.chars().next().is_some_and(char::is_uppercase) || about.ends_with('.') {
            issues.push(format!("{at}: about starts lower-case and has no trailing period"));
            return false;
        }
        true
    };
    let chain = |at: &str, plays: &[String], meta: &PackMeta, issues: &mut Vec<String>| -> bool {
        if let Some(x) = plays.iter().find(|x| !meta.behaviors.contains_key(*x)) {
            issues.push(format!("{at}: unknown behavior {x}"));
            return false;
        }
        if meta.first_available(plays).is_none() {
            issues.push(format!("{at}: no available behavior in [{}]", plays.join(", ")));
            return false;
        }
        true
    };
    for (name, e) in &b.activities {
        if !common("activities", name, e, &mut issues) {
            continue;
        }
        let at = format!("body: activities.{name}");
        if e.intent.is_some() {
            issues.push(format!("{at}: an activity has plays or station, not intent"));
            continue;
        }
        if let Some(st) = &e.station {
            if !meta.stations.contains_key(st) {
                issues.push(format!("{at}: unknown station {st}"));
                continue;
            }
            // a declared station whose art is not rendered yet can't play: left out (studio lint warns)
            if meta.available(st) {
                let a = ActivityMeta { plays: vec![st.clone()], kind: e.kind.unwrap_or_default(), about: e.about.trim().to_string(), station: Some(st.clone()) };
                body.activities.push((name.clone(), a));
            }
            continue;
        }
        let plays = e.plays.clone().unwrap_or_default();
        if !chain(&at, &plays, meta, &mut issues) {
            continue;
        }
        body.activities.push((name.clone(), ActivityMeta { plays, kind: e.kind.unwrap_or_default(), about: e.about.trim().to_string(), station: None }));
    }
    for (name, e) in &b.emotes {
        if !common("emotes", name, e, &mut issues) {
            continue;
        }
        let at = format!("body: emotes.{name}");
        if e.station.is_some() {
            issues.push(format!("{at}: an emote has plays or intent, not station"));
            continue;
        }
        if e.kind.is_some() {
            issues.push(format!("{at}: kind is for activities"));
            continue;
        }
        let act = match (&e.intent, &e.plays) {
            (Some(i), _) => match Intent::parse(i) {
                Some(i) => EmoteAct::Intent(i),
                None => {
                    issues.push(format!("{at}: unknown intent {i}"));
                    continue;
                }
            },
            (None, Some(plays)) => {
                if !chain(&at, plays, meta, &mut issues) {
                    continue;
                }
                EmoteAct::Plays(plays.clone())
            }
            (None, None) => continue,
        };
        // an emote is a one-shot cue: what it plays must end by itself, or it never leaves the queue
        let played = match &act {
            EmoteAct::Intent(Intent::Idle) => {
                issues.push(format!("{at}: intent idle is not a gesture"));
                continue;
            }
            EmoteAct::Intent(i) => meta.resolve(*i).to_string(),
            EmoteAct::Plays(plays) => meta.first_available(plays).unwrap_or("rest").to_string(),
        };
        if meta.behaviors.get(&played).is_some_and(|d| d.seq.iter().any(|st| st.loops == mascot_protocol::pack::Loops::Hold)) {
            issues.push(format!("{at}: {played} holds until stopped; an emote must end by itself"));
            continue;
        }
        body.emotes.push((name.clone(), EmoteMeta { act, about: e.about.trim().to_string() }));
    }
    meta.body = body;
    meta.body_issues = issues;
}
