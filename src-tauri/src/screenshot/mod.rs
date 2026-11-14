//! Screenshot detection: a separate `ImageAnalyzer`, independent of photo quality, so that a scan can
//! use it alone or together with `QualityAnalyzer` (`pipeline::Pair`).
//!
//! The analyzer stores signals; `is_screenshot` applies the rules. The file name is not a signal in
//! the cache: the rules read it from the path, so the verdict follows a rename without a new analysis.
//!
//! Signals, strongest first:
//! 1. Camera EXIF (make, model, exposure, aperture, ISO, focal length, lens): a camera photo, never a
//!    screenshot.
//! 2. EXIF UserComment "Screenshot" (iOS, macOS) or a screenshot file name (iOS, Android, macOS,
//!    Windows, and some localized names).
//! 3. UI-like pixels (large flat areas in few colors) together with a lossless PNG file or a known
//!    screen resolution.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pipeline::{decode_input_for, AnalysisInput, ImageAnalyzer, InputSpec};

/// Long edge of the RGBX buffer for the pixel signals. The color statistics need no detail.
pub const SCREENSHOT_EDGE: u32 = 512;
/// A change makes the cached rows stale.
pub const ALGO_VERSION: u32 = 1;

/// Bits per channel for the color histogram (4096 bins).
const QUANT_BITS: u32 = 4;
/// Number of most frequent colors for `top_colors`.
const TOP_N: usize = 8;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotSignals {
    /// Stored pixel size, after the EXIF orientation.
    pub width: u32,
    pub height: u32,
    /// The file is a PNG.
    pub png: bool,
    /// The file has camera EXIF fields.
    pub camera_exif: bool,
    /// The EXIF UserComment is "Screenshot".
    pub os_comment: bool,
    /// Fraction of pixels with the same RGB value as their right neighbor.
    pub flat: f64,
    /// Fraction of pixels in the `TOP_N` most frequent colors, at `QUANT_BITS` bits per channel.
    pub top_colors: f64,
}

/// Assumption: first estimates; calibrate on a labelled set of screenshots and photos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenshotRules {
    pub flat_min: f64,
    pub top_colors_min: f64,
}

impl Default for ScreenshotRules {
    fn default() -> Self {
        Self { flat_min: 0.5, top_colors_min: 0.5 }
    }
}

impl ScreenshotRules {
    /// Large flat areas in few colors, as in an app or desktop UI.
    pub fn ui_like(&self, s: &ScreenshotSignals) -> bool {
        s.flat >= self.flat_min && s.top_colors >= self.top_colors_min
    }

    pub fn is_screenshot(&self, path: &Path, s: &ScreenshotSignals) -> bool {
        if s.camera_exif {
            return false;
        }
        if s.os_comment || name_matches(path) {
            return true;
        }
        self.ui_like(s) && (s.png || screen_size(s.width, s.height))
    }
}

/// `is_screenshot` with the default rules.
pub fn is_screenshot(path: &Path, s: &ScreenshotSignals) -> bool {
    ScreenshotRules::default().is_screenshot(path, s)
}

/// Lowercase file name parts that the OS screenshot tools write.
const NAME_PARTS: &[&str] = &[
    "screenshot",  // iOS, Android, macOS 10.14+, Windows, Simulator
    "screen shot", // macOS 10.13 and earlier
    "bildschirmfoto",
    "capture d’écran",
    "capture d'écran",
    "captura de pantalla",
    "captura de ecrã",
    "schermata",
    "schermafbeelding",
    "skärmavbild",
    "skjermbilde",
    "zrzut ekranu",
    "ekrano nuotrauka",
    "снимок экрана",
    "スクリーンショット",
    "截屏",
    "屏幕截图",
];

pub fn name_matches(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let name = name.to_lowercase();
    NAME_PARTS.iter().any(|p| name.contains(p))
}

/// Native screen resolutions of common phones, tablets, and displays, in portrait (short × long).
const SCREENS: &[(u32, u32)] = &[
    // iPhone
    (640, 1136), (750, 1334), (828, 1792), (1080, 1920), (1125, 2436), (1170, 2532), (1179, 2556),
    (1206, 2622), (1242, 2208), (1242, 2688), (1284, 2778), (1290, 2796), (1320, 2868), (1080, 2340),
    // Android
    (720, 1280), (720, 1520), (720, 1600), (1080, 2160), (1080, 2220), (1080, 2280), (1080, 2400),
    (1080, 2412), (1220, 2712), (1260, 2800), (1344, 2992), (1440, 2560), (1440, 2960), (1440, 3040),
    (1440, 3088), (1440, 3120), (1440, 3200),
    // iPad
    (1536, 2048), (1620, 2160), (1640, 2360), (1668, 2224), (1668, 2388), (1488, 2266), (2048, 2732),
    (2064, 2752),
    // Desktop and laptop
    (768, 1366), (800, 1280), (864, 1536), (900, 1440), (1050, 1680), (1080, 1920), (1200, 1920),
    (1440, 2560), (1600, 2560), (1664, 2560), (1800, 2880), (1912, 2940), (1964, 3024), (2160, 3840),
    (2224, 3420), (2234, 3456), (2880, 5120),
];

