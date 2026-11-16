//! `settings.json` v2 (design §3.2; v2 only changes a default: `dnd.fullscreen` is off, and a v1 file
//! is read with it off once, see `load`). Every key has a default; unknown keys
//! (top level and inside each object) are kept and written back. A file that does not
//! parse is renamed `settings.json.bad` and defaults are used. Writes are atomic.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const VERSION: u32 = 2;

type Extra = Map<String, Value>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub settings_version: u32,
    pub pack: String,
    pub zoom: u32,
    pub position: Option<Position>,
    pub dnd: Dnd,
    pub quiet_hours: QuietHours,
    pub attention: Attention,
    pub model: Model,
    pub edges: Edges,
    pub studio: Studio,
    /// The room pack (floors and pocket scenes): a folder under `rooms/`.
    pub room: String,
    /// The floor under the mascot: `grid` (the drawn iso grid), `none`, or a floor id of the room pack.
    pub floor: String,
    /// Show the room's pocket scenes around the mascot during the activities they belong to.
    pub scenes: bool,
    /// The first-launch hello (or the setup bubble) was shown: it never repeats.
    pub onboarded: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            settings_version: VERSION,
            pack: "bear".into(),
            zoom: 1,
            position: None,
            dnd: Dnd::default(),
            quiet_hours: QuietHours::default(),
            attention: Attention::default(),
            model: Model::default(),
            edges: Edges::default(),
            studio: Studio::default(),
            room: "starter".into(),
            floor: "grid".into(),
            scenes: true,
            onboarded: false,
            extra: Extra::new(),
        }
    }
}

