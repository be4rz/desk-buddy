//! The mascot package standard (`mascots/README.md` § package) and the studio's writes to a
//! package: check, bundle, create, save an edit, remove a clip group, delete.
//!
//! A standard package is one self-contained folder named after the mascot's id:
//! `pack.json` with `"art_root": "art"`, its art under `art/<group>/`, its palettes under
//! `palettes/`, an optional `README.md`, and `review/` (comments, not part of the mascot).
//! Every write is staged and loaded before it replaces anything: a change that would stop the
//! pack loading is refused and nothing is written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use mascot_core::PackMeta;
use mascot_protocol::pack::PackFile;
use serde::{Deserialize, Serialize};

use crate::jsonedit::{self, Seg};
use crate::{err, Error, Result};

/// Top-level entries a package may hold (dotfiles are ignored).
pub const ENTRIES: [&str; 5] = ["pack.json", "art", "palettes", "README.md", "review"];

/// `[a-z0-9][a-z0-9_-]{0,31}`: a mascot id, also its folder's name.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty() && b.len() <= 32 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// An id made from a display name: `"Sea Otter!"` → `sea-otter`.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            s.push(c);
        } else if !s.is_empty() && !s.ends_with('-') {
            s.push('-');
        }
        if s.len() >= 32 {
            break;
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        "mascot".into()
    } else {
        s
    }
}

/// One way a folder misses the standard: the file (relative to the folder; "" = the folder) and
/// what is wrong.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Finding {
    pub path: String,
    pub problem: String,
}

fn finding(path: impl Into<String>, problem: impl Into<String>) -> Finding {
    Finding { path: path.into(), problem: problem.into() }
}

/// What is wrong with a manifest path (`None` = fine): relative, no `..`, no backslashes.
fn bad_path(p: &str) -> Option<&'static str> {
    if p.is_empty() {
        return Some("is empty");
    }
    if p.contains('\\') || p.contains(':') {
        return Some("must use / and no drive letters");
    }
    let path = Path::new(p);
    if path.is_absolute() {
        return Some("must be relative to the package");
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Some("must not leave the package (no ..)");
    }
    None
}

/// The manifest-level rules, for `text` as the pack.json of a folder named `folder`.
pub fn manifest_findings(text: &str, folder: &str) -> Vec<Finding> {
    let mut f = vec![];
    let pf: PackFile = match serde_json::from_str(text) {
        Ok(p) => p,
        Err(e) => return vec![finding("pack.json", format!("doesn't parse: {e}"))],
    };
    if !valid_id(&pf.id) {
        f.push(finding("pack.json", format!("id “{}”: use 1–32 lowercase letters, digits, - and _ (starting with a letter or digit)", pf.id)));
    }
    if pf.id != folder {
        f.push(finding("pack.json", format!("id “{}” must match the folder's name “{folder}”", pf.id)));
    }
    if pf.name.trim().is_empty() {
        f.push(finding("pack.json", "name is empty"));
    }
    if pf.art_root != "art" {
        f.push(finding("pack.json", format!("art_root is “{}”; a package keeps its art in “art”", pf.art_root)));
    }
    for (i, p) in pf.palettes.iter().enumerate() {
        if let Some(why) = bad_path(p) {
            f.push(finding("pack.json", format!("palettes[{i}] “{p}” {why}")));
        } else if !(p.starts_with("palettes/") && p.ends_with(".json") && Path::new(p).components().count() == 2) {
            f.push(finding("pack.json", format!("palettes[{i}] “{p}”: palettes live in palettes/<name>.json")));
        }
    }
    for (i, s) in pf.sources.iter().enumerate() {
        for (what, p) in [("path", Some(&s.path)), ("props", s.props.as_ref())] {
            if let Some(why) = p.and_then(|p| bad_path(p)) {
                f.push(finding("pack.json", format!("sources[{i}].{what} “{}” {why}", p.unwrap())));
            }
        }
    }
    f
}

/// Every entry under `dir` (not following symlinks), relative.
fn walk(dir: &Path) -> Vec<(PathBuf, std::fs::Metadata)> {
    let mut out = vec![];
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(dir.join(&rel)) else { continue };
        let mut names: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let r = rel.join(e.file_name());
            let Ok(m) = std::fs::symlink_metadata(dir.join(&r)) else { continue };
            if m.is_dir() {
                todo.push(r.clone());
            }
            out.push((r, m));
        }
    }
    out
}

