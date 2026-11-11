//! Fallback decode with the `image` and `fast_image_resize` crates, for files ImageIO cannot open.

use std::path::Path;

use fast_image_resize::{images::Image, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ImageDecoder, ImageReader};

use super::Encoded;

pub fn thumbnail(path: &Path, max: u32) -> Option<Encoded> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let alpha = img.color().has_alpha();
    let src = if alpha { DynamicImage::ImageRgba8(img.to_rgba8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
    let (w, h) = (src.width(), src.height());
    let scale = (max as f64 / w.max(h) as f64).min(1.0);
    let (tw, th) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    let pixel = if alpha { PixelType::U8x4 } else { PixelType::U8x3 };
    let mut dst = Image::new(tw, th, pixel);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(fast_image_resize::FilterType::Bilinear));
    Resizer::new().resize(&src, &mut dst, &opts).ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    if alpha {
        image::RgbaImage::from_raw(tw, th, dst.into_vec())?.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(Encoded { bytes: out.into_inner(), mime: "image/png" })
    } else {
        let rgb = image::RgbImage::from_raw(tw, th, dst.into_vec())?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&rgb).ok()?;
        Some(Encoded { bytes: out.into_inner(), mime: "image/jpeg" })
    }
}
