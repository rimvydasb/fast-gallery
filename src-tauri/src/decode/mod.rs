//! ImageDecoder: macOS ImageIO bindings (through the `objc2` crates) for
//! subsampled thumbnail decode, encode, full image transcode, and metadata.
//! The `image` + `fast_image_resize` crates are a fallback for files that
//! ImageIO cannot open.

mod exif_thumb;
mod fallback;

use std::path::Path;

use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFMutableData, CFNumber, CFRetained, CFString, CFType, CFURL,
};
use objc2_core_graphics::{CGImage, CGImageAlphaInfo};
use objc2_image_io::{
    kCGImageDestinationLossyCompressionQuality, kCGImagePropertyExifDateTimeOriginal,
    kCGImagePropertyExifDictionary, kCGImagePropertyExifExposureTime, kCGImagePropertyExifFNumber,
    kCGImagePropertyExifFocalLength, kCGImagePropertyExifISOSpeedRatings,
    kCGImagePropertyExifLensModel, kCGImagePropertyGPSDictionary, kCGImagePropertyGPSLatitude,
    kCGImagePropertyGPSLatitudeRef, kCGImagePropertyGPSLongitude, kCGImagePropertyGPSLongitudeRef,
    kCGImagePropertyOrientation, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
    kCGImagePropertyTIFFDateTime, kCGImagePropertyTIFFDictionary, kCGImagePropertyTIFFMake,
    kCGImagePropertyTIFFModel, kCGImageSourceCreateThumbnailFromImageAlways,
    kCGImageSourceCreateThumbnailFromImageIfAbsent, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceShouldCache, kCGImageSourceSubsampleFactor, kCGImageSourceThumbnailMaxPixelSize,
    CGImageDestination,
    CGImageSource,
};
use serde::Serialize;

/// Encoded image bytes and their MIME type.
#[derive(Clone)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

pub const JPEG_QUALITY: f64 = 0.8;

fn cf_key(k: &'static CFString) -> &'static CFString {
    k
}

fn source(path: &Path) -> Option<CFRetained<CGImageSource>> {
    let url = CFURL::from_file_path(path)?;
    let no_cache = CFDictionary::<CFString, CFType>::from_slices(
        &[cf_key(unsafe { kCGImageSourceShouldCache })],
        &[CFBoolean::new(false).as_ref()],
    );
    unsafe { CGImageSource::with_url(&url, Some(no_cache.as_opaque())) }
}

fn thumb_options(max: u32, always: bool) -> CFRetained<CFDictionary<CFString, CFType>> {
    let max_n = CFNumber::new_i32(max as i32);
    let t = CFBoolean::new(true);
    unsafe {
        let from_key = if always {
            kCGImageSourceCreateThumbnailFromImageAlways
        } else {
            kCGImageSourceCreateThumbnailFromImageIfAbsent
        };
        CFDictionary::<CFString, CFType>::from_slices(
            &[
                from_key,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
                kCGImageSourceShouldCache,
            ],
            &[t.as_ref(), t.as_ref(), max_n.as_ref(), CFBoolean::new(false).as_ref()],
        )
    }
}


fn is_jpeg(path: &Path) -> bool {
    matches!(crate::scan::ext_lower(path).as_deref(), Some("jpg" | "jpeg"))
}

/// Decodes a thumbnail with a long edge of at most `max` px, with the EXIF orientation applied (FMT-05).
/// - JPEG: the embedded EXIF thumbnail if it has the requested size (THM-03); else an ImageIO
///   subsampled decode (kCGImageSourceSubsampleFactor, DCT scaling) and a small CoreGraphics
///   downscale, which is about 35 % faster than the thumbnail API for a 24 MP JPEG.
/// - RAW: ImageIO uses the embedded preview if present (THM-03), else decodes.
/// - Other files, and JPEG files too small to subsample: ImageIO thumbnail decode with
///   kCGImageSourceCreateThumbnailFromImageAlways (THM-04).
pub fn thumbnail_image(path: &Path, max: u32) -> Option<CFRetained<CGImage>> {
    let src = source(path)?;
    if is_jpeg(path) {
        if let Some(img) = jpeg_thumbnail(path, &src, max) {
            return Some(img);
        }
    } else if crate::scan::is_raw(path) {
        let opts = thumb_options(max, false);
        return unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) };
    }
    let opts = thumb_options(max, true);
    unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) }
}

