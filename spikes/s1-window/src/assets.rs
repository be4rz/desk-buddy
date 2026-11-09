//! Loads the pre-rendered bear clips described by `anim.json` into
//! premultiplied BGRA frames (the native CoreGraphics byte order).

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DIRS: [&str; 8] = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"];

#[derive(Deserialize)]
struct AnimFile {
    anims: HashMap<String, ClipJson>,
}

#[derive(Deserialize)]
struct ClipJson {
    #[serde(rename = "loop")]
    looping: bool,
    frame_w: usize,
    frame_h: usize,
    anchor: [i32; 2],
    frames_n: usize,
    files: HashMap<String, String>,
    frames: Vec<FrameJson>,
}

#[derive(Deserialize)]
struct FrameJson {
    dur: u64,
    root_px: HashMap<String, [f32; 2]>,
}

/// One frame: premultiplied BGRA pixels, `w*h*4` bytes.
pub struct Frame {
    pub px: Vec<u8>,
    /// content hash: identical frames (idle holds) share it, so the renderer
    /// can skip re-uploading a frame that would look the same
    pub id: u64,
}

pub struct Clip {
    /// from anim.json; the behaviour loop decides repeats itself
    #[allow(dead_code)]
    pub looping: bool,
    pub w: usize,
    pub h: usize,
    pub anchor: [i32; 2],
    pub durs_ms: Vec<u64>,
    /// per direction: frames and per-frame root motion in screen px
    pub dirs: HashMap<&'static str, (Vec<Frame>, Vec<[f32; 2]>)>,
}

impl Clip {
    pub fn len(&self) -> usize {
        self.durs_ms.len()
    }
    pub fn has_dir(&self, d: &str) -> bool {
        self.dirs.contains_key(d)
    }
}

pub struct Library {
    pub clips: HashMap<String, Clip>,
}

pub fn default_anim_dir() -> PathBuf {
    if let Ok(p) = std::env::var("S1_ANIM_DIR") {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../experiments/mascot/out/bear3d/anim")
}

pub fn load(dir: &Path) -> Result<Library, String> {
    let json = std::fs::read_to_string(dir.join("anim.json"))
        .map_err(|e| format!("read anim.json in {}: {e}", dir.display()))?;
    let file: AnimFile = serde_json::from_str(&json).map_err(|e| format!("anim.json: {e}"))?;
    let mut clips = HashMap::new();
    for (name, c) in file.anims {
        let mut dirs = HashMap::new();
        for d in DIRS {
            let Some(fname) = c.files.get(d) else { continue };
            let (w, h, rgba) = read_png(&dir.join(fname))?;
            if h != c.frame_h || w < c.frame_w * c.frames_n {
                return Err(format!("{fname}: {w}x{h} does not fit {} frames", c.frames_n));
            }
            let mut frames = Vec::with_capacity(c.frames_n);
            for f in 0..c.frames_n {
                let mut px = vec![0u8; c.frame_w * c.frame_h * 4];
                for y in 0..c.frame_h {
                    for x in 0..c.frame_w {
                        let s = (y * w + f * c.frame_w + x) * 4;
                        let o = (y * c.frame_w + x) * 4;
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
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    px.hash(&mut h);
                    h.finish()
                };
                frames.push(Frame { px, id });
            }
            let roots = c
                .frames
                .iter()
                .map(|fr| fr.root_px.get(d).copied().unwrap_or([0.0, 0.0]))
                .collect();
            dirs.insert(d, (frames, roots));
        }
        let durs_ms = c.frames.iter().map(|f| f.dur.max(1)).collect();
        clips.insert(
            name,
            Clip { looping: c.looping, w: c.frame_w, h: c.frame_h, anchor: c.anchor, durs_ms, dirs },
        );
    }
    Ok(Library { clips })
}

fn read_png(p: &Path) -> Result<(usize, usize, Vec<u8>), String> {
    let f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| format!("{}: {e}", p.display()))?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| format!("{}: {e}", p.display()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf[..w * h * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect()
        }
        png::ColorType::Grayscale => buf[..w * h].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(format!("{}: indexed not expanded", p.display())),
    };
    Ok((w, h, rgba))
}
