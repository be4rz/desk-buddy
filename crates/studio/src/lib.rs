//! Studio (design `## studio`, §4.10): everything the studio window and the headless
//! `buddy studio …` commands do, as plain functions over a pack. It only *shows and checks*
//! art: every sprite pixel comes from the pack's PNGs through `mascot-render`; the only pixels
//! generated here are backgrounds, the diff overlay and the audit chart. Output is
//! deterministic: the same inputs give the same bytes.

pub mod aseprite;
pub mod audit;
pub mod comments;
pub mod diff;
pub mod frames;
pub mod lint;
pub mod session;
pub mod sim;

use std::path::Path;

pub use frames::{gif, render, sheet, Bg, Frame, RenderOpts};

/// A studio failure: one line, names the file when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

impl From<mascot_render::RenderError> for Error {
    fn from(e: mascot_render::RenderError) -> Self {
        Error(e.to_string())
    }
}
impl From<mascot_core::PackError> for Error {
    fn from(e: mascot_core::PackError) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

/// Straight RGBA as PNG bytes (8-bit RGBA, the png crate's default filter and compression:
/// the same pixels always give the same bytes).
pub fn png_bytes(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w as u32, h as u32);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().expect("png header to memory");
        wr.write_image_data(rgba).expect("png data to memory");
    }
    out
}

/// Write `bytes` to `path` (its parent made first).
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| Error(format!("{}: {e}", d.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| Error(format!("{}: {e}", path.display())))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Standard base64 with padding (data: URLs for the studio window).
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// `secs` since the Unix epoch as RFC 3339 UTC (`2026-11-10T12:00:00Z`).
pub fn rfc3339(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // civil from days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_and_time() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_794_312_000), "2026-11-10T12:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }
}
