//! Photo similarity: the second client of the image scan pipeline. `SimilarityAnalyzer` computes a
//! small `Signature` for each image (2 perceptual hashes, a color layout, and a color histogram);
//! the functions in `cluster` compare signatures. The file name, size, and type are not part of a signature.

pub mod cluster;

use serde::{Deserialize, Serialize};

use crate::pipeline::{AnalysisInput, ImageAnalyzer, InputSpec, Plane};

pub use cluster::{cluster_bursts, cluster_similar, similarity_order, BurstKind, Level};

/// Long edge of the analysis buffers. Small, so that ImageIO can subsample the decode.
pub const ANALYSIS_EDGE: u32 = 128;
/// A change makes the cached rows stale.
pub const ALGO_VERSION: u32 = 1;

/// Side of the square luminance buffer for the DCT hash.
const DCT_N: usize = 32;
/// The DCT hash uses the 8 × 8 lowest frequencies.
const DCT_K: usize = 8;
/// The color layout is a 4 × 4 grid of mean RGB values.
const COLOR_GRID: usize = 4;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Signature {
    /// pHash: the signs of the low DCT frequencies against their median (structure).
    pub p: u64,
    /// dHash: the signs of the horizontal gradients of a 9 × 8 buffer (edges).
    pub d: u64,
    /// Mean RGB of a 4 × 4 grid, 48 bytes (color layout); compared by correlation.
    pub c: Vec<u8>,
    /// Color histogram of 4 × 4 × 4 RGB bins, each the share of the pixels × 255 (64 bytes). It
    /// does not change when the camera moves a little between 2 photos.
    pub h: Vec<u8>,
}

impl Signature {
    /// Hamming distance of both hashes: 0 (equal) to 128; about 64 for 2 unrelated photos.
    pub fn hash_dist(&self, o: &Signature) -> u32 {
        (self.p ^ o.p).count_ones() + (self.d ^ o.d).count_ones()
    }

    /// Difference of the color histograms: 0 (equal) to 255 (no common color).
    pub fn hist_dist(&self, o: &Signature) -> u32 {
        if self.h.len() != o.h.len() {
            return 255;
        }
        let sum: u32 = self.h.iter().zip(&o.h).map(|(a, b)| a.abs_diff(*b) as u32).sum();
        (sum / 2).min(255)
    }
}

#[derive(Default)]
pub struct SimilarityAnalyzer;

impl ImageAnalyzer for SimilarityAnalyzer {
    const ID: &'static str = "similarity";
    const VERSION: u32 = ALGO_VERSION;
    type Output = Signature;

    fn input(&self) -> InputSpec {
        InputSpec { gray: Some(ANALYSIS_EDGE), rgba: Some(ANALYSIS_EDGE), meta: false }
    }

    fn analyze(&self, input: &AnalysisInput) -> Option<Signature> {
        let g = input.gray.as_ref()?;
        let c = input.rgba.as_ref()?;
        if g.width < 2 || g.height < 2 || c.width < 1 || c.height < 1 {
            return None;
        }
        Some(signature(g, c))
    }
}

/// The signature of a luminance plane and an RGBX plane of the same image.
pub fn signature(gray: &Plane, rgbx: &Plane) -> Signature {
    let small = resample(&gray.data, gray.width, gray.height, 1, 0, DCT_N, DCT_N);
    let wide = resample(&gray.data, gray.width, gray.height, 1, 0, 9, 8);
    let mut c = Vec::with_capacity(COLOR_GRID * COLOR_GRID * 3);
    let planes: Vec<Vec<f32>> =
        (0..3).map(|ch| resample(&rgbx.data, rgbx.width, rgbx.height, 4, ch, COLOR_GRID, COLOR_GRID)).collect();
    for i in 0..COLOR_GRID * COLOR_GRID {
        for p in &planes {
            c.push(p[i].round().clamp(0.0, 255.0) as u8);
        }
    }
    let mut bins = [0u32; 64];
    for px in rgbx.data.chunks_exact(4) {
        bins[((px[0] >> 6) as usize) << 4 | ((px[1] >> 6) as usize) << 2 | (px[2] >> 6) as usize] += 1;
    }
    let n = (rgbx.width * rgbx.height) as u32;
    let h = bins.iter().map(|&b| ((b * 255 + n / 2) / n) as u8).collect();
    Signature { p: phash(&small), d: dhash(&wide), c, h }
}

