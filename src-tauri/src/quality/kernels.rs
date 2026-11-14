//! Focus and exposure kernels over an 8-bit grayscale buffer, in one pass for the sharpness metrics
//! and one for the histogram. Integer arithmetic per pixel, no `unsafe`.

use super::analyzer::QualityMetrics;

/// Side of one tile for the local sharpness metric.
pub const TILE: usize = 128;
/// A tile at the right or bottom edge counts only if it has at least this part of a full tile.
const MIN_TILE_FILL: usize = TILE * TILE / 4;
const LOCAL_PERCENTILE: f64 = 0.9;
const DARK: u8 = 5;
const BRIGHT: u8 = 250;

#[derive(Clone, Copy, Default)]
struct Acc {
    sum: i64,
    sq: i64,
    n: usize,
}

impl Acc {
    fn var(&self) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        let n = self.n as f64;
        let mean = self.sum as f64 / n;
        (self.sq as f64 / n - mean * mean).max(0.0)
    }
}

/// Computes all 8 metrics of `QualityMetrics`. `px` is row-major with stride `w`.
pub fn metrics(w: usize, h: usize, px: &[u8]) -> QualityMetrics {
    assert_eq!(px.len(), w * h);
    let mut m = QualityMetrics::default();
    if w == 0 || h == 0 {
        return m;
    }

    // Histogram: brightness, contrast, clipping.
    let mut hist = [0u64; 256];
    for &v in px {
        hist[v as usize] += 1;
    }
    let n = (w * h) as f64;
    let (mut s, mut sq) = (0f64, 0f64);
    for (v, &c) in hist.iter().enumerate() {
        let (v, c) = (v as f64, c as f64);
        s += v * c;
        sq += v * v * c;
    }
    m.mean_luma = s / n;
    m.std_luma = (sq / n - m.mean_luma * m.mean_luma).max(0.0).sqrt();
    m.clip_dark = hist[..=DARK as usize].iter().sum::<u64>() as f64 / n;
    m.clip_bright = hist[BRIGHT as usize..].iter().sum::<u64>() as f64 / n;

    // Brenner: horizontal difference at distance 2.
    if w > 2 {
        let mut b: i64 = 0;
        for row in px.chunks_exact(w) {
            for (a, c) in row.iter().zip(&row[2..]) {
                let d = *c as i32 - *a as i32;
                b += (d * d) as i64;
            }
        }
        m.brenner = b as f64 / ((w - 2) * h) as f64;
    }

    if w < 3 || h < 3 {
        return m;
    }
    // Laplacian [0 1 0; 1 -4 1; 0 1 0] and Sobel over the interior pixels, with per-tile sums.
    let tiles_x = w.div_ceil(TILE);
    let tiles_y = h.div_ceil(TILE);
    let mut tiles = vec![Acc::default(); tiles_x * tiles_y];
    let mut lap = Acc::default();
    let mut ten: i64 = 0;
    for y in 1..h - 1 {
        let up = &px[(y - 1) * w..y * w];
        let row = &px[y * w..(y + 1) * w];
        let dn = &px[(y + 1) * w..(y + 2) * w];
        let trow = &mut tiles[(y / TILE) * tiles_x..(y / TILE + 1) * tiles_x];
        for x in 1..w - 1 {
            let (ul, u, ur) = (up[x - 1] as i32, up[x] as i32, up[x + 1] as i32);
            let (l, c, r) = (row[x - 1] as i32, row[x] as i32, row[x + 1] as i32);
            let (dl, d, dr) = (dn[x - 1] as i32, dn[x] as i32, dn[x + 1] as i32);
            let lv = (u + d + l + r - 4 * c) as i64;
            let gx = (ur + 2 * r + dr) - (ul + 2 * l + dl);
            let gy = (dl + 2 * d + dr) - (ul + 2 * u + ur);
            ten += (gx * gx + gy * gy) as i64;
            let t = &mut trow[x / TILE];
            t.sum += lv;
            t.sq += lv * lv;
            t.n += 1;
        }
    }
    for t in &tiles {
        lap.sum += t.sum;
        lap.sq += t.sq;
        lap.n += t.n;
    }
    m.lap_var = lap.var();
    m.tenengrad = ten as f64 / lap.n as f64;
    let mut local: Vec<f64> = tiles.iter().filter(|t| t.n >= MIN_TILE_FILL).map(Acc::var).collect();
    m.lap_var_local = if local.is_empty() {
        m.lap_var
    } else {
        local.sort_by(f64::total_cmp);
        local[((local.len() - 1) as f64 * LOCAL_PERCENTILE).round() as usize]
    };
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkerboard(w: usize, h: usize, sq: usize) -> Vec<u8> {
        (0..w * h).map(|i| if ((i % w) / sq + (i / w) / sq) % 2 == 0 { 30 } else { 220 }).collect()
    }

    fn box_blur(w: usize, h: usize, px: &[u8], r: usize) -> Vec<u8> {
        let mut out = vec![0u8; px.len()];
        for y in 0..h {
            for x in 0..w {
                let (mut s, mut n) = (0u32, 0u32);
                for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                    for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                        s += px[yy * w + xx] as u32;
                        n += 1;
                    }
                }
                out[y * w + x] = (s / n) as u8;
            }
        }
        out
    }

    #[test]
    fn flat_image_has_no_sharpness() {
        let m = metrics(300, 200, &vec![128u8; 300 * 200]);
        assert_eq!(m.lap_var, 0.0);
        assert_eq!(m.lap_var_local, 0.0);
        assert_eq!(m.tenengrad, 0.0);
        assert_eq!(m.brenner, 0.0);
        assert_eq!(m.mean_luma, 128.0);
        assert_eq!(m.std_luma, 0.0);
        assert_eq!(m.clip_dark, 0.0);
    }

    #[test]
    fn blur_lowers_all_sharpness_metrics() {
        let (w, h) = (512, 384);
        let sharp = checkerboard(w, h, 16);
        let blurred = box_blur(w, h, &sharp, 3);
        let a = metrics(w, h, &sharp);
        let b = metrics(w, h, &blurred);
        assert!(b.lap_var < a.lap_var / 4.0, "{} {}", a.lap_var, b.lap_var);
        assert!(b.lap_var_local < a.lap_var_local / 4.0);
        assert!(b.tenengrad < a.tenengrad);
        assert!(b.brenner < a.brenner);
        // Blur keeps the mean.
        assert!((a.mean_luma - b.mean_luma).abs() < 1.0);
    }

    #[test]
    fn local_metric_sees_a_sharp_subject_on_a_blurred_background() {
        let (w, h) = (1024, 768);
        let mut px = box_blur(w, h, &checkerboard(w, h, 16), 4);
        // A sharp 384 × 384 "subject" in the middle (about 10 % of the image).
        let sharp = checkerboard(w, h, 4);
        for y in 192..576 {
            for x in 320..704 {
                px[y * w + x] = sharp[y * w + x];
            }
        }
        let m = metrics(w, h, &px);
        assert!(m.lap_var_local > 2.0 * m.lap_var, "{} {}", m.lap_var_local, m.lap_var);
    }

    #[test]
    fn clipping_fractions() {
        let mut px = vec![0u8; 100];
        px[..30].fill(255);
        let m = metrics(10, 10, &px);
        assert!((m.clip_dark - 0.7).abs() < 1e-9);
        assert!((m.clip_bright - 0.3).abs() < 1e-9);
    }
}