/// Files and bytes in a folder (what the studio shows as a package's size).
pub fn size(dir: &Path) -> (usize, u64) {
    walk(dir).iter().filter(|(_, m)| m.is_file()).fold((0, 0), |(n, b), (_, m)| (n + 1, b + m.len()))
}

/// Check a folder against the standard: every problem, empty = a standard package.
pub fn check(dir: &Path) -> Vec<Finding> {
    let folder = dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    check_as(dir, &folder)
}

/// `check` for a folder that will be named `folder` (a staged copy).
fn check_as(dir: &Path, folder: &str) -> Vec<Finding> {
    let mut f = vec![];
    if !dir.is_dir() {
        return vec![finding("", "not a folder")];
    }
    if !valid_id(folder) {
        f.push(finding("", format!("folder name “{folder}” isn't a valid id (1–32 lowercase letters, digits, - and _)")));
    }
    let text = match std::fs::read_to_string(dir.join("pack.json")) {
        Ok(t) => t,
        Err(_) => {
            f.push(finding("pack.json", "missing"));
            return f;
        }
    };
    f.extend(manifest_findings(&text, folder));
    let Ok(canon) = dir.canonicalize() else {
        f.push(finding("", "can't resolve the folder"));
        return f;
    };
    for (rel, m) in walk(dir) {
        let name = rel.to_string_lossy().to_string();
        let top = rel.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        if top.starts_with('.') {
            continue;
        }
        if rel.components().count() == 1 && !ENTRIES.contains(&top.as_str()) {
            f.push(finding(&name, "isn't part of a package (pack.json, art/, palettes/, README.md, review/)"));
        }
        if m.file_type().is_symlink() {
            let inside = dir.join(&rel).canonicalize().is_ok_and(|t| t.starts_with(&canon));
            if !inside {
                f.push(finding(&name, "is a link to something outside the package"));
            }
        }
    }
    if f.iter().any(|x| x.problem.starts_with("doesn't parse")) {
        return f;
    }
    match mascot_core::pack::load_report(dir) {
        Err(e) => f.push(finding("pack.json", format!("doesn't load: {e}"))),
        Ok(pack) => {
            for b in &pack.body_issues {
                f.push(finding("pack.json", format!("body: {b}")));
            }
            for p in &pack.persona_issues {
                f.push(finding("pack.json", p.clone()));
            }
            for file in crate::session::files(dir, Some(&pack)) {
                let rel = file.strip_prefix(dir).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| file.display().to_string());
                match file.canonicalize() {
                    Ok(c) if !c.starts_with(&canon) => f.push(finding(rel, "is read from outside the package")),
                    _ => {}
                }
            }
            if let Err(e) = mascot_render::Art::load(&pack) {
                f.push(finding("art", format!("doesn't draw: {e}")));
            }
        }
    }
    f
}

/// What `bundle` made.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Bundled {
    pub dir: String,
    pub id: String,
    pub files: usize,
    pub bytes: u64,
}

fn io(p: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |e| Error(format!("{}: {e}", p.display()))
}

/// A path made lexically clean (`a/./b/../c` → `a/c`); `None` when it climbs above its start.
fn clean(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(n) => out.push(n),
            _ => return None,
        }
    }
    Some(out)
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).expect("json")
}