/// Area-average resample of 1 channel to `ow` × `oh`, with no regard to the aspect ratio.
fn resample(data: &[u8], w: usize, h: usize, channels: usize, ch: usize, ow: usize, oh: usize) -> Vec<f32> {
    let mut out = vec![0f32; ow * oh];
    for oy in 0..oh {
        let y0 = oy * h / oh;
        let y1 = ((oy + 1) * h / oh).max(y0 + 1).min(h);
        for ox in 0..ow {
            let x0 = ox * w / ow;
            let x1 = ((ox + 1) * w / ow).max(x0 + 1).min(w);
            let mut sum = 0u32;
            for y in y0..y1 {
                let row = &data[y * w * channels..];
                for x in x0..x1 {
                    sum += row[x * channels + ch] as u32;
                }
            }
            out[oy * ow + ox] = sum as f32 / ((y1 - y0) * (x1 - x0)) as f32;
        }
    }
    out
}

/// The 8 lowest DCT-II basis functions over 32 samples.
fn dct_basis() -> &'static [[f32; DCT_N]; DCT_K] {
    static B: std::sync::OnceLock<[[f32; DCT_N]; DCT_K]> = std::sync::OnceLock::new();
    B.get_or_init(|| {
        let mut b = [[0f32; DCT_N]; DCT_K];
        for (u, row) in b.iter_mut().enumerate() {
            for (x, v) in row.iter_mut().enumerate() {
                *v = ((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI / (2 * DCT_N) as f32).cos();
            }
        }
        b
    })
}

/// pHash of a 32 × 32 buffer: bit i is set if the low frequency i is above the median of the 63
/// frequencies without DC. The DC bit stays 0.
fn phash(px: &[f32]) -> u64 {
    let basis = dct_basis();
    // Rows first: tmp[y][u].
    let mut tmp = [[0f32; DCT_K]; DCT_N];
    for y in 0..DCT_N {
        let row = &px[y * DCT_N..(y + 1) * DCT_N];
        for u in 0..DCT_K {
            tmp[y][u] = row.iter().zip(&basis[u]).map(|(p, b)| p * b).sum();
        }
    }
    let mut coef = [0f32; DCT_K * DCT_K];
    for v in 0..DCT_K {
        for u in 0..DCT_K {
            coef[v * DCT_K + u] = (0..DCT_N).map(|y| tmp[y][u] * basis[v][y]).sum();
        }
    }
    let mut sorted: Vec<f32> = coef[1..].to_vec();
    sorted.sort_unstable_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];
    let mut bits = 0u64;
    for (i, &c) in coef.iter().enumerate().skip(1) {
        if c > median {
            bits |= 1 << i;
        }
    }
    bits
}