fn jpeg_thumbnail(path: &Path, src: &CGImageSource, max: u32) -> Option<CFRetained<CGImage>> {
    let props = unsafe { src.properties_at_index(0, None) }?;
    let props: &CFDictionary<CFString, CFType> = unsafe { props.cast_unchecked() };
    let w = num(props, unsafe { kCGImagePropertyPixelWidth })? as u32;
    let h = num(props, unsafe { kCGImagePropertyPixelHeight })? as u32;
    let orient = num(props, unsafe { kCGImagePropertyOrientation }).unwrap_or(1.0) as u32;
    // THM-03: the embedded thumbnail is stored with the orientation of the main image.
    if let Some(t) = exif_thumb::find(path) {
        if t.width.max(t.height) >= max {
            let data = CFData::from_bytes(&t.bytes);
            if let Some(img) = unsafe { CGImageSource::with_data(&data, None) }
                .and_then(|s| unsafe { s.image_at_index(0, None) })
            {
                return draw_oriented(&img, max, orient);
            }
        }
    }
    let factor = [4u32, 2].into_iter().find(|f| w.max(h) / f >= max)?;
    let f = CFNumber::new_i32(factor as i32);
    let opts = CFDictionary::<CFString, CFType>::from_slices(
        &[cf_key(unsafe { kCGImageSourceSubsampleFactor })],
        &[f.as_ref()],
    );
    let img = unsafe { src.image_at_index(0, Some(opts.as_opaque())) }?;
    draw_oriented(&img, max, orient)
}

/// A display-size version of a large JPEG for the viewer: an ImageIO subsampled decode (1/2 or 1/4)
/// whose long edge is at least `min_long` px, with the EXIF orientation applied. Returns None when
/// the image is not a JPEG or is too small to subsample; the caller then serves the original file.
pub fn screen_image(path: &Path, min_long: u32) -> Option<Encoded> {
    if !is_jpeg(path) {
        return None;
    }
    let src = source(path)?;
    let props = unsafe { src.properties_at_index(0, None) }?;
    let props: &CFDictionary<CFString, CFType> = unsafe { props.cast_unchecked() };
    let w = num(props, unsafe { kCGImagePropertyPixelWidth })? as u32;
    let h = num(props, unsafe { kCGImagePropertyPixelHeight })? as u32;
    let orient = num(props, unsafe { kCGImagePropertyOrientation }).unwrap_or(1.0) as u32;
    let factor = [4u32, 2].into_iter().find(|f| w.max(h) / f >= min_long)?;
    let f = CFNumber::new_i32(factor as i32);
    let opts = CFDictionary::<CFString, CFType>::from_slices(
        &[cf_key(unsafe { kCGImageSourceSubsampleFactor })],
        &[f.as_ref()],
    );
    let img = unsafe { src.image_at_index(0, Some(opts.as_opaque())) }?;
    let img = if orient > 1 { draw_oriented(&img, long_edge_of(&img), orient)? } else { img };
    encode(&img, 0.85, false)
}

fn long_edge_of(img: &CGImage) -> u32 {
    CGImage::width(Some(img)).max(CGImage::height(Some(img))) as u32
}

