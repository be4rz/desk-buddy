//! Room packs (D13: packs split into mascot packs and room packs): floors and pocket scenes that any
//! mascot can use. A room pack is a folder with `room.json` and its PNGs (the bear's first one is
//! `rooms/starter`, exported by `experiments/env-proto/export.py`). No pixels here: this checks the
//! text and that the files exist; `mascot-render` decodes them.
//!
//! A **floor** is one patch drawn under the mascot's feet. A **scene** is decoration drawn around the
//! mascot while it plays one of the behaviors (or sets) in `for`; its `origin` pixel sits on the
//! mascot's floor anchor. A scene has one or more variants; a variant with `hours` shows in those
//! local hours (`[from, to)`, wrapping midnight), and timed variants must cover the day exactly once.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const ROOM_VERSION: u32 = 1;

#[derive(Debug)]
pub enum RoomError {
    /// `room.json` (or a file it names) is missing or unreadable.
    Read { path: PathBuf, msg: String },
    /// `room.json` is not valid JSON of the room shape.
    Parse { path: PathBuf, msg: String },
    /// The text is well formed but says something a room cannot be.
    Invalid { path: PathBuf, msg: String },
}

impl fmt::Display for RoomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RoomError::Read { path, msg } => write!(f, "room {}: {msg}", path.display()),
            RoomError::Parse { path, msg } => write!(f, "room {}: not a room.json: {msg}", path.display()),
            RoomError::Invalid { path, msg } => write!(f, "room {}: {msg}", path.display()),
        }
    }
}
impl std::error::Error for RoomError {}

#[derive(Deserialize)]
struct RoomFile {
    room_version: u32,
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    floors: Vec<FloorFile>,
    #[serde(default)]
    scenes: Vec<SceneFile>,
}

#[derive(Deserialize)]
struct FloorFile {
    id: String,
    #[serde(default)]
    name: String,
    file: String,
    origin: [i32; 2],
}

#[derive(Deserialize)]
struct SceneFile {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "for")]
    for_: Vec<String>,
    origin: [i32; 2],
    variants: Vec<VariantFile>,
}

#[derive(Deserialize)]
struct VariantFile {
    name: String,
    #[serde(default)]
    hours: Option<[u8; 2]>,
    back: String,
    #[serde(default)]
    front: Option<String>,
}

