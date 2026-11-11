//! `lint --pack <dir>` (§4.10 codes). It reads every strip, props frame and face patch itself
//! (not through `Art::load`), so a bad file is an issue, not a crash. Severity is additive to
//! the frozen issue shape: only `error`s make the command exit 1.
//!
//! - `off_palette`: an opaque pixel whose colour is in none of the pack's `palettes` files
//!   (every `#rrggbb` string in them); skipped when the pack lists no palette (one warning).
//! - `partial_alpha`: alpha other than 0 or 255.
//! - `stray_pixel`: an opaque pixel with no non-transparent 8-neighbour inside its frame.
//! - `non_integer_root` (warn): a clip/dir whose root motion has fractions (the brain keeps
//!   the fraction, so this is allowed; it is listed so a reviewer knows).
//! - `missing_dir`: a behavior's free clip lacks a direction `idle` has (error without `S`). The
//!   listen behavior's clips need only the front dirs (the brain turns the bear first, §1.4).
//! - `turn_clips_missing` (warn): the pack's `turning` block names a clip it doesn't have, or one
//!   without every direction `idle` has; those facing changes snap.
//! - `frame_count`: a strip isn't `frames_n` frames of `frame_w x frame_h`.
//! - `bad_anchor`: an anchor outside its frame.
//! - `missing_face_pos`: face positions whose count isn't the clip's frame count, or a face
//!   clip/dir/slot without a layer patch.
//! - `station_art_missing` (warn): pack.json declares a station (`stations`) whose set art isn't
//!   loaded (an optional source not rendered yet): the bear thinks and answers standing, and the
//!   station's activities aren't offered. Its `work` block then does nothing either.
//! - Set faces (face v2 `set_clips`) are checked like `face.clips`: `missing_face_pos` when a count
//!   isn't the set clip's frame count or the patch dir has no layer for a slot.
//! - `bad_body`: a problem in pack.json's `body` block (activities and emotes), one per entry, from
//!   `pack::load_report` (the app refuses such a pack).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mascot_core::{Dir, Intent, PackMeta};
use serde::Serialize;

/// Issues of one code reported per file at most; the rest are summed in one more line.
const PER_FILE: usize = 20;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Issue {
    pub code: &'static str,
    pub severity: &'static str,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<usize>,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Report {
    pub pack: String,
    pub files: usize,
    pub errors: usize,
    pub warnings: usize,
    pub issues: Vec<Issue>,
}

impl Report {
    fn push(&mut self, i: Issue) {
        if i.severity == "error" {
            self.errors += 1;
        } else {
            self.warnings += 1;
        }
        self.issues.push(i);
    }
}

fn issue(code: &'static str, severity: &'static str, file: &str, detail: String) -> Issue {
    Issue { code, severity, file: file.into(), frame: None, x: None, y: None, detail }
}

