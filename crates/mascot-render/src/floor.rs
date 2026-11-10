//! S1's faint 2:1 iso floor grid with a soft vignette (moved from `render.rs`).
//! The grid repeats every (2T, T) px, so drawing it is a table lookup; line and fill
//! colours are pre-multiplied by the vignette per pixel.

pub(crate) struct Floor {
    w: usize,
    h: usize,
    tile: i32,
    /// with vignette / without, premultiplied BGRA as little-endian u32
    line_v: Vec<u32>,
    fill_v: Vec<u32>,
    line: u32,
    fill: u32,
    /// one (2T x T) period: true on a line pixel
    pattern: Vec<bool>,
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn pm(rgb: [u32; 3], a0: u32, v: u8) -> u32 {
    let a = a0 * v as u32 / 255;
    let c = |x: u32| x * a / 255;
    c(rgb[2]) | (c(rgb[1]) << 8) | (c(rgb[0]) << 16) | (a << 24)
}

const LINE: ([u32; 3], u32) = ([92, 120, 150], 120);
const FILL: ([u32; 3], u32) = ([150, 176, 196], 34);

impl Floor {
    pub fn new(w: usize, h: usize, tile: i32) -> Floor {
        let mut vignette = vec![0u8; w * h];
        let (cx, cy) = (w as f32 / 2.0, h as f32 * 0.55);
        for y in 0..h {
            for x in 0..w {
                let dx = (x as f32 + 0.5 - cx) / (w as f32 / 2.0);
                let dy = (y as f32 + 0.5 - cy) / if (y as f32) < cy { cy } else { h as f32 - cy };
                let r = (dx * dx + dy * dy).sqrt();
                vignette[y * w + x] = ((1.0 - smoothstep(0.35, 0.97, r)) * 255.0).round() as u8;
            }
        }
        let pw = 2 * tile;
        let mut pattern = vec![false; (pw * tile) as usize];
        for y in 0..tile {
            for x in 0..pw {
                let hx = x.div_euclid(2);
                pattern[(y * pw + x) as usize] = (y - hx).rem_euclid(tile) == 0 || (y + hx).rem_euclid(tile) == 0;
            }
        }
        Floor {
            w,
            h,
            tile,
            line_v: vignette.iter().map(|&v| pm(LINE.0, LINE.1, v)).collect(),
            fill_v: vignette.iter().map(|&v| pm(FILL.0, FILL.1, v)).collect(),
            line: pm(LINE.0, LINE.1, 255),
            fill: pm(FILL.0, FILL.1, 255),
            pattern,
        }
    }

    /// `cam` = world px of the canvas origin's offset (the follow camera).
    pub fn draw(&self, cam: [i32; 2], vignette: bool, out: &mut [u8]) {
        let (t, w) = (self.tile, self.w);
        let pw = (2 * t) as usize;
        let x0 = cam[0].rem_euclid(pw as i32) as usize;
        let mut ty = cam[1].rem_euclid(t) as usize;
        for y in 0..self.h {
            let row = &self.pattern[ty * pw..(ty + 1) * pw];
            let mut tx = x0;
            for x in 0..w {
                let i = y * w + x;
                let px = match (row[tx], vignette) {
                    (true, true) => self.line_v[i],
                    (false, true) => self.fill_v[i],
                    (true, false) => self.line,
                    (false, false) => self.fill,
                };
                out[i * 4..i * 4 + 4].copy_from_slice(&px.to_le_bytes());
                tx += 1;
                if tx == pw {
                    tx = 0;
                }
            }
            ty += 1;
            if ty == t as usize {
                ty = 0;
            }
        }
    }
}