/// A loaded room pack: every path is absolute and exists.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomMeta {
    pub id: String,
    pub name: String,
    pub dir: PathBuf,
    pub floors: Vec<FloorMeta>,
    pub scenes: Vec<SceneMeta>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FloorMeta {
    pub id: String,
    pub name: String,
    pub file: PathBuf,
    /// the image pixel that sits on the mascot's floor anchor
    pub origin: [i32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneMeta {
    pub id: String,
    pub name: String,
    /// behavior or set names of the mascot pack
    pub for_: Vec<String>,
    pub origin: [i32; 2],
    pub variants: Vec<VariantMeta>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariantMeta {
    pub name: String,
    pub hours: Option<[u8; 2]>,
    /// drawn behind the mascot
    pub back: PathBuf,
    /// drawn over the mascot
    pub front: Option<PathBuf>,
}

/// Is local hour `h` (0..24) inside `[from, to)`, wrapping midnight?
pub fn in_hours(hours: [u8; 2], h: u8) -> bool {
    let [a, b] = [hours[0] % 24, hours[1] % 24];
    if a < b {
        (a..b).contains(&h)
    } else {
        h >= a || h < b
    }
}

impl SceneMeta {
    /// The variant to show at local hour `h`: the timed one that holds it, else the first.
    pub fn variant_at(&self, h: u8) -> usize {
        self.variants.iter().position(|v| v.hours.is_some_and(|hr| in_hours(hr, h % 24))).unwrap_or(0)
    }
}

impl RoomMeta {
    pub fn floor(&self, id: &str) -> Option<usize> {
        self.floors.iter().position(|f| f.id == id)
    }

    /// The scene for a behavior, or for the set it plays in.
    pub fn scene_for(&self, behavior: &str, set: Option<&str>) -> Option<usize> {
        self.scenes.iter().position(|s| s.for_.iter().any(|n| n == behavior || Some(n.as_str()) == set))
    }
}

use crate::pack::body_name_ok as name_ok;

/// Floor ids the app keeps for its own choices (the drawn grid, and no floor).
pub const RESERVED_FLOORS: [&str; 2] = ["grid", "none"];

/// Load `dir/room.json`.
pub fn load(dir: &Path) -> Result<RoomMeta, RoomError> {
    let path = dir.join("room.json");
    let text = std::fs::read_to_string(&path).map_err(|e| RoomError::Read { path: path.clone(), msg: e.to_string() })?;
    let file: RoomFile = serde_json::from_str(&text).map_err(|e| RoomError::Parse { path: path.clone(), msg: e.to_string() })?;
    let bad = |msg: String| RoomError::Invalid { path: path.clone(), msg };
    if file.room_version != ROOM_VERSION {
        return Err(bad(format!("room_version {} is not {ROOM_VERSION}", file.room_version)));
    }
    if !name_ok(&file.id) {
        return Err(bad(format!("id {:?} is not a-z 0-9 _ - (1 to 32)", file.id)));
    }
    if file.floors.is_empty() && file.scenes.is_empty() {
        return Err(bad("no floors and no scenes".into()));
    }
    // a file named in room.json: inside the pack, and there
    let need = |what: &str, rel: &str| -> Result<PathBuf, RoomError> {
        let p = Path::new(rel);
        if rel.is_empty() || p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(bad(format!("{what}: file {rel:?} must be a path inside the room pack")));
        }
        let full = dir.join(p);
        if !full.is_file() {
            return Err(RoomError::Read { path: full, msg: format!("{what}: file is missing") });
        }
        Ok(full)
    };
    let mut ids = BTreeSet::new();
    let mut unique = |kind: &str, id: &str| -> Result<(), RoomError> {
        if !name_ok(id) {
            return Err(bad(format!("{kind} id {id:?} is not a-z 0-9 _ - (1 to 32)")));
        }
        if !ids.insert(format!("{kind} {id}")) {
            return Err(bad(format!("{kind} {id:?} is listed twice")));
        }
        Ok(())
    };
    let mut floors = vec![];
    for f in file.floors {
        unique("floor", &f.id)?;
        if RESERVED_FLOORS.contains(&f.id.as_str()) {
            return Err(bad(format!("floor id {:?} is reserved (the app's own grid and none)", f.id)));
        }
        let file = need(&format!("floor {}", f.id), &f.file)?;
        floors.push(FloorMeta { name: if f.name.is_empty() { f.id.clone() } else { f.name }, id: f.id, file, origin: f.origin });
    }
    let mut scenes = vec![];
    let mut claimed = BTreeSet::new();
    for s in file.scenes {
        unique("scene", &s.id)?;
        if s.for_.is_empty() {
            return Err(bad(format!("scene {}: \"for\" names no behavior", s.id)));
        }
        for n in &s.for_ {
            // any name a mascot pack may give a behavior or set, but not none
            if n.trim().is_empty() {
                return Err(bad(format!("scene {}: \"for\" has an empty name", s.id)));
            }
            if !claimed.insert(n.clone()) {
                return Err(bad(format!("scene {}: {n:?} already has a scene", s.id)));
            }
        }
        if s.variants.is_empty() {
            return Err(bad(format!("scene {}: no variants", s.id)));
        }
        let mut variants = vec![];
        let mut cover = [0u8; 24];
        let mut timed = 0;
        for v in s.variants {
            let what = format!("scene {}/{}", s.id, v.name);
            if let Some([a, b]) = v.hours {
                if a > 24 || b > 24 || a % 24 == b % 24 {
                    return Err(bad(format!("{what}: hours [{a}, {b}] are not two different hours in 0..24")));
                }
                timed += 1;
                (0..24u8).filter(|h| in_hours([a, b], *h)).for_each(|h| cover[h as usize] += 1);
            }
            let back = need(&what, &v.back)?;
            let front = v.front.as_deref().map(|f| need(&format!("{what} front"), f)).transpose()?;
            variants.push(VariantMeta { name: v.name, hours: v.hours, back, front });
        }
        if timed > 0 && (timed != variants.len() || cover.iter().any(|c| *c != 1)) {
            return Err(bad(format!("scene {}: timed variants must cover the 24 hours exactly once", s.id)));
        }
        scenes.push(SceneMeta { name: if s.name.is_empty() { s.id.clone() } else { s.name }, id: s.id, for_: s.for_, origin: s.origin, variants });
    }
    Ok(RoomMeta { name: if file.name.is_empty() { file.id.clone() } else { file.name }, id: file.id, dir: dir.to_path_buf(), floors, scenes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starter() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rooms/starter")
    }

    /// A room pack in a temp dir: `json` plus empty files for `files`.
    fn pack(name: &str, json: &str, files: &[&str]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("room-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("room.json"), json).unwrap();
        for f in files {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        d
    }

    fn err(name: &str, json: &str, files: &[&str]) -> String {
        load(&pack(name, json, files)).expect_err("must be refused").to_string()
    }

    #[test]
    fn the_starter_room_loads_with_five_floors_and_five_scenes() {
        let r = load(&starter()).expect("rooms/starter loads");
        assert_eq!(r.floors.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["tatami", "parquet", "tile", "moss", "snow"]);
        assert_eq!(r.scenes.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["nook", "kitchen", "tearoom", "grove", "library"]);
        assert_eq!(r.scene_for("laptop", Some("laptop")), r.scene_for("notebook", None));
        assert_eq!(r.scenes[r.scene_for("x", Some("read")).unwrap()].id, "library");
        assert_eq!(r.scene_for("idle", None), None);
        assert!(r.scenes[2].variants[0].front.is_some(), "the tea table is drawn over the bear");
    }

    #[test]
    fn the_nook_follows_the_hour_and_night_wraps_midnight() {
        let r = load(&starter()).unwrap();
        let nook = &r.scenes[0];
        let at = |h| nook.variants[nook.variant_at(h)].name.as_str();
        assert_eq!([at(5), at(7), at(8), at(16), at(17), at(19), at(20), at(23), at(0), at(4)], ["dawn", "dawn", "day", "day", "dusk", "dusk", "night", "night", "night", "night"]);
        let grove = &r.scenes[3];
        assert_eq!([grove.variant_at(6), grove.variant_at(18), grove.variant_at(19), grove.variant_at(5)], [0, 0, 1, 1]);
        // untimed: always the first
        assert_eq!(r.scenes[1].variant_at(3), 0);
    }

    #[test]
    fn a_missing_or_malformed_room_is_refused_with_the_reason() {
        let none = std::env::temp_dir().join("room-test-does-not-exist");
        assert!(load(&none).unwrap_err().to_string().contains("room.json"));
        assert!(err("json", "{ not json", &[]).contains("not a room.json"));
        assert!(err("version", r#"{"room_version":9,"id":"a","floors":[]}"#, &[]).contains("room_version 9"));
        assert!(err("empty", r#"{"room_version":1,"id":"a"}"#, &[]).contains("no floors and no scenes"));
        let floor = |file: &str| format!(r#"{{"room_version":1,"id":"a","floors":[{{"id":"f","file":"{file}","origin":[0,0]}}]}}"#);
        assert!(err("missing", &floor("f.png"), &[]).contains("file is missing"));
        assert!(err("escape", &floor("../f.png"), &[]).contains("inside the room pack"));
        assert!(load(&pack("ok", &floor("f.png"), &["f.png"])).is_ok());
        let twice = r#"{"room_version":1,"id":"a","floors":[{"id":"f","file":"f.png","origin":[0,0]},{"id":"f","file":"f.png","origin":[0,0]}]}"#;
        assert!(err("twice", twice, &["f.png"]).contains("listed twice"));
        for id in RESERVED_FLOORS {
            let json = floor("f.png").replace(r#""id":"f""#, &format!(r#""id":"{id}""#));
            assert!(err(id, &json, &["f.png"]).contains("is reserved"), "{id}");
        }
    }

    #[test]
    fn scene_rules_are_checked() {
        let scene = |for_: &str, variants: &str| format!(r#"{{"room_version":1,"id":"a","scenes":[{{"id":"s","for":{for_},"origin":[0,0],"variants":{variants}}}]}}"#);
        assert!(err("nofor", &scene("[]", r#"[{"name":"v","back":"b.png"}]"#), &["b.png"]).contains("names no behavior"));
        assert!(err("novar", &scene(r#"["tea"]"#, "[]"), &[]).contains("no variants"));
        assert!(err("forname", &scene(r#"[""]"#, r#"[{"name":"v","back":"b.png"}]"#), &["b.png"]).contains("empty name"));
        let gap = r#"[{"name":"d","hours":[6,18],"back":"b.png"},{"name":"n","hours":[19,6],"back":"b.png"}]"#;
        assert!(err("gap", &scene(r#"["tea"]"#, gap), &["b.png"]).contains("exactly once"));
        let mixed = r#"[{"name":"d","hours":[6,6],"back":"b.png"}]"#;
        assert!(err("same", &scene(r#"["tea"]"#, mixed), &["b.png"]).contains("two different hours"));
        let front = r#"[{"name":"v","back":"b.png","front":"f.png"}]"#;
        assert!(err("front", &scene(r#"["tea"]"#, front), &["b.png"]).contains("front: file is missing"));
        let two = r#"{"room_version":1,"id":"a","scenes":[{"id":"s","for":["tea"],"origin":[0,0],"variants":[{"name":"v","back":"b.png"}]},{"id":"t","for":["tea"],"origin":[0,0],"variants":[{"name":"v","back":"b.png"}]}]}"#;
        assert!(err("claimed", two, &["b.png"]).contains("already has a scene"));
    }
}
