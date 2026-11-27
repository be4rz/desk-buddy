//! Frames: the area downscale and the PNG for the picture the agent may see. Pure; nothing is
//! written anywhere (the PNG is encoded into memory and handed to the reply).

use super::capture::Frame;

/// The longest edge of the picture shown to the model, in pixels.
pub const IMAGE_MAX_EDGE: u32 = 1280;
/// The edges tried in turn until the PNG is small enough.
pub const IMAGE_STEPS: [u32; 3] = [IMAGE_MAX_EDGE, 1024, 768];
/// The PNG's size limit, in bytes (the `look` tool's limit is about 3.4 MB).
pub const IMAGE_MAX_BYTES: usize = 3_000_000;

/// A PNG, in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Png {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// `frame` shrunk (never enlarged) so its longest edge is `max_edge`, by averaging every source pixel
/// of each target pixel's box, as 8-bit RGB (a screenshot is opaque).
pub fn downscale(frame: &Frame, max_edge: u32) -> (u32, u32, Vec<u8>) {
    let (sw, sh) = (frame.width as usize, frame.height as usize);
    let long = sw.max(sh).max(1);
    let (dw, dh) = if long as u32 <= max_edge {
        (sw, sh)
    } else {
        let k = max_edge as f64 / long as f64;
        (((sw as f64 * k).round() as usize).max(1), ((sh as f64 * k).round() as usize).max(1))
    };
    let mut out = Vec::with_capacity(dw * dh * 3);
    for dy in 0..dh {
        let (y0, y1) = (dy * sh / dh, ((dy + 1) * sh / dh).max(dy * sh / dh + 1).min(sh));
        for dx in 0..dw {
            let (x0, x1) = (dx * sw / dw, ((dx + 1) * sw / dw).max(dx * sw / dw + 1).min(sw));
            let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
            for y in y0..y1 {
                let row = &frame.bgra[(y * sw + x0) * 4..(y * sw + x1) * 4];
                for px in row.chunks_exact(4) {
                    b += px[0] as u64;
                    g += px[1] as u64;
                    r += px[2] as u64;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            out.extend_from_slice(&[((r + n / 2) / n) as u8, ((g + n / 2) / n) as u8, ((b + n / 2) / n) as u8]);
        }
    }
    (dw as u32, dh as u32, out)
}

fn png_of(w: u32, h: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut enc = png::Encoder::new(&mut buf, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgb).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(buf)
}

/// The first of `steps` (longest edges, largest first) whose PNG is under `max_bytes`.
pub fn encode_under(frame: &Frame, steps: &[u32], max_bytes: usize) -> Result<Png, String> {
    if frame.width == 0 || frame.height == 0 || frame.bgra.len() != frame.width as usize * frame.height as usize * 4 {
        return Err("The picture is empty.".into());
    }
    for &edge in steps {
        let (width, height, rgb) = downscale(frame, edge);
        let data = png_of(width, height, &rgb)?;
        if data.len() < max_bytes {
            return Ok(Png { data, width, height });
        }
    }
    Err("The picture is too large to send.".into())
}

/// The picture for the model: at most `IMAGE_MAX_EDGE` on a side, stepped down to 1024 then 768 until
/// the PNG is under `IMAGE_MAX_BYTES`.
pub fn encode(frame: &Frame) -> Result<Png, String> {
    encode_under(frame, &IMAGE_STEPS, IMAGE_MAX_BYTES)
}
