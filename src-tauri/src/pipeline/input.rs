//! AnalysisInput: the decoded image that an analyzer reads. The decode is in the pipeline, not in the
//! analyzer, so that analyzers with the same `InputSpec` can share 1 decode.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputSpec {
    /// 8-bit luminance with a long edge of at most `edge` px, EXIF orientation applied.
    Gray { edge: u32 },
}

/// A row-major 8-bit grayscale buffer; the row stride is `width`.
#[derive(Clone, Debug)]
pub struct AnalysisInput {
    pub width: usize,
    pub height: usize,
    pub gray: Vec<u8>,
}

impl AnalysisInput {
    pub fn from_gray(width: usize, height: usize, gray: Vec<u8>) -> Self {
        assert_eq!(gray.len(), width * height);
        Self { width, height, gray }
    }
}

/// Decodes `path` for the spec: `decode::thumbnail_image` (ImageIO subsampling), then a draw into
/// an 8-bit `DeviceGray` bitmap context, which does the color conversion in native code.
pub fn decode_input(path: &Path, spec: InputSpec) -> Option<AnalysisInput> {
    match spec {
        InputSpec::Gray { edge } => decode_gray(path, edge),
    }
}

/// Decodes the input that `analyzer` needs.
pub fn decode_input_for<A: super::ImageAnalyzer>(analyzer: &A, path: &Path) -> Option<AnalysisInput> {
    decode_input(path, analyzer.input())
}

fn decode_gray(path: &Path, edge: u32) -> Option<AnalysisInput> {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality};

    let img = crate::decode::thumbnail_image(path, edge)?;
    let (iw, ih) = (CGImage::width(Some(&img)) as f64, CGImage::height(Some(&img)) as f64);
    if iw < 1.0 || ih < 1.0 {
        return None;
    }
    let s = (edge as f64 / iw.max(ih)).min(1.0);
    let (w, h) = ((iw * s).round().max(1.0) as usize, (ih * s).round().max(1.0) as usize);
    let mut buf = vec![0u8; w * h];
    let cs = CGColorSpace::new_device_gray()?;
    // SAFETY: `buf` has w * h bytes and outlives the context, which is dropped at the end of this
    // function; bytes per row = w for 8-bit gray without alpha.
    let ctx = unsafe {
        CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w, Some(&cs), CGImageAlphaInfo::None.0)
    }?;
    let c = Some(&*ctx);
    CGContext::set_interpolation_quality(c, CGInterpolationQuality::Medium);
    CGContext::draw_image(c, CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(&img));
    drop(ctx);
    Some(AnalysisInput::from_gray(w, h, buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_examples_to_gray_with_the_edge_limit() {
        let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery/IMG_2439.JPG");
        if !ex.exists() {
            return;
        }
        let a = decode_input(&ex, InputSpec::Gray { edge: 1024 }).unwrap();
        assert_eq!(a.width.max(a.height), 1024);
        assert_eq!(a.gray.len(), a.width * a.height);
        // A real photo is not flat.
        let (lo, hi) = (a.gray.iter().min().unwrap(), a.gray.iter().max().unwrap());
        assert!(hi - lo > 50);
    }
}
