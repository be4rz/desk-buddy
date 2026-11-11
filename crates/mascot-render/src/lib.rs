//! Headless compositor (design §4.5): decodes a pack's pre-rendered PNG strips into
//! premultiplied BGRA sprites and composes one `Show` into a canvas at sprite-pixel
//! resolution. Ported from spike S1 (`assets.rs` + `render.rs`). Never draws art:
//! every opaque pixel comes from a pack PNG; only the faint floor grid and vignette
//! are generated (as in S1).
//!
//! Layout: the bear's floor anchor sits at a fixed canvas point and never moves; the
//! floor scrolls under it with `View::cam` (the follow camera). A set is placed so its
//! hand-off spot is the bear's anchor, so entering or leaving a set never jumps.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use mascot_core::{Dir, EyeState, MouthState, Ori, PackMeta, Show, Visibility};

mod floor;
mod png_io;

pub use png_io::{read_png, write_png};

/// Margin (sprite px) around the union of every frame the canvas must hold.
pub const MARGIN: i32 = 16;
/// Iso floor grid spacing (tiles are 2T x T), as S1.
pub const TILE: i32 = 32;

#[derive(Debug)]
pub enum RenderError {
    /// A file could not be read or decoded.
    Read { path: PathBuf, msg: String },
    /// A strip does not have the size its metadata promises.
    Size { path: PathBuf, msg: String },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RenderError::Read { path, msg } => write!(f, "art {}: {msg}", path.display()),
            RenderError::Size { path, msg } => write!(f, "art {}: {msg}", path.display()),
        }
    }
}
impl std::error::Error for RenderError {}

/// One frame cropped to its opaque bounding box: premultiplied BGRA, `w*h*4` bytes,
/// top-left at (`x`, `y`) in its frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sprite {
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    /// content hash (position included): repeated frames (idle holds) share it
    pub id: u64,
}

impl Sprite {
    /// Crop `fw x fh` frame `f` out of a straight-RGBA strip `w` px wide.
    pub(crate) fn crop(rgba: &[u8], stride_w: usize, x0: usize, fw: usize, fh: usize) -> Sprite {
        let (mut bx0, mut by0, mut bx1, mut by1) = (fw, fh, 0, 0);
        for y in 0..fh {
            for x in 0..fw {
                if rgba[(y * stride_w + x0 + x) * 4 + 3] > 0 {
                    bx0 = bx0.min(x);
                    by0 = by0.min(y);
                    bx1 = bx1.max(x + 1);
                    by1 = by1.max(y + 1);
                }
            }
        }
        if bx1 == 0 {
            return Sprite::default();
        }
        let (w, h) = (bx1 - bx0, by1 - by0);
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let s = ((by0 + y) * stride_w + x0 + bx0 + x) * 4;
                let o = (y * w + x) * 4;
                let a = rgba[s + 3] as u32;
                let pm = |v: u8| ((v as u32 * a + 127) / 255) as u8;
                px[o] = pm(rgba[s + 2]);
                px[o + 1] = pm(rgba[s + 1]);
                px[o + 2] = pm(rgba[s]);
                px[o + 3] = a as u8;
            }
        }
        let id = {
            use std::hash::{Hash, Hasher};
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            (bx0, by0, w, h, &px).hash(&mut hs);
            hs.finish()
        };
        Sprite { x: bx0 as i32, y: by0 as i32, w, h, px, id }
    }
}

struct ClipArt {
    anchor: [i32; 2],
    dirs: HashMap<Dir, Vec<Sprite>>,
    oris: HashMap<Ori, Vec<Sprite>>,
    set: Option<String>,
}

impl ClipArt {
    /// See `Art::frame_id`.
    fn frame_id(&self, dir: Dir, ori: Option<Ori>, f: usize) -> u64 {
        let s = if self.set.is_some() {
            ori.and_then(|o| self.oris.get(&o)).and_then(|v| v.get(f))
        } else {
            self.dirs.get(&dir).and_then(|v| v.get(f))
        };
        let set = self.set.is_some() as u64 | ((ori.map(|o| o as u64 + 1).unwrap_or(0)) << 1);
        s.map(|s| s.id ^ set.rotate_left(17)).unwrap_or(0)
    }
}

