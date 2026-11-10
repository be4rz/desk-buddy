//! Review comments (§4.10): `<pack>/review/comments.jsonl`, one object per line. The file's
//! path always comes from the open pack's dir; callers give only the comment's fields, which
//! are checked against the pack (clip, view, frame, region inside the frame).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mascot_core::PackMeta;
use serde::{Deserialize, Serialize};

use crate::{err, Error, Result};

pub const MAX_TEXT: usize = 2000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Comment {
    pub v: u32,
    pub id: String,
    pub clip: String,
    pub dir: String,
    pub frame: u32,
    pub t_ms: u64,
    pub region: Option<[u32; 4]>,
    pub text: String,
    pub author: String,
    pub at: String,
    pub status: String,
}

/// What the studio window or a caller gives for a new comment.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NewComment {
    pub clip: String,
    pub dir: String,
    pub frame: u32,
    pub t_ms: u64,
    #[serde(default)]
    pub region: Option<[u32; 4]>,
    pub text: String,
}

pub fn file(pack_dir: &Path) -> PathBuf {
    pack_dir.join("review").join("comments.jsonl")
}

/// Every comment of the pack (none when the file doesn't exist). Unreadable lines are errors
/// naming the line, so a hand edit gone wrong is seen, not dropped.
pub fn read(pack_dir: &Path) -> Result<Vec<Comment>> {
    let f = file(pack_dir);
    let text = match std::fs::read_to_string(&f) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return err(format!("{}: {e}", f.display())),
    };
    let mut out = vec![];
    for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        out.push(serde_json::from_str(line).map_err(|e| Error(format!("{}:{}: {e}", f.display(), n + 1)))?);
    }
    Ok(out)
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn new_id(c: &NewComment, now: u64) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (now, std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed), &c.text, &c.clip, c.frame).hash(&mut h);
    format!("c_{:08x}", h.finish() as u32)
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 10 && id.starts_with("c_") && id[2..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Check a new comment against the pack; the checked comment (trimmed text).
pub fn check(pack: &PackMeta, c: &NewComment) -> Result<NewComment> {
    let Some(clip) = pack.clips.get(&c.clip) else { return err(format!("no clip “{}” in pack {}", c.clip, pack.id)) };
    if !crate::frames::views(pack, &c.clip).contains(&c.dir) {
        return err(format!("clip “{}” has no view “{}”", c.clip, c.dir));
    }
    if c.frame as usize >= clip.durs_ms.len() {
        return err(format!("clip “{}” has {} frames", c.clip, clip.durs_ms.len()));
    }
    let text = c.text.trim();
    if text.is_empty() {
        return err("a comment needs some text");
    }
    if text.chars().count() > MAX_TEXT {
        return err(format!("a comment is at most {MAX_TEXT} characters"));
    }
    if let Some([x, y, w, h]) = c.region {
        let (fw, fh) = match (&clip.set, crate::frames::views(pack, &c.clip).first()) {
            (Some(s), _) => pack.sets.get(s).and_then(|s| s.oris.values().next()).map_or((0, 0), |o| (o.frame_w, o.frame_h)),
            _ => (clip.frame_w, clip.frame_h),
        };
        if w == 0 || h == 0 || x.saturating_add(w) > fw || y.saturating_add(h) > fh {
            return err(format!("region {x},{y} {w}x{h} is outside the {fw}x{fh} frame"));
        }
    }
    Ok(NewComment { text: text.to_string(), ..c.clone() })
}

/// Append a comment (status `open`, author `user`); `now` = seconds since the epoch.
pub fn add(pack: &PackMeta, c: &NewComment, now: u64) -> Result<Comment> {
    let c = check(pack, c)?;
    let out = Comment {
        v: 1,
        id: new_id(&c, now),
        clip: c.clip,
        dir: c.dir,
        frame: c.frame,
        t_ms: c.t_ms,
        region: c.region,
        text: c.text,
        author: "user".into(),
        at: crate::rfc3339(now),
        status: "open".into(),
    };
    let f = file(&pack.dir);
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| Error(format!("{}: {e}", f.display())))?;
    let line = serde_json::to_string(&out).expect("comment serializes") + "\n";
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&f)
        .and_then(|mut h| h.write_all(line.as_bytes()))
        .map_err(|e| Error(format!("{}: {e}", f.display())))?;
    Ok(out)
}

/// Set a comment's status (`open` | `resolved`): the file is rewritten through a temp file.
pub fn set_status(pack_dir: &Path, id: &str, status: &str) -> Result<Comment> {
    if !valid_id(id) {
        return err(format!("“{id}” is not a comment id"));
    }
    if status != "open" && status != "resolved" {
        return err("status is open or resolved");
    }
    let mut all = read(pack_dir)?;
    let Some(c) = all.iter_mut().find(|c| c.id == id) else { return err(format!("no comment {id}")) };
    c.status = status.into();
    let changed = c.clone();
    let f = file(pack_dir);
    let tmp = f.with_extension("jsonl.tmp");
    let body: String = all.iter().map(|c| serde_json::to_string(c).expect("comment serializes") + "\n").collect();
    std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &f)).map_err(|e| Error(format!("{}: {e}", f.display())))?;
    Ok(changed)
}
