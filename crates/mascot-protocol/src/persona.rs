//! Persona schemas: serde structs and the caps only, no IO.
//! `pack.json` `"persona"` holds a mascot's presets (a voice, examples, idle lines, a temperament);
//! `UserPersona` is the person's layer on top (`settings.persona`). `mascot_core::persona` validates,
//! resolves and renders them. The persona changes tone only: nothing here names a tool or a policy.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::pack::Temperament;

/// The caps, in characters (shared by the loader, the studio and the Personality page).
pub mod caps {
    pub const NAME: usize = 24;
    pub const TAGLINE: usize = 32;
    pub const IDENTITY: usize = 120;
    pub const VOICE_RULE: usize = 80;
    pub const LIKE: usize = 32;
    pub const CATCHPHRASE: usize = 40;
    pub const CATCHPHRASE_WHEN: usize = 80;
    pub const EXAMPLE: usize = 160;
    pub const IDLE_LINE: usize = 100;
    /// What the person may write about the personality.
    pub const NOTES: usize = 500;
    /// What the person may call themselves in the bear's mouth.
    pub const CALL_ME: usize = 24;
    /// Voice rules per preset.
    pub const VOICE_MIN: usize = 3;
    pub const VOICE_MAX: usize = 6;
    /// Likes per preset.
    pub const LIKES_MAX: usize = 8;
    /// Idle lines per moment.
    pub const IDLE_MIN: usize = 1;
    pub const IDLE_MAX: usize = 8;
    /// A preset id: `^[a-z][a-z0-9_]{0,23}$`.
    pub const ID_MAX: usize = 24;
}

/// The five situations an example (and a signature gesture) is written for.
pub const SITUATIONS: [&str; 5] = ["greeting", "done", "error", "checkin", "disagree"];
/// The moments an idle line is written for.
pub const MOMENTS: [&str; 4] = ["morning", "evening", "back", "long_session"];

/// Whether `id` is a legal preset id.
pub fn id_ok(id: &str) -> bool {
    let mut c = id.chars();
    matches!(c.next(), Some(f) if f.is_ascii_lowercase()) && id.len() <= caps::ID_MAX && c.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `pack.json` `"persona"`: optional, in pack_version 1 and 2 alike.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PersonaFile {
    /// The preset a person who picked none gets.
    pub default: String,
    /// In pack order (the tray and the page keep it).
    #[serde(with = "ordered")]
    pub presets: Vec<(String, PresetFile)>,
}

/// One preset: a character the person can pick.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PresetFile {
    pub name: String,
    /// "cheeky sidekick": the tray and the page show it beside the name.
    #[serde(default)]
    pub tagline: String,
    /// One line: who it is. May use `{me}`, `{you}` and `{catchphrase}`.
    pub identity: String,
    /// 3 to 6 short rules for how it sounds.
    pub voice: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catchphrase: Option<Catchphrase>,
    /// Situation (`SITUATIONS`) → an emote of the pack's `body`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub gestures: BTreeMap<String, String>,
    /// Situation (all of `SITUATIONS`) → a line at speech-bubble length.
    pub examples: BTreeMap<String, String>,
    /// Moment (`MOMENTS`) → 1 to 8 canned lines (mood only, never facts).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub idle_lines: BTreeMap<String, Vec<String>>,
    /// Only what differs from the pack's `temperament`.
    #[serde(default)]
    pub temperament: PartialTemperament,
    #[serde(default)]
    pub style: Style,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catchphrase {
    pub text: String,
    /// When to say it.
    #[serde(default)]
    pub when: String,
}

/// A preset's temperament: unset traits use the pack's.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct PartialTemperament {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curiosity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shyness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chattiness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleepiness: Option<f64>,
}

impl PartialTemperament {
    /// `base` with this preset's traits over it.
    pub fn over(&self, base: Temperament) -> Temperament {
        Temperament {
            energy: self.energy.unwrap_or(base.energy),
            curiosity: self.curiosity.unwrap_or(base.curiosity),
            shyness: self.shyness.unwrap_or(base.shyness),
            chattiness: self.chattiness.unwrap_or(base.chattiness),
            sleepiness: self.sleepiness.unwrap_or(base.sleepiness),
        }
    }