/// Draws `img` downscaled to a long edge of `max` px, with the EXIF orientation applied.
fn draw_oriented(img: &CGImage, max: u32, orient: u32) -> Option<CFRetained<CGImage>> {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        kCGColorSpaceSRGB, CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGColorSpaceModel,
        CGContext, CGInterpolationQuality,
    };
    use std::f64::consts::{FRAC_PI_2, PI};

    let (iw, ih) = (CGImage::width(Some(img)) as f64, CGImage::height(Some(img)) as f64);
    let s = (max as f64 / iw.max(ih)).min(1.0);
    let (sw, sh) = ((iw * s).round().max(1.0), (ih * s).round().max(1.0));
    let (cw, ch) = if orient >= 5 { (sh, sw) } else { (sw, sh) };
    let cs = CGImage::color_space(Some(img))
        .filter(|c| CGColorSpace::model(Some(c)) == CGColorSpaceModel::RGB)
        .or_else(|| CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })))?;
    // kCGImageAlphaNoneSkipFirst | kCGBitmapByteOrder32Little
    let info = CGImageAlphaInfo::NoneSkipFirst.0 | (2 << 12);
    let ctx = unsafe { CGBitmapContextCreate(std::ptr::null_mut(), cw as usize, ch as usize, 8, 0, Some(&cs), info) }?;
    let c = Some(&*ctx);
    CGContext::set_interpolation_quality(c, CGInterpolationQuality::Medium);
    // EXIF orientation to a CG transform (origin at the bottom left).
    match orient {
        3 | 4 => {
            CGContext::translate_ctm(c, cw, ch);
            CGContext::rotate_ctm(c, PI);
        }
        5 | 8 => {
            CGContext::translate_ctm(c, cw, 0.0);
            CGContext::rotate_ctm(c, FRAC_PI_2);
        }
        6 | 7 => {
            CGContext::translate_ctm(c, 0.0, ch);
            CGContext::rotate_ctm(c, -FRAC_PI_2);
        }
        _ => {}
    }
    match orient {
        2 | 4 => {
            CGContext::translate_ctm(c, cw, 0.0);
            CGContext::scale_ctm(c, -1.0, 1.0);
        }
        5 | 7 => {
            CGContext::translate_ctm(c, ch, 0.0);
            CGContext::scale_ctm(c, -1.0, 1.0);
        }
        _ => {}
    }
    CGContext::draw_image(c, CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(sw, sh)), Some(img));
    CGBitmapContextCreateImage(c)
}

fn has_alpha(img: &CGImage) -> bool {
    !matches!(
        CGImage::alpha_info(Some(img)),
        CGImageAlphaInfo::None | CGImageAlphaInfo::NoneSkipFirst | CGImageAlphaInfo::NoneSkipLast
    )
}

/// Encodes with CGImageDestination (THM-06): JPEG at `quality`, or PNG for images with alpha (THM-05).
pub fn encode(img: &CGImage, quality: f64, allow_png: bool) -> Option<Encoded> {
    let png = allow_png && has_alpha(img);
    let (uti, mime) = if png { ("public.png", "image/png") } else { ("public.jpeg", "image/jpeg") };
    let data = CFMutableData::new(None, 0)?;
    let uti = CFString::from_static_str(uti);
    unsafe {
        let dst = CGImageDestination::with_data(&data, &uti, 1, None)?;
        let q = CFNumber::new_f64(quality);
        let props = CFDictionary::<CFString, CFType>::from_slices(
            &[kCGImageDestinationLossyCompressionQuality],
            &[q.as_ref()],
        );
        dst.add_image(img, if png { None } else { Some(props.as_opaque()) });
        if !dst.finalize() {
            return None;
        }
    }
    let data: &CFData = &data;
    Some(Encoded { bytes: data.to_vec(), mime })
}

/// Generates thumbnail bytes for one image (decode, downscale, encode).
/// Falls back to the `image` crate if ImageIO cannot open the file.
pub fn make_thumbnail(path: &Path, max: u32) -> Option<Encoded> {
    match thumbnail_image(path, max) {
        Some(img) => encode(&img, JPEG_QUALITY, true),
        None => fallback::thumbnail(path, max),
    }
}

/// Converts a file that the WebView cannot display natively into a full-size JPEG.
/// For RAW files the embedded full-size preview is used when present.
pub fn transcode_full(path: &Path) -> Option<Encoded> {
    let src = source(path)?;
    let img = if crate::scan::is_raw(path) {
        let t = CFBoolean::new(true);
        let opts = CFDictionary::<CFString, CFType>::from_slices(
            unsafe {
                &[kCGImageSourceCreateThumbnailFromImageIfAbsent, kCGImageSourceCreateThumbnailWithTransform]
            },
            &[t.as_ref(), t.as_ref()],
        );
        unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) }?
    } else {
        // Full-size decode with orientation applied: thumbnail with max = long edge.
        let (w, h) = dims_of(&src).unwrap_or((8192, 8192));
        let opts = thumb_options(w.max(h).max(1), true);
        unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) }?
    };
    encode(&img, 0.9, true)
}

fn dims_of(src: &CGImageSource) -> Option<(u32, u32)> {
    let props = unsafe { src.properties_at_index(0, None) }?;
    let props: &CFDictionary<CFString, CFType> = unsafe { props.cast_unchecked() };
    let w = num(props, unsafe { kCGImagePropertyPixelWidth })? as u32;
    let h = num(props, unsafe { kCGImagePropertyPixelHeight })? as u32;
    Some((w, h))
}

