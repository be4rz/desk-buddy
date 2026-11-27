//! CoreGraphics glue for screen look: a `CGImage` read back into a BGRA `Frame` (scaled down to the
//! size the recognizer wants) and a `Frame` turned back into a `CGImage` for Vision. Memory only: no
//! disk, no log. `CGBitmapContextCreate` is not in the generated `objc2-core-graphics` bindings, so it
//! is declared by hand below (plan C.11: a hand-written `extern "C"`, never a shim).

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage, CGInterpolationQuality};

use super::capture::Frame;

/// `kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little`: BGRA in memory, what `Frame` holds.
const BGRA_INFO: u32 = 2 | (2 << 12);

extern "C-unwind" {
    // https://developer.apple.com/documentation/coregraphics/cgbitmapcontextcreate(_:_:_:_:_:_:_:)
    fn CGBitmapContextCreate(data: *mut c_void, width: usize, height: usize, bits_per_component: usize, bytes_per_row: usize, space: Option<&CGColorSpace>, bitmap_info: u32) -> Option<NonNull<CGContext>>;
}

/// `(w, h)` scaled down, keeping the aspect, so the longer edge is at most `max_edge`; never up.
pub fn fit_edge(w: usize, h: usize, max_edge: u32) -> (usize, usize) {
    let max = max_edge.max(1) as usize;
    let long = w.max(h);
    if long <= max || long == 0 {
        return (w.max(1), h.max(1));
    }
    let scale = |v: usize| ((v * max + long / 2) / long).max(1);
    (scale(w), scale(h))
}

/// A BGRA bitmap context over `buf` (`w * h * 4` bytes, sRGB).
fn context(buf: &mut [u8], w: usize, h: usize) -> Result<CFRetained<CGContext>, String> {
    debug_assert!(buf.len() >= w * h * 4);
    let space = CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB })).ok_or("no colour space")?;
    // SAFETY: `buf` is valid for `w * h * 4` bytes and outlives every use of the context in the callers.
    let ctx = unsafe { CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), BGRA_INFO) }.ok_or("no bitmap context")?;
    Ok(unsafe { CFRetained::from_raw(ctx) })
}

/// Read `image` back into a `Frame`, scaled down so its longer edge is at most `max_edge`.
pub fn image_to_frame(image: &CGImage, max_edge: u32) -> Result<Frame, String> {
    let (iw, ih) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    if iw == 0 || ih == 0 {
        return Err("The screen picture was empty.".into());
    }
    let (w, h) = fit_edge(iw, ih, max_edge);
    let mut bgra = vec![0u8; w * h * 4];
    {
        let ctx = context(&mut bgra, w, h).map_err(|e| format!("Couldn’t read the screen picture ({e})."))?;
        CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::High);
        CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(image));
    }
    Ok(Frame { width: w as u32, height: h as u32, bgra })
}

/// A `CGImage` holding a copy of the frame's pixels (for Vision).
pub fn frame_to_image(frame: &Frame) -> Result<CFRetained<CGImage>, String> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w == 0 || h == 0 || frame.bgra.len() != w * h * 4 {
        return Err("The screen picture has a wrong size.".into());
    }
    let mut copy = frame.bgra.clone();
    let ctx = context(&mut copy, w, h).map_err(|e| format!("Couldn’t prepare the picture for reading ({e})."))?;
    // the image is a copy of the bitmap (CoreGraphics copies on create), so `copy` may drop after this
    CGBitmapContextCreateImage(Some(&ctx)).ok_or_else(|| "Couldn’t prepare the picture for reading.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_edge_only_scales_down_and_keeps_the_aspect() {
        assert_eq!(fit_edge(2880, 1800, 2048), (2048, 1280));
        assert_eq!(fit_edge(1440, 900, 2048), (1440, 900));
        assert_eq!(fit_edge(900, 3000, 2048), (614, 2048));
        assert_eq!(fit_edge(1, 5000, 2048), (1, 2048));
    }

    /// Pure CoreGraphics, no screen involved: a frame goes to a CGImage and back unchanged, and a big
    /// one comes back smaller.
    #[test]
    fn frame_round_trips_through_a_cgimage() {
        let (w, h) = (6usize, 4usize);
        let mut bgra = Vec::new();
        for y in 0..h {
            for x in 0..w {
                bgra.extend_from_slice(&[(x * 40) as u8, (y * 60) as u8, 200, 255]);
            }
        }
        let f = Frame { width: w as u32, height: h as u32, bgra };
        let img = frame_to_image(&f).unwrap();
        assert_eq!((CGImage::width(Some(&img)), CGImage::height(Some(&img))), (w, h));
        assert_eq!(image_to_frame(&img, 2048).unwrap(), f);
        let small = image_to_frame(&img, 3).unwrap();
        assert_eq!((small.width, small.height), (3, 2));
        assert_eq!(small.bgra.len(), 3 * 2 * 4);
    }

    #[test]
    fn a_wrong_sized_frame_is_an_error_not_a_crash() {
        assert!(frame_to_image(&Frame { width: 4, height: 4, bgra: vec![0; 10] }).is_err());
        assert!(frame_to_image(&Frame { width: 0, height: 0, bgra: vec![] }).is_err());
    }
}
