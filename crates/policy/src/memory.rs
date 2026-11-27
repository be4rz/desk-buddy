//! What the agent may do about the memory store (design I3): its folder is private to the app,
//! and while the conversation is marked by the web, changing memory asks every time.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{rules, AgentCtx};

/// The deny reason for a tool or shell word that reaches a private dir.
pub(crate) const PRIVATE: &str = "That folder holds what the bear remembers about the person: reach it only with buddy memory.";

/// The nearest existing ancestor canonicalized, plus the rest: the spelling `rules::target` gives a path.
fn resolved(p: &Path) -> PathBuf {
    let mut base = p.to_path_buf();
    let mut rest = vec![];
    loop {
        if let Ok(c) = base.canonicalize() {
            return rest.iter().rev().fold(c, |out, r| out.join(r));
        }
        match (base.file_name().map(|n| n.to_os_string()), base.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                base = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

impl AgentCtx {
    /// Whether `path` (from the work folder) is a private dir or inside one, case-folded where the file system is.
    pub(crate) fn is_private(&self, path: &str) -> bool {
        if self.private_dirs.is_empty() {
            return false;
        }
        let abs = rules::folded(&rules::target(&self.work_dir, self.home.as_deref(), path).abs);
        self.private_dirs.iter().any(|d| abs.starts_with(rules::folded(&resolved(d))))
    }
}

/// `buddy memory remember|forget …` anywhere in `command`: the same token scan as `outbound`
/// (quotes, backslashes, flags, `$(…)`, words built at run time), so it errs toward asking.
pub(crate) fn memory_write(command: &str) -> bool {
    const WRITES: [&str; 2] = ["remember", "forget"];
    let plain: String = command.chars().filter(|c| !matches!(c, '\'' | '"' | '\\')).collect();
    let w: Vec<&str> = plain.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).filter(|w| !w.is_empty() && !w.starts_with('-')).collect();
    let seq = |want: &[&[&str]]| {
        let mut it = w.iter();
        want.iter().all(|alts| it.any(|x| alts.contains(x)))
    };
    if seq(&[&["buddy"], &["memory"], &WRITES]) {
        return true;
    }
    let built = plain.contains('$') || plain.contains('`') || w.contains(&"eval");
    built && w.iter().any(|x| *x == "memory" || WRITES.contains(x))
}

/// The card for a memory write while the conversation is marked: no "Always".
pub(crate) fn write_card(command: &str) -> Value {
    json!({
        "kind": "command",
        "tool": "bash",
        "title": "Let the bear change what it remembers?",
        "detail": "This conversation read a web page, so changing memory asks every time.",
        "command": command,
        "suggestedRule": null,
        "offerAlways": false,
    })
}
