//! QualityAnalyzer: sharpness and exposure metrics on a 1024 px grayscale buffer (QUAL-15).

use serde::{Deserialize, Serialize};

use crate::pipeline::{AnalysisInput, ImageAnalyzer, InputSpec};

/// Long edge of the analysis buffer. A fixed size makes one blur threshold valid for all cameras.
pub const ANALYSIS_EDGE: u32 = 1024;
/// A change makes the cached rows stale.
pub const ALGO_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityMetrics {
    /// Variance of the 3×3 Laplacian over all interior pixels: global sharpness.
    pub lap_var: f64,
    /// 90th percentile of the per-tile Laplacian variances: sharpness of the sharpest areas.
    pub lap_var_local: f64,
    /// Mean of Gx² + Gy² from the 3×3 Sobel operators.
    pub tenengrad: f64,
    /// Mean of (I(x+2, y) − I(x, y))².
    pub brenner: f64,
    pub mean_luma: f64,
    pub std_luma: f64,
    /// Fraction of pixels with luminance ≤ 5.
    pub clip_dark: f64,
    /// Fraction of pixels with luminance ≥ 250.
    pub clip_bright: f64,
}

#[derive(Default)]
pub struct QualityAnalyzer;

impl ImageAnalyzer for QualityAnalyzer {
    const ID: &'static str = "quality";
    const VERSION: u32 = ALGO_VERSION;
    type Output = QualityMetrics;

    fn input(&self) -> InputSpec {
        InputSpec::gray(ANALYSIS_EDGE)
    }

    fn analyze(&self, input: &AnalysisInput) -> Option<QualityMetrics> {
        let g = input.gray.as_ref()?;
        if g.width < 3 || g.height < 3 {
            return None;
        }
        Some(super::kernels::metrics(g.width, g.height, &g.data))
    }
}
