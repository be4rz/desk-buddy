//! Personality: the person's layer over the mascot's persona, `settings.persona`.
//! Tray **Personality ▸** picks a preset; **Customize…** opens the `persona` window (page
//! `web/persona/index.html`). Both go through `run` on the main thread (the app owns settings.json):
//! a change is checked, written, and only then kept; it then reaches the brain's temperament and the
//! agent's "Who you are" section at once (`Effects`), with no restart.
//!
//! Who owns what: the preset owns name, catchphrase, chattiness, playfulness and emotes; the person
//! owns `call_me` and `notes`, which survive a preset switch and Reset. A value equal to the preset's
//! is stored as `null` ("follows the preset"). Over-cap text is refused, never cut (a hand-edited
//! file is cut on read instead, `settings::Persona::sanitize`). The notes are tone only: nothing here
//! or in the policy reads them for anything but the prompt's labelled block.
//!
//! The commands answer only the `persona` window (`caller.rs`, `capabilities/persona.json`); the
//! agent has no way to change any of this.

pub mod window;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mascot_protocol::pack::Temperament;
use mascot_core::persona::{self, caps, UserPersona};
use mascot_core::PackMeta;
use serde_json::{json, Map, Value};

use crate::driver::{post, App, Inbox};
use crate::settings::Settings;

/// Every field of `settings.persona`.
pub const FIELDS: [&str; 8] = ["preset", "name", "call_me", "catchphrase", "chattiness", "playfulness", "emotes", "notes"];

/// The mascot's display name (`pack.json` `name`), else its id.
pub fn pack_name(pack: &PackMeta) -> String {
    std::fs::read_to_string(pack.dir.join("pack.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string))
        .unwrap_or_else(|| pack.id.clone())
}

/// The preset a layer resolves to (the default when it names none, or one the pack lacks).
fn preset_of<'a>(pack: &'a PackMeta, user: &UserPersona) -> Option<(&'a str, &'a persona::PresetFile)> {
    let p = pack.persona.as_ref()?;
    let id = user.preset.as_deref().filter(|w| p.preset(w).is_some()).unwrap_or(&p.default);
    p.presets.iter().find(|(i, _)| i == id).map(|(i, f)| (i.as_str(), f))
}

/// The page's view (`persona_state`, `persona://state`): the mascot, its presets (null without a
/// persona), the person's layer, a preset the settings name that the pack lacks, and the caps.
pub fn view(pack: &PackMeta, pack_name: &str, user: &UserPersona) -> Value {
    let presets = pack.persona.as_ref().map(|p| {
        let list: Vec<Value> = p
            .presets
            .iter()
            .map(|(id, f)| {
                let gestures: Map<String, Value> = f
                    .gestures
                    .iter()
                    .map(|(sit, emote)| {
                        let about = pack.body.emote(emote).map(|e| e.about.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| emote.replace('_', " "));
                        (sit.clone(), json!({ "emote": emote, "about": about }))
                    })
                    .collect();
                json!({
                    "id": id,
                    "name": f.name,
                    "tagline": f.tagline,
                    "catchphrase": f.catchphrase.as_ref().map_or("", |c| c.text.as_str()),
                    "chattiness": f.temperament.over(pack.temperament).chattiness,
                    "playfulness": f.style.playfulness,
                    "emotes": f.style.emotes,
                    "examples": f.examples,
                    "gestures": gestures,
                })
            })
            .collect();
        json!({ "default": p.default, "presets": list })
    });
    let missing = persona::resolve(pack, user).and_then(|e| e.missing);
    json!({
        "pack": { "id": pack.id, "name": pack_name },
        "persona": presets,
        "user": user,
        "missing": missing,
        "caps": { "name": caps::NAME, "call_me": caps::CALL_ME, "catchphrase": caps::CATCHPHRASE, "notes": caps::NOTES },
    })
}

fn no_preset(id: &str) -> String {
    format!("no preset “{id}” in this mascot")
}

/// A text field of a patch: null or text, trimmed (notes as written); empty = unset. Over its cap
/// it is refused with the page's wording.
fn text(k: &str, v: &Value, cap: usize) -> Result<Option<String>, String> {
    let s = match v {
        Value::Null => return Ok(None),
        Value::String(s) => s,
        _ => return Err(format!("{k}: must be text or null")),
    };
    let s = if k == "notes" { s.as_str() } else { s.trim() };
    let n = s.chars().count();
    if n > cap {
        return Err(format!("{k}: {n} characters, the limit is {cap}"));
    }
    Ok((!s.trim().is_empty()).then(|| s.to_string()))
}

fn unit(k: &str, v: &Value) -> Result<Option<f64>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => match n.as_f64() {
            Some(x) if x.is_finite() && (0.0..=1.0).contains(&x) => Ok(Some(x)),
            _ => Err(format!("{k}: must be between 0 and 1")),
        },
        _ => Err(format!("{k}: must be a number or null")),
    }
}