struct SetArt {
    anchor: [i32; 2],
    /// the props-only frame on the set canvas
    props: Sprite,
    /// where the bear's anchor is, relative to the set anchor, at the hand-off spot
    handoff_px: [i32; 2],
}

/// Decoded art for one pack.
pub struct Art {
    w: usize,
    h: usize,
    /// canvas pixel of the bear's floor anchor
    anchor: [i32; 2],
    clips: HashMap<String, ClipArt>,
    sets: HashMap<(String, Ori), SetArt>,
    behavior_set: HashMap<String, String>,
    face: Option<face::FaceArt>,
    floor: floor::Floor,
    /// the last floor drawn, keyed by (camera, vignette): the floor only changes when
    /// the camera moves, so most frames copy it instead of redrawing (S1's bg cache)
    floor_cache: std::sync::Mutex<(Option<([i32; 2], bool)>, Vec<u8>)>,
}

/// Premultiplied BGRA canvas, `w*h*4` bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: vec![0; w * h * 4] }
    }
}

/// `cam` = the bear's integer world position in sprite px (the follow camera);
/// the floor grid scrolls against it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub cam: [i32; 2],
    pub floor: bool,
    pub vignette: bool,
}

impl Default for View {
    fn default() -> Self {
        View { cam: [0, 0], floor: true, vignette: true }
    }
}

/// Bounds (min x, min y, max x, max y) relative to the bear's anchor that the canvas
/// must hold, and the canvas size and anchor that follow.
fn layout(pack: &PackMeta) -> (usize, usize, [i32; 2]) {
    let (mut x0, mut y0, mut x1, mut y1) = (0i32, 0i32, 0i32, 0i32);
    let mut grow = |ax: i32, ay: i32, w: i32, h: i32| {
        x0 = x0.min(-ax);
        y0 = y0.min(-ay);
        x1 = x1.max(w - ax);
        y1 = y1.max(h - ay);
    };
    for c in pack.clips.values() {
        if c.set.is_none() {
            grow(c.anchor[0], c.anchor[1], c.frame_w as i32, c.frame_h as i32);
        }
    }
    for (name, set) in &pack.sets {
        for (ori, so) in &set.oris {
            if let Some(h) = handoff_px(pack, name, *ori) {
                // set anchor sits at bear anchor - h
                grow(so.anchor[0] + h[0], so.anchor[1] + h[1], so.frame_w as i32, so.frame_h as i32);
            }
        }
    }
    let w = (x1 - x0 + 2 * MARGIN + 1) & !1;
    let h = (y1 - y0 + 2 * MARGIN + 1) & !1;
    (w as usize, h as usize, [MARGIN - x0, MARGIN - y0])
}

fn handoff_px(pack: &PackMeta, set: &str, ori: Ori) -> Option<[i32; 2]> {
    pack.clips
        .values()
        .filter(|c| c.set.as_deref() == Some(set))
        .find_map(|c| c.handoff.get(&ori).map(|h| h.px))
}

/// Fits the bear in every free clip and every set frame at its hand-off spot, plus a margin.
pub fn canvas_size(pack: &PackMeta) -> (usize, usize) {
    let (w, h, _) = layout(pack);
    (w, h)
}

fn load_strip(path: &Path, fw: u32, fh: u32, n: usize) -> Result<Vec<Sprite>, RenderError> {
    let (w, h, rgba) = read_png(path)?;
    let (fw, fh) = (fw as usize, fh as usize);
    if h != fh || w < fw * n {
        return Err(RenderError::Size {
            path: path.into(),
            msg: format!("{w}x{h} does not fit {n} frames of {fw}x{fh}"),
        });
    }
    Ok((0..n).map(|f| Sprite::crop(&rgba, w, f * fw, fw, fh)).collect())
}

