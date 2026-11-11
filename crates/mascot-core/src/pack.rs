//! Pack loader (design §3.3, §4.2): `pack.json` + the art JSON it points at →
//! `PackMeta`. Metadata only: strip files are checked to exist, never decoded.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use mascot_protocol::pack::{
    ActivityKind, BehaviorDef, BodyEntry, BodyFile, Capabilities, FaceFile, Memory, PackFile, PoolEntry, PropsFile, SourceKind, StageAFile, StageBFile, Temperament,
    Timers, PACK_VERSION,
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
}

impl FaceMeta {
    /// Whether the face layers cover this clip in this direction.
    pub fn covers(&self, clip: &str, dir: Dir) -> bool {
        self.clips.get(clip).is_some_and(|d| d.contains_key(&dir))
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

    /// The first available behavior of a fallback chain.
    pub fn first_available<'a>(&self, chain: &'a [String]) -> Option<&'a str> {
        chain.iter().find(|b| self.available(b)).map(String::as_str)
    }

    /// Every clip it names exists (free clips in at least `S`, set clips in
    /// both orientations, in its set) and its set exists.
    pub fn available(&self, behavior: &str) -> bool {
        let Some(b) = self.behaviors.get(behavior) else { return false };
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
        for src in &file.sources {
            let src_rel = art_rel.join(&src.path);
            if src.optional && !self.abs(&src_rel).exists() {
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
            face = Some(self.face(&rel, &base, f, &clips)?);
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
        };
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

    fn face(&self, rel: &Path, base: &Path, f: FaceFile, clips: &BTreeMap<String, ClipMeta>) -> Result<FaceMeta, PackError> {
        if f.face_version != 1 {
            return Err(self.invalid(rel, format!("face_version {} is not supported (this build reads 1)", f.face_version)));
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
        let mut fclips = BTreeMap::new();
        for (clip, by_dir) in f.clips {
            let Some(c) = clips.get(&clip) else { return Err(self.invalid(rel, format!("clips: unknown clip {clip}"))) };
            let mut m = BTreeMap::new();
            for (d, slots) in by_dir {
                let dir = dir_of(&d)?;
                for (slot, states) in NEEDED {
                    if !slots.contains_key(slot) {
                        return Err(self.invalid(rel, format!("clips: {clip} {d}: no {slot} positions")));
                    }
                    for st in states {
                        if !layers.get(slot).and_then(|m: &BTreeMap<String, BTreeMap<Dir, FaceLayer>>| m.get(*st)).is_some_and(|m| m.contains_key(&dir)) {
                            return Err(self.invalid(rel, format!("clips: {clip} {d}: no {slot} {st} layer for {d}")));
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
        Ok(FaceMeta { dirs, layers, clips: fclips })
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
            // stations arrive with the room (p3); none exist yet
            issues.push(format!("{at}: unknown station {st}"));
            continue;
        }
        let plays = e.plays.clone().unwrap_or_default();
        if !chain(&at, &plays, meta, &mut issues) {
            continue;
        }
        body.activities.push((name.clone(), ActivityMeta { plays, kind: e.kind.unwrap_or_default(), about: e.about.trim().to_string() }));
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
