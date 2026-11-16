//! The studio window (design `## studio`, §4.10): the pack previewed by the app's own renderer
//! (`crates/studio` over `mascot-render`), hot reload, lint, comments, audit and a harness
//! simulator with its own `Brain`. Opened from the tray's Studio… or `buddy-app --studio`.
//!
//! Every `studio_*` command is granted only to the window labelled `studio` on the app's own
//! origin (`capabilities/studio.json`) and checks its caller again (`caller.rs`). The page
//! gives names and numbers, never paths: the pack is one of the dirs the app allows
//! (`allowed`), and comments always go to that pack's `review/comments.jsonl`.
//!
//! The library (`#studio-library`): every mascot the app can find, each of one kind, decided here
//! from the folder it lives in, never by the page. `yours` = a folder in `BUDDY_HOME/packs` (the
//! library: the only place the studio writes), `customized` = a library folder with a built-in of
//! the same id (it shadows it; Restore original deletes the copy), `builtin` = a folder under
//! `BUDDY_PACKS` or the repo's `mascots/` (read-only; editing first makes a library copy),
//! `folder` = a pack opened by path (`--studio <dir>`, `BUDDY_PACK`; read-only). Edits are
//! `package::save` (checked, nothing written when refused); art comes in as `.aseprite` bytes.
//!
//! Cost: nothing runs while the window is closed. The page polls `studio_poll` every 500 ms
//! and runs its preview loop only while it exists; closing the window destroys it and drops the
//! session. An audit (up to 240 simulated minutes) runs outside the studio's lock, so polls and
//! the window's close (on the main thread) never wait for it; the other commands are short.

pub mod window;

use std::collections::HashMap;
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
use ::studio::package::{self, Op};
use ::studio::{audit, frames, lint, RenderOpts};

/// The studio's state across window opens: the pack it starts on (none = the library),
/// the pack dirs the page may open (by id), and the open pack while the window exists.
pub struct Desk {
    start: Option<PathBuf>,
    /// Packs given by path (they are `folder`s unless a root holds them).
    allowed: Vec<PathBuf>,
    roots: Roots,
    desktop: Desktop,
    open: Option<Studio>,
    /// Why the start pack didn't open.
    error: Option<String>,
    /// Library thumbnails by folder: (pack.json signature, data URL).
    thumbs: HashMap<PathBuf, (u64, String)>,
}

/// Where mascots live: the library (writable; `None` = the studio writes nothing) and the
/// built-in folders, each holding `<id>/pack.json`.
#[derive(Clone, Debug, Default)]
pub struct Roots {
    pub library: Option<PathBuf>,
    pub builtin: Vec<PathBuf>,
}

/// The mascot on the desktop (`settings.pack`) and how to change it (applies at next launch).
pub struct Desktop {
    pub current: String,
    pub set: Box<dyn Fn(&str) -> Result<(), String> + Send>,
}

impl Desktop {
    /// Remembers the choice only (tests, and a studio with nowhere to save it).
    #[cfg(test)]
    pub fn memory(current: &str) -> Desktop {
        Desktop { current: current.into(), set: Box::new(|_| Ok(())) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Yours,
    Customized,
    Builtin,
    Folder,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Yours => "yours",
            Kind::Customized => "customized",
            Kind::Builtin => "builtin",
            Kind::Folder => "folder",
        }
    }
    /// Its folder is in the library: it may be edited (and, if `yours`, deleted).
    pub fn writable(self) -> bool {
        matches!(self, Kind::Yours | Kind::Customized)
    }
}

/// One mascot in the library listing.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub dir: PathBuf,
    pub kind: Kind,
    /// `customized`: the built-in it shadows.
    pub original: Option<PathBuf>,
}

/// Biggest `.aseprite` the page may send (decoded).
const MAX_UPLOAD: usize = 32 << 20;

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
pub fn configure(start: Option<PathBuf>, roots: Roots, desktop: Desktop) {
    *desk() = Some(Desk::with(start, roots, desktop));
}

/// Sub-folders of `root` holding a pack.json, sorted (dot folders are staging, never packs).
fn scan(root: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    let mut v: Vec<PathBuf> = rd.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).map(|e| e.path()).filter(|d| d.join("pack.json").is_file()).collect();
    v.sort();
    v
}