/// Values equal to the preset's own become `null` (they follow the preset).
fn normalize(pack: &PackMeta, u: &mut UserPersona) {
    let Some((_, p)) = preset_of(pack, u) else { return };
    let same = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 0.005);
    if u.name.as_deref() == Some(p.name.as_str()) {
        u.name = None;
    }
    if u.catchphrase.is_some() && u.catchphrase.as_deref() == p.catchphrase.as_ref().map(|c| c.text.as_str()) {
        u.catchphrase = None;
    }
    if same(u.chattiness, p.temperament.over(pack.temperament).chattiness) {
        u.chattiness = None;
    }
    if same(u.playfulness, p.style.playfulness) {
        u.playfulness = None;
    }
    if same(u.emotes, p.style.emotes) {
        u.emotes = None;
    }
}

/// `user` with `patch` (`persona_set`) over it, or why not (nothing is changed then).
pub fn patch(pack: &PackMeta, user: &UserPersona, patch: &Value) -> Result<UserPersona, String> {
    patch_in(pack, user, patch, false)
}

/// `patch`; `restore` (the page's Undo of a pick) also takes back a preset the pack lacks, so a pick
/// away from a missing preset can be undone.
fn patch_in(pack: &PackMeta, user: &UserPersona, patch: &Value, restore: bool) -> Result<UserPersona, String> {
    let obj = patch.as_object().ok_or("the change must be an object")?;
    if let Some(k) = obj.keys().find(|k| !FIELDS.contains(&k.as_str())) {
        return Err(format!("unknown field “{k}”"));
    }
    let mut next = user.clone();
    for (k, v) in obj {
        match k.as_str() {
            "preset" => {
                next.preset = match v {
                    Value::Null => None,
                    // the one the settings already name may stay even when the pack lacks it, and an undo
                    // may bring it back
                    Value::String(id) if restore || user.preset.as_deref() == Some(id.as_str()) => Some(id.clone()),
                    Value::String(id) if pack.persona.as_ref().is_some_and(|p| p.preset(id).is_some()) => Some(id.clone()),
                    Value::String(id) => return Err(no_preset(id)),
                    _ => return Err("preset: must be text or null".into()),
                }
            }
            "name" => next.name = text(k, v, caps::NAME)?,
            "call_me" => next.call_me = text(k, v, caps::CALL_ME)?,
            "catchphrase" => next.catchphrase = text(k, v, caps::CATCHPHRASE)?,
            "notes" => next.notes = text(k, v, caps::NOTES)?,
            "chattiness" => next.chattiness = unit(k, v)?,
            "playfulness" => next.playfulness = unit(k, v)?,
            _ => next.emotes = unit(k, v)?,
        }
    }
    normalize(pack, &mut next);
    Ok(next)
}

