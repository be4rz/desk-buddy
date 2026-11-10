//! `.aseprite` → pack clips (D11), our own reader of the documented file format
//! (aseprite/docs/ase-file-specs.md): header, frames, layer / cel / tags / slice chunks; RGBA
//! sprites only. The person's pixels are copied unchanged: visible normal layers at full
//! opacity are stacked, and anything that would need blending (partial alpha over a pixel,
//! another blend mode, a layer, group or cel opacity below 255) is refused, never resampled, and so
//! is a tag that plays reverse or ping-pong (the strip would be in the wrong order).
//!
//! Every size is bounded (sprite and cel ≤ 4096 px a side, a capped decompressed size per cel
//! and for all cels together, chunk lengths checked against the file), so truncated or garbage input is an error,
//! never a panic. The import stages its output and moves it into the pack only at the end;
//! it never overwrites an existing clip or file.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use mascot_core::Dir;

use crate::{Error, Result};

pub const MAX_SIDE: u32 = 4096;
pub const MAX_FRAMES: usize = 1024;
/// All frames' pixels together, and separately all decoded cels' pixels together.
pub const MAX_TOTAL: usize = 256 << 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub kind: u16,
    pub level: u16,
    pub blend: u16,
    pub opacity: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub name: String,
    pub from: usize,
    pub to: usize,
    pub repeat: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    pub name: String,
    /// (x, y, w, h) of its first key.
    pub rect: [i32; 4],
    pub pivot: Option<[i32; 2]>,
}

/// A parsed sprite: each frame flattened to straight RGBA `w*h*4`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    pub frames: Vec<Vec<u8>>,
    pub durs_ms: Vec<u32>,
    pub layers: Vec<Layer>,
    pub tags: Vec<Tag>,
    pub slices: Vec<Slice>,
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
    /// file offset of `b[0]`, for messages
    base: usize,
}