/// Every `#rrggbb` in a palette JSON (any nesting), as RGB.
pub fn palette_colors(v: &serde_json::Value, out: &mut BTreeSet<[u8; 3]>) {
    match v {
        serde_json::Value::String(s) => {
            let h = s.trim();
            if h.len() == 7 && h.starts_with('#') {
                if let Ok(n) = u32::from_str_radix(&h[1..], 16) {
                    out.insert([(n >> 16) as u8, (n >> 8) as u8, n as u8]);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| palette_colors(x, out)),
        serde_json::Value::Object(o) => o.iter().filter(|(k, _)| k.as_str() != "note").for_each(|(_, x)| palette_colors(x, out)),
        _ => {}
    }
}

/// A file's name for reports: relative to the pack's art root when under it.
fn name(pack: &PackMeta, p: &Path) -> String {
    p.strip_prefix(&pack.art_root).or_else(|_| p.strip_prefix(&pack.dir)).unwrap_or(p).display().to_string()
}

/// Pixel checks over the frames of one image: `frames` frames `fw` wide side by side.
pub fn check_pixels(file: &str, w: usize, h: usize, rgba: &[u8], fw: usize, palette: Option<&BTreeSet<[u8; 3]>>, r: &mut Report) {
    let fw = fw.clamp(1, w.max(1));
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut add = |r: &mut Report, code: &'static str, x: usize, y: usize, detail: String| {
        let n = counts.entry(code).or_default();
        *n += 1;
        if *n <= PER_FILE {
            r.push(Issue { code, severity: "error", file: file.into(), frame: Some(x / fw), x: Some(x % fw), y: Some(y), detail });
        }
    };
    let a = |x: usize, y: usize| rgba[(y * w + x) * 4 + 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let al = rgba[i + 3];
            if al == 0 {
                continue;
            }
            if al != 255 {
                add(r, "partial_alpha", x, y, format!("alpha {al}"));
                continue;
            }
            if let Some(p) = palette {
                let c = [rgba[i], rgba[i + 1], rgba[i + 2]];
                if !p.contains(&c) {
                    add(r, "off_palette", x, y, format!("#{:02x}{:02x}{:02x} is not in the pack palettes", c[0], c[1], c[2]));
                }
            }
            // stray: no non-transparent neighbour within the same frame
            let (f0, f1) = (x / fw * fw, (x / fw * fw + fw).min(w));
            let mut lonely = true;
            'n: for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < f0 as i32 || nx >= f1 as i32 || ny < 0 || ny >= h as i32 {
                        continue;
                    }
                    if a(nx as usize, ny as usize) > 0 {
                        lonely = false;
                        break 'n;
                    }
                }
            }
            if lonely {
                add(r, "stray_pixel", x, y, "opaque pixel with no neighbour".into());
            }
        }
    }
    for (code, n) in counts {
        if n > PER_FILE {
            r.push(issue(code, "error", file, format!("… and {} more", n - PER_FILE)));
        }
    }
}

