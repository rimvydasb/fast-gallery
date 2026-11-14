//! BadPhotoSink: applies the thresholds to the metrics and the screenshot rules to the screenshot
//! signals, and fills the "Bad photos" collection. The cache stores only metrics and signals, so a
//! threshold or sensitivity change needs no new analysis.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::analyzer::QualityMetrics;
use crate::screenshot::{is_screenshot, ScreenshotSignals};
use crate::collections::{Collection, CollectionItem, CollectionOrigin, CollectionStore};
use crate::pipeline::{PipelineSummary, ResultSink};
use crate::tasks::TaskKind;

pub const BAD_PHOTOS_ID: &str = "bad-photos";
pub const BAD_PHOTOS_NAME: &str = "Bad photos";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QualityFlag {
    Blurry,
    Underexposed,
    Overexposed,
    LowContrast,
    /// Not a quality problem: the image is a screenshot, not a camera photo.
    Screenshot,
}

impl QualityFlag {
    pub fn badge(self) -> &'static str {
        match self {
            QualityFlag::Blurry => "quality.blurry",
            QualityFlag::Underexposed => "quality.underexposed",
            QualityFlag::Overexposed => "quality.overexposed",
            QualityFlag::LowContrast => "quality.lowContrast",
            QualityFlag::Screenshot => "quality.screenshot",
        }
    }
}

/// Assumption: all defaults are first estimates; calibrate with `examples/quality_dump.rs`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub blur: f64,
    pub dark_mean: f64,
    pub dark_clip: f64,
    pub bright_mean: f64,
    pub bright_clip: f64,
    pub contrast_min: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { blur: 100.0, dark_mean: 50.0, dark_clip: 0.40, bright_mean: 205.0, bright_clip: 0.25, contrast_min: 20.0 }
    }
}

impl Thresholds {
    /// The sensitivity multiplies the blur threshold by 0.5, 1.0, or 2.0.
    pub fn for_sensitivity(s: Sensitivity) -> Self {
        let k = match s {
            Sensitivity::Low => 0.5,
            Sensitivity::Medium => 1.0,
            Sensitivity::High => 2.0,
        };
        let d = Self::default();
        Self { blur: d.blur * k, ..d }
    }

    /// QUAL-16: the blur flag uses `lap_var_local`.
    pub fn flags(&self, m: &QualityMetrics) -> Vec<QualityFlag> {
        let mut f = Vec::new();
        if m.lap_var_local < self.blur {
            f.push(QualityFlag::Blurry);
        }
        if m.mean_luma < self.dark_mean && m.clip_dark > self.dark_clip {
            f.push(QualityFlag::Underexposed);
        }
        if m.mean_luma > self.bright_mean && m.clip_bright > self.bright_clip {
            f.push(QualityFlag::Overexposed);
        }
        if m.std_luma < self.contrast_min {
            f.push(QualityFlag::LowContrast);
        }
        f
    }
}

/// The flags of 1 image for the bad photo scan. A screenshot gets only the `Screenshot` flag: the
/// blur and exposure rules are made for camera photos and give no useful result for a screen.
pub fn bad_photo_flags(th: &Thresholds, path: &Path, m: &QualityMetrics, s: &ScreenshotSignals) -> Vec<QualityFlag> {
    if is_screenshot(path, s) {
        return vec![QualityFlag::Screenshot];
    }
    th.flags(m)
}

pub struct BadPhotoSink {
    thresholds: Thresholds,
    store: Arc<CollectionStore>,
    root: String,
    items: Vec<CollectionItem>,
}

impl BadPhotoSink {
    pub fn new(thresholds: Thresholds, store: Arc<CollectionStore>, root: String) -> Self {
        Self { thresholds, store, root, items: Vec::new() }
    }
}

impl ResultSink<(QualityMetrics, ScreenshotSignals)> for BadPhotoSink {
    fn accept(&mut self, path: &Path, (m, s): &(QualityMetrics, ScreenshotSignals)) {
        let flags = bad_photo_flags(&self.thresholds, path, m, s);
        if !flags.is_empty() {
            let badges = flags.into_iter().map(|f| f.badge().to_string()).collect();
            self.items.push(CollectionItem::new(path.to_string_lossy().into_owned(), badges));
        }
    }

