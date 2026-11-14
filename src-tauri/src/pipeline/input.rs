//! AnalysisInput: the decoded image that an analyzer reads. The decode is in the pipeline, not in the
//! analyzer, so that analyzers that run together (`Pair`) share 1 decode.

use std::path::Path;

use crate::decode::ImageMeta;

/// The parts of the decoded image that an analyzer needs. Each part is optional.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InputSpec {
    /// 8-bit luminance with a long edge of at most this many px, EXIF orientation applied.
    pub gray: Option<u32>,
    /// 8-bit RGBX (4 bytes per pixel) with a long edge of at most this many px, nearest-neighbor
    /// downscale from the decoded thumbnail, so that flat areas keep their exact colors.
    pub rgba: Option<u32>,
    /// File format, pixel size, and EXIF fields from the image source (`decode::image_meta`).
    pub meta: bool,
}

impl InputSpec {
    pub const fn gray(edge: u32) -> Self {
        Self { gray: Some(edge), rgba: None, meta: false }
    }

    pub const fn rgba(edge: u32) -> Self {
        Self { gray: None, rgba: Some(edge), meta: false }
    }

    pub const fn with_meta(self) -> Self {
        Self { meta: true, ..self }
    }

    /// The union of 2 specs. If both request the same plane, the larger edge wins, so give
    /// analyzers that run together the same edge for a shared plane.
    pub fn merge(self, o: Self) -> Self {
        let max = |a: Option<u32>, b: Option<u32>| a.max(b);
        Self { gray: max(self.gray, o.gray), rgba: max(self.rgba, o.rgba), meta: self.meta || o.meta }
    }
}

/// A row-major 8-bit pixel buffer; the row stride is `width * channels`.
#[derive(Clone, Debug)]
pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Plane {
    pub fn new(width: usize, height: usize, channels: usize, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), width * height * channels);
        Self { width, height, data }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AnalysisInput {
    pub gray: Option<Plane>,
    pub rgba: Option<Plane>,
    pub meta: Option<ImageMeta>,
}

impl AnalysisInput {
    pub fn from_gray(width: usize, height: usize, gray: Vec<u8>) -> Self {
        Self { gray: Some(Plane::new(width, height, 1, gray)), ..Default::default() }
    }
}

/// Decodes `path` for the spec: 1 `decode::thumbnail_image` (ImageIO subsampling) at the largest
/// requested edge, then a draw into an 8-bit bitmap context for each requested plane, which does the
/// color conversion in native code. None if a requested part is not available.
pub fn decode_input(path: &Path, spec: InputSpec) -> Option<AnalysisInput> {
    let mut out = AnalysisInput::default();
    if let Some(edge) = spec.gray.max(spec.rgba) {
        let img = crate::decode::thumbnail_image(path, edge)?;
        if let Some(e) = spec.gray {
            out.gray = Some(draw(&img, e, Format::Gray)?);
        }
        if let Some(e) = spec.rgba {
            out.rgba = Some(draw(&img, e, Format::Rgbx)?);
        }
    }
    if spec.meta {
        let m = crate::decode::image_meta(path);
        if m.width.is_none() || m.format.is_none() {
            return None;
        }
        out.meta = Some(m);
    }
    Some(out)
}

/// Decodes the input that `analyzer` needs.
pub fn decode_input_for<A: super::ImageAnalyzer>(analyzer: &A, path: &Path) -> Option<AnalysisInput> {
    decode_input(path, analyzer.input())
}

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Gray,
    Rgbx,
}

fn draw(img: &objc2_core_graphics::CGImage, edge: u32, f: Format) -> Option<Plane> {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality,
    };

    let (iw, ih) = (CGImage::width(Some(img)) as f64, CGImage::height(Some(img)) as f64);
    if iw < 1.0 || ih < 1.0 {
        return None;
    }
    let s = (edge as f64 / iw.max(ih)).min(1.0);
    let (w, h) = ((iw * s).round().max(1.0) as usize, (ih * s).round().max(1.0) as usize);
    let (ch, cs, alpha, interp) = match f {
        Format::Gray => (1, CGColorSpace::new_device_gray()?, CGImageAlphaInfo::None, CGInterpolationQuality::Medium),
        Format::Rgbx => (4, CGColorSpace::new_device_rgb()?, CGImageAlphaInfo::NoneSkipLast, CGInterpolationQuality::None),
    };
    let mut buf = vec![0u8; w * h * ch];
    // SAFETY: `buf` has w * h * ch bytes and outlives the context, which is dropped at the end of
    // this function; bytes per row = w * ch for 8-bit gray, or 8-bit RGB with 1 skipped byte.
    let ctx = unsafe { CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w * ch, Some(&cs), alpha.0) }?;
    let c = Some(&*ctx);
    CGContext::set_interpolation_quality(c, interp);
    CGContext::draw_image(c, CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(img));
    drop(ctx);
    Some(Plane::new(w, h, ch, buf))
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
        let a = decode_input(&ex, InputSpec::gray(1024)).unwrap();
        let g = a.gray.unwrap();
        assert_eq!(g.width.max(g.height), 1024);
        assert_eq!(g.data.len(), g.width * g.height);
        assert!(a.rgba.is_none() && a.meta.is_none());
        // A real photo is not flat.
        let (lo, hi) = (g.data.iter().min().unwrap(), g.data.iter().max().unwrap());
        assert!(hi - lo > 50);
    }

    #[test]
    fn one_decode_gives_all_requested_parts() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.png");
        image::RgbImage::from_fn(400, 200, |x, _| if x < 200 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) })
            .save(&p)
            .unwrap();
        let a = decode_input(&p, InputSpec::gray(100).merge(InputSpec::rgba(50).with_meta())).unwrap();
        let (g, c, m) = (a.gray.unwrap(), a.rgba.unwrap(), a.meta.unwrap());
        assert_eq!((g.width, g.height), (100, 50));
        assert_eq!((c.width, c.height, c.data.len()), (50, 25, 50 * 25 * 4));
        // The nearest-neighbor downscale keeps the exact colors.
        assert_eq!(&c.data[..3], &[255, 0, 0]);
        assert_eq!(m.format.as_deref(), Some("public.png"));
        assert_eq!((m.width, m.height), (Some(400), Some(200)));
    }
}