/// Turn any loadable pack into a standard package at `out` (which must not exist and must be
/// named after the id): the art the pack reads goes to `art/` keeping its layout under the old
/// art root, palettes to `palettes/`, README.md comes along, `review/` doesn't. `id` / `name`
/// rename the copy. The copy is checked before it is moved into place; it draws the same pixels.
pub fn bundle(src: &Path, out: &Path, id: Option<&str>, name: Option<&str>) -> Result<Bundled> {
    let pj = src.join("pack.json");
    let text = std::fs::read_to_string(&pj).map_err(io(&pj))?;
    let pf: PackFile = serde_json::from_str(&text).map_err(|e| Error(format!("{}: {e}", pj.display())))?;
    let pack = mascot_core::pack::load_report(src)?;
    // a personality problem is the author's to fix first (unlike a body one, it isn't carried over)
    if let Some(i) = pack.persona_issues.first() {
        return err(format!("{}: {i}; fix the personality before bundling", pj.display()));
    }
    let id = id.unwrap_or(&pf.id).to_string();
    if !valid_id(&id) {
        return err(format!("id “{id}”: use 1–32 lowercase letters, digits, - and _ (starting with a letter or digit)"));
    }
    if out.file_name().map(|f| f.to_string_lossy().to_string()).as_deref() != Some(id.as_str()) {
        return err(format!("{}: a package's folder is named after its id (“{id}”)", out.display()));
    }
    if out.exists() {
        return err(format!("{} already exists", out.display()));
    }
    let root = src.join(&pf.art_root).canonicalize().map_err(io(&src.join(&pf.art_root)))?;
    let mut copies: BTreeMap<PathBuf, PathBuf> = BTreeMap::new(); // dest (relative) → source
    // palettes, by file name
    let mut palettes = vec![];
    for p in &pf.palettes {
        let abs = src.join(p).canonicalize().map_err(io(&src.join(p)))?;
        let stem = abs.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "palette".into());
        let mut n = 1;
        let dest = loop {
            let name = if n == 1 { format!("{stem}.json") } else { format!("{stem}-{n}.json") };
            let d = PathBuf::from("palettes").join(&name);
            match copies.get(&d) {
                Some(s) if *s != abs => n += 1,
                _ => break d,
            }
        };
        palettes.push(dest.to_string_lossy().to_string());
        copies.insert(dest, abs);
    }
    // the art: every file the pack reads, kept at its place under the art root
    let pal_abs: BTreeSet<PathBuf> = copies.values().cloned().collect();
    for f in crate::session::files(src, Some(&pack)) {
        if f == pj {
            continue;
        }
        let Ok(abs) = f.canonicalize() else { continue }; // an optional source that isn't there
        if pal_abs.contains(&abs) {
            continue;
        }
        let rel = abs.strip_prefix(&root).map_err(|_| Error(format!("{}: outside the pack's art root ({}); can't bundle", f.display(), root.display())))?;
        copies.insert(Path::new("art").join(rel), abs);
    }
    // sources: same paths when they are already clean
    let mut new = text.clone();
    for (i, s) in pf.sources.iter().enumerate() {
        for (key, p) in [("path", Some(&s.path)), ("props", s.props.as_ref())] {
            let Some(p) = p else { continue };
            let resolved = root.join(p);
            let rel = match resolved.canonicalize() {
                Ok(c) => c.strip_prefix(&root).map(Path::to_path_buf).map_err(|_| Error(format!("sources[{i}].{key} “{p}”: outside the art root; can't bundle")))?,
                Err(_) => clean(Path::new(p)).ok_or_else(|| Error(format!("sources[{i}].{key} “{p}”: outside the art root; can't bundle")))?,
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if rel != *p {
                new = jsonedit::set(&new, &[Seg::Key("sources".into()), Seg::Index(i), Seg::Key(key.into())], &json_str(&rel))?;
            }
        }
    }
    new = jsonedit::set(&new, &["art_root".into()], "\"art\"")?;
    let pal_json = format!("[{}]", palettes.iter().map(|p| json_str(p)).collect::<Vec<_>>().join(", "));
    if jsonedit::get(&new, &["palettes".into()]).is_some() || !palettes.is_empty() {
        new = jsonedit::set(&new, &["palettes".into()], &pal_json)?;
    }
    new = jsonedit::set(&new, &["id".into()], &json_str(&id))?;
    if let Some(n) = name {
        new = jsonedit::set(&new, &["name".into()], &json_str(n))?;
    }
    if src.join("README.md").is_file() {
        copies.insert("README.md".into(), src.join("README.md"));
    }
    // stage beside the target, check, move into place
    let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(io(parent))?;
    let stage = parent.join(format!(".bundle-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let res = (|| -> Result<(usize, u64)> {
        let mut bytes = 0;
        for (dest, from) in &copies {
            let d = stage.join(dest);
            std::fs::create_dir_all(d.parent().unwrap()).map_err(io(&d))?;
            bytes += std::fs::copy(from, &d).map_err(io(from))?;
        }
        std::fs::write(stage.join("pack.json"), &new).map_err(io(&stage))?;
        bytes += new.len() as u64;
        // the source's own body problems come along (a bundle changes no behavior); anything
        // else is the copy's fault
        let carried: BTreeSet<String> = pack.body_issues.iter().map(|b| format!("body: {b}")).collect();
        let f: Vec<Finding> = check_as(&stage, &id).into_iter().filter(|x| !(x.path == "pack.json" && carried.contains(&x.problem))).collect();
        if let Some(x) = f.first() {
            return err(format!("the bundled copy isn't standard: {} {}", x.path, x.problem));
        }
        Ok((copies.len() + 1, bytes))
    })();
    match res {
        Ok((files, bytes)) => {
            if out.exists() {
                let _ = std::fs::remove_dir_all(&stage);
                return err(format!("{} appeared while bundling; nothing written", out.display()));
            }
            std::fs::rename(&stage, out).map_err(|e| {
                let _ = std::fs::remove_dir_all(&stage);
                io(out)(e)
            })?;
            Ok(Bundled { dir: out.display().to_string(), id, files, bytes })
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            Err(Error(format!("{e}; nothing written")))
        }
    }
}