/// The fields a preset owns (name, catchphrase, chattiness, playfulness, emotes): a preset switch
/// and Reset clear them.
fn clear_owned(u: &mut UserPersona) {
    u.name = None;
    u.catchphrase = None;
    u.chattiness = None;
    u.playfulness = None;
    u.emotes = None;
}

/// `persona_pick` and the tray: the preset, with the preset-owned fields cleared.
pub fn pick(pack: &PackMeta, user: &UserPersona, id: &str) -> Result<UserPersona, String> {
    if !pack.persona.as_ref().is_some_and(|p| p.preset(id).is_some()) {
        return Err(no_preset(id));
    }
    let mut next = user.clone();
    next.preset = Some(id.to_string());
    clear_owned(&mut next);
    Ok(next)
}

/// `persona_reset`: the preset as written (`call_me` and the notes stay).
pub fn reset(user: &UserPersona) -> UserPersona {
    let mut next = user.clone();
    clear_owned(&mut next);
    next
}

/// Some preset-owned field differs from the preset.
pub fn customized(user: &UserPersona) -> bool {
    user.name.is_some() || user.catchphrase.is_some() || user.chattiness.is_some() || user.playfulness.is_some() || user.emotes.is_some()
}

/// The tray's Personality submenu.
#[derive(Clone, Debug, PartialEq)]
pub struct TrayView {
    pub title: String,
    /// `(id "persona:<preset>", label, checked)`, in pack order; empty = no persona.
    pub items: Vec<(String, String, bool)>,
}

pub fn tray(pack: &PackMeta, user: &UserPersona) -> TrayView {
    let (Some(p), Some(e)) = (pack.persona.as_ref(), persona::resolve(pack, user)) else {
        return TrayView { title: "Personality".into(), items: vec![] };
    };
    let title = if user.name.is_some() {
        format!("Personality: {}", e.name)
    } else if customized(user) {
        format!("Personality: {} (customized)", e.name)
    } else {
        format!("Personality: {}", e.name)
    };
    let items = p
        .presets
        .iter()
        .map(|(id, f)| {
            let label = if f.tagline.is_empty() { f.name.clone() } else { format!("{} · {}", f.name, f.tagline) };
            (format!("persona:{id}"), label, *id == e.preset && e.missing.is_none())
        })
        .collect();
    TrayView { title, items }
}

/// What a layer does to the rest of the app.
#[derive(Clone, Debug, PartialEq)]
pub struct Effects {
    /// The brain's: the pack's, the preset's over it, the person's chattiness over that.
    pub temperament: Temperament,
    /// The agent's "Who you are" section; `None` without a persona.
    pub prompt: Option<String>,
}

pub fn effects(pack: &PackMeta, user: &UserPersona) -> Effects {
    match persona::resolve(pack, user) {
        Some(e) => Effects { temperament: e.temperament, prompt: Some(persona::render(&e)) },
        None => Effects { temperament: pack.temperament, prompt: None },
    }
}

/// What the page (or the tray) asks for.
#[derive(Clone, Debug)]
pub enum Op {
    Set(Value),
    /// The page's Undo of a pick or Reset: a `Set` that may bring back a preset the pack lacks.
    Undo(Value),
    Pick(String),
    Reset,
}

/// Apply `op` to `settings.persona`: checked, then written to `home`, then kept. `Ok((reply, changed))`;
/// on `Err` nothing changed, in memory or on disk.
pub fn run(pack: &PackMeta, pack_name: &str, settings: &mut Settings, home: &Path, op: Op) -> Result<(Value, bool), String> {
    let before = settings.persona.user.clone();
    let next = match &op {
        Op::Set(p) => patch(pack, &before, p)?,
        Op::Undo(p) => patch_in(pack, &before, p, true)?,
        Op::Pick(id) => pick(pack, &before, id)?,
        Op::Reset => reset(&before),
    };
    let changed = next != before;
    if changed {
        let mut s = settings.clone();
        s.persona.user = next.clone();
        s.save(home).map_err(|e| format!("couldn't write {}: {e}", crate::settings::path(home).display()))?;
        *settings = s;
    }
    let mut reply = view(pack, pack_name, &next);
    if matches!(op, Op::Pick(_)) {
        reply["undo"] = json!(before);
    }
    Ok((reply, changed))
}

