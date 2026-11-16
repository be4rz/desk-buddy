//! `render`, `sheet` and `gif`: pack frames composed by `mascot-render` (the app's own
//! renderer) and cropped to the clip's frame box, so a render is exactly what the bear shows.

use std::borrow::Cow;

use mascot_core::{Dir, EyeState, Intent, MouthState, Ori, PackMeta, Show, Visibility};
use mascot_render::{compose, to_rgba, upscale, Art, Canvas, View};

use crate::{err, Error, Result};

/// One frame to render. `view` is `D` (a direction, free clips) or `A`/`B` (set clips).
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOpts {
    pub clip: String,
    pub view: String,
    pub frame: usize,
    pub zoom: usize,
    pub eyes: EyeState,
    pub mouth: MouthState,
}

/// A rendered frame: straight RGBA, `w*h*4`.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

impl Frame {
    pub fn png(&self) -> Vec<u8> {
        crate::png_bytes(self.w, self.h, &self.rgba)
    }
}

pub fn parse_eyes(s: &str) -> Option<EyeState> {
    match s {
        "open" => Some(EyeState::Open),
        "closed" => Some(EyeState::Closed),
        _ => None,
    }
}

pub fn parse_mouth(s: &str) -> Option<MouthState> {
    match s {
        "closed" => Some(MouthState::Closed),
        "open" => Some(MouthState::Open),
        "wide" => Some(MouthState::Wide),
        _ => None,
    }
}

/// The views (`S`… for free clips, `A`/`B` for set clips) a clip has art for, in `Dir::ALL` order.
pub fn views(pack: &PackMeta, clip: &str) -> Vec<String> {
    let Some(c) = pack.clips.get(clip) else { return vec![] };
    if c.set.is_some() {
        [Ori::A, Ori::B].into_iter().filter(|o| c.oris.contains_key(o)).map(|o| ori_str(o).to_string()).collect()
    } else {
        Dir::ALL.into_iter().filter(|d| c.dirs.contains_key(d)).map(|d| d.as_str().to_string()).collect()
    }
}

pub fn ori_str(o: Ori) -> &'static str {
    match o {
        Ori::A => "A",
        Ori::B => "B",
    }
}

/// The `Show` that draws `opts` (nothing scheduled: a still pose).
pub fn show_of(pack: &PackMeta, opts: &RenderOpts) -> Result<Show> {
    let Some(c) = pack.clips.get(&opts.clip) else {
        return err(format!("pack {} has no clip “{}”", pack.id, opts.clip));
    };
    let (dir, ori) = if c.set.is_some() {
        let Some(o) = Ori::parse(&opts.view).filter(|o| c.oris.contains_key(o)) else {
            return err(format!("clip “{}” is a set clip: --dir A or B (it has {})", opts.clip, views(pack, &opts.clip).join(", ")));
        };
        (Dir::S, Some(o))
    } else {
        let Some(d) = Dir::parse(&opts.view).filter(|d| c.dirs.contains_key(d)) else {
            return err(format!("clip “{}” has no direction “{}” (it has {})", opts.clip, opts.view, views(pack, &opts.clip).join(", ")));
        };
        (d, None)
    };
    let n = c.durs_ms.len();
    if opts.frame >= n {
        return err(format!("clip “{}” has {n} frames (0..{}), not frame {}", opts.clip, n.saturating_sub(1), opts.frame));
    }
    Ok(Show {
        intent: Intent::Idle,
        behavior: String::new(),
        clip: opts.clip.clone(),
        dir,
        ori,
        frame: opts.frame as u16,
        eyes: opts.eyes,
        mouth: opts.mouth,
        effect: None,
        visibility: Visibility::Shown,
        moved_px: [0, 0],
        cut: false,
        place: None,
    })
}

/// The canvas rectangle `[x, y, w, h]` the clip's frame occupies (set clips: the set's frame).
pub fn frame_box(pack: &PackMeta, art: &Art, clip: &str, ori: Option<Ori>) -> Option<[i32; 4]> {
    let c = pack.clips.get(clip)?;
    let [ax, ay] = art.anchor();
    match (&c.set, ori) {
        (Some(set), Some(o)) => {
            let so = pack.sets.get(set)?.oris.get(&o)?;
            let h = pack.clips.values().filter(|k| k.set.as_deref() == Some(set)).find_map(|k| k.handoff.get(&o).map(|h| h.px))?;
            Some([ax - h[0] - so.anchor[0], ay - h[1] - so.anchor[1], so.frame_w as i32, so.frame_h as i32])
        }
        (Some(_), None) => None,
        (None, _) => Some([ax - c.anchor[0], ay - c.anchor[1], c.frame_w as i32, c.frame_h as i32]),
    }
}