/// A new package `lib/<id>` from an `.aseprite` file's bytes (its clips are `art/<id>/`,
/// rest/hold play the first clip, every intent falls back to rest).
pub fn create_from_aseprite(lib: &Path, id: &str, name: &str, bytes: &[u8], label: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return err(format!("id “{id}”: use 1–32 lowercase letters, digits, - and _ (starting with a letter or digit)"));
    }
    if name.trim().is_empty() {
        return err("give the mascot a name");
    }
    let out = lib.join(id);
    if out.exists() {
        return err(format!("a mascot “{id}” already exists"));
    }
    let mut prefix: String = id.chars().map(|c| if c == '-' { '_' } else { c }).collect();
    if !crate::aseprite::valid_name(&prefix) {
        prefix = format!("m_{prefix}");
    }
    std::fs::create_dir_all(lib).map_err(io(lib))?;
    let stage = lib.join(format!(".new-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let res = (|| -> Result<()> {
        crate::aseprite::import_bytes(bytes, label, &stage, &prefix)?;
        let pj = stage.join("pack.json");
        let text = std::fs::read_to_string(&pj).map_err(io(&pj))?;
        let text = jsonedit::set(&text, &["id".into()], &json_str(id))?;
        let text = jsonedit::set(&text, &["name".into()], &json_str(name.trim()))?;
        std::fs::write(&pj, text).map_err(io(&pj))?;
        if let Some(x) = check_as(&stage, id).first() {
            return err(format!("the new mascot isn't standard: {} {}", x.path, x.problem));
        }
        if out.exists() {
            return err(format!("a mascot “{id}” appeared meanwhile"));
        }
        std::fs::rename(&stage, &out).map_err(io(&out))
    })();
    if res.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    res.map(|_| out)
}