/// A short line for the log: the preset and whether it is customized, never the person's words.
pub fn log_line(pack: &PackMeta, user: &UserPersona) -> String {
    match persona::resolve(pack, user) {
        Some(e) => format!("persona: {}{}{}", e.preset, if customized(user) { " (customized)" } else { "" }, if e.notes.is_empty() { "" } else { " + notes" }),
        None => "persona: none in this mascot".into(),
    }
}

// --- the agent's section ------------------------------------------------------------------------

/// The newest section not sent yet; one sender at a time, so the last change always lands last.
static LATEST: Mutex<Option<Option<String>>> = Mutex::new(None);
static SENDING: Mutex<()> = Mutex::new(());

/// Hand `prompt` to the agent (kept for its next start; a running sidecar swaps it now). Off the
/// main thread: the sidecar's answer can take a moment.
pub fn send_prompt(prompt: Option<String>) {
    let Some(h) = crate::harness::get() else { return };
    *LATEST.lock().unwrap() = Some(prompt);
    std::thread::spawn(move || push_latest(h.agent()));
}

fn push_latest(agent: &Arc<bridge::agent::Agent>) {
    let _one = SENDING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = LATEST.lock().unwrap().take() else { return };
    if let Err(e) = agent.set_persona(p) {
        eprintln!("[buddy] persona: the agent didn't take it ({}); it applies at its next start", e.message);
    }
}

// --- the page's commands --------------------------------------------------------------------------

/// The Personality window only (`caller`).
fn mine(w: &tauri::WebviewWindow, cmd: &str) -> Result<(), String> {
    if crate::caller::is(w.label(), window::LABEL, cmd) {
        Ok(())
    } else {
        Err("not allowed".into())
    }
}

/// Run `f` on the app (the main thread owns the settings) and wait for its answer.
fn on_app(f: impl FnOnce(&mut App) -> Result<Value, String> + Send + 'static) -> Result<Value, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    post(Inbox::Call(Box::new(move |a| {
        let _ = tx.send(f(a));
    })));
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "the app didn't answer in time".to_string())?
}

// Async: they wait for the main thread (never block it).
#[tauri::command]
pub async fn persona_state(w: tauri::WebviewWindow) -> Result<Value, String> {
    mine(&w, "persona_state")?;
    on_app(|a| Ok(a.persona_view()))
}

#[tauri::command]
pub async fn persona_set(w: tauri::WebviewWindow, patch: Value, undo: Option<bool>) -> Result<Value, String> {
    mine(&w, "persona_set")?;
    let op = if undo == Some(true) { Op::Undo(patch) } else { Op::Set(patch) };
    on_app(move |a| a.persona_op(op))
}

#[tauri::command]
pub async fn persona_pick(w: tauri::WebviewWindow, preset: String) -> Result<Value, String> {
    mine(&w, "persona_pick")?;
    on_app(move |a| a.persona_op(Op::Pick(preset)))
}

#[tauri::command]
pub async fn persona_reset(w: tauri::WebviewWindow) -> Result<Value, String> {
    mine(&w, "persona_reset")?;
    on_app(|a| a.persona_op(Op::Reset))
}

#[tauri::command]
pub fn persona_open_studio(w: tauri::WebviewWindow) -> Result<Value, String> {
    mine(&w, "persona_open_studio")?;
    crate::studio::window::open(false);
    Ok(Value::Null)
}

#[tauri::command]
pub fn persona_close(w: tauri::WebviewWindow) -> Result<Value, String> {
    mine(&w, "persona_close")?;
    window::hide();
    Ok(Value::Null)
}

#[cfg(test)]
mod tests;
