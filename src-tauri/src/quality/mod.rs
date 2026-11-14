//! Photo quality detection: the first client of the image scan pipeline. Supplies only
//! `QualityAnalyzer` (metrics) and `BadPhotoSink` (thresholds, flags, the "Bad photos" collection).

pub mod analyzer;
pub mod kernels;
pub mod sink;

pub use analyzer::{QualityAnalyzer, QualityMetrics, ANALYSIS_EDGE};
pub use sink::{BadPhotoSink, QualityFlag, Sensitivity, Thresholds, BAD_PHOTOS_ID};