    /// Every trait, by name, with what is set.
    pub fn each(&self) -> [(&'static str, Option<f64>); 5] {
        [("energy", self.energy), ("curiosity", self.curiosity), ("shyness", self.shyness), ("chattiness", self.chattiness), ("sleepiness", self.sleepiness)]
    }
}

/// The two sliders a preset starts at (the person's window can move them).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Style {
    /// Straight (0) to playful (1): how many jokes and bits.
    #[serde(default = "half")]
    pub playfulness: f64,
    /// Rarely (0) to often (1): how many gestures.
    #[serde(default = "half")]
    pub emotes: f64,
}

fn half() -> f64 {
    0.5
}

impl Default for Style {
    fn default() -> Self {
        Style { playfulness: 0.5, emotes: 0.5 }
    }
}

/// The person's layer (`settings.persona`). `None` = follow the preset (or the pack default for
/// `preset`). The person owns `call_me` and `notes`; the preset owns the rest.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct UserPersona {
    pub preset: Option<String>,
    pub name: Option<String>,
    pub call_me: Option<String>,
    pub catchphrase: Option<String>,
    pub chattiness: Option<f64>,
    pub playfulness: Option<f64>,
    pub emotes: Option<f64>,
    /// Free text about the personality, at most `caps::NOTES` characters. Tone only.
    pub notes: Option<String>,
}

/// A JSON object as `(key, value)` pairs in document order.
mod ordered {
    use serde::de::{Deserializer, MapAccess, Visitor};
    use serde::ser::{SerializeMap, Serializer};

    use super::PresetFile;

    pub fn serialize<S: Serializer>(v: &[(String, PresetFile)], s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(v.len()))?;
        for (k, e) in v {
            m.serialize_entry(k, e)?;
        }
        m.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, PresetFile)>, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vec<(String, PresetFile)>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object of presets")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = a.next_entry::<String, PresetFile>()? {
                    out.push((k, v));
                }
                Ok(out)
            }
        }
        d.deserialize_map(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_ids() {
        for ok in ["biscuit", "a", "mochi_2", "a23456789012345678901234"] {
            assert!(id_ok(ok), "{ok}");
        }
        for bad in ["", "Biscuit", "2fast", "_x", "a-b", "a b", "a234567890123456789012345", "é"] {
            assert!(!id_ok(bad), "{bad}");
        }
    }

    #[test]
    fn presets_keep_document_order_and_unknown_fields_fail() {
        let p: PersonaFile = serde_json::from_str(
            r#"{"default":"z","presets":{"z":{"name":"Z","identity":"i","voice":["a","b","c"],"examples":{}},"a":{"name":"A","identity":"i","voice":["a","b","c"],"examples":{}}}}"#,
        )
        .unwrap();
        assert_eq!(p.presets.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["z", "a"]);
        assert_eq!(p.presets[0].1.style, Style::default());
        let bad = serde_json::from_str::<PersonaFile>(r#"{"default":"z","presets":{},"colour":1}"#);
        assert!(bad.unwrap_err().to_string().contains("unknown field `colour`"));
    }

    #[test]
    fn partial_temperament_goes_over_the_base() {
        let t = PartialTemperament { energy: Some(0.9), ..Default::default() }.over(Temperament { energy: 0.1, curiosity: 0.2, shyness: 0.3, chattiness: 0.4, sleepiness: 0.5 });
        assert_eq!((t.energy, t.curiosity, t.chattiness), (0.9, 0.2, 0.4));
    }

    #[test]
    fn a_user_layer_reads_with_missing_and_null_fields() {
        let u: UserPersona = serde_json::from_str(r#"{"preset":"mochi","call_me":null,"future":1}"#).unwrap();
        assert_eq!(u.preset.as_deref(), Some("mochi"));
        assert_eq!(UserPersona::default(), serde_json::from_str("{}").unwrap());
    }
}
