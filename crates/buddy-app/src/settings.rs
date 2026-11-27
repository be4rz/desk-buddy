//! `settings.json` v4 (design §3.2; v2 only changes a default: `dnd.fullscreen` is off, and a v1 file
//! is read with it off once, see `load`; v3 adds `persona`, the person's layer over the mascot's
//! personality, and a v2 file reads with none: the pack's default; v4 adds `activity` (the activity
//! sense, on) and `screen_look` (off, with its deny list), and a file older than v4 reads with both at
//! their defaults: it cannot have meant them; v5 adds `memory`, and a file from before it reads as
//! memory on, admission off). Every key has a default; unknown keys
//! (top level and inside each object) are kept and written back. A file that does not
//! parse is renamed `settings.json.bad` and defaults are used. A hand-edited persona value over its cap
//! or out of 0..1 is cut or clamped on read (with a `[buddy]` line), never a reset. Writes are atomic.

use std::path::{Path, PathBuf};

use mascot_core::persona::{caps, UserPersona};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const VERSION: u32 = 5;

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
    /// v3: the person's layer over the mascot's personality (tray Personality ▸, the Personality window).
    pub persona: Persona,
    /// v4: the activity sense (which app is in front, for how long; D18). On by default.
    pub activity: Activity,
    /// v4: the agent may read the screen (`buddy screen look`). Off by default.
    pub screen_look: ScreenLook,
    /// v5: memory (the "What I remember" window's switch; `BUDDY_MEMORY=0|1` overrides `enabled`).
    pub memory: Memory,
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
            persona: Persona::default(),
            activity: Activity::default(),
            screen_look: ScreenLook::default(),
            memory: Memory::default(),
            extra: Extra::new(),
        }
    }
}

/// `settings.activity`: the tray's "Activity awareness".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Activity {
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Activity {
    fn default() -> Self {
        Activity { enabled: true, extra: Extra::new() }
    }
}

/// The apps the agent never reads the screen in front of (bundle ids; an exact id or a trailing `*`):
/// password managers, messengers, Mail, Keychain. A missing `deny` key means this list; an explicit
/// `[]` is the person's choice and is kept.
pub const DEFAULT_SCREEN_DENY: &[&str] = &[
    "com.1password.*",
    "com.agilebits.*",
    "com.bitwarden.desktop",
    "org.keepassxc.keepassxc",
    "com.lastpass.*",
    "com.dashlane.*",
    "com.apple.Passwords",
    "com.apple.keychainaccess",
    "com.apple.MobileSMS",
    "com.apple.mail",
    "net.whatsapp.WhatsApp",
    "ru.keepcoder.Telegram",
    "org.whispersystems.signal-desktop",
];

/// `settings.screen_look`: read by the host's screen look (`screen::look`: off by default, then the
/// don't-look list); the tray item **Screen look** toggles `enabled` and asks for Screen Recording when
/// it is turned on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ScreenLook {
    pub enabled: bool,
    pub deny: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for ScreenLook {
    fn default() -> Self {
        ScreenLook { enabled: false, deny: DEFAULT_SCREEN_DENY.iter().map(|s| s.to_string()).collect(), extra: Extra::new() }
    }
}

