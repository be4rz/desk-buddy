//! Test helper: a pack copied into a temp dir with absolute art and palette paths, edited, then
//! loaded (one copy helper for every brain test module).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::pack::{self, PackError, PackMeta};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Removes the copy's dir on drop (also when the test panics).
pub struct TempPack(pub PathBuf);

impl Drop for TempPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The temp dir `copy` uses for `tag`.
pub fn dir_for(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mascot-core-pack-{}-{tag}", std::process::id()))
}

/// `src`'s pack.json (a repo-relative dir) with `art_root` and `palettes` made absolute, edited
/// by `edit` (given the temp dir, for files beside it) and written there; then loaded.
pub fn copy(src: &str, tag: &str, edit: impl FnOnce(&mut Value, &Path)) -> (TempPack, Result<PackMeta, PackError>) {
    let src = repo().join(src).canonicalize().unwrap();
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(src.join("pack.json")).unwrap()).unwrap();
    let abs = |s: &Value| json!(src.join(s.as_str().unwrap()));
    v["art_root"] = abs(&v["art_root"]);
    v["palettes"] = Value::Array(v["palettes"].as_array().unwrap().iter().map(abs).collect());
    let dir = dir_for(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let guard = TempPack(dir.clone());
    edit(&mut v, &dir);
    std::fs::write(dir.join("pack.json"), v.to_string()).unwrap();
    let r = pack::load(&dir);
    (guard, r)
}
