//! The predicate registry. Keys, cardinality and kind live here, never in the model's output.

use serde_json::{json, Value};

pub struct Predicate {
    pub key: &'static str,
    pub kind: &'static str,
    /// Single-valued: one live value per slot.
    pub one: bool,
    /// What the qualifier means ("relation", "species"), if the predicate takes one.
    pub qualifier: Option<&'static str>,
    /// The window's name for the value field.
    pub label: &'static str,
}

const fn p(key: &'static str, kind: &'static str, one: bool, qualifier: Option<&'static str>, label: &'static str) -> Predicate {
    Predicate { key, kind, one, qualifier, label }
}

pub const REGISTRY: [Predicate; 12] = [
    p("home_city", "profile", true, None, "City"),
    p("timezone", "profile", true, None, "Time zone"),
    p("job", "profile", true, None, "Job"),
    p("employer", "profile", true, None, "Employer"),
    p("language", "profile", false, None, "Language"),
    p("reply_style", "preference", true, None, "Reply style"),
    p("checkin_hours", "preference", true, None, "Check-in hours"),
    p("likes", "preference", false, None, "Likes"),
    p("dislikes", "preference", false, None, "Dislikes"),
    p("people", "profile", false, Some("relation"), "Name"),
    p("pets", "profile", false, Some("species"), "Name"),
    p("projects", "profile", false, None, "Project"),
];

pub fn get(key: &str) -> Option<&'static Predicate> {
    REGISTRY.iter().find(|p| p.key == key)
}

pub fn label(key: &str) -> Option<&'static str> {
    get(key).map(|p| p.label)
}

/// `pred` or `pred:qual` → (pred, qual).
pub fn split_key(key: &str) -> (&str, Option<&str>) {
    match key.split_once(':') {
        Some((p, q)) => (p.trim(), Some(q.trim()).filter(|q| !q.is_empty())),
        None => (key.trim(), None),
    }
}

/// The `registry` of a memory.extract request.
pub fn registry_json() -> Value {
    REGISTRY
        .iter()
        .map(|p| json!({"key": p.key, "kind": p.kind, "one": p.one, "qualifier": p.qualifier}))
        .collect()
}