/// `settings.persona`: `mascot_core::persona::UserPersona` (null = follow the preset, or the pack's
/// default preset) plus any unknown keys, kept.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Persona {
    /// The mascot (pack id) this layer was made for; `None` in a file written before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    #[serde(flatten)]
    pub user: UserPersona,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Persona {
    /// The layer is the person's take on one mascot: on another one (put on the desktop since) its
    /// preset and the fields a preset owns are cleared, so the new mascot keeps its own name and
    /// voice; what it calls the person and the notes stay. A layer with no mascot yet takes this
    /// one. `Some(line)` for the log when something was cleared.
    pub fn for_pack(&mut self, id: &str) -> Option<String> {
        let was = self.pack.replace(id.to_string())?;
        if was == id || (self.user.preset.is_none() && !crate::persona::customized(&self.user)) {
            return None;
        }
        self.user = UserPersona { preset: None, ..crate::persona::reset(&self.user) };
        Some(format!("persona: made for mascot \"{was}\"; {id} starts from its own default"))
    }

    /// A hand-edited value over its cap is cut to it, one out of 0..1 clamped (a non-finite one
    /// dropped): one line each for the log. The rest of the file stands.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut notes = vec![];
        let u = &mut self.user;
        for (k, v, cap) in [("name", &mut u.name, caps::NAME), ("call_me", &mut u.call_me, caps::CALL_ME), ("catchphrase", &mut u.catchphrase, caps::CATCHPHRASE), ("notes", &mut u.notes, caps::NOTES)] {
            if let Some(t) = v.as_mut() {
                let n = t.chars().count();
                if n > cap {
                    *t = t.chars().take(cap).collect();
                    notes.push(format!("settings: persona.{k} was {n} characters, cut to {cap}"));
                }
            }
        }
        for (k, v) in [("chattiness", &mut u.chattiness), ("playfulness", &mut u.playfulness), ("emotes", &mut u.emotes)] {
            if let Some(x) = *v {
                if !x.is_finite() {
                    *v = None;
                    notes.push(format!("settings: persona.{k} is not a number, following the preset"));
                } else if !(0.0..=1.0).contains(&x) {
                    *v = Some(x.clamp(0.0, 1.0));
                    notes.push(format!("settings: persona.{k} {x} is out of 0..1, clamped"));
                }
            }
        }
        notes
    }
}

/// `settings.memory` (v5): `enabled` is the window's "Remember new things" switch (on by default);
/// `admission` (p3, off by default) sends recall's top 10 to a quick model call that keeps the ones that matter;
/// `embed` (p3, serde defaults, no version bump) is the local Ollama recall's vectors come from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Memory {
    pub enabled: bool,
    pub admission: bool,
    pub embed: Embed,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Memory {
    fn default() -> Self {
        Memory { enabled: true, admission: false, embed: Embed::default(), extra: Extra::new() }
    }
}

/// `settings.memory.embed` (I7): `url` must be `http://` on `127.0.0.1`, `localhost` or `[::1]`; any other is
/// ignored (`[buddy] memory embed url ignored (not local)`) and recall stays keyword only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Embed {
    pub url: String,
    pub model: String,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Default for Embed {
    fn default() -> Self {
        Embed { url: memory::embed::DEFAULT_URL.into(), model: memory::embed::DEFAULT_MODEL.into(), extra: Extra::new() }
    }
}

impl Memory {
    /// `enabled`, unless `BUDDY_MEMORY=0|1` says otherwise.
    pub fn enabled_now(&self) -> bool {
        env_override(std::env::var("BUDDY_MEMORY").ok().as_deref()).unwrap_or(self.enabled)
    }
}

