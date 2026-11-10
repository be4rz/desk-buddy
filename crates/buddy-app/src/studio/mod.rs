//! The studio window (design `## studio`, §4.10): the pack previewed by the app's own renderer
//! (`crates/studio` over `mascot-render`), hot reload, lint, comments, audit and a harness
//! simulator with its own `Brain`. Opened from the tray's Studio… or `buddy-app --studio`.
//!
//! Every `studio_*` command is granted only to the window labelled `studio` on the app's own
//! origin (`capabilities/studio.json`) and checks its caller again (`caller.rs`). The page
//! gives names and numbers, never paths: the pack is one of the dirs the app allows
//! (`allowed`), and comments always go to that pack's `review/comments.jsonl`.
//!
//! Cost: nothing runs while the window is closed. The page polls `studio_poll` every 500 ms
//! and runs its preview loop only while it exists; closing the window destroys it and drops the
//! session. An audit (up to 240 simulated minutes) runs outside the studio's lock, so polls and
//! the window's close (on the main thread) never wait for it; the other commands are short.

pub mod window;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mascot_core::replay::{SensesPatch, Timeline};
use mascot_core::{EyeState, MouthState, PackMeta, Poke};
use mascot_protocol::pack::Temperament;
use mascot_protocol::MascotEvent;
use serde_json::{json, Value};
use ::studio::comments::{self, NewComment};
use ::studio::session::{Poll, Session};
use ::studio::sim::Sim;
use ::studio::{audit, frames, lint, RenderOpts};

/// The studio's state across window opens: the pack it starts on (none = the empty state),
/// the pack dirs the page may open (by id), and the open pack while the window exists.
pub struct Desk {
    start: Option<PathBuf>,
    allowed: Vec<PathBuf>,
    open: Option<Studio>,
    /// Why the start pack didn't open.
    error: Option<String>,
}

/// The open pack (hot-reloaded) and its simulator.
pub struct Studio {
    pub session: Session,
    sim: Sim,
}

static DESK: Mutex<Option<Desk>> = Mutex::new(None);

/// The studio's state; a command that panicked doesn't lock the studio (or the main thread's
/// close) out for good.
fn desk() -> MutexGuard<'static, Option<Desk>> {
    DESK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// At startup: the pack the window opens on (`None`: the empty state) and every pack dir the
/// page may open.
pub fn configure(start: Option<PathBuf>, allowed: Vec<PathBuf>) {
    *desk() = Some(Desk::new(start, allowed));
}

/// `start` (when given) and every `<parent>/<id>/` holding a pack.json, one dir per pack id
/// (the page opens packs by id; the first dir with an id wins).
pub fn allowed_packs(start: Option<&Path>, parents: &[PathBuf]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = start.map(Path::to_path_buf).into_iter().collect();
    // a start dir without a pack.json (a typo) doesn't hide a real pack of that name
    let mut ids: Vec<String> = v.iter().filter(|d| d.join("pack.json").is_file()).map(|d| pack_id_of(d)).collect();
    for p in parents {
        let Ok(rd) = std::fs::read_dir(p) else { continue };
        let mut found: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|d| d.join("pack.json").is_file()).collect();
        found.sort();
        for d in found {
            let id = pack_id_of(&d);
            if !ids.contains(&id) {
                ids.push(id);
                v.push(d);
            }
        }
    }
    v
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

const SIM_SEED: u64 = 1;

