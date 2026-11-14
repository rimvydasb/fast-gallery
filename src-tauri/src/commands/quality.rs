//! Photo quality commands: the bad photo scan (QUAL-07..QUAL-13) and the metrics of one image.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tauri::{Manager, State, WebviewWindow};

use super::{blocking, CmdError, CmdResult};
use crate::pipeline::{
    decode_input_for, max_in_flight, run_pipeline, AnalysisCache, ImageAnalyzer, Pair, TreeWalker, WalkOptions,
};
use crate::quality::{bad_photo_flags, BadPhotoSink, QualityAnalyzer, QualityFlag, QualityMetrics, Thresholds};
use crate::screenshot::ScreenshotAnalyzer;

/// The analyzers of the bad photo scan: quality metrics and screenshot signals from 1 decode.
type BadPhotoAnalyzer = Pair<QualityAnalyzer, ScreenshotAnalyzer>;
use crate::tasks::{TaskKind, TaskStatus};
use crate::AppState;

/// Starts the scan of `root` and all its subfolders, up to the scan cap. No dialog (QUAL-08).
#[tauri::command]
pub fn start_scan_bad_photos(window: WebviewWindow, state: State<'_, AppState>, root: String) -> CmdResult<TaskStatus> {
    state.tasks.guard()?;
    let root = if root.len() > 1 { root.trim_end_matches('/').to_string() } else { root };
    match std::fs::metadata(&root) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(CmdError { kind: "notDir", message: "A scan needs a folder.".into() }),
        Err(e) => return Err(CmdError::io(Path::new(&root), &e)),
    }
    let s = state.settings.get();
    let thresholds = Thresholds::for_sensitivity(s.quality_sensitivity);
    let cap = s.scan_cap as usize;
    let store = Arc::clone(&state.collections);
    let index = Arc::clone(&state.previews.index);
    let app = window.app_handle().clone();
    let r = root.clone();
    let status = state.tasks.start(
        TaskKind::ScanBadPhotos,
        window.label(),
        Some(root.clone()),
        vec![root],
        Arc::new(app),
        move |ctx| {
            let walker = TreeWalker::new(Path::new(&r), WalkOptions { max_depth: None, max_images: Some(cap) })
                .with_cancel(ctx.cancel_token().clone());
            let sink = BadPhotoSink::new(thresholds, store, r.clone());
            let mut cache = AnalysisCache::new(index);
            Ok(run_pipeline(walker, Arc::new(Pair(QualityAnalyzer, ScreenshotAnalyzer)), sink, &mut cache, ctx, max_in_flight()))
        },
    )?;
    Ok(status)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityInfo {
    pub metrics: QualityMetrics,
    pub flags: Vec<QualityFlag>,
}

/// The quality metrics of one image (QUAL-23): from the cache, else a new analysis.
#[tauri::command]
pub async fn get_quality(state: State<'_, AppState>, path: String) -> CmdResult<Option<QualityInfo>> {
    let index = Arc::clone(&state.previews.index);
    let thresholds = Thresholds::for_sensitivity(state.settings.get().quality_sensitivity);
    blocking(move || {
        let p = Path::new(&path);
        let md = std::fs::metadata(p).map_err(|e| CmdError::io(p, &e))?;
        let (mtime, size) = (crate::scan::mtime_ms(&md), md.len());
        let mut cache = AnalysisCache::new(index);
        let (metrics, shot) = match cache.get::<BadPhotoAnalyzer>(p, mtime, size) {
            Some(r) => r,
            None => {
                let a = Pair(QualityAnalyzer, ScreenshotAnalyzer);
                let Some(r) = decode_input_for(&a, p).and_then(|i| a.analyze(&i)) else { return Ok(None) };
                cache.put::<BadPhotoAnalyzer>(p, mtime, size, &r);
                r
            }
        };
        Ok(Some(QualityInfo { flags: bad_photo_flags(&thresholds, p, &metrics, &shot), metrics }))
    })
    .await?
}
