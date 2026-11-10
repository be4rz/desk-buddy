//! PNG decode to straight RGBA (S1's `read_png`) and an RGBA writer.

use std::path::Path;

use crate::RenderError;

/// Decode any non-indexed-after-expansion PNG to straight RGBA: (w, h, bytes).
pub fn read_png(p: &Path) -> Result<(usize, usize, Vec<u8>), RenderError> {
    let err = |msg: String| RenderError::Read { path: p.into(), msg };
    let f = std::fs::File::open(p).map_err(|e| err(e.to_string()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| err(e.to_string()))?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| err(e.to_string()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..w * h * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..w * h].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(err("indexed PNG not expanded".into())),
    };
    Ok((w, h, rgba))
}

/// Write straight RGBA as an 8-bit PNG.
pub fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let mut wr = e.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)
}