/// dHash of a 9 × 8 buffer: bit is set if a pixel is brighter than its left neighbor. A margin of
/// half a gray level keeps flat areas at 0.
fn dhash(px: &[f32]) -> u64 {
    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            if px[y * 9 + x + 1] > px[y * 9 + x] + 0.5 {
                bits |= 1 << (y * 8 + x);
            }
        }
    }
    bits
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pipeline::decode_input_for;
    use std::path::Path;

    /// A synthetic "photo": smooth gradients and a few shapes that depend on `seed`.
    pub(crate) fn scene(seed: u32, w: u32, h: u32) -> image::RgbImage {
        let s = seed as f32;
        image::RgbImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let cx = 0.25 + 0.5 * ((s * 0.37).sin() * 0.5 + 0.5);
            let cy = 0.25 + 0.5 * ((s * 0.71).cos() * 0.5 + 0.5);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let blob = if d < 0.18 + 0.05 * (s * 1.3).sin() { 1.0 } else { 0.0 };
            let wave = ((fx * (3.0 + s % 5.0) + fy * (2.0 + s % 3.0)) * std::f32::consts::PI).sin() * 0.5 + 0.5;
            let r = 40.0 + 150.0 * fx * ((s * 0.9).sin() * 0.5 + 0.5) + 60.0 * blob;
            let g = 30.0 + 120.0 * wave + 70.0 * blob * (s * 0.3).cos().abs();
            let b = 50.0 + 150.0 * fy * ((s * 0.5).cos() * 0.5 + 0.5);
            image::Rgb([r.min(255.0) as u8, g.min(255.0) as u8, b.min(255.0) as u8])
        })
    }

    pub(crate) fn sig_of(p: &Path) -> Signature {
        let a = SimilarityAnalyzer;
        a.analyze(&decode_input_for(&a, p).unwrap()).unwrap()
    }

    #[test]
    fn the_same_photo_in_another_size_and_format_has_a_near_equal_signature() {
        let t = tempfile::tempdir().unwrap();
        let big = t.path().join("big.png");
        let small = t.path().join("small.jpg");
        let other = t.path().join("other.png");
        scene(1, 800, 600).save(&big).unwrap();
        image::imageops::resize(&scene(1, 800, 600), 320, 240, image::imageops::FilterType::Triangle).save(&small).unwrap();
        scene(7, 800, 600).save(&other).unwrap();
        let (a, b, c) = (sig_of(&big), sig_of(&small), sig_of(&other));
        assert_eq!((a.c.len(), a.h.len()), (48, 64));
        assert!(a.hash_dist(&b) <= 6, "{}", a.hash_dist(&b));
        assert!(a.hist_dist(&b) <= 12, "{}", a.hist_dist(&b));
        assert!(a.hash_dist(&c) > 30, "{}", a.hash_dist(&c));
    }

    #[test]
    fn a_flat_image_has_a_stable_signature() {
        let g = Plane::new(64, 64, 1, vec![128; 64 * 64]);
        let c = Plane::new(64, 64, 4, vec![128; 64 * 64 * 4]);
        let s = signature(&g, &c);
        assert_eq!(s.d, 0);
        assert_eq!(s, signature(&g, &c));
    }

    /// The example folder "somebursts": IMG_0633..0635 are 1 hand-held series with 3 s between the
    /// photos; the other photos are other scenes. The thresholds of `Level` come from this folder.
    #[test]
    fn the_example_burst_is_found() {
        use super::cluster::{cluster_bursts, cluster_similar, BurstKind, Level};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery/somebursts");
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| crate::scan::is_image_path(p)).collect();
        files.sort();
        if files.len() != 8 {
            return;
        }
        let sigs: Vec<Signature> = files.iter().map(|p| sig_of(p)).collect();
        let refs: Vec<Option<&Signature>> = sigs.iter().map(Some).collect();
        assert_eq!(cluster_similar(&refs, Level::Same), Vec::<Vec<usize>>::new());
        assert_eq!(cluster_similar(&refs, Level::High), vec![vec![4, 5]]);
        assert_eq!(cluster_similar(&refs, Level::Medium), vec![vec![3, 4, 5]]);
        let timed: Vec<(i64, Option<&Signature>)> = files
            .iter()
            .zip(&sigs)
            .map(|(p, s)| (crate::decode::date_taken(p).unwrap_or(0), Some(s)))
            .collect();
        assert_eq!(cluster_bursts(&timed, BurstKind::Dense), vec![vec![3, 4, 5]]);
        assert_eq!(cluster_bursts(&timed, BurstKind::Sparse), vec![vec![3, 4, 5]]);
    }
}