/// Delete a package folder (the caller decides it may be deleted). It is first moved aside, so a
/// failure half way never leaves a half-deleted mascot in the library.
pub fn delete(dir: &Path) -> Result<()> {
    let m = std::fs::symlink_metadata(dir).map_err(io(dir))?;
    if !m.is_dir() || !dir.join("pack.json").is_file() {
        return err(format!("{}: not a mascot folder", dir.display()));
    }
    let id = dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let trash = dir.with_file_name(format!(".trash-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&trash);
    std::fs::rename(dir, &trash).map_err(io(dir))?;
    std::fs::remove_dir_all(&trash).map_err(io(&trash))
}

/// One edit of pack.json: `set` a path to a JSON value (written on one line, keys in the given
/// order) or `remove` it. Paths are lists of keys (strings) and indexes (numbers).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Op {
    pub op: String,
    pub path: Vec<serde_json::Value>,
    /// `set`: the value, as JSON text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<String>,
}

fn segs(path: &[serde_json::Value]) -> Result<Vec<Seg>> {
    path.iter()
        .map(|v| match v {
            serde_json::Value::String(s) => Ok(Seg::Key(s.clone())),
            serde_json::Value::Number(n) if n.as_u64().is_some() => Ok(Seg::Index(n.as_u64().unwrap() as usize)),
            _ => err(format!("bad path step {v}")),
        })
        .collect()
}

/// Apply `ops` in order to a pack.json text.
pub fn apply(text: &str, ops: &[Op]) -> Result<String> {
    let mut t = text.to_string();
    for o in ops {
        let p = segs(&o.path)?;
        if p.is_empty() {
            return err("an edit needs a path");
        }
        t = match o.op.as_str() {
            "set" => jsonedit::set(&t, &p, o.json.as_deref().ok_or_else(|| Error("set needs a value".into()))?)?,
            "remove" => jsonedit::remove(&t, &p)?,
            other => return err(format!("unknown edit “{other}”")),
        };
    }
    Ok(t)
}

/// Check `text` as the next pack.json of the package `dir`; the loaded pack when it is fine.
/// Refused: JSON that doesn't parse, a changed id, a manifest off the standard, a pack that
/// doesn't load or draw, new `body` or `persona` problems, and a behavior that would need a clip the pack
/// doesn't have (one that could play before, or a new one).
pub fn validate_edit(dir: &Path, text: &str) -> Result<PackMeta> {
    jsonedit::validate(text).map_err(|e| Error(format!("pack.json: {e}")))?;
    let folder = dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let pf: PackFile = serde_json::from_str(text).map_err(|e| Error(format!("pack.json: {e}")))?;
    if pf.id != folder {
        return err(format!("the id can't change (it is the folder's name, “{folder}”)"));
    }
    if let Some(f) = manifest_findings(text, &folder).first() {
        return err(f.problem.clone());
    }
    let cur = mascot_core::pack::load_report(dir).ok();
    let pack = mascot_core::pack::load_report_text(dir, text).map_err(|e| Error(format!("this would stop the mascot loading: {e}")))?;
    let old_issues: BTreeSet<&String> = cur.iter().flat_map(|c| c.body_issues.iter()).collect();
    if let Some(i) = pack.body_issues.iter().find(|i| !old_issues.contains(i)) {
        return err(format!("body: {i}"));
    }
    // persona issues already read like "persona.presets.x.voice: …"
    let old_persona: BTreeSet<&String> = cur.iter().flat_map(|c| c.persona_issues.iter()).collect();
    if let Some(i) = pack.persona_issues.iter().find(|i| !old_persona.contains(i)) {
        return err(i.clone());
    }
    for b in pack.behaviors.keys() {
        if pack.available(b) {
            continue;
        }
        let was_ok = cur.as_ref().map(|c| !c.behaviors.contains_key(b) || c.available(b)).unwrap_or(true);
        if was_ok {
            let clip = pack.missing_clip(b).unwrap_or("?");
            return err(format!("behavior “{b}” would need clip “{clip}”, which this mascot doesn't have"));
        }
    }
    mascot_render::Art::load(&pack).map_err(|e| Error(format!("this would stop the mascot drawing: {e}")))?;
    Ok(pack)
}

/// Write `text` as `dir/pack.json` after `validate_edit`, atomically (temp file + rename).
pub fn save(dir: &Path, text: &str) -> Result<PackMeta> {
    let pack = validate_edit(dir, text)?;
    write_atomic(&dir.join("pack.json"), text.as_bytes())?;
    Ok(pack)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_file_name(format!(".{}.tmp-{}", path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), std::process::id()));
    std::fs::write(&tmp, bytes).map_err(io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(path)(e)
    })
}

/// A clip group: one folder under `art/` and the sources that read it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    /// Indexes into pack.json `sources`.
    pub sources: Vec<usize>,
    pub clips: Vec<String>,
    pub sets: Vec<String>,
    /// What plays its clips: `behavior rest`, `station desk`, `turning`.
    pub used_by: Vec<String>,
    pub files: usize,
    pub bytes: u64,
}

/// The package's clip groups (the first folder of each source path), with what uses them.
pub fn groups(dir: &Path, pack: &PackMeta) -> Result<Vec<Group>> {
    let pj = dir.join("pack.json");
    let text = std::fs::read_to_string(&pj).map_err(io(&pj))?;
    let pf: PackFile = serde_json::from_str(&text).map_err(|e| Error(format!("{}: {e}", pj.display())))?;
    let root = dir.join(&pf.art_root);
    let mut by: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, s) in pf.sources.iter().enumerate() {
        let g = Path::new(&s.path).components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        by.entry(g).or_default().push(i);
    }
    let mut out = vec![];
    for (name, sources) in by {
        let gdir = root.join(&name);
        let under = |p: &Path| p.starts_with(&gdir);
        let clips: Vec<String> = pack.clips.values().filter(|c| c.dirs.values().map(|s| &s.file).chain(c.oris.values().map(|s| &s.file)).any(|f| under(f))).map(|c| c.name.clone()).collect();
        let sets: Vec<String> = pack.sets.values().filter(|s| s.oris.values().any(|o| under(&o.props_frame))).map(|s| s.name.clone()).collect();
        let uses = |b: &mascot_protocol::pack::BehaviorDef| b.seq.iter().any(|s| clips.contains(&s.clip)) || b.set.as_ref().is_some_and(|s| sets.contains(s));
        let mut used_by: Vec<String> = pack.behaviors.iter().filter(|(_, b)| uses(b)).map(|(n, _)| format!("behavior {n}")).collect();
        used_by.extend(pack.stations.iter().filter(|(_, s)| uses(&s.behavior)).map(|(n, _)| format!("station {n}")));
        if pack.turning.as_ref().is_some_and(|t| clips.contains(&t.ccw) || clips.contains(&t.cw)) {
            used_by.push("turning".into());
        }
        // the face layers are drawn over every clip
        if sources.iter().any(|i| matches!(pf.sources[*i].kind, mascot_protocol::pack::SourceKind::Face | mascot_protocol::pack::SourceKind::FacePart)) {
            used_by.push("face layers".into());
        }
        let (files, bytes) = size(&gdir);
        out.push(Group { name, sources, clips, sets, used_by, files, bytes });
    }
    Ok(out)
}

