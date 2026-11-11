//! FsWatcher: watches the open folder of each window with FSEvents (FSW-01).
//! Events are batched for a short time, then the watcher invalidates caches (FSW-03, FSW-04)
//! and sends an `fs-changed` event to the window (FSW-02).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, EventTarget, Runtime};

use crate::scan::index::PreviewIndex;
use crate::thumbs::ThumbnailService;

const BATCH: Duration = Duration::from_millis(120);

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FsChange {
    pub dir: String,
    /// true if a direct entry of `dir` was added, removed, renamed, or changed.
    pub dir_changed: bool,
    /// Direct child folders of `dir` with a change inside their tree.
    pub folders: Vec<String>,
    /// Changed files (direct children of `dir`), for thumbnail reload.
    pub files: Vec<String>,
}

/// Classifies changed paths relative to the watched folder.
pub fn classify(dir: &Path, paths: &BTreeSet<PathBuf>) -> FsChange {
    let mut c = FsChange { dir: dir.to_string_lossy().into_owned(), ..Default::default() };
    let mut folders = BTreeSet::new();
    for p in paths {
        let Ok(rel) = p.strip_prefix(dir) else { continue };
        let mut comps = rel.components();
        let Some(first) = comps.next() else {
            c.dir_changed = true;
            continue;
        };
        let child = dir.join(first);
        if comps.next().is_none() {
            c.dir_changed = true;
            if crate::scan::is_image_path(p) {
                c.files.push(p.to_string_lossy().into_owned());
            } else {
                folders.insert(child.to_string_lossy().into_owned());
            }
        } else {
            folders.insert(child.to_string_lossy().into_owned());
        }
    }
    c.folders = folders.into_iter().collect();
    c
}

#[derive(Default)]
pub struct FsWatcher {
    watchers: Mutex<HashMap<String, (PathBuf, RecommendedWatcher)>>,
}

impl FsWatcher {

    pub fn unwatch(&self, window: &str) {
        self.watchers.lock().remove(window);
    }

    pub fn watch<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        window: &str,
        dir: &Path,
        thumbs: Arc<ThumbnailService>,
        index: Arc<PreviewIndex>,
    ) {
        let mut w = self.watchers.lock();
        if w.get(window).is_some_and(|(d, _)| d == dir) {
            return;
        }
        w.remove(window);
        let (tx, rx) = crossbeam_channel::unbounded::<PathBuf>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                for p in ev.paths {
                    let _ = tx.send(p);
                }
            }
        });
        let Ok(mut watcher) = watcher else { return };
        if watcher.watch(dir, RecursiveMode::Recursive).is_err() {
            return;
        }
        let app = app.clone();
        let label = window.to_string();
        let root = dir.to_path_buf();
        std::thread::Builder::new()
            .name("fswatch".into())
            .spawn(move || {
                // Ends when the watcher (and with it the sender) is dropped.
                while let Ok(first) = rx.recv() {
                    let mut batch = BTreeSet::new();
                    batch.insert(first);
                    let deadline = std::time::Instant::now() + BATCH;
                    while let Ok(p) = rx.recv_deadline(deadline) {
                        batch.insert(p);
                    }
                    for p in &batch {
                        if crate::scan::is_image_path(p) {
                            thumbs.invalidate(p);
                        }
                        index.invalidate_ancestors(p.parent().unwrap_or(p));
                        index.invalidate_ancestors(p);
                    }
                    let change = classify(&root, &batch);
                    let _ = app.emit_to(EventTarget::webview_window(&label), "fs-changed", &change);
                }
            })
            .ok();
        w.insert(window.to_string(), (dir.to_path_buf(), watcher));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths() {
        let dir = Path::new("/g");
        let paths: BTreeSet<PathBuf> =
            ["/g/a.jpg", "/g/2026/01/x.jpg", "/g/new", "/other/z.jpg"].iter().map(PathBuf::from).collect();
        let c = classify(dir, &paths);
        assert!(c.dir_changed);
        assert_eq!(c.files, ["/g/a.jpg"]);
        assert_eq!(c.folders, ["/g/2026", "/g/new"]);
    }
}