impl<'a> Cur<'a> {
    fn need(&self, n: usize) -> Result<()> {
        if self.b.len() - self.p < n {
            return Err(Error(format!("truncated at 0x{:x}", self.base + self.p)));
        }
        Ok(())
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let s = self.bytes(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32> {
        let s = self.bytes(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        let s = self.bytes(n)?;
        String::from_utf8(s.to_vec()).map_err(|_| Error(format!("bad string at 0x{:x}", self.base + self.p - n)))
    }
    fn off(&self) -> usize {
        self.base + self.p
    }
}

fn bad<T>(what: String) -> Result<T> {
    Err(Error(what))
}

/// A cel as parsed: its layer, position and pixels (or a link to an earlier frame's cel).
enum CelData {
    Px { w: u32, h: u32, rgba: Vec<u8> },
    Link(usize),
}

struct Cel {
    layer: usize,
    x: i32,
    y: i32,
    opacity: u8,
    data: CelData,
}

/// Parse a whole `.aseprite` file held in memory.
pub fn parse(b: &[u8]) -> Result<Sprite> {
    let mut c = Cur { b, p: 0, base: 0 };
    if b.len() < 128 {
        return bad(format!("not an .aseprite file: {} bytes (the header is 128)", b.len()));
    }
    let _size = c.u32()?;
    if c.u16()? != 0xA5E0 {
        return bad("not an .aseprite file: bad magic at 0x4".into());
    }
    let nframes = c.u16()? as usize;
    let (w, h) = (c.u16()? as u32, c.u16()? as u32);
    let depth = c.u16()?;
    let flags = c.u32()?;
    c.skip(128 - 18)?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return bad(format!("sprite size {w}x{h} is out of range (1..{MAX_SIDE})"));
    }
    if depth != 32 {
        return bad(format!("colour depth {depth} bpp: only RGBA (32) sprites import"));
    }
    if nframes == 0 || nframes > MAX_FRAMES {
        return bad(format!("{nframes} frames is out of range (1..{MAX_FRAMES})"));
    }
    let frame_px = (w * h * 4) as usize;
    if frame_px.saturating_mul(nframes) > MAX_TOTAL {
        return bad(format!("{nframes} frames of {w}x{h} is too large"));
    }
    let layer_uuid = flags & 4 != 0;
    let mut layers: Vec<Layer> = vec![];
    let mut tags = vec![];
    let mut slices = vec![];
    let mut durs = vec![];
    let mut cels: Vec<Vec<Cel>> = vec![];
    // decoded cel bytes so far (each cel is held until the frames are flattened)
    let mut cel_bytes = 0usize;
    for f in 0..nframes {
        let at = c.off();
        let fbytes = c.u32()? as usize;
        if c.u16()? != 0xF1FA {
            return bad(format!("bad frame header at 0x{at:x}"));
        }
        if fbytes < 16 || fbytes - 6 > b.len() - c.p {
            return bad(format!("bad frame length {fbytes} at 0x{at:x}"));
        }
        let old_n = c.u16()? as usize;
        let dur = c.u16()?;
        c.skip(2)?;
        let new_n = c.u32()? as usize;
        let n = if new_n == 0 { old_n } else { new_n };
        let body = c.bytes(fbytes - 16)?;
        let mut fc = Cur { b: body, p: 0, base: at + 16 };
        let mut fcels = vec![];
        for _ in 0..n {
            let cat = fc.off();
            let size = fc.u32()? as usize;
            if size < 6 || size - 4 > fc.b.len() - fc.p {
                return bad(format!("bad chunk length {size} at 0x{cat:x}"));
            }
            let kind = fc.u16()?;
            let data = fc.bytes(size - 6)?;
            let mut k = Cur { b: data, p: 0, base: cat + 6 };
            match kind {
                0x2004 => {
                    let fl = k.u16()?;
                    let ty = k.u16()?;
                    let level = k.u16()?;
                    k.skip(4)?;
                    let blend = k.u16()?;
                    let opacity = k.u8()?;
                    k.skip(3)?;
                    let name = k.string()?;
                    if ty == 2 {
                        k.skip(4)?;
                    }
                    if layer_uuid {
                        k.skip(16)?;
                    }
                    if layers.len() >= 256 {
                        return bad("more than 256 layers".into());
                    }
                    layers.push(Layer { name, visible: fl & 1 != 0, kind: ty, level, blend, opacity: if flags & 1 != 0 { opacity } else { 255 } });
                }
                0x2005 => {
                    let layer = k.u16()? as usize;
                    let x = k.i16()? as i32;
                    let y = k.i16()? as i32;
                    let opacity = k.u8()?;
                    let ty = k.u16()?;
                    k.skip(2 + 5)?;
                    let data = match ty {
                        0 | 2 => {
                            let (cw, ch) = (k.u16()? as u32, k.u16()? as u32);
                            if cw == 0 || ch == 0 || cw > MAX_SIDE || ch > MAX_SIDE {
                                return bad(format!("cel size {cw}x{ch} at 0x{cat:x} is out of range"));
                            }
                            let n = (cw * ch * 4) as usize;
                            cel_bytes += n;
                            if cel_bytes > MAX_TOTAL {
                                return bad(format!("cel at 0x{cat:x}: the cels hold more than {} MB of pixels", MAX_TOTAL >> 20));
                            }
                            let rgba = if ty == 0 {
                                k.bytes(n).map_err(|_| Error(format!("truncated cel at 0x{cat:x}")))?.to_vec()
                            } else {
                                let rest = &k.b[k.p..];
                                let mut out = Vec::with_capacity(n);
                                flate2::read::ZlibDecoder::new(rest).take(n as u64 + 1).read_to_end(&mut out).map_err(|e| Error(format!("bad compressed cel at 0x{cat:x}: {e}")))?;
                                if out.len() != n {
                                    return bad(format!("compressed cel at 0x{cat:x} holds {} bytes, not {n}", out.len()));
                                }
                                out
                            };
                            CelData::Px { w: cw, h: ch, rgba }
                        }
                        1 => {
                            let to = k.u16()? as usize;
                            if to >= f {
                                return bad(format!("cel at 0x{cat:x} links to frame {to}, not an earlier one"));
                            }
                            CelData::Link(to)
                        }
                        _ => return bad(format!("cel type {ty} at 0x{cat:x} (tilemaps) is not supported")),
                    };
                    fcels.push(Cel { layer, x, y, opacity, data });
                }
                0x2018 => {
                    let n = k.u16()? as usize;
                    k.skip(8)?;
                    for _ in 0..n {
                        let from = k.u16()? as usize;
                        let to = k.u16()? as usize;
                        let direction = k.u8()?;
                        let repeat = k.u16()?;
                        k.skip(6 + 3 + 1)?;
                        let name = k.string()?;
                        if from > to || to >= nframes {
                            return bad(format!("tag “{name}” spans frames {from}..{to} of {nframes}"));
                        }
                        if direction != 0 {
                            return bad(format!("tag “{name}” plays reverse or ping-pong (direction {direction}): only forward tags import unchanged"));
                        }
                        tags.push(Tag { name, from, to, repeat });
                    }
                }
                0x2022 => {
                    let nkeys = k.u32()?;
                    let fl = k.u32()?;
                    k.skip(4)?;
                    let name = k.string()?;
                    if nkeys == 0 {
                        continue;
                    }
                    let _frame = k.u32()?;
                    let rect = [k.i32()?, k.i32()?, k.u32()? as i32, k.u32()? as i32];
                    if fl & 1 != 0 {
                        k.skip(16)?;
                    }
                    let pivot = if fl & 2 != 0 { Some([k.i32()?, k.i32()?]) } else { None };
                    slices.push(Slice { name, rect, pivot });
                }
                _ => {} // palettes, colour profile, user data, …: not needed for RGBA
            }
        }
        durs.push(dur as u32);
        cels.push(fcels);
    }
    // flatten each frame
    let visible = effective_visibility(&layers);
    for (l, _) in layers.iter().zip(&visible).filter(|(l, v)| **v && l.kind == 1) {
        if l.blend != 0 || l.opacity != 255 {
            return bad(format!("group “{}”: only normal blending at full opacity imports unchanged (blend {}, opacity {})", l.name, l.blend, l.opacity));
        }
    }
    let mut frames = Vec::with_capacity(nframes);
    for f in 0..nframes {
        let mut out = vec![0u8; frame_px];
        let mut order: Vec<&Cel> = cels[f].iter().collect();
        order.sort_by_key(|c| c.layer); // layer index order = bottom to top
        for cel in order {
            let Some(l) = layers.get(cel.layer) else { return bad(format!("frame {f}: a cel names layer {} of {}", cel.layer, layers.len())) };
            if !visible[cel.layer] || l.kind != 0 {
                continue;
            }
            if l.blend != 0 || l.opacity != 255 || cel.opacity != 255 {
                return bad(format!("layer “{}”: only normal blending at full opacity imports unchanged (blend {}, opacity {}, cel {})", l.name, l.blend, l.opacity, cel.opacity));
            }
            let (cw, ch, px) = match &cel.data {
                CelData::Px { w, h, rgba } => (*w, *h, rgba),
                CelData::Link(to) => match cels[*to].iter().find(|c| c.layer == cel.layer).map(|c| &c.data) {
                    Some(CelData::Px { w, h, rgba }) => (*w, *h, rgba),
                    _ => return bad(format!("frame {f}: layer “{}” links to an empty cel", l.name)),
                },
            };
            for y in 0..ch as i32 {
                let ty = cel.y + y;
                if ty < 0 || ty >= h as i32 {
                    continue;
                }
                for x in 0..cw as i32 {
                    let tx = cel.x + x;
                    if tx < 0 || tx >= w as i32 {
                        continue;
                    }
                    let s = ((y * cw as i32 + x) * 4) as usize;
                    let a = px[s + 3];
                    if a == 0 {
                        continue;
                    }
                    let o = ((ty * w as i32 + tx) * 4) as usize;
                    if a != 255 && out[o + 3] != 0 {
                        return bad(format!("frame {f} ({tx},{ty}): a partly transparent pixel over another would need blending"));
                    }
                    out[o..o + 4].copy_from_slice(&px[s..s + 4]);
                }
            }
        }
        frames.push(out);
    }
    Ok(Sprite { w, h, frames, durs_ms: durs, layers, tags, slices })
}

/// A layer shows when it and every group above it are visible.
fn effective_visibility(layers: &[Layer]) -> Vec<bool> {
    let mut stack: Vec<(u16, bool)> = vec![];
    layers
        .iter()
        .map(|l| {
            while stack.last().is_some_and(|(lv, _)| *lv >= l.level) {
                stack.pop();
            }
            let v = l.visible && stack.iter().all(|(_, v)| *v);
            if l.kind == 1 {
                stack.push((l.level, v));
            }
            v
        })
        .collect()
}

/// One imported clip: name → direction → frame range.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPlan {
    pub name: String,
    pub looping: bool,
    pub dirs: BTreeMap<Dir, (usize, usize)>,
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 48 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') && s.as_bytes()[0].is_ascii_lowercase()
}

/// Tags → clips `<prefix>_<tag>`; a tag ending in `_<DIR>` (`walk_SE`) is that direction of
/// the clip, any other is its `S`. No tags: the whole sprite is clip `<prefix>`, `S`.
pub fn plan(s: &Sprite, prefix: &str) -> Result<Vec<ClipPlan>> {
    if !valid_name(prefix) {
        return bad(format!("--name “{prefix}”: use lowercase letters, digits and _"));
    }
    let mut clips: BTreeMap<String, ClipPlan> = BTreeMap::new();
    let tags: Vec<Tag> = if s.tags.is_empty() { vec![Tag { name: String::new(), from: 0, to: s.frames.len() - 1, repeat: 0 }] } else { s.tags.clone() };
    for t in &tags {
        let (base, dir) = match t.name.rsplit_once('_').and_then(|(b, d)| Dir::parse(d).map(|d| (b, d))) {
            Some((b, d)) => (b.to_string(), d),
            None => (t.name.clone(), Dir::S),
        };
        let base = base.to_ascii_lowercase().replace([' ', '-'], "_");
        let name = if base.is_empty() { prefix.to_string() } else { format!("{prefix}_{base}") };
        if !valid_name(&name) {
            return bad(format!("tag “{}” doesn't make a clip name (letters, digits, _)", t.name));
        }
        let c = clips.entry(name.clone()).or_insert(ClipPlan { name: name.clone(), looping: t.repeat == 0, dirs: BTreeMap::new() });
        if c.dirs.insert(dir, (t.from, t.to)).is_some() {
            return bad(format!("two tags make clip {name} {}", dir.as_str()));
        }
    }
    for c in clips.values() {
        let lens: Vec<usize> = c.dirs.values().map(|(a, b)| b - a + 1).collect();
        if lens.iter().any(|n| *n != lens[0]) {
            return bad(format!("clip {}: its directions have different frame counts {lens:?}", c.name));
        }
    }
    Ok(clips.into_values().collect())
}

/// The anchor: the slice `anchor`'s pivot (or its bottom centre), else the bottom centre.
pub fn anchor(s: &Sprite) -> [i32; 2] {
    match s.slices.iter().find(|x| x.name == "anchor") {
        Some(Slice { rect, pivot: Some(p), .. }) => [rect[0].saturating_add(p[0]), rect[1].saturating_add(p[1])],
        Some(Slice { rect, .. }) => [rect[0].saturating_add(rect[2] / 2), rect[1].saturating_add(rect[3])],
        None => [s.w as i32 / 2, s.h as i32],
    }
}

/// What an import wrote.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Imported {
    pub pack: String,
    pub created_pack: bool,
    pub clips: Vec<String>,
    pub files: Vec<String>,
}