/// Copy `[x, y, w, h]` out of `c` (outside the canvas reads as transparent).
pub fn crop(c: &Canvas, r: [i32; 4]) -> Canvas {
    let [x0, y0, w, h] = r;
    let mut out = Canvas::new(w.max(0) as usize, h.max(0) as usize);
    for y in 0..h {
        let sy = y0 + y;
        if sy < 0 || sy >= c.h as i32 {
            continue;
        }
        for x in 0..w {
            let sx = x0 + x;
            if sx < 0 || sx >= c.w as i32 {
                continue;
            }
            let s = (sy as usize * c.w + sx as usize) * 4;
            let o = (y as usize * out.w + x as usize) * 4;
            out.px[o..o + 4].copy_from_slice(&c.px[s..s + 4]);
        }
    }
    out
}

/// One frame, composed by `mascot-render` (no floor), cropped to its frame box, upscaled.
pub fn render(pack: &PackMeta, art: &Art, opts: &RenderOpts) -> Result<Frame> {
    let show = show_of(pack, opts)?;
    let canvas = composed(pack, art, &show)?;
    let up = upscale(&canvas, opts.zoom.clamp(1, 16));
    Ok(Frame { w: up.w, h: up.h, rgba: to_rgba(&up) })
}

/// `show` composed and cropped to its clip's frame box, at sprite resolution (premultiplied).
pub fn composed(pack: &PackMeta, art: &Art, show: &Show) -> Result<Canvas> {
    let mut c = art.canvas();
    compose(art, show, &View { cam: [0, 0], floor: false, vignette: false, ..View::default() }, &mut c);
    let r = frame_box(pack, art, &show.clip, show.ori).ok_or_else(|| Error(format!("clip “{}” has no frame box", show.clip)))?;
    Ok(crop(&c, r))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bg {
    None,
    Grid,
    Desktop,
}

impl Bg {
    pub fn parse(s: &str) -> Option<Bg> {
        match s {
            "none" => Some(Bg::None),
            "grid" => Some(Bg::Grid),
            "desktop" => Some(Bg::Desktop),
            _ => None,
        }
    }

    /// The backdrop colour at (x, y): a checker for `grid` (8 px), a flat blue for `desktop`.
    fn at(self, x: usize, y: usize) -> [u8; 4] {
        match self {
            Bg::None => [0, 0, 0, 0],
            Bg::Grid => {
                if (x / 8 + y / 8) % 2 == 0 {
                    [0x2b, 0x2d, 0x36, 255]
                } else {
                    [0x35, 0x38, 0x43, 255]
                }
            }
            Bg::Desktop => [0x3a, 0x6e, 0xa5, 255],
        }
    }
}

/// One row of a sheet: which clip/view it shows and where.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SheetRow {
    pub clip: String,
    pub view: String,
    pub y: usize,
    pub frames: usize,
}

