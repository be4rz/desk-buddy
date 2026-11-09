//! CPU composite at sprite-pixel resolution: a faint 2:1 iso floor grid that
//! scrolls with the follow camera, faded by a soft vignette, with the bear
//! blitted on top at a fixed (centred) position. Output is premultiplied
//! BGRA; the window scales it up by a whole number with nearest filtering.

use crate::assets::Frame;

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    /// where the sprite anchor (floor point between the feet) sits
    pub anchor: [i32; 2],
    /// iso grid spacing in sprite px: tiles are 2T wide and T tall (2:1)
    pub tile: i32,
    /// premultiplied BGRA (as little-endian u32) per pixel, vignette applied
    line_px: Vec<u32>,
    fill_px: Vec<u32>,
    /// one (2T x T) period of the grid: true on a line pixel
    pattern: Vec<bool>,
    bg: Vec<u8>,
    bg_cam: Option<[i32; 2]>,
    pub out: Vec<u8>,
    /// true where the bear is opaque (alpha > 0): the hit-test mask
    pub hit: Vec<bool>,
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Canvas {
    pub fn new(w: usize, h: usize, anchor: [i32; 2], tile: i32) -> Self {
        let mut vignette = vec![0u8; w * h];
        let (cx, cy) = (w as f32 / 2.0, h as f32 * 0.55);
        for y in 0..h {
            for x in 0..w {
                let dx = (x as f32 + 0.5 - cx) / (w as f32 / 2.0);
                let dy = (y as f32 + 0.5 - cy) / if (y as f32) < cy { cy } else { h as f32 - cy };
                let r = (dx * dx + dy * dy).sqrt();
                let v = 1.0 - smoothstep(0.35, 0.97, r);
                vignette[y * w + x] = (v * 255.0).round() as u8;
            }
        }
        // colours: (r, g, b) straight, peak alpha before the vignette
        let pm = |rgb: [u32; 3], a0: u32, v: u8| -> u32 {
            let a = a0 * v as u32 / 255;
            let c = |x: u32| x * a / 255;
            c(rgb[2]) | (c(rgb[1]) << 8) | (c(rgb[0]) << 16) | (a << 24)
        };
        let line_px = vignette.iter().map(|&v| pm([92, 120, 150], 120, v)).collect();
        let fill_px = vignette.iter().map(|&v| pm([150, 176, 196], 34, v)).collect();
        let pw = 2 * tile;
        let mut pattern = vec![false; (pw * tile) as usize];
        for y in 0..tile {
            for x in 0..pw {
                let hx = x.div_euclid(2);
                pattern[(y * pw + x) as usize] = (y - hx).rem_euclid(tile) == 0 || (y + hx).rem_euclid(tile) == 0;
            }
        }
        Canvas {
            w,
            h,
            anchor,
            tile,
            line_px,
            fill_px,
            pattern,
            bg: vec![0; w * h * 4],
            bg_cam: None,
            out: vec![0; w * h * 4],
            hit: vec![false; w * h],
        }
    }

    /// Redraw the floor for camera offset `cam` (world px of the canvas origin).
    /// The 2:1 grid repeats every (2T, T) px, so it is a table lookup; the
    /// line and fill colours are pre-multiplied by the vignette per pixel.
    fn floor(&mut self, cam: [i32; 2]) {
        if self.bg_cam == Some(cam) {
            return;
        }
        self.bg_cam = Some(cam);
        let (t, w) = (self.tile, self.w);
        let pw = 2 * t;
        let x0 = cam[0].rem_euclid(pw) as usize;
        let mut ty = cam[1].rem_euclid(t) as usize;
        let out = &mut self.bg;
        for y in 0..self.h {
            let row = &self.pattern[ty * pw as usize..(ty + 1) * pw as usize];
            let mut tx = x0;
            for x in 0..w {
                let i = y * w + x;
                let px = if row[tx] { self.line_px[i] } else { self.fill_px[i] };
                out[i * 4..i * 4 + 4].copy_from_slice(&px.to_le_bytes());
                tx += 1;
                if tx == pw as usize {
                    tx = 0;
                }
            }
            ty += 1;
            if ty == t as usize {
                ty = 0;
            }
        }
    }

    /// Composite one frame. `cam` is the follow camera's integer world offset.
    pub fn compose(&mut self, cam: [i32; 2], sprite: &Frame, sw: usize, sh: usize, sanchor: [i32; 2]) {
        self.floor(cam);
        self.out.copy_from_slice(&self.bg);
        self.hit.iter_mut().for_each(|h| *h = false);
        let ox = self.anchor[0] - sanchor[0];
        let oy = self.anchor[1] - sanchor[1];
        for sy in 0..sh as i32 {
            let y = oy + sy;
            if y < 0 || y >= self.h as i32 {
                continue;
            }
            for sx in 0..sw as i32 {
                let x = ox + sx;
                if x < 0 || x >= self.w as i32 {
                    continue;
                }
                let s = ((sy as usize) * sw + sx as usize) * 4;
                let sa = sprite.px[s + 3] as u32;
                if sa == 0 {
                    continue;
                }
                let i = y as usize * self.w + x as usize;
                self.hit[i] = true;
                let o = i * 4;
                let inv = 255 - sa;
                for c in 0..4 {
                    let d = self.out[o + c] as u32;
                    self.out[o + c] = (sprite.px[s + c] as u32 + (d * inv + 127) / 255).min(255) as u8;
                }
            }
        }
    }
}