/// Every intent name (§4.3), for a new pack's `intents`.
const INTENTS: [&str; 20] = ["idle", "think", "work", "talk", "ask", "oops", "happy", "celebrate", "sad", "surprised", "sleepy", "wave", "nod", "shake", "jump", "spin", "greet", "stretch", "remind", "dodge"];

/// `import-aseprite <file> --pack <dir> --name <prefix>`: the clips go to
/// `<pack>/art/<prefix>/` (a Stage A source: `anim.json` + one strip per clip and direction)
/// and pack.json gains that source, or is created (rest/hold play the first clip, every intent
/// falls back to rest). Nothing is written unless everything parsed and checked.
pub fn import(file: &Path, pack_dir: &Path, prefix: &str) -> Result<Imported> {
    let fname = file.display().to_string();
    let ctx = |e: Error| Error(format!("{fname}: {}; nothing written", e.0));
    let bytes = std::fs::read(file).map_err(|e| Error(format!("{fname}: {e}; nothing written")))?;
    let sprite = parse(&bytes).map_err(ctx)?;
    let clips = plan(&sprite, prefix).map_err(ctx)?;
    let existing = pack_dir.join("pack.json").is_file();
    let out_dir = pack_dir.join("art").join(prefix);
    if out_dir.exists() {
        return Err(ctx(Error(format!("{} already exists; pick another --name", out_dir.display()))));
    }
    if existing {
        let pack = mascot_core::pack::load(pack_dir).map_err(|e| ctx(Error(e.to_string())))?;
        if let Some(c) = clips.iter().find(|c| pack.clips.contains_key(&c.name)) {
            return Err(ctx(Error(format!("pack {} already has clip “{}”", pack.id, c.name))));
        }
    }
    // build everything in memory
    let (w, h) = (sprite.w as usize, sprite.h as usize);
    let anchor = anchor(&sprite);
    let mut anims = serde_json::Map::new();
    let mut strips: Vec<(String, Vec<u8>)> = vec![];
    for c in &clips {
        let n = c.dirs.values().next().map_or(0, |(a, b)| b - a + 1);
        let mut files = serde_json::Map::new();
        let mut durs = vec![0u32; n];
        for (d, (from, _)) in &c.dirs {
            let mut strip = vec![0u8; w * n * h * 4];
            for k in 0..n {
                let fr = &sprite.frames[from + k];
                for y in 0..h {
                    let s = y * w * 4;
                    let o = (y * w * n + k * w) * 4;
                    strip[o..o + w * 4].copy_from_slice(&fr[s..s + w * 4]);
                }
                durs[k] = sprite.durs_ms[from + k].max(1);
            }
            let fname = format!("{}_{}.png", c.name, d.as_str());
            files.insert(d.as_str().into(), fname.clone().into());
            strips.push((fname, crate::png_bytes(w * n, h, &strip)));
        }
        let avg = durs.iter().sum::<u32>() as f64 / n.max(1) as f64;
        let frames: Vec<serde_json::Value> = durs
            .iter()
            .map(|d| serde_json::json!({ "dur": d, "root_world": 0.0, "root_px": c.dirs.keys().map(|d| (d.as_str().to_string(), serde_json::json!([0.0, 0.0]))).collect::<serde_json::Map<_, _>>() }))
            .collect();
        anims.insert(
            c.name.clone(),
            serde_json::json!({ "loop": c.looping, "fps": ((1000.0 / avg).round() as u32).max(1), "frame_w": w, "frame_h": h, "anchor": anchor, "frames_n": n, "files": files, "frames": frames }),
        );
    }
    let fps = anims.values().next().and_then(|a| a["fps"].as_u64()).unwrap_or(12);
    let anim_json = serde_json::to_string_pretty(&serde_json::json!({ "note": format!("imported from {} by buddy studio import-aseprite (pixels unchanged)", file.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()), "fps": fps, "ppu": 40.0, "directions": {}, "anims": anims })).expect("json") + "\n";
    // stage
    std::fs::create_dir_all(pack_dir).map_err(|e| ctx(Error(format!("{}: {e}", pack_dir.display()))))?;
    let stage = pack_dir.join(format!(".import-{prefix}-{}", std::process::id()));
    let cleanup = |e: Error| {
        let _ = std::fs::remove_dir_all(&stage);
        ctx(e)
    };
    let io = |p: &Path, e: std::io::Error| Error(format!("{}: {e}", p.display()));
    std::fs::create_dir_all(stage.join("art")).map_err(|e| cleanup(io(&stage, e)))?;
    let staged_art = stage.join("art").join(prefix);
    std::fs::create_dir(&staged_art).map_err(|e| cleanup(io(&staged_art, e)))?;
    let mut written = vec![];
    for (f, b) in &strips {
        std::fs::write(staged_art.join(f), b).map_err(|e| cleanup(io(&staged_art.join(f), e)))?;
        written.push(format!("art/{prefix}/{f}"));
    }
    std::fs::write(staged_art.join("anim.json"), &anim_json).map_err(|e| cleanup(io(&staged_art, e)))?;
    written.push(format!("art/{prefix}/anim.json"));
    let pj = pack_dir.join("pack.json");
    let (new_pack_json, old) = if existing {
        let old = std::fs::read_to_string(&pj).map_err(|e| cleanup(io(&pj, e)))?;
        let pf: mascot_protocol::pack::PackFile = serde_json::from_str(&old).map_err(|e| cleanup(Error(format!("{}: {e}", pj.display()))))?;
        let rel = relative(&pack_dir.join(&pf.art_root), &out_dir.join("anim.json")).ok_or_else(|| cleanup(Error("can't reach the pack's art from its art_root".into())))?;
        let at = old.find("\"sources\"").and_then(|i| old[i..].find('[').map(|j| i + j + 1)).ok_or_else(|| cleanup(Error("pack.json: no sources list".into())))?;
        let entry = format!("\n    {{ \"kind\": \"stage_a\", \"path\": {} }},", serde_json::to_string(&rel).expect("json"));
        (format!("{}{}{}", &old[..at], entry, &old[at..]), Some(old))
    } else {
        let first = &clips[0].name;
        let intents: serde_json::Map<_, _> = INTENTS.iter().map(|i| (i.to_string(), serde_json::json!(["rest"]))).collect();
        let pf = serde_json::json!({
            "pack_version": 1, "id": prefix, "name": prefix, "art_root": ".", "palettes": [],
            "sources": [{ "kind": "stage_a", "path": format!("art/{prefix}/anim.json") }],
            "behaviors": { "rest": { "seq": [{ "clip": first, "loops": [1, 3] }] }, "hold": { "seq": [{ "clip": first, "loops": "hold" }] } },
            "intents": intents,
            "idle_pool": [{ "behavior": "rest", "weight": 1, "cooldown_s": 0 }],
        });
        (serde_json::to_string_pretty(&pf).expect("json") + "\n", None)
    };
    let staged_pj = stage.join("pack.json");
    std::fs::write(&staged_pj, &new_pack_json).map_err(|e| cleanup(io(&staged_pj, e)))?;
    // move into place: the art dir, then pack.json; if the result doesn't load, undo both
    if out_dir.exists() || (!existing && pj.exists()) {
        return Err(cleanup(Error("the pack changed while importing; nothing written".into())));
    }
    std::fs::create_dir_all(pack_dir.join("art")).map_err(|e| cleanup(io(pack_dir, e)))?;
    std::fs::rename(&staged_art, &out_dir).map_err(|e| cleanup(io(&out_dir, e)))?;
    if let Err(e) = std::fs::rename(&staged_pj, &pj) {
        let _ = std::fs::remove_dir_all(&out_dir);
        return Err(cleanup(io(&pj, e)));
    }
    let _ = std::fs::remove_dir_all(&stage);
    if let Err(e) = mascot_core::pack::load(pack_dir) {
        let _ = std::fs::remove_dir_all(&out_dir);
        match old {
            Some(o) => {
                let _ = std::fs::write(&pj, o);
            }
            None => {
                let _ = std::fs::remove_file(&pj);
                let _ = std::fs::remove_dir(pack_dir.join("art"));
            }
        }
        return Err(ctx(Error(format!("the imported pack doesn't load: {e}"))));
    }
    if !existing {
        written.push("pack.json".into());
    }
    Ok(Imported { pack: pack_dir.display().to_string(), created_pack: !existing, clips: clips.iter().map(|c| c.name.clone()).collect(), files: written })
}