impl Art {
    pub fn load(pack: &PackMeta) -> Result<Art, RenderError> {
        let (w, h, anchor) = layout(pack);
        let mut clips = HashMap::new();
        for (name, c) in &pack.clips {
            let n = c.durs_ms.len();
            let mut dirs = HashMap::new();
            for (d, s) in &c.dirs {
                dirs.insert(*d, load_strip(&s.file, c.frame_w, c.frame_h, n)?);
            }
            let mut oris = HashMap::new();
            for (o, s) in &c.oris {
                oris.insert(*o, load_strip(&s.file, c.frame_w, c.frame_h, n)?);
            }
            clips.insert(name.clone(), ClipArt { anchor: c.anchor, dirs, oris, set: c.set.clone() });
        }
        let mut sets = HashMap::new();
        for (name, set) in &pack.sets {
            for (ori, so) in &set.oris {
                let Some(handoff_px) = handoff_px(pack, name, *ori) else { continue };
                let props = load_strip(&so.props_frame, so.frame_w, so.frame_h, 1)?.remove(0);
                sets.insert((name.clone(), *ori), SetArt { anchor: so.anchor, props, handoff_px });
            }
        }
        // set behaviors and stations (their synthesized behavior is named after the station)
        let behavior_set = pack
            .behaviors
            .iter()
            .filter_map(|(b, d)| d.set.clone().map(|s| (b.clone(), s)))
            .chain(pack.stations.iter().map(|(n, st)| (n.clone(), st.file.set.clone())))
            .collect();
        let face = face::load(pack)?;
        Ok(Art {
            w,
            h,
            anchor,
            clips,
            sets,
            behavior_set,
            face,
            floor: floor::Floor::new(w, h, TILE),
            floor_cache: Default::default(),
        })
    }

    /// The canvas pixel of the bear's floor anchor (fixed for a pack).
    pub fn anchor(&self) -> [i32; 2] {
        self.anchor
    }

    /// Content id of the body (or baked set) frame `show` draws: two shows with the
    /// same id, face states, effect, visibility and camera compose the same canvas, so
    /// the driver can skip composing held poses (S1's identical-frame skip).
    pub fn frame_id(&self, show: &Show) -> u64 {
        self.clips.get(&show.clip).map(|c| c.frame_id(show.dir, show.ori, show.frame as usize)).unwrap_or(0)
    }

    /// A blank canvas of this art's size.
    pub fn canvas(&self) -> Canvas {
        Canvas::new(self.w, self.h)
    }
}

mod face;

/// Source-over blit of a premultiplied sprite whose frame origin is at (`ox`, `oy`).
fn blit(out: &mut Canvas, s: &Sprite, ox: i32, oy: i32) {
    for sy in 0..s.h as i32 {
        let y = oy + s.y + sy;
        if y < 0 || y >= out.h as i32 {
            continue;
        }
        for sx in 0..s.w as i32 {
            let x = ox + s.x + sx;
            if x < 0 || x >= out.w as i32 {
                continue;
            }
            let si = ((sy as usize) * s.w + sx as usize) * 4;
            let sa = s.px[si + 3] as u32;
            if sa == 0 {
                continue;
            }
            let o = (y as usize * out.w + x as usize) * 4;
            if sa == 255 {
                out.px[o..o + 4].copy_from_slice(&s.px[si..si + 4]);
                continue;
            }
            let inv = 255 - sa;
            for c in 0..4 {
                let d = out.px[o + c] as u32;
                out.px[o + c] = (s.px[si + c] as u32 + (d * inv + 127) / 255).min(255) as u8;
            }
        }
    }
}