/// The size is a known screen resolution, or has a tall phone aspect ratio (19.5:9 and similar)
/// that cameras do not use.
pub fn screen_size(w: u32, h: u32) -> bool {
    let (short, long) = (w.min(h), w.max(h));
    if short == 0 {
        return false;
    }
    let aspect = long as f64 / short as f64;
    SCREENS.contains(&(short, long)) || (2.0..=2.4).contains(&aspect)
}

/// Computes `flat` and `top_colors` on a row-major RGBX buffer (4 bytes per pixel).
pub fn pixel_signals(w: usize, h: usize, rgbx: &[u8]) -> (f64, f64) {
    assert_eq!(rgbx.len(), w * h * 4);
    if w < 2 || h == 0 {
        return (0.0, 0.0);
    }
    let shift = 8 - QUANT_BITS;
    let mut hist = vec![0u32; 1 << (3 * QUANT_BITS)];
    let mut same = 0u64;
    for row in rgbx.chunks_exact(w * 4) {
        let mut prev: Option<&[u8]> = None;
        for px in row.chunks_exact(4) {
            let (r, g, b) = (px[0] as usize >> shift, px[1] as usize >> shift, px[2] as usize >> shift);
            hist[(r << (2 * QUANT_BITS)) | (g << QUANT_BITS) | b] += 1;
            if prev.is_some_and(|p| p[..3] == px[..3]) {
                same += 1;
            }
            prev = Some(px);
        }
    }
    hist.sort_unstable_by(|a, b| b.cmp(a));
    let top: u64 = hist[..TOP_N].iter().map(|&c| c as u64).sum();
    let flat = same as f64 / ((w - 1) * h) as f64;
    (flat, top as f64 / (w * h) as f64)
}

#[derive(Default)]
pub struct ScreenshotAnalyzer;

impl ImageAnalyzer for ScreenshotAnalyzer {
    const ID: &'static str = "screenshot";
    const VERSION: u32 = ALGO_VERSION;
    type Output = ScreenshotSignals;

    fn input(&self) -> InputSpec {
        InputSpec::rgba(SCREENSHOT_EDGE).with_meta()
    }

    fn analyze(&self, input: &AnalysisInput) -> Option<ScreenshotSignals> {
        let (px, m) = (input.rgba.as_ref()?, input.meta.as_ref()?);
        let (flat, top_colors) = pixel_signals(px.width, px.height, &px.data);
        let camera_exif = m.camera.is_some()
            || m.lens.is_some()
            || m.exposure_time.is_some()
            || m.aperture.is_some()
            || m.iso.is_some()
            || m.focal_length.is_some();
        Some(ScreenshotSignals {
            width: m.width.unwrap_or(0),
            height: m.height.unwrap_or(0),
            png: m.format.as_deref() == Some("public.png"),
            camera_exif,
            os_comment: m.user_comment.as_deref().is_some_and(|c| c.trim().eq_ignore_ascii_case("screenshot")),
            flat,
            top_colors,
        })
    }
}