/// A contact sheet: one row per clip and view, every frame left to right in cells of the
/// largest frame box, each frame bottom-centred in its cell. `clip = None` = every clip.
/// `zoom` is 1–16; the zoomed sheet is at most 64 M px.
pub fn sheet(pack: &PackMeta, art: &Art, clip: Option<&str>, zoom: usize, bg: Bg) -> Result<(Frame, Vec<SheetRow>)> {
    let zoom = zoom.clamp(1, 16);
    let clips: Vec<&String> = match clip {
        Some(c) => vec![pack.clips.get_key_value(c).map(|(k, _)| k).ok_or_else(|| Error(format!("pack {} has no clip “{c}”", pack.id)))?],
        None => pack.clips.keys().collect(),
    };
    let mut rows = vec![];
    for c in &clips {
        for v in views(pack, c) {
            rows.push(((*c).clone(), v));
        }
    }
    if rows.is_empty() {
        return err("nothing to draw");
    }
    let (mut cw, mut ch, mut cols) = (1i32, 1i32, 1usize);
    for (c, v) in &rows {
        let b = frame_box(pack, art, c, Ori::parse(v)).ok_or_else(|| Error(format!("clip “{c}” has no frame box")))?;
        cw = cw.max(b[2]);
        ch = ch.max(b[3]);
        cols = cols.max(pack.clips[c].durs_ms.len());
    }
    let (cw, ch) = (cw as usize, ch as usize);
    let (w, h) = (cols * cw, rows.len() * ch);
    if (w * zoom).saturating_mul(h * zoom) > 64 << 20 {
        return err(format!("the sheet would be {}x{} px; pick one clip with --clip or a smaller --zoom", w * zoom, h * zoom));
    }
    let mut sheet = Canvas::new(w, h);
    let mut out_rows = vec![];
    for (r, (c, v)) in rows.iter().enumerate() {
        let n = pack.clips[c].durs_ms.len();
        for f in 0..n {
            let opts = RenderOpts { clip: c.clone(), view: v.clone(), frame: f, zoom: 1, eyes: EyeState::Open, mouth: MouthState::Closed };
            let fr = composed(pack, art, &show_of(pack, &opts)?)?;
            let (ox, oy) = (f * cw + (cw - fr.w) / 2, r * ch + (ch - fr.h));
            for y in 0..fr.h {
                let s = y * fr.w * 4;
                let o = ((oy + y) * w + ox) * 4;
                sheet.px[o..o + fr.w * 4].copy_from_slice(&fr.px[s..s + fr.w * 4]);
            }
        }
        out_rows.push(SheetRow { clip: c.clone(), view: v.clone(), y: r * ch * zoom, frames: n });
    }
    let up = upscale(&sheet, zoom);
    let mut rgba = to_rgba(&up);
    if bg != Bg::None {
        for y in 0..up.h {
            for x in 0..up.w {
                let i = (y * up.w + x) * 4;
                if rgba[i + 3] == 0 {
                    rgba[i..i + 4].copy_from_slice(&bg.at(x, y));
                }
            }
        }
    }
    Ok((Frame { w: up.w, h: up.h, rgba }, out_rows))
}

/// An animated GIF of one clip/view, looping, with the clip's frame durations
/// (centiseconds, rounded). Colours are the frames' own (≤ 255 plus transparent).
pub fn gif(pack: &PackMeta, art: &Art, clip: &str, view: &str, zoom: usize) -> Result<Vec<u8>> {
    let c = pack.clips.get(clip).ok_or_else(|| Error(format!("pack {} has no clip “{clip}”", pack.id)))?;
    let mut frames = vec![];
    for f in 0..c.durs_ms.len() {
        let opts = RenderOpts { clip: clip.into(), view: view.into(), frame: f, zoom, eyes: EyeState::Open, mouth: MouthState::Closed };
        frames.push(render(pack, art, &opts)?);
    }
    let (w, h) = (frames[0].w, frames[0].h);
    if w > u16::MAX as usize || h > u16::MAX as usize {
        return err("too large for a GIF");
    }
    // palette: index 0 transparent, then colours in first-seen order (deterministic)
    let mut pal: Vec<[u8; 3]> = vec![[0, 0, 0]];
    let mut index = std::collections::HashMap::new();
    let mut bufs = vec![];
    for fr in &frames {
        let mut buf = Vec::with_capacity(w * h);
        for p in fr.rgba.chunks_exact(4) {
            if p[3] == 0 {
                buf.push(0);
                continue;
            }
            if p[3] != 255 {
                return err(format!("clip “{clip}” has partly transparent pixels; GIF can't show them (run lint)"));
            }
            let k = [p[0], p[1], p[2]];
            let i = match index.get(&k) {
                Some(i) => *i,
                None => {
                    if pal.len() == 256 {
                        return err(format!("clip “{clip}” uses more than 255 colours"));
                    }
                    pal.push(k);
                    index.insert(k, (pal.len() - 1) as u8);
                    (pal.len() - 1) as u8
                }
            };
            buf.push(i);
        }
        bufs.push(buf);
    }
    while !pal.len().is_power_of_two() || pal.len() < 2 {
        pal.push([0, 0, 0]);
    }
    let flat: Vec<u8> = pal.iter().flatten().copied().collect();
    let mut out = Vec::new();
    {
        let mut e = ::gif::Encoder::new(&mut out, w as u16, h as u16, &flat).map_err(|e| Error(e.to_string()))?;
        e.set_repeat(::gif::Repeat::Infinite).map_err(|e| Error(e.to_string()))?;
        for (i, buf) in bufs.iter().enumerate() {
            let fr = ::gif::Frame {
                delay: ((c.durs_ms[i] + 5) / 10).max(1) as u16,
                dispose: ::gif::DisposalMethod::Background,
                transparent: Some(0),
                width: w as u16,
                height: h as u16,
                buffer: Cow::Borrowed(buf),
                ..Default::default()
            };
            e.write_frame(&fr).map_err(|e| Error(e.to_string()))?;
        }
    }
    Ok(out)
}