    fn failed(&mut self, _: &Path) {}

    /// A cancelled scan does not change the collection.
    fn finish(self, s: &PipelineSummary) -> serde_json::Value {
        let found = self.items.len();
        if !s.cancelled {
            let created_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let origin = CollectionOrigin {
                task: TaskKind::ScanBadPhotos,
                root: self.root,
                scanned: s.scanned,
                capped: s.capped,
                created_ms,
            };
            self.store.upsert(Collection::new(BAD_PHOTOS_ID, BAD_PHOTOS_NAME, "quality", Some(origin), self.items));
        }
        serde_json::json!({
            "collectionId": BAD_PHOTOS_ID,
            "found": found,
            "scanned": s.scanned,
            "capped": s.capped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::kernels::metrics;

    fn flags_of(v: u8) -> Vec<QualityFlag> {
        Thresholds::default().flags(&metrics(64, 64, &vec![v; 64 * 64]))
    }

    #[test]
    fn black_is_underexposed_and_white_is_overexposed() {
        assert!(flags_of(0).contains(&QualityFlag::Underexposed));
        assert!(!flags_of(0).contains(&QualityFlag::Overexposed));
        assert!(flags_of(255).contains(&QualityFlag::Overexposed));
        assert!(!flags_of(255).contains(&QualityFlag::Underexposed));
        // A flat image is also blurry and low in contrast.
        assert!(flags_of(128).contains(&QualityFlag::Blurry));
        assert!(flags_of(128).contains(&QualityFlag::LowContrast));
    }

    #[test]
    fn sensitivity_scales_only_the_blur_threshold() {
        let m = QualityMetrics { lap_var_local: 150.0, mean_luma: 120.0, std_luma: 50.0, ..Default::default() };
        assert!(Thresholds::for_sensitivity(Sensitivity::Low).flags(&m).is_empty());
        assert!(Thresholds::for_sensitivity(Sensitivity::Medium).flags(&m).is_empty());
        assert_eq!(Thresholds::for_sensitivity(Sensitivity::High).flags(&m), [QualityFlag::Blurry]);
    }

    #[test]
    fn the_example_screenshot_gets_only_the_screenshot_flag() {
        use crate::pipeline::{decode_input_for, ImageAnalyzer, Pair};
        use crate::quality::QualityAnalyzer;
        use crate::screenshot::ScreenshotAnalyzer;
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery/another/screenshot_examle.JPEG");
        if !p.exists() {
            return;
        }
        let a = Pair(QualityAnalyzer, ScreenshotAnalyzer);
        let (m, s) = a.analyze(&decode_input_for(&a, &p).unwrap()).unwrap();
        assert_eq!(bad_photo_flags(&Thresholds::default(), &p, &m, &s), [QualityFlag::Screenshot]);
    }

    #[test]
    fn finish_fills_the_collection_unless_cancelled() {
        let store = Arc::new(CollectionStore::default());
        let blurry = QualityMetrics { lap_var_local: 1.0, mean_luma: 120.0, std_luma: 50.0, ..Default::default() };
        let sharp = QualityMetrics { lap_var_local: 900.0, ..blurry };
        let photo = ScreenshotSignals { camera_exif: true, ..Default::default() };
        let run = |cancelled: bool| {
            let mut s = BadPhotoSink::new(Thresholds::default(), Arc::clone(&store), "/r".into());
            s.accept(Path::new("/r/a.jpg"), &(blurry, photo));
            s.accept(Path::new("/r/b.jpg"), &(sharp, photo));
            // A blurry screenshot gets only the screenshot badge.
            s.accept(Path::new("/r/Screenshot 1.png"), &(blurry, ScreenshotSignals::default()));
            s.finish(&PipelineSummary { scanned: 3, cancelled, ..Default::default() })
        };
        let r = run(true);
        assert_eq!(r["found"], 2);
        assert!(store.get(BAD_PHOTOS_ID).is_none());
        run(false);
        let c = store.get(BAD_PHOTOS_ID).unwrap();
        assert_eq!(c.items.len(), 2);
        assert_eq!(c.items[0].badges, ["quality.blurry"]);
        assert_eq!(c.items[1].badges, ["quality.screenshot"]);
    }
}