/// Compose `show` into `out` (resized to the art's canvas if needed).
/// Draw order (§4.5): floor → vignette (baked into the floor colours) → set props or
/// the baked set frame → body → eyes, mouth → effect. `Hidden` → all alpha 0.
pub fn compose(art: &Art, show: &Show, view: &View, out: &mut Canvas) {
    if out.w != art.w || out.h != art.h {
        *out = art.canvas();
    }
    if show.visibility == Visibility::Hidden {
        out.px.fill(0);
        return;
    }
    let [ax, ay] = art.anchor;
    if view.floor {
        let key = ([view.cam[0] - ax, view.cam[1] - ay], view.vignette);
        let mut cache = art.floor_cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.0 != Some(key) {
            cache.1.resize(out.px.len(), 0);
            art.floor.draw(key.0, key.1, &mut cache.1);
            cache.0 = Some(key);
        }
        out.px.copy_from_slice(&cache.1);
    } else {
        out.px.fill(0);
    }
    let Some(clip) = art.clips.get(&show.clip) else { return };
    let set_of = clip.set.as_ref().or_else(|| art.behavior_set.get(&show.behavior));
    let set = match (set_of, show.ori) {
        (Some(s), Some(o)) => art.sets.get(&(s.clone(), o)).map(|a| (a, o)),
        _ => None,
    };
    let frame = show.frame as usize;
    if clip.set.is_some() {
        // the baked frame (props + bear) at the set anchor
        if let Some((sa, ori)) = set {
            if let Some(f) = clip.oris.get(&ori).and_then(|v| v.get(frame)) {
                let (sx, sy) = (ax - sa.handoff_px[0], ay - sa.handoff_px[1]);
                let (ox, oy) = (sx - sa.anchor[0], sy - sa.anchor[1]);
                blit(out, f, ox, oy);
                if let Some(face) = &art.face {
                    face::draw_set(face, &show.clip, ori, frame, eyes_name(show.eyes), mouth_name(show.mouth), out, ox, oy);
                }
            }
        }
    } else {
        if let Some((sa, _)) = set {
            // a free clip inside a set behavior: props behind the bear (the bear is in
            // front of every prop pixel at the hand-off spot)
            let (sx, sy) = (ax - sa.handoff_px[0], ay - sa.handoff_px[1]);
            blit(out, &sa.props, sx - sa.anchor[0], sy - sa.anchor[1]);
        }
        let Some(f) = clip.dirs.get(&show.dir).and_then(|v| v.get(frame)) else { return };
        let (ox, oy) = (ax - clip.anchor[0], ay - clip.anchor[1]);
        blit(out, f, ox, oy);
        if let Some(face) = &art.face {
            face::draw(face, &show.clip, show.dir, frame, eyes_name(show.eyes), mouth_name(show.mouth), out, ox, oy);
        }
    }
    // effect: no effect art exists yet (design §4.5 draws it last); nothing to draw
}

/// The face.json state names (`snake_case`, as the brain serializes them).
fn eyes_name(e: EyeState) -> &'static str {
    match e {
        EyeState::Open => "open",
        EyeState::Closed => "closed",
    }
}

fn mouth_name(m: MouthState) -> &'static str {
    match m {
        MouthState::Closed => "closed",
        MouthState::Open => "open",
        MouthState::Wide => "wide",
    }
}

/// Nearest-neighbour upscale by a whole factor `k` (≥ 1).
pub fn upscale(c: &Canvas, k: usize) -> Canvas {
    let k = k.max(1);
    let (w, h) = (c.w * k, c.h * k);
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        let sr = (y / k) * c.w;
        for x in 0..w {
            let s = (sr + x / k) * 4;
            let o = (y * w + x) * 4;
            px[o..o + 4].copy_from_slice(&c.px[s..s + 4]);
        }
    }
    Canvas { w, h, px }
}

/// Straight (un-premultiplied) RGBA, for PNG writers.
pub fn to_rgba(c: &Canvas) -> Vec<u8> {
    let mut rgba = vec![0u8; c.w * c.h * 4];
    for i in 0..c.w * c.h {
        let p = &c.px[i * 4..i * 4 + 4];
        let a = p[3] as u32;
        let un = |v: u8| if a == 0 { 0 } else { ((v as u32 * 255 + a / 2) / a).min(255) as u8 };
        rgba[i * 4] = un(p[2]);
        rgba[i * 4 + 1] = un(p[1]);
        rgba[i * 4 + 2] = un(p[0]);
        rgba[i * 4 + 3] = a as u8;
    }
    rgba
}

/// Does (`x`, `y`) hit art? Sprites are alpha 0/255 only (pixel rules) while the floor
/// grid and vignette are translucent, so only fully opaque pixels count: clicks on
/// the floor pass through to the app beneath, as in S1.
pub fn hit(c: &Canvas, x: usize, y: usize) -> bool {
    x < c.w && y < c.h && opaque(&c.px[(y * c.w + x) * 4..])
}

/// `hit` for every pixel at once, row-major into `mask` (`w*h` long): the panel's
/// click-through mask, rebuilt per presented frame.
pub fn hit_mask(c: &Canvas, mask: &mut [bool]) {
    for (m, p) in mask.iter_mut().zip(c.px.chunks_exact(4)) {
        *m = opaque(p);
    }
}

fn opaque(p: &[u8]) -> bool {
    p[3] == 255
}

#[cfg(test)]
mod tests;