/// Analyzes 1 file with no cache. None if the file cannot be decoded.
pub fn detect(path: &Path) -> Option<bool> {
    let a = ScreenshotAnalyzer;
    let s = decode_input_for(&a, path).and_then(|i| a.analyze(&i))?;
    Some(is_screenshot(path, &s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake app screen: a white background, a colored header bar, and gray "text" lines.
    fn ui_image(w: u32, h: u32) -> image::RgbImage {
        image::RgbImage::from_fn(w, h, |x, y| {
            if y < h / 10 {
                image::Rgb([0, 122, 255])
            } else if (y / 20) % 3 == 0 && x > w / 10 && x < w * 8 / 10 && (x / 7) % 5 != 0 {
                image::Rgb([60, 60, 67])
            } else {
                image::Rgb([255, 255, 255])
            }
        })
    }

    /// Deterministic noise with a gradient, as a stand-in for a photo.
    fn photo_image(w: u32, h: u32) -> image::RgbImage {
        let mut s: u32 = 12345;
        image::RgbImage::from_fn(w, h, |x, y| {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let n = (s >> 16) as u8 % 24;
            let base = |v: u32| ((v * 200) / w.max(h)) as u8;
            image::Rgb([base(x) + n, base(y) + n / 2, 90 + n])
        })
    }

    fn rgbx(img: &image::RgbImage) -> Vec<u8> {
        img.pixels().flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    }

    #[test]
    fn ui_pixels_are_flat_and_photo_pixels_are_not() {
        let r = ScreenshotRules::default();
        let ui = ui_image(300, 600);
        let (flat, top) = pixel_signals(300, 600, &rgbx(&ui));
        assert!(r.ui_like(&ScreenshotSignals { flat, top_colors: top, ..Default::default() }), "{flat} {top}");
        let ph = photo_image(300, 400);
        let (flat, top) = pixel_signals(300, 400, &rgbx(&ph));
        assert!(!r.ui_like(&ScreenshotSignals { flat, top_colors: top, ..Default::default() }), "{flat} {top}");
    }

    #[test]
    fn names_and_sizes() {
        assert!(name_matches(Path::new("/a/Screenshot 2024-05-01 at 10.12.33.png")));
        assert!(name_matches(Path::new("/a/Screenshot_20240501-101233_Chrome.png")));
        assert!(name_matches(Path::new("/a/Screen Shot 2018-01-01 at 1.00.00 PM.png")));
        assert!(name_matches(Path::new("/a/Bildschirmfoto 2024-05-01 um 10.12.33.png")));
        assert!(!name_matches(Path::new("/a/IMG_2439.JPG")));
        assert!(screen_size(1170, 2532) && screen_size(2532, 1170) && screen_size(1080, 2400));
        assert!(!screen_size(4032, 3024) && !screen_size(3000, 2000));
    }

    #[test]
    fn rules_combine_the_signals() {
        let p = Path::new("/a/IMG_1.png");
        let ui = ScreenshotSignals { width: 1000, height: 700, flat: 0.8, top_colors: 0.9, ..Default::default() };
        // UI pixels alone are not enough: a JPEG with an odd size can be a scan or a graphic.
        assert!(!is_screenshot(p, &ui));
        assert!(is_screenshot(p, &ScreenshotSignals { png: true, ..ui }));
        assert!(is_screenshot(p, &ScreenshotSignals { width: 1170, height: 2532, ..ui }));
        // The OS comment and the name are enough alone.
        assert!(is_screenshot(p, &ScreenshotSignals { os_comment: true, ..Default::default() }));
        assert!(is_screenshot(Path::new("/a/Screenshot 1.jpg"), &ScreenshotSignals::default()));
        // Camera EXIF wins over all other signals.
        let cam = ScreenshotSignals { camera_exif: true, os_comment: true, png: true, ..ui };
        assert!(!is_screenshot(Path::new("/a/Screenshot 1.png"), &cam));
    }

    fn example(rel: &str) -> Option<std::path::PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery").join(rel);
        p.exists().then_some(p)
    }

    #[test]
    fn detects_the_example_screenshot_also_with_a_neutral_name() {
        let (Some(shot), Some(photo)) = (example("another/screenshot_examle.JPEG"), example("IMG_2439.JPG")) else {
            return;
        };
        assert_eq!(detect(&shot), Some(true));
        // A cropped iOS screenshot saved as JPEG: the name rule is not needed.
        let t = tempfile::tempdir().unwrap();
        let renamed = t.path().join("IMG_0001.JPEG");
        std::fs::copy(&shot, &renamed).unwrap();
        let a = ScreenshotAnalyzer;
        let s = a.analyze(&decode_input_for(&a, &renamed).unwrap()).unwrap();
        assert!(s.os_comment && !s.camera_exif && !s.png, "{s:?}");
        assert!(ScreenshotRules::default().ui_like(&s), "{s:?}");
        assert!(is_screenshot(&renamed, &s));
        // Without the OS comment, UI pixels and a phone aspect ratio are still enough.
        assert!(is_screenshot(&renamed, &ScreenshotSignals { os_comment: false, width: 1170, height: 2532, ..s }));
        // A camera photo is not a screenshot.
        assert_eq!(detect(&photo), Some(false));
    }

    #[test]
    fn detects_a_png_screen_and_not_a_photo() {
        let t = tempfile::tempdir().unwrap();
        let shot = t.path().join("IMG_0001.png");
        ui_image(1170, 2532).save(&shot).unwrap();
        assert_eq!(detect(&shot), Some(true));
        let photo = t.path().join("IMG_0002.jpg");
        photo_image(1200, 900).save(&photo).unwrap();
        assert_eq!(detect(&photo), Some(false));
    }
}