fn pack_id_of(d: &Path) -> String {
    std::fs::read_to_string(d.join("pack.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["id"].as_str().map(str::to_string)).unwrap_or_else(|| d.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
}

impl Desk {
    pub fn new(start: Option<PathBuf>, allowed: Vec<PathBuf>) -> Desk {
        Desk { start, allowed, open: None, error: None }
    }

    fn packs(&self) -> Vec<String> {
        self.allowed.iter().map(|d| pack_id_of(d)).collect()
    }

    /// Open the start pack when nothing is open yet.
    fn ensure_open(&mut self) {
        if self.open.is_none() {
            if let Some(d) = self.start.clone() {
                match Studio::open(&d) {
                    Ok(s) => self.open = Some(s),
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    /// The open pack, for work done outside the lock (an audit).
    pub fn pack(&mut self) -> Result<Arc<PackMeta>, String> {
        self.ensure_open();
        self.open.as_ref().map(|s| s.session.pack.clone()).ok_or_else(|| "no pack open".into())
    }

    /// One command from the page. `args` is the page's object; paths never come from it.
    pub fn handle(&mut self, cmd: &str, args: &Value) -> Result<Value, String> {
        if cmd != "studio_open" {
            self.ensure_open();
        }
        if cmd == "studio_open" {
            let id = args["id"].as_str().ok_or("open which pack?")?;
            let d = self.allowed.iter().find(|d| d.join("pack.json").is_file() && pack_id_of(d) == id).or_else(|| self.allowed.iter().find(|d| pack_id_of(d) == id)).cloned().ok_or_else(|| format!("no pack “{id}” here"))?;
            self.open = Some(Studio::open(&d)?);
            self.start = Some(d);
            self.error = None;
        }
        match (&self.open, cmd) {
            (Some(s), "studio_state" | "studio_open") => Ok(s.state(self.packs())),
            (Some(_), _) => self.open.as_mut().expect("open").handle(cmd, args),
            (None, "studio_state") => Ok(json!({ "pack": null, "packs": self.packs(), "error": self.error, "loads": 0 })),
            (None, _) => Err("no pack open".into()),
        }
    }

    /// The window closed.
    pub fn close(&mut self) {
        self.open = None;
    }
}

impl Studio {
    pub fn open(dir: &Path) -> Result<Studio, String> {
        let session = Session::open(dir).map_err(|e| e.0)?;
        let sim = Sim::new(session.pack.clone(), SIM_SEED);
        Ok(Studio { session, sim })
    }

    /// Everything the page shows about the pack (no frames).
    pub fn state(&self, packs: Vec<String>) -> Value {
        let p = &self.session.pack;
        let clips: Vec<Value> = p
            .clips
            .values()
            .map(|c| json!({ "name": c.name, "set": c.set, "loop": c.looping, "fps": c.fps, "w": c.frame_w, "h": c.frame_h, "durs": c.durs_ms, "views": frames::views(p, &c.name) }))
            .collect();
        let behaviors: Vec<Value> = p.behaviors.keys().map(|b| json!({ "name": b, "available": p.available(b) })).collect();
        json!({
            "pack": { "id": p.id, "dir": self.session.dir.display().to_string(), "clips": clips, "behaviors": behaviors,
                      "temperament": p.temperament, "face": p.face.is_some() },
            "packs": packs,
            "error": self.session.error,
            "loads": self.session.loads,
        })
    }

    fn handle(&mut self, cmd: &str, args: &Value) -> Result<Value, String> {
        match cmd {
            "studio_poll" => {
                let r = self.session.poll();
                if r == Poll::Reloaded {
                    // the simulator plays the new pack from the start, with the page's
                    // temperament and senses
                    self.sim.reload(self.session.pack.clone());
                }
                Ok(json!({ "changed": r != Poll::Same, "reloaded": r == Poll::Reloaded, "error": self.session.error, "loads": self.session.loads }))
            }
            "studio_frames" => {
                let clip = args["clip"].as_str().ok_or("which clip?")?.to_string();
                let view = args["dir"].as_str().ok_or("which direction?")?.to_string();
                let eyes = match args["eyes"].as_str() {
                    None => EyeState::Open,
                    Some(e) => frames::parse_eyes(e).ok_or_else(|| format!("eyes “{e}”"))?,
                };
                let mouth = match args["mouth"].as_str() {
                    None => MouthState::Closed,
                    Some(m) => frames::parse_mouth(m).ok_or_else(|| format!("mouth “{m}”"))?,
                };
                let s = &self.session;
                let c = s.pack.clips.get(&clip).ok_or_else(|| format!("no clip “{clip}”"))?;
                let mut out = vec![];
                let (mut w, mut h) = (0, 0);
                for frame in 0..c.durs_ms.len() {
                    let f = ::studio::render(&s.pack, &s.art, &RenderOpts { clip: clip.clone(), view: view.clone(), frame, zoom: 1, eyes, mouth }).map_err(|e| e.0)?;
                    (w, h) = (f.w, f.h);
                    out.push(format!("data:image/png;base64,{}", ::studio::base64(&f.png())));
                }
                Ok(json!({ "clip": clip, "dir": view, "w": w, "h": h, "durs": c.durs_ms, "loop": c.looping, "frames": out }))
            }
            "studio_lint" => Ok(serde_json::to_value(lint::lint(&self.session.pack)).unwrap_or(Value::Null)),
            "studio_audit" => audit_of(self.session.pack.clone(), args),
            "studio_comments" => Ok(json!({ "comments": comments::read(&self.session.dir).map_err(|e| e.0)? })),
            "studio_comment_add" => {
                let c: NewComment = serde_json::from_value(args.clone()).map_err(|e| format!("comment: {e}"))?;
                // the file is always the open pack's review/comments.jsonl (`pack.dir`)
                comments::add(&self.session.pack, &c, now_secs()).map(|c| json!(c)).map_err(|e| e.0)
            }
            "studio_comment_status" => {
                let id = args["id"].as_str().ok_or("which comment?")?;
                let status = args["status"].as_str().ok_or("which status?")?;
                comments::set_status(&self.session.dir, id, status).map(|c| json!(c)).map_err(|e| e.0)
            }
            "studio_sim" => self.sim_op(args),
            _ => Err(format!("unknown {cmd}")),
        }
    }

    fn sim_op(&mut self, a: &Value) -> Result<Value, String> {
        let bad = |what: &str, e: serde_json::Error| format!("{what}: {e}");
        let mut dt = 0;
        match a["op"].as_str().unwrap_or("") {
            "reset" => self.sim.reset(a["seed"].as_u64().unwrap_or(1)),
            "event" => self.sim.event(&serde_json::from_value::<MascotEvent>(a["event"].clone()).map_err(|e| bad("event", e))?),
            "senses" => self.sim.senses(&serde_json::from_value::<SensesPatch>(a["patch"].clone()).map_err(|e| bad("senses", e))?),
            "poke" => self.sim.poke(serde_json::from_value::<Poke>(a["poke"].clone()).map_err(|e| bad("poke", e))?),
            "timeline" => self.sim.timeline(serde_json::from_value::<Timeline>(a["timeline"].clone()).map_err(|e| bad("timeline", e))?).map_err(|e| format!("timeline: {e}"))?,
            "temperament" => self.sim.set_temperament(serde_json::from_value::<Temperament>(a["temperament"].clone()).map_err(|e| bad("temperament", e))?),
            "tick" => dt = a["dt_ms"].as_u64().unwrap_or(0),
            o => return Err(format!("unknown simulator op “{o}”")),
        }
        let show = self.sim.tick(dt);
        let trace: Vec<&mascot_core::TraceLine> = self.sim.trace().iter().rev().take(40).collect();
        Ok(json!({ "t_ms": self.sim.now(), "show": show, "pending": self.sim.pending(), "seed": self.sim.seed(), "trace": trace }))
    }
}

/// A quiet-day audit of `pack` (`minutes` 1–240, `seed`) with its chart.
fn audit_of(pack: Arc<PackMeta>, args: &Value) -> Result<Value, String> {
    let minutes = args["minutes"].as_f64().unwrap_or(30.0);
    if !(minutes > 0.0 && minutes <= 240.0) {
        return Err("an audit is 1–240 minutes".into());
    }
    let seed = args["seed"].as_u64().unwrap_or(3);
    let a = audit::run(pack.clone(), &audit::quiet(minutes, seed)).map_err(|e| e.0)?;
    let order: Vec<String> = pack.idle_pool.iter().map(|e| e.behavior.clone()).collect();
    let (w, h, px) = audit::chart(&a, &order);
    let mut v = serde_json::to_value(&a).unwrap_or(Value::Null);
    v["chart"] = json!(format!("data:image/png;base64,{}", ::studio::base64(&::studio::png_bytes(w, h, &px))));
    v["order"] = json!(order);
    Ok(v)
}

/// The window closed: drop the session (nothing runs with it closed).
pub fn close() {
    if let Some(d) = desk().as_mut() {
        d.close();
    }
}

fn run(w: &tauri::WebviewWindow, cmd: &str, args: Option<Value>) -> Result<Value, String> {
    if !crate::caller::is(w.label(), window::LABEL, cmd) {
        return Err("not allowed".into());
    }
    let args = args.unwrap_or(Value::Null);
    if cmd == "studio_audit" {
        // the long one: take the pack, then let go of the lock
        let pack = desk().as_mut().ok_or("the studio isn't set up")?.pack()?;
        return audit_of(pack, &args);
    }
    let mut g = desk();
    let desk = g.as_mut().ok_or("the studio isn't set up")?;
    desk.handle(cmd, &args)
}

// Async: they run off the main thread (rendering a clip or a 30-min audit takes a moment).
#[tauri::command]
pub async fn studio_state(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_state", args)
}

#[tauri::command]
pub async fn studio_poll(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_poll", args)
}

#[tauri::command]
pub async fn studio_open(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_open", args)
}

#[tauri::command]
pub async fn studio_frames(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_frames", args)
}

#[tauri::command]
pub async fn studio_lint(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_lint", args)
}

#[tauri::command]
pub async fn studio_audit(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_audit", args)
}

#[tauri::command]
pub async fn studio_comments(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_comments", args)
}

#[tauri::command]
pub async fn studio_comment_add(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_comment_add", args)
}

#[tauri::command]
pub async fn studio_comment_status(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_comment_status", args)
}

#[tauri::command]
pub async fn studio_sim(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_sim", args)
}

#[cfg(test)]
mod tests;
