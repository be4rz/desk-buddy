//! The text-recognition side of screen look: an `Ocr` trait, its error type, `RealOcr` (Vision's
//! `VNRecognizeTextRequest`, on-device) and the fakes.

use objc2::rc::autoreleasepool;

use super::capture::Frame;

#[derive(Clone, Debug, PartialEq)]
pub enum OcrError {
    /// In a short sentence (never a path, never the text).
    Failed(String),
}

pub trait Ocr: Send + Sync {
    /// The recognized lines, top to bottom, then left to right. Called on the host thread.
    fn recognize(&self, frame: &Frame) -> Result<Vec<String>, OcrError>;
}

/// Lines below this confidence are dropped.
pub const MIN_CONFIDENCE: f32 = 0.3;

/// The real recognizer: Vision's `VNRecognizeTextRequest` (accurate, language correction, language
/// detection on), on-device. The frame is only read, never kept.
pub struct RealOcr;

impl Ocr for RealOcr {
    fn recognize(&self, frame: &Frame) -> Result<Vec<String>, OcrError> {
        autoreleasepool(|_| vision::recognize(frame))
    }
}

mod vision {
    use objc2::runtime::NSObjectProtocol;
    use objc2::AnyThread;
    use objc2::sel;
    use objc2_foundation::{NSArray, NSDictionary};
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

    use super::*;
    use crate::screen::cg;

    pub fn recognize(frame: &Frame) -> Result<Vec<String>, OcrError> {
        let image = cg::frame_to_image(frame).map_err(OcrError::Failed)?;
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        // macOS 13+: older systems keep the default (English) instead of crashing on the selector
        if request.respondsToSelector(sel!(setAutomaticallyDetectsLanguage:)) {
            request.setAutomaticallyDetectsLanguage(true);
        }
        // SAFETY: an empty options dictionary is valid.
        let handler = unsafe { VNImageRequestHandler::initWithCGImage_options(VNImageRequestHandler::alloc(), &image, &NSDictionary::new()) };
        let base: &VNRequest = &request;
        handler.performRequests_error(&NSArray::from_slice(&[base])).map_err(|e| OcrError::Failed(format!("Text recognition failed (error {}).", e.code())))?;

        let mut seen: Vec<Seen> = Vec::new();
        if let Some(results) = request.results() {
            for obs in results.iter() {
                let top = obs.topCandidates(1);
                let Some(best) = top.iter().next() else { continue };
                if best.confidence() < MIN_CONFIDENCE {
                    continue;
                }
                let b = unsafe { obs.boundingBox() };
                // Vision's box is normalized with the origin at the bottom left
                seen.push(Seen { text: best.string().to_string(), x: b.origin.x, top: b.origin.y + b.size.height, h: b.size.height });
            }
        }
        Ok(reading_order(seen))
    }
}

/// One recognized line and where it sat: `x` is its left edge and `top` its upper edge, normalized, the
/// origin at the bottom left (Vision's); `h` is its height.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub text: String,
    pub x: f64,
    pub top: f64,
    pub h: f64,
}

/// Top to bottom, then left to right: lines whose vertical centres are within half a line of the row's
/// first line share a row and are ordered by their left edge. Blank lines are dropped.
pub fn reading_order(mut lines: Vec<Seen>) -> Vec<String> {
    lines.retain(|l| !l.text.trim().is_empty());
    lines.sort_by(|a, b| b.top.total_cmp(&a.top).then(a.x.total_cmp(&b.x)));
    let mut rows: Vec<Vec<Seen>> = Vec::new();
    for l in lines {
        let same_row = rows.last().is_some_and(|r| {
            let first = &r[0];
            let (c0, c1) = (first.top - first.h / 2.0, l.top - l.h / 2.0);
            (c0 - c1).abs() < 0.5 * first.h.min(l.h)
        });
        if same_row {
            rows.last_mut().unwrap().push(l);
        } else {
            rows.push(vec![l]);
        }
    }
    rows.into_iter().flat_map(|mut r| {
        r.sort_by(|a, b| a.x.total_cmp(&b.x));
        r.into_iter().map(|l| l.text)
    }).collect()
}

#[cfg(any(test, debug_assertions))]
/// A scripted recognizer: these lines, or this error.
pub struct FakeOcr {
    pub lines: Vec<String>,
    pub error: Option<OcrError>,
}

#[cfg(any(test, debug_assertions))]
impl FakeOcr {
    pub fn lines<S: Into<String>>(lines: impl IntoIterator<Item = S>) -> FakeOcr {
        FakeOcr { lines: lines.into_iter().map(Into::into).collect(), error: None }
    }
}

#[cfg(any(test, debug_assertions))]
impl Ocr for FakeOcr {
    fn recognize(&self, _frame: &Frame) -> Result<Vec<String>, OcrError> {
        match &self.error {
            Some(e) => Err(e.clone()),
            None => Ok(self.lines.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(text: &str, x: f64, top: f64) -> Seen {
        Seen { text: text.into(), x, top, h: 0.02 }
    }

    #[test]
    fn lines_read_top_to_bottom_then_left_to_right() {
        let got = reading_order(vec![l("bottom", 0.1, 0.2), l("right", 0.6, 0.9), l("left", 0.1, 0.9), l("middle", 0.3, 0.5)]);
        assert_eq!(got, ["left", "right", "middle", "bottom"]);
    }

    #[test]
    fn a_slightly_lower_box_on_the_same_row_still_sorts_by_x() {
        let got = reading_order(vec![l("b", 0.5, 0.899), l("a", 0.1, 0.9), l("c", 0.1, 0.8)]);
        assert_eq!(got, ["a", "b", "c"]);
    }

    #[test]
    fn blank_lines_are_dropped_and_nothing_is_nothing() {
        assert_eq!(reading_order(vec![l("  ", 0.1, 0.9), l("x", 0.1, 0.5)]), ["x"]);
        assert!(reading_order(vec![]).is_empty());
    }

    /// Vision on a blank frame: no text, no error, and nothing real is read. (Runs the on-device
    /// recognizer on synthetic pixels only.)
    #[test]
    fn the_real_recognizer_finds_no_text_in_a_blank_synthetic_frame() {
        let f = Frame { width: 64, height: 64, bgra: vec![255; 64 * 64 * 4] };
        assert_eq!(RealOcr.recognize(&f), Ok(vec![]));
    }
}