/// A folder as the page shows it (`~/…` under the home folder). Display only: paths never come back.
fn shown(p: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(h) if !h.as_os_str().is_empty() && p.starts_with(&h) => format!("~/{}", p.strip_prefix(&h).unwrap_or(p).display()),
        _ => p.display().to_string(),
    }
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The `name`, `description` and `author` of a pack (from pack.json; missing = empty).
fn details(dir: &Path) -> (String, String, String) {
    let v: Value = std::fs::read_to_string(dir.join("pack.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    (s("name"), s("description"), s("author"))
}

/// The bytes a copy of `dir` would take: a library folder's own size, or every file a linked
/// pack reads (cheap: no art is decoded, so listing the library stays quick).
fn footprint(dir: &Path, kind: Kind) -> (usize, u64) {
    if kind.writable() {
        return package::size(dir);
    }
    let pack = mascot_core::pack::load_report(dir).ok();
    let f = ::studio::session::files(dir, pack.as_ref());
    f.iter().filter_map(|p| std::fs::metadata(p).ok()).filter(|m| m.is_file()).fold((0, 0), |(n, b), m| (n + 1, b + m.len()))
}

fn bytes_arg(args: &Value) -> Result<Vec<u8>, String> {
    let b = args["bytes"].as_str().ok_or("no file")?;
    if b.len() > MAX_UPLOAD / 3 * 4 + 4 {
        return Err(format!("that file is too big (the studio takes up to {} MB)", MAX_UPLOAD >> 20));
    }
    ::studio::unbase64(b).ok_or_else(|| "the file didn't arrive whole; try again".into())
}

/// A file name to show (never used as a path).
fn label_arg(args: &Value) -> String {
    let n = args["file_name"].as_str().unwrap_or("file.aseprite");
    Path::new(n).file_name().map(|f| f.to_string_lossy().chars().take(80).collect()).unwrap_or_else(|| "file.aseprite".into())
}

/// `start` (when given) and every `<parent>/<id>/` holding a pack.json, one dir per pack id
/// (the page opens packs by id; the first dir with an id wins).
#[cfg(test)]
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
    /// A studio over packs given by path, with no library (it writes only comments).
    #[cfg(test)]
    pub fn new(start: Option<PathBuf>, allowed: Vec<PathBuf>) -> Desk {
        Desk { start, allowed, roots: Roots::default(), desktop: Desktop::memory("bear"), open: None, error: None, thumbs: HashMap::new() }
    }

    /// The app's studio: the library and built-ins in `roots`, opening on `start`.
    pub fn with(start: Option<PathBuf>, roots: Roots, desktop: Desktop) -> Desk {
        let allowed = start.iter().cloned().collect();
        Desk { start, allowed, roots, desktop, open: None, error: None, thumbs: HashMap::new() }
    }

    /// Every mascot, one per id: library folders first (`yours` / `customized`), then the
    /// built-ins they don't shadow, then packs given by path.
    pub fn entries(&self) -> Vec<Entry> {
        let mut builtin: Vec<(String, PathBuf)> = vec![];
        for r in &self.roots.builtin {
            for d in scan(r) {
                let id = pack_id_of(&d);
                if !builtin.iter().any(|(i, _)| *i == id) {
                    builtin.push((id, d));
                }
            }
        }
        let mut out: Vec<Entry> = vec![];
        if let Some(lib) = &self.roots.library {
            for d in scan(lib) {
                let id = d.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                // a folder whose name isn't an id (made by hand) is shown read-only
                if !package::valid_id(&id) {
                    out.push(Entry { id, dir: d, kind: Kind::Folder, original: None });
                    continue;
                }
                let original = builtin.iter().find(|(i, _)| *i == id).map(|(_, d)| d.clone());
                let kind = if original.is_some() { Kind::Customized } else { Kind::Yours };
                out.push(Entry { id, dir: d, kind, original });
            }
        }
        for (id, d) in builtin {
            if !out.iter().any(|e| e.id == id) {
                out.push(Entry { id, dir: d, kind: Kind::Builtin, original: None });
            }
        }
        for d in &self.allowed {
            if !d.join("pack.json").is_file() {
                continue;
            }
            let id = pack_id_of(d);
            if out.iter().any(|e| e.id == id) {
                continue;
            }
            let c = canon(d);
            let kind = if self.roots.builtin.iter().any(|r| c.parent() == Some(&canon(r))) { Kind::Builtin } else { Kind::Folder };
            out.push(Entry { id, dir: d.clone(), kind, original: None });
        }
        out
    }

    fn packs(&self) -> Vec<String> {
        self.entries().into_iter().map(|e| e.id).collect()
    }

    fn entry(&self, args: &Value) -> Result<Entry, String> {
        let id = args["id"].as_str().ok_or("which mascot?")?;
        self.entries().into_iter().find(|e| e.id == id).ok_or_else(|| format!("no mascot “{id}” here"))
    }

    /// What the folder `dir` is: the entry holding it, else a pack given by path.
    fn kind_of(&self, dir: &Path) -> Kind {
        let c = canon(dir);
        self.entries().into_iter().find(|e| canon(&e.dir) == c).map(|e| e.kind).unwrap_or(Kind::Folder)
    }

    fn library(&self) -> Result<&Path, String> {
        self.roots.library.as_deref().ok_or_else(|| "this studio has no mascot library (it can't make or change mascots)".into())
    }

    /// The open pack's folder, when the studio may write it.
    fn writable_open(&self) -> Result<PathBuf, String> {
        let s = self.open.as_ref().ok_or("no pack open")?;
        match self.kind_of(&s.session.dir) {
            k if k.writable() => Ok(s.session.dir.clone()),
            Kind::Builtin => Err("this is a built-in mascot: make your copy to change it".into()),
            _ => Err("this mascot was opened from a folder: duplicate it into your library to change it".into()),
        }
    }

    fn open_dir(&mut self, d: &Path) -> Result<(), String> {
        self.open = Some(Studio::open(d)?);
        self.start = Some(d.to_path_buf());
        self.error = None;
        Ok(())
    }

    fn state(&self) -> Value {
        match &self.open {
            Some(s) => {
                let kind = self.kind_of(&s.session.dir);
                let mut v = s.state(self.packs());
                let (name, description, author) = details(&s.session.dir);
                v["pack"]["kind"] = json!(kind.as_str());
                v["pack"]["writable"] = json!(kind.writable());
                v["pack"]["name"] = json!(name);
                v["pack"]["description"] = json!(description);
                v["pack"]["author"] = json!(author);
                v["pack"]["desktop"] = json!(s.session.pack.id == self.desktop.current);
                v["library"] = json!(self.roots.library.is_some());
                v
            }
            None => json!({ "pack": null, "packs": self.packs(), "error": self.error, "loads": 0, "library": self.roots.library.is_some() }),
        }
    }

    /// `#studio-library`: every mascot with its kind and what it offers.
    fn library_list(&self) -> Value {
        let open = self.open.as_ref().map(|s| canon(&s.session.dir));
        let packs: Vec<Value> = self
            .entries()
            .iter()
            .map(|e| {
                let (name, description, author) = details(&e.dir);
                let (files, bytes) = footprint(&e.dir, e.kind);
                let lib = self.roots.library.is_some();
                json!({
                    "id": e.id, "name": if name.is_empty() { e.id.clone() } else { name }, "description": description, "author": author,
                    "kind": e.kind.as_str(), "desktop": e.id == self.desktop.current, "open": open.as_ref() == Some(&canon(&e.dir)),
                    "files": files, "bytes": bytes,
                    "can": {
                        "edit": e.kind.writable(), "customize": lib && !e.kind.writable(), "duplicate": lib,
                        "delete": e.kind == Kind::Yours, "restore": e.kind == Kind::Customized, "use": e.kind != Kind::Folder,
                    },
                })
            })
            .collect();
        json!({ "packs": packs, "desktop": self.desktop.current, "library": self.roots.library.is_some(), "library_dir": self.roots.library.as_deref().map(shown) })
    }

    /// A pack's first rest frame as a data URL (cached until a file the pack reads changes).
    fn thumb(&mut self, e: &Entry) -> Result<Value, String> {
        let report = mascot_core::pack::load_report(&e.dir).ok();
        let sig = ::studio::session::signature(&::studio::session::files(&e.dir, report.as_ref()));
        if let Some((s, url)) = self.thumbs.get(&e.dir) {
            if *s == sig {
                return Ok(json!({ "id": e.id, "png": url }));
            }
        }
        let s = Session::open(&e.dir).map_err(|e| e.0)?;
        let p = &s.pack;
        let clip = p.behaviors.get("rest").and_then(|b| b.seq.first()).map(|st| st.clip.clone()).filter(|c| p.clips.contains_key(c)).or_else(|| p.clips.keys().next().cloned()).ok_or("no clips")?;
        let views = frames::views(p, &clip);
        let view = ["S", "SE", "A"].iter().map(|v| v.to_string()).find(|v| views.contains(v)).or_else(|| views.first().cloned()).ok_or("no views")?;
        let f = ::studio::render(p, &s.art, &RenderOpts { clip, view, frame: 0, zoom: 1, eyes: EyeState::Open, mouth: MouthState::Closed }).map_err(|e| e.0)?;
        let url = format!("data:image/png;base64,{}", ::studio::base64(&f.png()));
        self.thumbs.insert(e.dir.clone(), (sig, url.clone()));
        Ok(json!({ "id": e.id, "png": url }))
    }

    /// `#studio-edit`: the open pack's pack.json (the page's forms read it), its clip groups and
    /// whether the studio may write it.
    fn edit_view(&self) -> Result<Value, String> {
        let s = self.open.as_ref().ok_or("no pack open")?;
        let dir = &s.session.dir;
        let text = std::fs::read_to_string(dir.join("pack.json")).map_err(|e| format!("pack.json: {e}"))?;
        let kind = self.kind_of(dir);
        let groups = package::groups(dir, &s.session.pack).unwrap_or_default();
        let findings = if kind.writable() { package::check(dir) } else { vec![] };
        let (files, bytes) = footprint(dir, kind);
        Ok(json!({
            "id": s.session.pack.id, "kind": kind.as_str(), "writable": kind.writable(),
            "text": text, "sha": ::studio::sha256_hex(text.as_bytes()),
            "groups": groups, "findings": findings, "files": files, "bytes": bytes,
            "folder": dir.file_name().map(|f| f.to_string_lossy().to_string()),
            "intents": mascot_core::Intent::ALL.iter().chain(mascot_core::Intent::OPTIONAL.iter()).map(|i| json!({ "name": i.as_str(), "optional": mascot_core::Intent::OPTIONAL.contains(i) })).collect::<Vec<_>>(),
        }))
    }

    /// After a write to the open pack: reload it now (not at the next poll).
    fn reload_open(&mut self) {
        if let Some(s) = self.open.as_mut() {
            if s.session.poll() == Poll::Reloaded {
                s.sim.reload(s.session.pack.clone());
            }
        }
    }

    /// The commands that change mascots. Each decides from the folder's root what it may do.
    /// A `studio_save`'s folder, current pack.json and the text it would write (no checks yet).
    fn save_plan(&mut self, args: &Value) -> Result<(PathBuf, String, String), String> {
        self.ensure_open();
        let dir = self.writable_open()?;
        let cur = std::fs::read_to_string(dir.join("pack.json")).map_err(|e| format!("pack.json: {e}"))?;
        if args["sha"].as_str() != Some(&::studio::sha256_hex(cur.as_bytes())) {
            return Err("pack.json changed since this edit began (the art folder or another editor); reload to see it".into());
        }
        let next = match (args.get("raw").and_then(Value::as_str), args.get("ops")) {
            (Some(raw), _) => raw.to_string(),
            (None, Some(ops)) => {
                let ops: Vec<Op> = serde_json::from_value(ops.clone()).map_err(|e| format!("edit: {e}"))?;
                package::apply(&cur, &ops).map_err(|e| e.0)?
            }
            _ => return Err("nothing to save".into()),
        };
        Ok((dir, cur, next))
    }

    fn write_cmd(&mut self, cmd: &str, args: &Value) -> Result<Value, String> {
        match cmd {
            "studio_save" => {
                let (dir, cur, next) = self.save_plan(args)?;
                if args["dry"].as_bool() == Some(true) {
                    return dry_save(&dir, &cur, &next);
                }
                if next != cur {
                    package::save(&dir, &next).map_err(|e| e.0)?;
                    self.reload_open();
                }
                self.edit_view()
            }
            "studio_import" => {
                let dir = self.writable_open()?;
                let prefix = args["name"].as_str().ok_or("name the clips")?;
                let bytes = bytes_arg(args)?;
                let r = ::studio::aseprite::import_bytes(&bytes, &label_arg(args), &dir, prefix).map_err(|e| e.0)?;
                self.reload_open();
                let mut v = self.edit_view()?;
                v["imported"] = json!(r.clips);
                Ok(v)
            }
            "studio_remove_group" => {
                let dir = self.writable_open()?;
                let g = args["group"].as_str().ok_or("which clip group?")?;
                package::remove_group(&dir, g).map_err(|e| e.0)?;
                self.reload_open();
                self.edit_view()
            }
            "studio_create" => {
                let lib = self.library()?.to_path_buf();
                let name = args["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or("give the mascot a name")?;
                let id = args["id"].as_str().map(str::to_string).unwrap_or_else(|| package::slug(name));
                if self.entries().iter().any(|e| e.id == id) {
                    return Err(format!("a mascot “{id}” already exists"));
                }
                let dir = match args["from"].as_str() {
                    Some("aseprite") => package::create_from_aseprite(&lib, &id, name, &bytes_arg(args)?, &label_arg(args)).map_err(|e| e.0)?,
                    Some("copy") => {
                        let src = self.entry(&json!({ "id": args["source"] })).map_err(|_| "copy which mascot?".to_string())?;
                        let out = lib.join(&id);
                        package::bundle(&src.dir, &out, Some(&id), Some(name)).map_err(|e| e.0)?;
                        out
                    }
                    _ => return Err("make it from an .aseprite file or a copy".into()),
                };
                self.open_dir(&dir)?;
                Ok(self.state())
            }
            "studio_customize" => {
                let lib = self.library()?.to_path_buf();
                let e = self.entry(args)?;
                if e.kind.writable() {
                    return Err("this mascot is already yours to change".into());
                }
                // the copy's folder and id are the built-in's id, whatever its own folder is called
                let out = lib.join(&e.id);
                package::bundle(&e.dir, &out, Some(&e.id), None).map_err(|e| e.0)?;
                self.open_dir(&out)?;
                Ok(self.state())
            }
            "studio_restore" => {
                let e = self.entry(args)?;
                let (Kind::Customized, Some(orig)) = (e.kind, e.original.clone()) else {
                    return Err("only a customized built-in can be restored".into());
                };
                let was_open = self.open.as_ref().is_some_and(|s| canon(&s.session.dir) == canon(&e.dir));
                package::delete(&e.dir).map_err(|e| e.0)?;
                self.thumbs.remove(&e.dir);
                if was_open {
                    // the copy is gone either way: never leave it open
                    self.open = None;
                    self.start = Some(orig.clone());
                    if let Err(why) = self.open_dir(&orig) {
                        self.error = Some(why);
                    }
                }
                Ok(self.state())
            }
            "studio_delete" => {
                let e = self.entry(args)?;
                match e.kind {
                    Kind::Yours => {}
                    Kind::Customized => return Err("this is your copy of a built-in mascot: use Restore original".into()),
                    Kind::Builtin | Kind::Folder => return Err("built-in mascots can't be deleted".into()),
                }
                // the backend's own check: only a library folder is ever deleted
                let lib = canon(self.library()?);
                if canon(&e.dir).parent() != Some(&lib) {
                    return Err("only mascots in your library can be deleted".into());
                }
                package::delete(&e.dir).map_err(|e| e.0)?;
                self.thumbs.remove(&e.dir);
                if self.open.as_ref().is_some_and(|s| canon(&s.session.dir) == canon(&e.dir)) {
                    self.open = None;
                    self.start = None;
                }
                if self.desktop.current == e.id {
                    // the folder is gone: a launch with this setting runs the bear anyway
                    if let Err(why) = (self.desktop.set)("bear") {
                        eprintln!("[buddy] studio: deleted “{}” but couldn't put the bear on the desktop: {why}", e.id);
                    }
                    self.desktop.current = "bear".into();
                }
                Ok(self.state())
            }
            "studio_use" => {
                let e = self.entry(args)?;
                if e.kind == Kind::Folder {
                    return Err("a mascot opened from a folder can't go on the desktop; duplicate it into your library first".into());
                }
                (self.desktop.set)(&e.id)?;
                self.desktop.current = e.id;
                Ok(self.library_list())
            }
            _ => Err(format!("unknown {cmd}")),
        }
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
        match cmd {
            "studio_open" => {
                let id = args["id"].as_str().ok_or("open which pack?")?;
                let d = self.entries().into_iter().find(|e| e.id == id).map(|e| e.dir).ok_or_else(|| format!("no pack “{id}” here"))?;
                self.open_dir(&d)?;
                return Ok(self.state());
            }
            "studio_state" => return Ok(self.state()),
            "studio_library" => return Ok(self.library_list()),
            "studio_thumb" => {
                let e = self.entry(args)?;
                return self.thumb(&e);
            }
            "studio_edit" => return self.edit_view(),
            "studio_reveal" => {
                let e = self.entry(args)?;
                reveal(&e.dir)?;
                return Ok(json!({ "ok": true }));
            }
            "studio_save" | "studio_import" | "studio_remove_group" | "studio_create" | "studio_customize" | "studio_restore" | "studio_delete" | "studio_use" => return self.write_cmd(cmd, args),
            _ => {}
        }
        match &mut self.open {
            Some(s) => s.handle(cmd, args),
            None => Err("no pack open".into()),
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
    let order: Vec<String> = pack.idle_pool.iter().map(|e| e.name().to_string()).collect();
    let (w, h, px) = audit::chart(&a, &order);
    let mut v = serde_json::to_value(&a).unwrap_or(Value::Null);
    v["chart"] = json!(format!("data:image/png;base64,{}", ::studio::base64(&::studio::png_bytes(w, h, &px))));
    v["order"] = json!(order);
    Ok(v)
}

/// A dry `studio_save`: every check a save makes, writing nothing (slow for a big pack, so it
/// runs outside the studio lock).
fn dry_save(dir: &Path, cur: &str, next: &str) -> Result<Value, String> {
    package::validate_edit(dir, next).map_err(|e| e.0)?;
    Ok(json!({ "ok": true, "changed": next != cur }))
}

/// Show a mascot's folder in Finder (`open` gets none of the app's environment).
#[cfg(not(test))]
fn reveal(dir: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("/usr/bin/open")
        .arg("-R")
        .arg(dir.join("pack.json"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Finder: {e}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
fn reveal(dir: &Path) -> Result<(), String> {
    if dir.is_dir() { Ok(()) } else { Err("no folder".into()) }
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
    if cmd == "studio_save" && args["dry"].as_bool() == Some(true) {
        // checking an edit loads the whole pack: plan it under the lock, check it outside
        let (dir, cur, next) = desk().as_mut().ok_or("the studio isn't set up")?.save_plan(&args)?;
        return dry_save(&dir, &cur, &next);
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

#[tauri::command]
pub async fn studio_library(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_library", args)
}

#[tauri::command]
pub async fn studio_thumb(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_thumb", args)
}

#[tauri::command]
pub async fn studio_edit(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_edit", args)
}

#[tauri::command]
pub async fn studio_save(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_save", args)
}

#[tauri::command]
pub async fn studio_import(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_import", args)
}

#[tauri::command]
pub async fn studio_remove_group(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_remove_group", args)
}

#[tauri::command]
pub async fn studio_create(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_create", args)
}

#[tauri::command]
pub async fn studio_customize(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_customize", args)
}

#[tauri::command]
pub async fn studio_restore(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_restore", args)
}

#[tauri::command]
pub async fn studio_delete(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_delete", args)
}

#[tauri::command]
pub async fn studio_use(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_use", args)
}

#[tauri::command]
pub async fn studio_reveal(w: tauri::WebviewWindow, args: Option<Value>) -> Result<Value, String> {
    run(&w, "studio_reveal", args)
}

#[cfg(test)]
mod tests;
