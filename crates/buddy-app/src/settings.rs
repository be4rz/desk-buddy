//! `settings.json` v1 (design §3.2, frozen). Every key has a default; unknown keys
//! (top level and inside each object) are kept and written back. A file that does not
//! parse is renamed `settings.json.bad` and defaults are used. Writes are atomic.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const VERSION: u32 = 1;

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
        Dnd { action: DndAct::Hide, fullscreen: true, screen_share: true, extra: Extra::new() }
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

/// `BUDDY_HOME`, default `~/.desk-buddy`.
pub fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("BUDDY_HOME") {
        return PathBuf::from(h);
    }
    let base = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join(".desk-buddy")
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
            Ok(s) => match s.validate() {
                Ok(()) => return (s, None),
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
            r#"{"settings_version":1,"pack":"bear","zoom":1,"position":null,
            "dnd":{"action":"hide","fullscreen":true,"screen_share":true},
            "quiet_hours":{"enabled":true,"start":"22:30","end":"07:30"},
            "attention":{"per_hour":4},"model":{"provider":"anthropic","model":"claude-sonnet-5-5"},
            "edges":{"enabled":true},"studio":{"packs_dir":null}}"#,
        )
        .unwrap();
        assert_eq!(v, want);
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
}