/// `BUDDY_MEMORY`: "0" off, "1" on, anything else (or unset) no override.
pub fn env_override(v: Option<&str>) -> Option<bool> {
    match v.map(str::trim) {
        Some("0") => Some(false),
        Some("1") => Some(true),
        _ => None,
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
    let why = match read(home) {
        Ok(s) => return (s.unwrap_or_default(), None),
        Err(why) => why,
    };
    let bad = home.join("settings.json.bad");
    let _ = std::fs::rename(&p, &bad);
    (Settings::default(), Some(LoadNote::Reset { bad, why }))
}

/// `load` without its side effect: the settings (`None` when there is no file yet), or why the
/// file is bad; nothing is renamed (the studio reads the desktop's mascot with it).
pub fn read(home: &Path) -> Result<Option<Settings>, String> {
    let text = match std::fs::read_to_string(path(home)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(t) => t,
    };
    let mut s = serde_json::from_str::<Settings>(&text).map_err(|e| e.to_string())?;
    s.validate()?;
    // v1 wrote `dnd.fullscreen: true` as its default, so the file can't tell a choice from
    // the old default: v2 starts everyone at "stay visible" (the tray toggle turns it back on)
    if s.settings_version < 2 {
        s.dnd.fullscreen = false;
    }
    // v2 → v3: no `persona` yet, which reads as the pack's default personality
    // v3 → v4: `activity` and `screen_look` start at their defaults (an older file cannot have meant
    // them, whatever a hand edit put there)
    if s.settings_version < 4 {
        s.activity = Activity::default();
        s.screen_look = ScreenLook::default();
    }
    // v4 → v5: memory on, admission off (whatever a file from before memory says)
    if s.settings_version < 5 {
        s.memory = Memory::default();
    }
    s.settings_version = s.settings_version.max(VERSION);
    for line in s.persona.sanitize() {
        eprintln!("[buddy] {line}");
    }
    Ok(Some(s))
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
            r#"{"settings_version":5,"pack":"bear","zoom":1,"position":null,
            "dnd":{"action":"hide","fullscreen":false,"screen_share":true},
            "quiet_hours":{"enabled":true,"start":"22:30","end":"07:30"},
            "attention":{"per_hour":4},"model":{"provider":"anthropic","model":"claude-sonnet-5-5"},
            "edges":{"enabled":true},"studio":{"packs_dir":null},
            "room":"starter","floor":"grid","scenes":true,"onboarded":false,
            "persona":{"preset":null,"name":null,"call_me":null,"catchphrase":null,
            "chattiness":null,"playfulness":null,"emotes":null,"notes":null},
            "activity":{"enabled":true},
            "screen_look":{"enabled":false,"deny":["com.1password.*","com.agilebits.*","com.bitwarden.desktop",
            "org.keepassxc.keepassxc","com.lastpass.*","com.dashlane.*","com.apple.Passwords",
            "com.apple.keychainaccess","com.apple.MobileSMS","com.apple.mail","net.whatsapp.WhatsApp",
            "ru.keepcoder.Telegram","org.whispersystems.signal-desktop"]},
            "memory":{"enabled":true,"admission":false,"embed":{"url":"http://127.0.0.1:11434","model":"nomic-embed-text"}}}"#,
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
        assert_eq!((s.settings_version, s.dnd.fullscreen, s.dnd.screen_share, s.zoom), (5, false, true, 2));
        assert_eq!(s.persona, Persona::default());
        // the person turns it back on: v2 and later keep it
        s.dnd.fullscreen = true;
        s.save(&h).unwrap();
        let (s, _) = load(&h);
        assert_eq!((s.settings_version, s.dnd.fullscreen), (5, true));
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
    fn a_v2_file_reads_as_the_pack_default_personality_and_saves_as_v4() {
        let h = tmp_home("v2v3");
        std::fs::write(path(&h), r#"{"settings_version":2,"zoom":2,"dnd":{"action":"still","fullscreen":true}}"#).unwrap();
        let (s, note) = load(&h);
        assert_eq!(note, None);
        assert_eq!((s.settings_version, s.zoom, s.dnd.action, s.dnd.fullscreen), (5, 2, DndAct::Still, true));
        assert_eq!(s.persona.user, UserPersona::default());
        s.save(&h).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&h)).unwrap()).unwrap();
        assert_eq!(v["settings_version"], 5);
        assert!(v["persona"].as_object().unwrap().values().all(Value::is_null), "{v}");
    }

    #[test]
    fn a_v3_persona_round_trips_with_its_unknown_keys() {
        let h = tmp_home("v3rt");
        std::fs::write(
            path(&h),
            r#"{"settings_version":3,"persona":{"preset":"mochi","name":"Bun","call_me":"Sam","catchphrase":null,
               "chattiness":0.2,"playfulness":null,"emotes":0.9,"notes":"Keep it short.\nNo puns before 10.","later":{"x":1}}}"#,
        )
        .unwrap();
        let (mut s, note) = load(&h);
        assert_eq!(note, None);
        let u = &s.persona.user;
        assert_eq!((u.preset.as_deref(), u.name.as_deref(), u.call_me.as_deref(), u.catchphrase.as_deref()), (Some("mochi"), Some("Bun"), Some("Sam"), None));
        assert_eq!((u.chattiness, u.playfulness, u.emotes), (Some(0.2), None, Some(0.9)));
        assert_eq!(u.notes.as_deref(), Some("Keep it short.\nNo puns before 10."));
        s.persona.user.playfulness = Some(0.4);
        s.save(&h).unwrap();
        let (back, _) = load(&h);
        assert_eq!(back, s);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&h)).unwrap()).unwrap();
        assert_eq!(v["persona"]["later"]["x"], 1);
        assert_eq!(v["persona"]["playfulness"], 0.4);
    }

    #[test]
    fn a_hand_edited_persona_is_cut_and_clamped_not_reset() {
        let h = tmp_home("v3cap");
        let notes = "n".repeat(900);
        let name = "é".repeat(30);
        std::fs::write(
            path(&h),
            format!(r#"{{"settings_version":3,"zoom":2,"persona":{{"preset":"tofu","name":"{name}","notes":"{notes}","chattiness":1.7,"emotes":-3}}}}"#),
        )
        .unwrap();
        let (s, note) = load(&h);
        assert_eq!(note, None, "not a reset");
        assert!(path(&h).is_file() && !h.join("settings.json.bad").exists());
        let u = &s.persona.user;
        assert_eq!((s.zoom, u.preset.as_deref()), (2, Some("tofu")));
        assert_eq!(u.notes.as_ref().unwrap().chars().count(), 500);
        assert_eq!(u.name.as_ref().unwrap().chars().count(), 24);
        assert_eq!((u.chattiness, u.emotes), (Some(1.0), Some(0.0)));
        let mut p = s.persona.clone();
        assert!(p.sanitize().is_empty(), "clean after one pass");
    }

    #[test]
    fn the_persona_layer_belongs_to_its_mascot() {
        let mine = UserPersona { preset: Some("mochi".into()), name: Some("Bun".into()), catchphrase: Some("Yo".into()), chattiness: Some(0.2), call_me: Some("Sam".into()), notes: Some("No puns.".into()), ..Default::default() };
        // a layer from before it was kept takes the mascot it finds, unchanged
        let mut p = Persona { user: mine.clone(), ..Default::default() };
        assert_eq!(p.for_pack("bear"), None);
        assert_eq!((p.pack.as_deref(), &p.user), (Some("bear"), &mine));
        assert_eq!(p.for_pack("bear"), None, "the same mascot again");
        // another mascot on the desktop: its own preset and name, the person's own fields stay
        let line = p.for_pack("fox").unwrap();
        assert!(line.contains("\"bear\"") && !line.contains("Bun") && !line.contains("Sam"), "ids only: {line}");
        assert_eq!(p.user, UserPersona { call_me: Some("Sam".into()), notes: Some("No puns.".into()), ..Default::default() });
        assert_eq!(p.pack.as_deref(), Some("fox"));
        // nothing of the bear's left to clear: no line
        assert_eq!(p.for_pack("bear"), None);
        // the mascot id round-trips; a layer without one writes no key
        let h = tmp_home("v3pack");
        let mut s = Settings::default();
        s.save(&h).unwrap();
        assert!(!std::fs::read_to_string(path(&h)).unwrap().contains("\"pack\": null"));
        s.persona.pack = Some("bear".into());
        s.save(&h).unwrap();
        assert_eq!(load(&h).0.persona.pack.as_deref(), Some("bear"));
    }

    #[test]
    fn v1_v2_and_v3_files_read_as_v5_with_activity_on_and_screen_look_off() {
        for (i, text) in [
            r#"{"settings_version":1,"zoom":2}"#,
            r#"{"settings_version":2,"zoom":2}"#,
            r#"{"settings_version":3,"zoom":2,"persona":{"preset":"mochi"}}"#,
            // a v3 file cannot have meant these: they read as the defaults
            r#"{"settings_version":3,"zoom":2,"activity":{"enabled":false},"screen_look":{"enabled":true,"deny":[]}}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let h = tmp_home(&format!("tov4-{i}"));
            std::fs::write(path(&h), text).unwrap();
            let (s, note) = load(&h);
            assert_eq!(note, None, "{text}");
            assert_eq!((s.settings_version, s.zoom), (5, 2), "{text}");
            assert_eq!(s.activity, Activity { enabled: true, extra: Extra::new() }, "{text}");
            assert!(!s.screen_look.enabled && s.screen_look.deny.len() == 13 && s.screen_look.deny[0] == "com.1password.*", "{text}");
        }
    }

    #[test]
    fn a_v3_file_reads_as_memory_on_admission_off_and_v5_round_trips() {
        let h = tmp_home("v5");
        // a v3 file, even one with a stray memory key, starts at the v5 defaults
        std::fs::write(path(&h), r#"{"settings_version":3,"zoom":2,"memory":{"enabled":false,"admission":true}}"#).unwrap();
        let (mut s, note) = load(&h);
        assert_eq!(note, None);
        assert_eq!((s.settings_version, s.zoom, s.memory.enabled, s.memory.admission), (5, 2, true, false));
        // the person's choice is kept from v5 on, with unknown keys
        s.memory.enabled = false;
        s.memory.admission = true;
        s.memory.extra.insert("later".into(), Value::from(1));
        s.save(&h).unwrap();
        let (back, _) = load(&h);
        assert_eq!(back, s);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&h)).unwrap()).unwrap();
        assert_eq!((v["settings_version"].as_u64(), v["memory"]["enabled"].as_bool(), v["memory"]["later"].as_u64()), (Some(5), Some(false), Some(1)));
        // a v5 file without the key is on
        std::fs::write(path(&h), r#"{"settings_version":5}"#).unwrap();
        assert_eq!(load(&h).0.memory, Memory::default());
    }

    #[test]
    fn memory_embed_has_defaults_without_a_version_bump_and_keeps_the_persons_values() {
        let h = tmp_home("embed");
        std::fs::write(path(&h), r#"{"settings_version":5,"memory":{"enabled":true,"admission":true}}"#).unwrap();
        let (s, note) = load(&h);
        assert_eq!(note, None);
        assert_eq!((s.settings_version, s.memory.admission), (5, true));
        assert_eq!(s.memory.embed, Embed::default());
        assert_eq!((s.memory.embed.url.as_str(), s.memory.embed.model.as_str()), ("http://127.0.0.1:11434", "nomic-embed-text"));
        std::fs::write(path(&h), r#"{"settings_version":5,"memory":{"embed":{"url":"http://localhost:11500","model":"mxbai-embed-large","x":1}}}"#).unwrap();
        let (s, _) = load(&h);
        assert_eq!((s.memory.embed.url.as_str(), s.memory.embed.model.as_str()), ("http://localhost:11500", "mxbai-embed-large"));
        s.save(&h).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&h)).unwrap()).unwrap();
        assert_eq!((v["settings_version"].as_u64(), v["memory"]["embed"]["x"].as_u64()), (Some(5), Some(1)));
    }

    #[test]
    fn buddy_memory_overrides_the_switch() {
        assert_eq!(env_override(Some("0")), Some(false));
        assert_eq!(env_override(Some("1")), Some(true));
        assert_eq!(env_override(Some("yes")), None);
        assert_eq!(env_override(None), None);
        // enabled_now with no override set in this test process follows the switch
        if std::env::var_os("BUDDY_MEMORY").is_none() {
            assert!(Memory::default().enabled_now());
            assert!(!Memory { enabled: false, ..Default::default() }.enabled_now());
        }
    }

    #[test]
    fn a_v4_file_keeps_its_choices_unknown_keys_and_an_explicit_empty_deny_list() {
        let h = tmp_home("v4");
        std::fs::write(
            path(&h),
            r#"{"settings_version":4,"activity":{"enabled":false,"later":1},
               "screen_look":{"enabled":true,"deny":[],"note":{"x":2}},"future":true}"#,
        )
        .unwrap();
        let (mut s, note) = load(&h);
        assert_eq!(note, None);
        assert!(!s.activity.enabled);
        assert!(s.screen_look.enabled && s.screen_look.deny.is_empty(), "an explicit [] is honored");
        s.zoom = 3;
        s.save(&h).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&h)).unwrap()).unwrap();
        assert_eq!((v["activity"]["later"].clone(), v["screen_look"]["note"]["x"].clone(), v["future"].clone()), (1.into(), 2.into(), true.into()));
        assert_eq!(v["screen_look"]["deny"], serde_json::json!([]));
        // a v4 file with no `deny` key means the default list; with no activity key, on
        std::fs::write(path(&h), r#"{"settings_version":4,"screen_look":{"enabled":true}}"#).unwrap();
        let (s, _) = load(&h);
        assert!(s.screen_look.enabled && s.screen_look.deny == ScreenLook::default().deny && s.activity.enabled);
    }

    #[test]
    fn the_activity_toggle_is_written_and_read_back() {
        let h = tmp_home("toggle");
        let (mut s, _) = load(&h);
        assert!(s.activity.enabled);
        s.activity.enabled = false;
        s.save(&h).unwrap();
        let (s, _) = load(&h);
        assert!(!s.activity.enabled);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path(&h)).unwrap()).unwrap();
        assert_eq!((v["settings_version"].clone(), v["activity"]["enabled"].clone()), (5.into(), false.into()));
    }

    #[test]
    fn a_relative_home_is_made_absolute() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(absolute(Path::new("rel/home")), cwd.join("rel/home"));
        assert_eq!(absolute(Path::new("/abs/home")), PathBuf::from("/abs/home"));
        assert!(home().is_absolute());
    }
}
