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

use crate::scan::ignore::IgnoreSet;
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
    /// true if a change in the tree can change the photos of a flatten: an image file or a folder,
    /// not hidden and not ignored (FLAT-14). Other changes, for example app data files, do not
    /// start a new walk.
    pub photos: bool,
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

/// True if one of `paths` in the tree of `dir` is an image file or a folder that a walk reads.
pub fn touches_photos(dir: &Path, paths: &BTreeSet<PathBuf>, ignore: &IgnoreSet) -> bool {
    use crate::scan::{is_hidden_name, is_image_path, is_package_name};
    paths.iter().any(|p| {
        let Ok(rel) = p.strip_prefix(dir) else { return false };
        if rel.as_os_str().is_empty() {
            return false;
        }
        let skipped = rel.components().any(|c| c.as_os_str().to_str().is_none_or(|n| is_hidden_name(n) || is_package_name(n)));
        if skipped {
            return false;
        }
        // A folder that was added, removed, or renamed can hold photos. A removed path has no
        // metadata: one without an extension is taken as a folder.
        let candidate = is_image_path(p)
            || match std::fs::symlink_metadata(p) {
                Ok(m) => m.is_dir(),
                Err(_) => p.extension().is_none(),
            };
        candidate && !ignore.in_tree(p)
    })
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
        ignore: Arc<parking_lot::RwLock<IgnoreSet>>,
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
                    let mut change = classify(&root, &batch);
                    change.photos = touches_photos(&root, &batch, &ignore.read());
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

    #[test]
    fn only_image_and_folder_changes_touch_photos() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::fs::create_dir_all(r.join("Lib/Caches")).unwrap();
        std::fs::create_dir_all(r.join("Lib/Data.v2")).unwrap();
        let ignore = IgnoreSet::new(&[r.join("Lib/Caches").to_string_lossy()], "/");
        let one = |p: &str| touches_photos(r, &[r.join(p)].into_iter().collect(), &ignore);
        assert!(one("a/x.jpg"));
        assert!(one("gone-folder"));
        assert!(one("Lib/Data.v2"), "an existing folder with a dot");
        assert!(!one("Lib/db.sqlite-wal"));
        assert!(!one(".git/x.jpg"));
        assert!(!one("Lib/Caches/thumb.jpg"));
        assert!(!one("Photos.photoslibrary/originals/a.jpg"));
        assert!(!one(""));
    }
}