/// `to` relative to the directory `from` (both made absolute), with `..` as needed.
fn relative(from: &Path, to: &Path) -> Option<String> {
    let abs = |p: &Path| -> Option<PathBuf> {
        let p = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().ok()?.join(p) };
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                c => out.push(c),
            }
        }
        Some(out)
    };
    let (f, t) = (abs(from)?, abs(to)?);
    let fc: Vec<_> = f.components().collect();
    let tc: Vec<_> = t.components().collect();
    let common = fc.iter().zip(&tc).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), fc.len() - common).collect();
    parts.extend(tc[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/a/b/c"), Path::new("/a/d/e.json")).unwrap(), "../../d/e.json");
        assert_eq!(relative(Path::new("/a/./b"), Path::new("/a/b/x")).unwrap(), "x");
    }

    #[test]
    fn garbage_and_truncated_input_errors_never_panics() {
        assert!(parse(b"").unwrap_err().0.contains("not an .aseprite"));
        assert!(parse(&[0u8; 200]).unwrap_err().0.contains("bad magic"));
        // a valid header, then every truncation and a pseudo-random fill: errors only
        let mut h = vec![0u8; 128];
        h[4..6].copy_from_slice(&0xA5E0u16.to_le_bytes());
        h[6..8].copy_from_slice(&1u16.to_le_bytes());
        h[8..10].copy_from_slice(&4u16.to_le_bytes());
        h[10..12].copy_from_slice(&4u16.to_le_bytes());
        h[12..14].copy_from_slice(&32u16.to_le_bytes());
        assert!(parse(&h).unwrap_err().0.contains("truncated"));
        let mut x = 0x2545_f491_4f6c_dd1du64;
        for len in [130usize, 144, 160, 300, 4096] {
            let mut b = h.clone();
            for _ in 0..len {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b.push(x as u8);
            }
            for cut in (128..b.len()).step_by(7) {
                let _ = parse(&b[..cut]);
            }
            let _ = parse(&b);
        }
        // a huge declared size is refused before allocating
        let mut big = h.clone();
        big[8..10].copy_from_slice(&5000u16.to_le_bytes());
        assert!(parse(&big).unwrap_err().0.contains("out of range"));
    }

    /// A minimal RGBA file: `w`x`h`, flags 1 (layer opacity valid), one chunk list per frame.
    fn file(w: u16, h: u16, frames: &[Vec<(u16, Vec<u8>)>]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[4..6].copy_from_slice(&0xA5E0u16.to_le_bytes());
        b[6..8].copy_from_slice(&(frames.len() as u16).to_le_bytes());
        b[8..10].copy_from_slice(&w.to_le_bytes());
        b[10..12].copy_from_slice(&h.to_le_bytes());
        b[12..14].copy_from_slice(&32u16.to_le_bytes());
        b[14..18].copy_from_slice(&1u32.to_le_bytes());
        for chunks in frames {
            let body: Vec<u8> = chunks.iter().flat_map(|(k, d)| [((d.len() + 6) as u32).to_le_bytes().to_vec(), k.to_le_bytes().to_vec(), d.clone()].concat()).collect();
            b.extend(((body.len() + 16) as u32).to_le_bytes());
            b.extend(0xF1FAu16.to_le_bytes());
            b.extend((chunks.len() as u16).to_le_bytes());
            b.extend(100u16.to_le_bytes());
            b.extend([0, 0]);
            b.extend((chunks.len() as u32).to_le_bytes());
            b.extend(body);
        }
        b
    }

    fn string(s: &str) -> Vec<u8> {
        [(s.len() as u16).to_le_bytes().to_vec(), s.as_bytes().to_vec()].concat()
    }

    /// Layer chunk: visible, `kind` (0 image, 1 group), `level`, normal blend, `opacity`.
    fn layer(name: &str, kind: u16, level: u16, opacity: u8) -> (u16, Vec<u8>) {
        let mut d = vec![];
        d.extend(1u16.to_le_bytes());
        d.extend(kind.to_le_bytes());
        d.extend(level.to_le_bytes());
        d.extend([0; 4]);
        d.extend(0u16.to_le_bytes());
        d.push(opacity);
        d.extend([0; 3]);
        d.extend(string(name));
        (0x2004, d)
    }

    /// Compressed cel chunk on layer `l`: `cw`x`ch` transparent pixels.
    fn zcel(l: u16, cw: u16, ch: u16) -> (u16, Vec<u8>) {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::best());
        z.write_all(&vec![0u8; cw as usize * ch as usize * 4]).unwrap();
        let mut d = vec![];
        d.extend(l.to_le_bytes());
        d.extend([0; 4]);
        d.push(255);
        d.extend(2u16.to_le_bytes());
        d.extend([0; 7]);
        d.extend(cw.to_le_bytes());
        d.extend(ch.to_le_bytes());
        d.extend(z.finish().unwrap());
        (0x2005, d)
    }

    fn tag(name: &str, direction: u8) -> (u16, Vec<u8>) {
        let mut d = vec![];
        d.extend(1u16.to_le_bytes());
        d.extend([0; 8]);
        d.extend(0u16.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.push(direction);
        d.extend(0u16.to_le_bytes());
        d.extend([0; 10]);
        d.extend(string(name));
        (0x2018, d)
    }

    #[test]
    fn builder_makes_a_file_that_parses() {
        let s = parse(&file(4, 4, &[vec![layer("a", 0, 0, 255), zcel(0, 2, 2), tag("idle_S", 0)]])).unwrap();
        assert_eq!((s.w, s.h, s.frames.len(), s.tags.len()), (4, 4, 1, 1));
    }

    #[test]
    fn many_big_cels_are_refused_before_they_add_up() {
        // 5 cels of 4096x4096 (64 MB each decoded, a few kB compressed) in one 4x4 frame
        let big = zcel(0, 4096, 4096);
        let mut chunks = vec![layer("a", 0, 0, 255)];
        chunks.extend((0..5).map(|_| big.clone()));
        let e = parse(&file(4, 4, &[chunks])).unwrap_err();
        assert!(e.0.contains("more than 256 MB"), "{}", e.0);
    }

    #[test]
    fn reverse_tags_and_faded_groups_are_refused() {
        let e = parse(&file(4, 4, &[vec![layer("a", 0, 0, 255), tag("walk_S", 1)]])).unwrap_err();
        assert!(e.0.contains("reverse or ping-pong"), "{}", e.0);
        let e = parse(&file(4, 4, &[vec![layer("g", 1, 0, 128), layer("a", 0, 1, 255), zcel(1, 2, 2)]])).unwrap_err();
        assert!(e.0.contains("group “g”"), "{}", e.0);
    }

    #[test]
    fn tag_names_make_clips() {
        let s = Sprite { w: 2, h: 2, frames: vec![vec![0; 16]; 4], durs_ms: vec![100; 4], layers: vec![], tags: vec![Tag { name: "walk_S".into(), from: 0, to: 1, repeat: 0 }, Tag { name: "walk_SE".into(), from: 2, to: 3, repeat: 0 }], slices: vec![] };
        let p = plan(&s, "blob").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "blob_walk");
        assert_eq!(p[0].dirs.keys().copied().collect::<Vec<_>>(), vec![Dir::S, Dir::SE]);
        assert!(plan(&s, "Bad Name").is_err());
        assert_eq!(anchor(&s), [1, 2]);
    }
}