fn num(d: &CFDictionary<CFString, CFType>, k: &CFString) -> Option<f64> {
    let v = d.get(k)?;
    v.downcast_ref::<CFNumber>()?.as_f64()
}

fn string(d: &CFDictionary<CFString, CFType>, k: &CFString) -> Option<String> {
    let v = d.get(k)?;
    Some(v.downcast_ref::<CFString>()?.to_string())
}

fn sub(d: &CFDictionary<CFString, CFType>, k: &CFString) -> Option<CFRetained<CFDictionary<CFString, CFType>>> {
    let v = d.get(k)?;
    let dict = v.downcast::<CFDictionary>().ok()?;
    Some(unsafe { CFRetained::cast_unchecked(dict) })
}

/// Parses an EXIF date "YYYY:MM:DD HH:MM:SS" into seconds since 1970 (naive, for ordering).
pub fn parse_exif_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

/// Reads the EXIF "date taken" of one image, in seconds (naive local time).
pub fn date_taken(path: &Path) -> Option<i64> {
    let src = source(path)?;
    let props = unsafe { src.properties_at_index(0, None) }?;
    let props: &CFDictionary<CFString, CFType> = unsafe { props.cast_unchecked() };
    let exif = sub(props, unsafe { kCGImagePropertyExifDictionary });
    let s = exif
        .as_ref()
        .and_then(|e| string(e, unsafe { kCGImagePropertyExifDateTimeOriginal }))
        .or_else(|| {
            sub(props, unsafe { kCGImagePropertyTIFFDictionary })
                .and_then(|t| string(&t, unsafe { kCGImagePropertyTIFFDateTime }))
        })?;
    parse_exif_date(&s)
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImageMeta {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub focal_length: Option<f64>,
    pub aperture: Option<f64>,
    pub exposure_time: Option<f64>,
    pub iso: Option<f64>,
    pub date_taken: Option<String>,
    pub gps: Option<(f64, f64)>,
}

/// Reads dimensions (after EXIF orientation), format, and EXIF fields for the information panel.
pub fn image_meta(path: &Path) -> ImageMeta {
    let mut m = ImageMeta::default();
    let Some(src) = source(path) else { return m };
    m.format = unsafe { src.r#type() }.map(|t| t.to_string());
    let Some(props) = (unsafe { src.properties_at_index(0, None) }) else { return m };
    let props: &CFDictionary<CFString, CFType> = unsafe { props.cast_unchecked() };
    unsafe {
        let w = num(props, kCGImagePropertyPixelWidth).map(|v| v as u32);
        let h = num(props, kCGImagePropertyPixelHeight).map(|v| v as u32);
        let orient = num(props, kCGImagePropertyOrientation).unwrap_or(1.0) as u32;
        if orient >= 5 {
            (m.width, m.height) = (h, w);
        } else {
            (m.width, m.height) = (w, h);
        }
        if let Some(tiff) = sub(props, kCGImagePropertyTIFFDictionary) {
            let make = string(&tiff, kCGImagePropertyTIFFMake).unwrap_or_default();
            let model = string(&tiff, kCGImagePropertyTIFFModel).unwrap_or_default();
            let cam = if model.starts_with(make.trim()) { model } else { format!("{} {}", make.trim(), model) };
            let cam = cam.trim().to_string();
            if !cam.is_empty() {
                m.camera = Some(cam);
            }
        }
        if let Some(exif) = sub(props, kCGImagePropertyExifDictionary) {
            m.lens = string(&exif, kCGImagePropertyExifLensModel);
            m.focal_length = num(&exif, kCGImagePropertyExifFocalLength);
            m.aperture = num(&exif, kCGImagePropertyExifFNumber);
            m.exposure_time = num(&exif, kCGImagePropertyExifExposureTime);
            m.date_taken = string(&exif, kCGImagePropertyExifDateTimeOriginal);
            if let Some(iso) = exif.get(kCGImagePropertyExifISOSpeedRatings) {
                if let Some(arr) = iso.downcast_ref::<objc2_core_foundation::CFArray>() {
                    let arr: &objc2_core_foundation::CFArray<CFNumber> = arr.cast_unchecked();
                    m.iso = arr.get(0).and_then(|n| n.as_f64());
                }
            }
        }
        if let Some(gps) = sub(props, kCGImagePropertyGPSDictionary) {
            if let (Some(lat), Some(lon)) =
                (num(&gps, kCGImagePropertyGPSLatitude), num(&gps, kCGImagePropertyGPSLongitude))
            {
                let lat_s = if string(&gps, kCGImagePropertyGPSLatitudeRef).as_deref() == Some("S") { -1.0 } else { 1.0 };
                let lon_s = if string(&gps, kCGImagePropertyGPSLongitudeRef).as_deref() == Some("W") { -1.0 } else { 1.0 };
                m.gps = Some((lat * lat_s, lon * lon_s));
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a 1200 × 800 JPEG with 4 colored quadrants and the given EXIF orientation.
    fn quadrant_jpeg(dir: &Path, orient: i32) -> std::path::PathBuf {
        let img = image::RgbImage::from_fn(1200, 800, |x, y| match (x < 600, y < 400) {
            (true, true) => image::Rgb([255, 0, 0]),
            (false, true) => image::Rgb([0, 255, 0]),
            (true, false) => image::Rgb([0, 0, 255]),
            (false, false) => image::Rgb([255, 255, 255]),
        });
        let mut plain = std::io::Cursor::new(Vec::new());
        img.write_to(&mut plain, image::ImageFormat::Jpeg).unwrap();
        let data = CFData::from_bytes(plain.get_ref());
        let out = CFMutableData::new(None, 0).unwrap();
        unsafe {
            let src = CGImageSource::with_data(&data, None).unwrap();
            let dst = CGImageDestination::with_data(&out, &CFString::from_static_str("public.jpeg"), 1, None).unwrap();
            let o = CFNumber::new_i32(orient);
            let props = CFDictionary::<CFString, CFType>::from_slices(&[kCGImagePropertyOrientation], &[o.as_ref()]);
            dst.add_image_from_source(&src, 0, Some(props.as_opaque()));
            assert!(dst.finalize());
        }
        let p = dir.join(format!("o{orient}.jpg"));
        std::fs::write(&p, out.to_vec()).unwrap();
        p
    }

    fn quadrants(img: &CGImage) -> (u32, u32, Vec<[u8; 3]>) {
        let enc = encode(img, 0.95, false).unwrap();
        let d = image::load_from_memory(&enc.bytes).unwrap().to_rgb8();
        let (w, h) = d.dimensions();
        let pts = [(w / 4, h / 4), (3 * w / 4, h / 4), (w / 4, 3 * h / 4), (3 * w / 4, 3 * h / 4)];
        let q = pts.iter().map(|&(x, y)| d.get_pixel(x, y).0.map(|c| if c > 127 { 255 } else { 0 })).collect();
        (w, h, q)
    }

    #[test]
    fn subsampled_thumbnail_applies_all_orientations() {
        let t = tempfile::tempdir().unwrap();
        for o in 1..=8 {
            let p = quadrant_jpeg(t.path(), o);
            let src = source(&p).unwrap();
            let ours = jpeg_thumbnail(&p, &src, 256).expect("subsampled path");
            let opts = thumb_options(256, true);
            let reference = unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) }.unwrap();
            assert_eq!(quadrants(&ours), quadrants(&reference), "orientation {o}");
            assert_eq!(long_edge_of(&ours), 256);
        }
    }

    #[test]
    fn thumbnails_of_examples() {
        let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery");
        let files = ["IMG_2439.JPG", "deep/IMG_8763.HEIC", "another/IMG_6462.HEIC"];
        for f in files {
            let p = ex.join(f);
            if !p.exists() {
                continue;
            }
            for size in [256, 512] {
                let e = make_thumbnail(&p, size).unwrap_or_else(|| panic!("thumbnail of {f}"));
                let d = image::load_from_memory(&e.bytes).unwrap();
                assert_eq!(d.width().max(d.height()), size, "{f} at {size}");
            }
            let m = image_meta(&p);
            assert!(m.width.is_some() && m.height.is_some(), "{f} meta");
        }
    }

    #[test]
    fn exif_date_parses() {
        assert_eq!(parse_exif_date("1970:01:01 00:00:00"), Some(0));
        assert_eq!(parse_exif_date("2000:03:01 00:00:01"), Some(951868801));
        assert_eq!(parse_exif_date("bad"), None);
    }
}