/// Lint a loaded pack.
pub fn lint(pack: &PackMeta) -> Report {
    let mut r = Report { pack: pack.id.clone(), ..Default::default() };
    for b in &pack.body_issues {
        r.push(issue("bad_body", "error", "pack.json", b.clone()));
    }
    // palettes
    let mut pal = BTreeSet::new();
    for p in &pack.palettes {
        match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string())) {
            Ok(v) => palette_colors(&v, &mut pal),
            Err(e) => r.push(issue("off_palette", "error", &name(pack, p), format!("palette unreadable: {e}"))),
        }
    }
    let palette = if pack.palettes.is_empty() {
        r.push(issue("off_palette", "warn", "pack.json", "the pack lists no palettes; colours not checked".into()));
        None
    } else {
        Some(&pal)
    };
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut image = |r: &mut Report, path: &Path, fw: usize, fh: usize, n: usize| {
        let file = name(pack, path);
        let fresh = seen.insert(path.to_path_buf());
        match mascot_render::read_png(path) {
            Ok((w, h, px)) => {
                if fresh {
                    r.files += 1;
                    check_pixels(&file, w, h, &px, fw, palette, r);
                }
                if h != fh || w != fw * n {
                    r.push(issue("frame_count", "error", &file, format!("{w}x{h} is not {n} frames of {fw}x{fh}")));
                }
            }
            Err(e) => r.push(issue("frame_count", "error", &file, format!("unreadable: {e}"))),
        }
    };
    for (cname, c) in &pack.clips {
        let n = c.durs_ms.len();
        let (fw, fh) = (c.frame_w as usize, c.frame_h as usize);
        if c.anchor[0] < 0 || c.anchor[1] < 0 || c.anchor[0] > c.frame_w as i32 || c.anchor[1] > c.frame_h as i32 {
            r.push(issue("bad_anchor", "error", cname, format!("anchor {:?} is outside the {fw}x{fh} frame", c.anchor)));
        }
        for (d, s) in &c.dirs {
            image(&mut r, &s.file, fw, fh, n);
            if s.root_px.len() != n {
                r.push(issue("frame_count", "error", &name(pack, &s.file), format!("{} root_px entries for {n} frames", s.root_px.len())));
            }
            if s.root_px.iter().flatten().any(|v| v.fract() != 0.0 || !v.is_finite()) {
                r.push(issue("non_integer_root", "warn", &name(pack, &s.file), format!("{cname} {}: root motion has sub-pixel steps", d.as_str())));
            }
        }
        for s in c.oris.values() {
            image(&mut r, &s.file, fw, fh, n);
        }
    }
    for (sname, set) in &pack.sets {
        for (o, so) in &set.oris {
            if so.anchor[0] < 0 || so.anchor[1] < 0 || so.anchor[0] > so.frame_w as i32 || so.anchor[1] > so.frame_h as i32 {
                r.push(issue("bad_anchor", "error", sname, format!("set {sname} {}: anchor {:?} outside {}x{}", crate::frames::ori_str(*o), so.anchor, so.frame_w, so.frame_h)));
            }
            image(&mut r, &so.props_frame, so.frame_w as usize, so.frame_h as usize, 1);
            for p in &so.props {
                image(&mut r, &p.file, p.size[0] as usize, p.size[1] as usize, 1);
            }
        }
    }
    // stations: declared, but their set's art not loaded (the loader checked the rest)
    for (sname, st) in &pack.stations {
        if !pack.available(sname) {
            let mut d = format!("station {sname}: set {} isn't loaded (an optional source not rendered yet?); turns and answers stay standing", st.file.set);
            if pack.work.as_ref().is_some_and(|w| &w.station == sname) {
                d.push_str(", and `work` does nothing");
            }
            r.push(issue("station_art_missing", "warn", "pack.json", d));
        }
    }
    // a behavior's free clips in every direction idle has
    let idle_dirs: Vec<Dir> = pack.clips.get("idle").map(|c| c.dirs.keys().copied().collect()).unwrap_or_else(|| vec![Dir::S]);
    let mut used = BTreeSet::new();
    for b in pack.behaviors.values() {
        for st in &b.seq {
            used.insert(st.clip.clone());
        }
    }
    // listen plays facing the person: back views turn (or snap) to the front first
    // (a pack without a listen behavior resolves listen to `rest`: no exemption then)
    let lb = pack.resolve(Intent::Listen);
    let listen: BTreeSet<&str> = if lb == pack.resolve(Intent::Idle) {
        BTreeSet::new()
    } else {
        pack.behaviors.get(lb).map(|b| b.seq.iter().map(|s| s.clip.as_str()).collect()).unwrap_or_default()
    };
    for cname in &used {
        let Some(c) = pack.clips.get(cname) else { continue };
        if c.set.is_some() {
            continue;
        }
        let back = |d: &Dir| listen.contains(cname.as_str()) && matches!(d, Dir::NE | Dir::N | Dir::NW);
        let missing: Vec<&str> = idle_dirs.iter().filter(|d| !c.dirs.contains_key(d) && !back(d)).map(|d| d.as_str()).collect();
        if !missing.is_empty() {
            let sev = if c.dirs.contains_key(&Dir::S) { "warn" } else { "error" };
            r.push(issue("missing_dir", sev, cname, format!("no {}", missing.join(", "))));
        }
    }
    if let Some(t) = &pack.turning {
        for (role, name) in [("ccw", &t.ccw), ("cw", &t.cw)] {
            match pack.clips.get(name) {
                None => r.push(issue("turn_clips_missing", "warn", name, format!("turning {role}: no clip {name}; facing changes snap"))),
                Some(c) => {
                    let missing: Vec<&str> = idle_dirs.iter().filter(|d| !c.dirs.contains_key(d)).map(|d| d.as_str()).collect();
                    if !missing.is_empty() {
                        r.push(issue("turn_clips_missing", "warn", name, format!("turning {role}: no {}; turns from there snap", missing.join(", "))));
                    }
                }
            }
        }
    }
    if let Some(face) = &pack.face {
        for states in face.layers.values() {
            for dirs in states.values() {
                for l in dirs.values() {
                    image(&mut r, &l.file, l.w as usize, l.h as usize, 1);
                }
            }
        }
        for (cname, dirs) in &face.clips {
            let n = pack.clips.get(cname).map(|c| c.durs_ms.len());
            for (d, slots) in dirs {
                for (slot, pos) in slots {
                    let what = format!("{cname} {} {slot}", d.as_str());
                    match n {
                        None => r.push(issue("missing_face_pos", "error", "face/face.json", format!("{what}: no such clip"))),
                        Some(n) if pos.len() != n => r.push(issue("missing_face_pos", "error", "face/face.json", format!("{what}: {} positions for {n} frames", pos.len()))),
                        _ => {}
                    }
                    // v2: the clip may draw another dir's patches (patch_dirs)
                    let pd = face.patch_dir(cname, *d);
                    let has_layer = face.layers.get(slot).is_some_and(|st| st.values().any(|m| m.contains_key(&pd)));
                    if !has_layer {
                        r.push(issue("missing_face_pos", "error", "face/face.json", format!("{what}: no {slot} patch for {}", pd.as_str())));
                    }
                }
            }
        }
        for (cname, oris) in &face.set_clips {
            let n = pack.clips.get(cname).map(|c| c.durs_ms.len());
            for (o, sf) in oris {
                for (slot, pos) in &sf.slots {
                    let what = format!("set_clips {cname} {} {slot}", crate::frames::ori_str(*o));
                    match n {
                        None => r.push(issue("missing_face_pos", "error", "face/face.json", format!("{what}: no such clip"))),
                        Some(n) if pos.len() != n => r.push(issue("missing_face_pos", "error", "face/face.json", format!("{what}: {} positions for {n} frames", pos.len()))),
                        _ => {}
                    }
                    if !face.layers.get(slot).is_some_and(|st| st.values().any(|m| m.contains_key(&sf.patch))) {
                        r.push(issue("missing_face_pos", "error", "face/face.json", format!("{what}: no {slot} patch for {}", sf.patch.as_str())));
                    }
                }
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(w: usize, h: usize, set: &[(usize, usize, [u8; 4])]) -> Vec<u8> {
        let mut v = vec![0u8; w * h * 4];
        for &(x, y, c) in set {
            v[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&c);
        }
        v
    }

    #[test]
    fn body_problems_are_errors() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots/bear");
        let mut pack = mascot_core::pack::load_report(&dir).unwrap();
        assert!(pack.body_issues.is_empty(), "{:?}", pack.body_issues);
        assert!(!lint(&pack).issues.iter().any(|i| i.code == "bad_body"));
        pack.body_issues.push("body: activities.juggle: unknown behavior \"juggle\"".into());
        let r = lint(&pack);
        let bad: Vec<_> = r.issues.iter().filter(|i| i.code == "bad_body").map(|i| (i.severity, i.file.as_str(), i.detail.as_str())).collect();
        assert_eq!(bad, vec![("error", "pack.json", "body: activities.juggle: unknown behavior \"juggle\"")]);
        assert!(r.errors >= 1);
    }

    /// p3 desk: the bear plays its desk (no warning); the bear without the `anim_d` source declares
    /// a desk it can't play (warn); the desk fixture plays it; a set face whose count is off is an error.
    #[test]
    fn stations_without_art_warn_and_set_faces_are_counted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bear = mascot_core::pack::load_report(&root.join("mascots/bear")).unwrap();
        assert!(bear.available("desk"));
        assert!(!lint(&bear).issues.iter().any(|i| i.code == "station_art_missing"));
        // the same pack over an art folder without anim_d/ (the optional source missing: links to the rest)
        let bear_dir = root.join("mascots/bear");
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(bear_dir.join("pack.json")).unwrap()).unwrap();
        let art = bear_dir.join(v["art_root"].as_str().unwrap());
        for p in v["palettes"].as_array_mut().unwrap() {
            *p = bear_dir.join(p.as_str().unwrap()).to_string_lossy().into();
        }
        let tmp = std::env::temp_dir().join(format!("lint-no-anim-d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("art")).unwrap();
        for e in std::fs::read_dir(&art).unwrap() {
            let e = e.unwrap();
            if e.file_name() != "anim_d" {
                std::os::unix::fs::symlink(e.path(), tmp.join("art").join(e.file_name())).unwrap();
            }
        }
        v["art_root"] = "art".into();
        std::fs::write(tmp.join("pack.json"), serde_json::to_string(&v).unwrap()).unwrap();
        let bear = mascot_core::pack::load_report(&tmp).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(!bear.available("desk"));
        let r = lint(&bear);
        let st: Vec<_> = r.issues.iter().filter(|i| i.code == "station_art_missing").map(|i| (i.severity, i.detail.as_str())).collect();
        assert_eq!(st.len(), 1, "{st:?}");
        assert_eq!(st[0].0, "warn");
        assert!(st[0].1.starts_with("station desk: set desk isn't loaded") && st[0].1.ends_with("`work` does nothing"), "{}", st[0].1);
        let mut desk = mascot_core::pack::load_report(&root.join("crates/mascot-core/tests/fixtures/desk_pack")).unwrap();
        let r = lint(&desk);
        assert!(!r.issues.iter().any(|i| i.code == "station_art_missing" || i.code == "missing_face_pos"), "{:?}", r.issues);
        assert_eq!(r.errors, 0, "{:?}", r.issues.iter().filter(|i| i.severity == "error").collect::<Vec<_>>());
        let face = desk.face.as_mut().unwrap();
        face.set_clips.get_mut("desk_talk").unwrap().get_mut(&mascot_core::Ori::A).unwrap().slots.get_mut("mouth").unwrap().pop();
        let r = lint(&desk);
        let bad: Vec<_> = r.issues.iter().filter(|i| i.code == "missing_face_pos").map(|i| i.detail.as_str()).collect();
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].starts_with("set_clips desk_talk A mouth: 15 positions for 16 frames"), "{}", bad[0]);
    }

    #[test]
    fn pixel_checks() {
        let pal: BTreeSet<[u8; 3]> = [[10, 10, 10]].into();
        let ok = [10, 10, 10, 255];
        // a 2x2 block (no strays), one lonely pixel, one off-palette, one partial
        let img = px(8, 4, &[(0, 0, ok), (1, 0, ok), (0, 1, [200, 0, 0, 255]), (1, 1, [10, 10, 10, 128]), (5, 2, ok)]);
        let mut r = Report::default();
        check_pixels("t.png", 8, 4, &img, 8, Some(&pal), &mut r);
        let codes: Vec<_> = r.issues.iter().map(|i| (i.code, i.x.unwrap(), i.y.unwrap())).collect();
        assert_eq!(codes, vec![("off_palette", 0, 1), ("partial_alpha", 1, 1), ("stray_pixel", 5, 2)]);
        assert_eq!(r.errors, 3);
        // a frame edge splits neighbours: x=3 and x=4 are in different 4-wide frames
        let img = px(8, 1, &[(3, 0, ok), (4, 0, ok)]);
        let mut r = Report::default();
        check_pixels("t.png", 8, 1, &img, 4, Some(&pal), &mut r);
        assert_eq!(r.issues.iter().map(|i| (i.frame.unwrap(), i.x.unwrap())).collect::<Vec<_>>(), vec![(0, 3), (1, 0)]);
    }

    #[test]
    fn palette_strings_anywhere() {
        let v: serde_json::Value = serde_json::from_str(r##"{"note":"#ffffff","ink":["#1b1820"],"x":{"y":["#000001"]}}"##).unwrap();
        let mut s = BTreeSet::new();
        palette_colors(&v, &mut s);
        assert_eq!(s, [[0x1b, 0x18, 0x20], [0, 0, 1]].into());
    }
}