/// Points, on the display with this CGDisplay UUID.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Position {
    pub display: String,
    pub x: i32,
    pub y: i32,
    /// The mascot's feet in sprite px from that origin when this was saved. The canvas grows with
    /// the room art, so the feet (not the window corner) are what a restore keeps in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feet: Option<[i32; 2]>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DndAct {
    #[default]
    Hide,
    Still,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Dnd {
    pub action: DndAct,
    pub fullscreen: bool,
    pub screen_share: bool,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Dnd {
    fn default() -> Self {
        // the bear stays over a fullscreen app unless the person turns "Hide over fullscreen apps" on
        Dnd { action: DndAct::Hide, fullscreen: false, screen_share: true, extra: Extra::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct QuietHours {
    pub enabled: bool,
    pub start: String,
    pub end: String,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for QuietHours {
    fn default() -> Self {
        QuietHours { enabled: true, start: "22:30".into(), end: "07:30".into(), extra: Extra::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Attention {
    pub per_hour: u32,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Attention {
    fn default() -> Self {
        Attention { per_hour: 4, extra: Extra::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Model {
    pub provider: String,
    pub model: String,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Model {
    fn default() -> Self {
        Model { provider: "anthropic".into(), model: "claude-sonnet-5-5".into(), extra: Extra::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Edges {
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Edges {
    fn default() -> Self {
        Edges { enabled: true, extra: Extra::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Studio {
    pub packs_dir: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `BUDDY_HOME`, default `~/.desk-buddy`; always absolute (the sidecar and the agent's
/// commands run in other folders, so a relative home would point somewhere else for them).
pub fn home() -> PathBuf {
    let h = match std::env::var_os("BUDDY_HOME").filter(|h| !h.is_empty()) {
        Some(h) => PathBuf::from(h),
        None => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".desk-buddy"),
    };
    absolute(&h)
}

/// `p` against the current folder when relative.
pub fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

pub fn path(home: &Path) -> PathBuf {
    home.join("settings.json")
}

/// What `load` had to do besides reading.
#[derive(Debug, PartialEq)]
pub enum LoadNote {
    /// The file did not parse (or had invalid values); it was renamed here.
    Reset { bad: PathBuf, why: String },
}

/// Read `settings.json` under `home`. Missing → defaults. Unreadable, unparsable or out of
/// range → renamed to `settings.json.bad` (so the next save can't overwrite the user's
/// file), defaults, and a note for the tray.
pub fn load(home: &Path) -> (Settings, Option<LoadNote>) {
    let p = path(home);
    let why = match std::fs::read_to_string(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => e.to_string(),
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(mut s) => match s.validate() {
                Ok(()) => {
                    // v1 wrote `dnd.fullscreen: true` as its default, so the file can't tell a choice from
                    // the old default: v2 starts everyone at "stay visible" (the tray toggle turns it back on)
                    if s.settings_version < 2 {
                        s.dnd.fullscreen = false;
                        s.settings_version = VERSION;
                    }
                    return (s, None);
                }
                Err(why) => why,
            },
            Err(e) => e.to_string(),
        },
    };
    let bad = home.join("settings.json.bad");
    let _ = std::fs::rename(&p, &bad);
    (Settings::default(), Some(LoadNote::Reset { bad, why }))
}

impl Settings {
    fn validate(&self) -> Result<(), String> {
        if !(1..=3).contains(&self.zoom) {
            return Err(format!("zoom {} is not 1, 2 or 3", self.zoom));
        }
        Ok(())
    }

    /// Atomic write: `settings.json.tmp` then rename.
    pub fn save(&self, home: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(home)?;
        let tmp = home.join("settings.json.tmp");
        let mut text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        text.push('\n');
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path(home))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("buddy-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_file_gives_the_frozen_defaults() {
        let h = tmp_home("missing");
        let (s, note) = load(&h);
        assert_eq!(note, None);
        let v = serde_json::to_value(&s).unwrap();
        let want: Value = serde_json::from_str(
            r#"{"settings_version":2,"pack":"bear","zoom":1,"position":null,
            "dnd":{"action":"hide","fullscreen":false,"screen_share":true},
            "quiet_hours":{"enabled":true,"start":"22:30","end":"07:30"},
            "attention":{"per_hour":4},"model":{"provider":"anthropic","model":"claude-sonnet-5-5"},
            "edges":{"enabled":true},"studio":{"packs_dir":null},
            "room":"starter","floor":"grid","scenes":true,"onboarded":false}"#,
        )
        .unwrap();
        assert_eq!(v, want);
    }

    #[test]
    fn a_v1_file_stops_hiding_over_fullscreen_once_and_a_v2_choice_is_kept() {
        let h = tmp_home("v1");
        std::fs::write(path(&h), r#"{"settings_version":1,"zoom":2,"dnd":{"action":"hide","fullscreen":true,"screen_share":true}}"#).unwrap();
        let (mut s, note) = load(&h);
        assert_eq!(note, None);
        assert_eq!((s.settings_version, s.dnd.fullscreen, s.dnd.screen_share, s.zoom), (2, false, true, 2));
        // the person turns it back on: v2 keeps it
        s.dnd.fullscreen = true;
        s.save(&h).unwrap();
        let (s, _) = load(&h);
        assert_eq!((s.settings_version, s.dnd.fullscreen), (2, true));
    }

    #[test]
    fn a_file_from_before_rooms_gets_the_grid_and_scenes_and_a_choice_is_kept() {
        let h = tmp_home("rooms");
        std::fs::write(path(&h), r#"{"settings_version":2,"zoom":2}"#).unwrap();
        let (mut s, note) = load(&h);
        assert_eq!(note, None);
        assert_eq!((s.room.as_str(), s.floor.as_str(), s.scenes), ("starter", "grid", true));
        s.floor = "moss".into();
        s.scenes = false;
        s.save(&h).unwrap();
        let (s, _) = load(&h);
        assert_eq!((s.floor.as_str(), s.scenes, s.zoom), ("moss", false, 2));
    }

    #[test]
    fn round_trip_keeps_unknown_keys() {
        let h = tmp_home("unknown");
        std::fs::write(
            path(&h),
            r#"{"zoom":2,"future_key":{"a":[1,2]},"dnd":{"action":"still","later":true},
               "position":{"display":"ABC","x":10,"y":-5,"note":"x"}}"#,
        )
        .unwrap();
        let (mut s, note) = load(&h);
        assert_eq!(note, None);
        assert_eq!(s.zoom, 2);
        assert_eq!(s.dnd.action, DndAct::Still);
        assert_eq!(s.position.as_ref().unwrap().x, 10);
        s.zoom = 3;
        s.save(&h).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&h)).unwrap()).unwrap();
        assert_eq!(v["zoom"], 3);
        assert_eq!(v["future_key"]["a"][1], 2);
        assert_eq!(v["dnd"]["later"], true);
        assert_eq!(v["position"]["note"], "x");
        assert_eq!(v["quiet_hours"]["start"], "22:30");
        assert!(!h.join("settings.json.tmp").exists());
    }

    #[test]
    fn corrupt_file_resets_to_defaults_and_keeps_a_bad_copy() {
        let h = tmp_home("corrupt");
        std::fs::write(path(&h), "{ zoom: 2,").unwrap();
        let (s, note) = load(&h);
        assert_eq!(s, Settings::default());
        let Some(LoadNote::Reset { bad, .. }) = note else { panic!("expected a reset note") };
        assert_eq!(std::fs::read_to_string(bad).unwrap(), "{ zoom: 2,");
        assert!(!path(&h).exists());
    }

    #[test]
    fn out_of_range_zoom_is_a_reset_too() {
        let h = tmp_home("zoom");
        std::fs::write(path(&h), r#"{"zoom":7}"#).unwrap();
        let (s, note) = load(&h);
        assert_eq!(s.zoom, 1);
        assert!(matches!(note, Some(LoadNote::Reset { .. })));
    }

    #[test]
    fn an_unreadable_file_is_a_reset_too_not_silently_overwritten() {
        let d = std::env::temp_dir().join(format!("buddy-settings-utf8-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(path(&d), [0xff, 0xfe, b'{']).unwrap();
        let (s, note) = load(&d);
        assert_eq!(s, Settings::default());
        assert!(matches!(note, Some(LoadNote::Reset { .. })), "{note:?}");
        assert!(d.join("settings.json.bad").is_file() && !path(&d).exists());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_relative_home_is_made_absolute() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(absolute(Path::new("rel/home")), cwd.join("rel/home"));
        assert_eq!(absolute(Path::new("/abs/home")), PathBuf::from("/abs/home"));
        assert!(home().is_absolute());
    }
}