/// Remove a clip group nothing uses: its sources leave pack.json (checked like any edit), then
/// its folder goes.
pub fn remove_group(dir: &Path, group: &str) -> Result<()> {
    if group.is_empty() || group.starts_with('.') || Path::new(group).components().count() != 1 || clean(Path::new(group)).is_none() {
        return err(format!("no clip group “{group}”"));
    }
    let pack = mascot_core::pack::load_report(dir)?;
    let g = groups(dir, &pack)?.into_iter().find(|g| g.name == group).ok_or_else(|| Error(format!("no clip group “{group}”")))?;
    if !g.used_by.is_empty() {
        return err(format!("“{group}” is still used by {}; change those first", g.used_by.join(", ")));
    }
    let pj = dir.join("pack.json");
    let mut text = std::fs::read_to_string(&pj).map_err(io(&pj))?;
    for i in g.sources.iter().rev() {
        text = jsonedit::remove(&text, &["sources".into(), Seg::Index(*i)])?;
    }
    // move the folder aside first: the save is checked without it (a source that still reads it
    // fails), and a refused save puts it back as it was
    let gdir = dir.join("art").join(group);
    let aside = dir.join("art").join(format!(".removing-{group}-{}", std::process::id()));
    let moved = gdir.is_dir();
    if moved {
        let _ = std::fs::remove_dir_all(&aside);
        std::fs::rename(&gdir, &aside).map_err(io(&gdir))?;
    }
    if let Err(e) = save(dir, &text) {
        if moved {
            let _ = std::fs::rename(&aside, &gdir);
        }
        return Err(e);
    }
    if moved {
        std::fs::remove_dir_all(&aside).map_err(io(&aside))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        for ok in ["bear", "sea-otter", "a", "x_1", "0x"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "Bear", "-a", "_a", "a b", "a/b", "..", &"a".repeat(33)] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert_eq!(slug("Sea Otter!"), "sea-otter");
        assert_eq!(slug("  ¡¡ "), "mascot");
        assert_eq!(slug("Ünï 2"), "n-2");
    }

    #[test]
    fn manifest_rules() {
        let ok = r#"{"pack_version": 1, "id": "m", "name": "M", "art_root": "art", "palettes": ["palettes/p.json"], "sources": [{"kind": "stage_a", "path": "m/anim.json"}], "behaviors": {}, "intents": {}, "idle_pool": []}"#;
        assert_eq!(manifest_findings(ok, "m"), vec![]);
        let bad = ok.replace("\"art\"", "\"../x\"").replace("palettes/p.json", "/etc/p.json").replace("m/anim.json", "../../anim.json");
        let f = manifest_findings(&bad, "other");
        let all = f.iter().map(|f| f.problem.clone()).collect::<Vec<_>>().join("\n");
        assert!(all.contains("folder's name"), "{all}");
        assert!(all.contains("art_root"), "{all}");
        assert!(all.contains("palettes[0]"), "{all}");
        assert!(all.contains("sources[0].path"), "{all}");
        assert!(manifest_findings("{", "m")[0].problem.contains("parse"));
    }

    #[test]
    fn ops_apply_in_order() {
        let t = "{\n  \"id\": \"m\",\n  \"name\": \"M\"\n}\n";
        let ops: Vec<Op> = serde_json::from_str(r#"[{"op": "set", "path": ["description"], "json": "\"hi\""}, {"op": "remove", "path": ["name"]}]"#).unwrap();
        assert_eq!(apply(t, &ops).unwrap(), "{\n  \"id\": \"m\",\n  \"description\": \"hi\"\n}\n");
        assert!(apply(t, &[Op { op: "zap".into(), path: vec!["id".into()], json: None }]).is_err());
        assert!(apply(t, &[Op { op: "set".into(), path: vec![], json: Some("1".into()) }]).is_err());
        assert!(apply(t, &[Op { op: "set".into(), path: vec![serde_json::json!(-1)], json: Some("1".into()) }]).is_err());
    }
}
