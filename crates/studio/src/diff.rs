//! `diff a.png b.png`: how many pixels differ and where. Two fully transparent pixels are
//! equal whatever their colour bytes.

use serde::Serialize;

use crate::{err, Result};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DiffReport {
    pub changed_px: usize,
    /// `[x, y, w, h]` of the changed pixels, `null` when none changed.
    pub bbox: Option<[usize; 4]>,
    pub w: usize,
    pub h: usize,
}

/// Compare two straight-RGBA images of the same size; the overlay (when wanted) shows `a`
/// faded with every changed pixel magenta.
pub fn diff(a: (usize, usize, &[u8]), b: (usize, usize, &[u8]), overlay: bool) -> Result<(DiffReport, Option<Vec<u8>>)> {
    let ((aw, ah, ap), (bw, bh, bp)) = (a, b);
    if (aw, ah) != (bw, bh) {
        return err(format!("sizes differ: {aw}x{ah} vs {bw}x{bh}"));
    }
    let (mut n, mut x0, mut y0, mut x1, mut y1) = (0, usize::MAX, usize::MAX, 0, 0);
    let mut ov = overlay.then(|| vec![0u8; aw * ah * 4]);
    for y in 0..ah {
        for x in 0..aw {
            let i = (y * aw + x) * 4;
            let (p, q) = (&ap[i..i + 4], &bp[i..i + 4]);
            let same = p == q || (p[3] == 0 && q[3] == 0);
            if let Some(o) = ov.as_mut() {
                o[i..i + 4].copy_from_slice(&if same { [p[0], p[1], p[2], p[3] / 3] } else { [255, 0, 255, 255] });
            }
            if !same {
                n += 1;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    let bbox = (n > 0).then(|| [x0, y0, x1 - x0, y1 - y0]);
    Ok((DiffReport { changed_px: n, bbox, w: aw, h: ah }, ov))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_changed_pixel() {
        let a = vec![0u8; 4 * 4 * 4];
        let mut b = a.clone();
        b[(2 * 4 + 1) * 4..(2 * 4 + 1) * 4 + 4].copy_from_slice(&[1, 2, 3, 255]);
        let (r, ov) = diff((4, 4, &a), (4, 4, &b), true).unwrap();
        assert_eq!((r.changed_px, r.bbox), (1, Some([1, 2, 1, 1])));
        assert_eq!(&ov.unwrap()[(2 * 4 + 1) * 4..(2 * 4 + 1) * 4 + 4], &[255, 0, 255, 255]);
        // transparent pixels with different colour bytes are the same
        let mut c = a.clone();
        c[0] = 9;
        assert_eq!(diff((4, 4, &a), (4, 4, &c), false).unwrap().0.changed_px, 0);
        assert!(diff((4, 4, &a), (2, 8, &a), false).is_err());
    }
}
