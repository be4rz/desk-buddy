//! The studio's open pack with hot reload: `poll` re-reads the pack when any file it uses
//! changed (size or mtime of pack.json, the source JSONs, every strip, props frame and face
//! patch). A reload that fails keeps the last good pack and reports the error; until one loads
//! again the folders holding those files count too, so a strip that was missing and is then
//! added triggers the next try.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use mascot_core::PackMeta;
use mascot_protocol::pack::PackFile;
use mascot_render::Art;

use crate::{Error, Result};

pub struct Session {
    pub dir: PathBuf,
    pub pack: Arc<PackMeta>,
    pub art: Art,
    sig: u64,
    /// The last reload's error (the last good pack is still shown).
    pub error: Option<String>,
    /// How many times the pack (re)loaded fine.
    pub loads: u64,
}

/// Load a pack and its art.
pub fn load(dir: &Path) -> Result<(Arc<PackMeta>, Art)> {
    let pack = Arc::new(mascot_core::pack::load(dir)?);
    let art = Art::load(&pack)?;
    Ok((pack, art))
}

/// Every file the pack reads; from pack.json itself when it parses, plus the loaded pack's.
pub fn files(dir: &Path, pack: Option<&PackMeta>) -> BTreeSet<PathBuf> {
    let mut v = BTreeSet::new();
    let pj = dir.join("pack.json");
    v.insert(pj.clone());
    if let Some(pf) = std::fs::read_to_string(&pj).ok().and_then(|t| serde_json::from_str::<PackFile>(&t).ok()) {
        let root = dir.join(&pf.art_root);
        for s in &pf.sources {
            v.insert(root.join(&s.path));
            if let Some(p) = &s.props {
                v.insert(root.join(p));
            }
        }
        for p in &pf.palettes {
            v.insert(dir.join(p));
        }
    }
    if let Some(p) = pack {
        for c in p.clips.values() {
            v.extend(c.dirs.values().map(|s| s.file.clone()));
            v.extend(c.oris.values().map(|s| s.file.clone()));
        }
        for s in p.sets.values() {
            for o in s.oris.values() {
                v.insert(o.props_frame.clone());
                v.extend(o.props.iter().map(|p| p.file.clone()));
            }
        }
        if let Some(f) = &p.face {
            for st in f.layers.values() {
                for d in st.values() {
                    v.extend(d.values().map(|l| l.file.clone()));
                }
            }
        }
    }
    v
}

/// A cheap fingerprint of `files`: path, size, mtime (absent files count too).
pub fn signature(files: &BTreeSet<PathBuf>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in files {
        f.hash(&mut h);
        match std::fs::metadata(f) {
            Ok(m) => {
                m.len().hash(&mut h);
                m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).hash(&mut h);
            }
            Err(_) => 0u8.hash(&mut h),
        }
    }
    h.finish()
}

/// What a poll found.
#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    Same,
    Reloaded,
    /// The pack changed but doesn't load; the last good one stays.
    Failed,
}

impl Session {
    pub fn open(dir: &Path) -> Result<Session> {
        if !dir.join("pack.json").is_file() {
            return Err(Error(format!("{}: no pack.json here", dir.display())));
        }
        let (pack, art) = load(dir)?;
        let mut s = Session { dir: dir.to_path_buf(), pack, art, sig: 0, error: None, loads: 1 };
        s.sig = s.signature();
        Ok(s)
    }

    /// The fingerprint polls compare: the pack's files, plus their folders while a reload is
    /// failing (adding a file changes its folder's mtime).
    fn signature(&self) -> u64 {
        let mut f = files(&self.dir, Some(&self.pack));
        if self.error.is_some() {
            let dirs: Vec<PathBuf> = f.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).collect();
            f.extend(dirs);
        }
        signature(&f)
    }

    /// Reload when a file changed. Called by the studio window every ≤ 1 s while it is open.
    pub fn poll(&mut self) -> Poll {
        if self.signature() == self.sig {
            return Poll::Same;
        }
        let r = match load(&self.dir) {
            Ok((pack, art)) => {
                self.pack = pack;
                self.art = art;
                self.error = None;
                self.loads += 1;
                Poll::Reloaded
            }
            Err(e) => {
                self.error = Some(e.0);
                Poll::Failed
            }
        };
        // the new pack (or the failing state) may watch other files: fingerprint those
        self.sig = self.signature();
        r
    }
}
