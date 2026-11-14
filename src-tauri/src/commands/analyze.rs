//! The analysis task of the grouping and the analysis sorts: quality metrics and similarity
//! signatures for the images of 1 location (a folder without its subfolders, or a collection).
//! The results go only to the `analysis` cache; the listing reads them (`scan::arrange`).

use std::path::Path;
use std::sync::Arc;

use tauri::{Manager, State, WebviewWindow};

use super::{CmdError, CmdResult};
use crate::collections::CollectionStore;
use crate::pipeline::{
    max_in_flight, run_pipeline, AnalysisCache, ImageAnalyzer, ListSource, Pair, PipelineSummary, Progress, ResultSink,
    WalkItem,
};
use crate::quality::QualityAnalyzer;
use crate::scan::index::PreviewIndex;
use crate::scan::{mtime_ms, read_entries, ListOptions};
use crate::screenshot::ScreenshotAnalyzer;
use crate::similarity::SimilarityAnalyzer;
use crate::tasks::{TaskKind, TaskStatus};
use crate::AppState;

const COLLECTION_PREFIX: &str = "collection:";

/// Counts the results; the pipeline writes them to the cache.
#[derive(Default)]
struct CountSink {
    done: u32,
}

impl<O> ResultSink<O> for CountSink {
    fn accept(&mut self, _: &Path, _: &O) {
        self.done += 1;
    }
    fn failed(&mut self, _: &Path) {}
    fn finish(self, s: &PipelineSummary) -> serde_json::Value {
        serde_json::json!({ "scanned": s.scanned, "analyzed": s.analyzed, "failed": s.failed })
    }
}

fn run<A: ImageAnalyzer>(a: A, items: Vec<WalkItem>, index: Arc<PreviewIndex>, ctx: &dyn Progress) -> serde_json::Value {
    let mut cache = AnalysisCache::new(index);
    run_pipeline(ListSource::new(items), Arc::new(a), CountSink::default(), &mut cache, ctx, max_in_flight())
}

/// Runs the analyzers of `need` ("quality", "similarity", or "both") on the images. A cached result
/// is not computed again.
pub fn analyze(need: &str, items: Vec<WalkItem>, index: Arc<PreviewIndex>, ctx: &dyn Progress) -> serde_json::Value {
    match need {
        "quality" => run(Pair(QualityAnalyzer, ScreenshotAnalyzer), items, index, ctx),
        "similarity" => run(SimilarityAnalyzer, items, index, ctx),
        _ => run(Pair(Pair(QualityAnalyzer, ScreenshotAnalyzer), SimilarityAnalyzer), items, index, ctx),
    }
}

/// The images of a location: `collection:<id>`, or a folder path (without its subfolders).
fn images_of(location: &str, show_hidden: bool, store: &CollectionStore) -> Result<Vec<WalkItem>, String> {
    if let Some(id) = location.strip_prefix(COLLECTION_PREFIX) {
        let c = store.get(id).ok_or("The collection no longer exists.")?;
        return Ok(c
            .items
            .iter()
            .filter_map(|it| {
                let md = std::fs::metadata(&it.path).ok()?;
                Some(WalkItem { path: it.path.clone().into(), size: md.len(), mtime: mtime_ms(&md) })
            })
            .collect());
    }
    let dir = Path::new(location);
    let (entries, _) = read_entries(dir, &ListOptions { show_hidden, max_entries: usize::MAX, follow_links: true })
        .map_err(|e| format!("Could not read the folder: {e}"))?;
    Ok(entries.into_iter().filter(|e| !e.d).map(|e| WalkItem { path: dir.join(&e.n), size: e.s, mtime: e.m }).collect())
}

/// Starts the analysis of the images that the location shows. The task locks the folder (for a
/// collection: the root of the scan that made it).
#[tauri::command]
pub fn start_analyze_photos(
    window: WebviewWindow,
    state: State<'_, AppState>,
    location: String,
    need: String,
    show_hidden: bool,
) -> CmdResult<TaskStatus> {
    state.tasks.guard()?;
    if !matches!(need.as_str(), "quality" | "similarity" | "both") {
        return Err(CmdError { kind: "other", message: format!("Unknown analysis “{need}”.") });
    }
    let store = Arc::clone(&state.collections);
    let (root, locks) = match location.strip_prefix(COLLECTION_PREFIX) {
        Some(id) => (None, store.get(id).and_then(|c| c.origin).map(|o| vec![o.root]).unwrap_or_default()),
        None => {
            let dir = if location.len() > 1 { location.trim_end_matches('/').to_string() } else { location.clone() };
            (Some(dir.clone()), vec![dir])
        }
    };
    let index = Arc::clone(&state.previews.index);
    let app = window.app_handle().clone();
    let status = state.tasks.start(TaskKind::AnalyzePhotos, window.label(), root, locks, Arc::new(app), move |ctx| {
        let items = images_of(&location, show_hidden, &store)?;
        Ok(analyze(&need, items, index, ctx))
    })?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::Thresholds;
    use crate::scan::arrange::{list_disk, GroupKey};
    use crate::scan::{SortKey, SortSpec};
    use crate::similarity::tests::scene;

    struct NoCancel;
    impl Progress for NoCancel {
        fn is_cancelled(&self) -> bool {
            false
        }
        fn report_progress(&self, _: u32, _: u32) {}
    }

    /// The whole path: analysis of a folder, then a listing with groups from the cache.
    #[test]
    fn a_folder_is_analyzed_once_and_then_grouped() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        scene(1, 640, 480).save(d.join("a.png")).unwrap();
        scene(9, 640, 480).save(d.join("b.png")).unwrap();
        // The same photo as a.png: smaller, and a JPEG.
        image::imageops::resize(&scene(1, 640, 480), 320, 240, image::imageops::FilterType::Triangle)
            .save(d.join("c.jpg"))
            .unwrap();
        let index = Arc::new(PreviewIndex::in_memory().unwrap());
        let store = CollectionStore::default();
        let th = Thresholds::default();
        let name = SortSpec { key: SortKey::Name, desc: false };

        let before = list_disk(d, name, false, GroupKey::Same, &index, &th).unwrap();
        assert_eq!((before.missing, before.need), (3, Some("similarity")));
        assert_eq!(before.groups.len(), 1);
        assert_eq!(before.groups[0].kind, Some("missing"));

        let items = images_of(d.to_str().unwrap(), false, &store).unwrap();
        let r = analyze("both", items.clone(), Arc::clone(&index), &NoCancel);
        assert_eq!((r["scanned"].as_u64(), r["analyzed"].as_u64()), (Some(3), Some(3)));
        assert_eq!(analyze("similarity", items, Arc::clone(&index), &NoCancel)["analyzed"], 0);

        let after = list_disk(d, name, false, GroupKey::Same, &index, &th).unwrap();
        assert_eq!(after.missing, 0);
        let names: Vec<_> = after.entries.iter().map(|e| e.n.as_str()).collect();
        assert_eq!(names, ["a.png", "c.jpg", "b.png"]);
        let groups: Vec<_> = after.groups.iter().map(|g| (g.label.as_str(), g.count)).collect();
        assert_eq!(groups, [("Duplicates 1", 2), ("No duplicates", 1)]);
        // "both" also filled the quality rows.
        assert_eq!(list_disk(d, name, false, GroupKey::Quality, &index, &th).unwrap().missing, 0);
    }
}
