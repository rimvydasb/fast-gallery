//! Photo quality detection: the first client of the image scan pipeline. Supplies only
//! `QualityAnalyzer` (metrics) and `BadPhotoSink` (thresholds, flags, the "Bad photos" collection).
//! The bad photo scan also runs `screenshot::ScreenshotAnalyzer` in the same decode.

pub mod analyzer;
pub mod kernels;
pub mod sink;

pub use analyzer::{QualityAnalyzer, QualityMetrics, ANALYSIS_EDGE};
pub use sink::{bad_photo_flags, BadPhotoSink, QualityFlag, Sensitivity, Thresholds, BAD_PHOTOS_ID};
