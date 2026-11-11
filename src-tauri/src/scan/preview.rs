//! FolderPreviewService: resolves the first 4 images of a folder tree (FPV-01..FPV-18).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, EventTarget, Runtime};

use super::index::PreviewIndex;
use super::{fill_taken, read_entries, sort_entries, ListOptions, SortKey, SortSpec};

pub const PREVIEW_COUNT: usize = 4;
pub const DEFAULT_DEPTH: usize = 6;
pub const MAX_ENTRIES: usize = 2000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PreviewImage {
    /// Absolute path.
    pub p: String,
    /// Modification time, ms (thumbnail URL version).
    pub m: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FolderPreview {
    pub folder: String,
    pub sort: String,
    pub images: Vec<PreviewImage>,
    /// Number of direct image children (FPV-13).
    pub count: u32,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub depth: usize,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { depth: DEFAULT_DEPTH, max_entries: MAX_ENTRIES }
    }
}

struct State<'a> {
    sort: SortSpec,
    limits: Limits,
    budget: usize,
    images: Vec<PreviewImage>,
    count: u32,
    index: &'a PreviewIndex,
}

/// Resolves the preview of one folder: direct images first (FPV-05), then subfolders depth first
/// (FPV-06), within the depth and entry limits (FPV-07, FPV-08). Symbolic links, hidden folders,
/// and package folders are skipped (FPV-09, FPV-10).
pub fn resolve(folder: &Path, sort: SortSpec, limits: Limits, index: &PreviewIndex) -> FolderPreview {
    let mut st = State { sort, limits, budget: limits.max_entries, images: Vec::new(), count: 0, index };
    visit(folder, 0, &mut st);
    FolderPreview {
        folder: folder.to_string_lossy().into_owned(),
        sort: sort.as_key(),
        images: st.images,
        count: st.count,
    }
}

fn visit(dir: &Path, depth: usize, st: &mut State) {
    if depth > 0 && st.budget == 0 {
        return;
    }
    // The root folder is read in full, so that the direct image count is exact.
    let max = if depth == 0 { usize::MAX } else { st.budget };
    let Ok((mut entries, read)) =
        read_entries(dir, &ListOptions { show_hidden: false, max_entries: max, follow_links: false })
    else {
        return;
    };
    st.budget = st.budget.saturating_sub(read);
    if depth == 0 {
        st.count = entries.iter().filter(|e| !e.d).count() as u32;
    }
    if st.sort.key == SortKey::Taken && entries.iter().any(|e| !e.d) {
        fill_taken(dir, &mut entries, st.index);
    }
    sort_entries(&mut entries, st.sort);
    for e in entries.iter().filter(|e| !e.d) {
        if st.images.len() >= PREVIEW_COUNT {
            return;
        }
        st.images.push(PreviewImage { p: dir.join(&e.n).to_string_lossy().into_owned(), m: e.m });
    }
    if st.images.len() >= PREVIEW_COUNT || depth >= st.limits.depth {
        return;
    }
    for e in entries.iter().filter(|e| e.d) {
        if st.budget == 0 || st.images.len() >= PREVIEW_COUNT {
            return;
        }
        visit(&dir.join(&e.n), depth + 1, st);
    }
}

pub struct FolderPreviewService {
    pub index: Arc<PreviewIndex>,
    generations: Mutex<HashMap<String, u64>>,
}

impl FolderPreviewService {
    pub fn new(index: Arc<PreviewIndex>) -> Self {
        Self { index, generations: Mutex::new(HashMap::new()) }
    }

    fn next_gen(&self, window: &str) -> u64 {
        let mut g = self.generations.lock();
        let v = g.entry(window.to_string()).or_insert(0);
        *v += 1;
        *v
    }

    fn is_current(&self, window: &str, gen: u64) -> bool {
        self.generations.lock().get(window).copied() == Some(gen)
    }

    /// Returns cached previews at once (FPV-18) and resolves all `folders` again in the background,
    /// in the given order (visible tiles first, FPV-14). Each result is stored (FPV-16) and sent as a
    /// `folder-preview-ready` event (FPV-15). A new request from the same window cancels the jobs of
    /// the previous one that have not started.
    pub fn request<R: Runtime>(
        self: &Arc<Self>,
        app: &AppHandle<R>,
        window: &str,
        folders: Vec<String>,
        sort: SortSpec,
        limits: Limits,
    ) -> Vec<FolderPreview> {
        let key = sort.as_key();
        let cached = self.index.get_many(&folders, &key);
        let cached_map: HashMap<String, FolderPreview> =
            cached.iter().map(|p| (p.folder.clone(), p.clone())).collect();
        let gen = self.next_gen(window);
        let cached_map = Arc::new(cached_map);
        for folder in folders {
            let this = Arc::clone(self);
            let app = app.clone();
            let window = window.to_string();
            let cached_map = Arc::clone(&cached_map);
            crate::pools::scan().spawn_fifo(move || {
                if !this.is_current(&window, gen) {
                    return;
                }
                let p = resolve(Path::new(&folder), sort, limits, &this.index);
                this.index.put(&p);
                if cached_map.get(&folder) != Some(&p) {
                    let _ = app.emit_to(EventTarget::webview_window(&window), "folder-preview-ready", &p);
                }
            });
        }
        cached
    }

    /// Resolves one folder in the background after a file system change.
    pub fn refresh<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>, window: &str, folder: String, sort: SortSpec, limits: Limits) {
        let this = Arc::clone(self);
        let app = app.clone();
        let window = window.to_string();
        crate::pools::scan().spawn_fifo(move || {
            let p = resolve(Path::new(&folder), sort, limits, &this.index);
            this.index.put(&p);
            let _ = app.emit_to(EventTarget::webview_window(&window), "folder-preview-ready", &p);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn names(p: &FolderPreview) -> Vec<String> {
        p.images.iter().map(|i| Path::new(&i.p).file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn direct_images_first_then_depth_first() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("2026");
        touch(&root.join("01/image1.jpg"));
        touch(&root.join("01/deeper/image5.jpg"));
        touch(&root.join("02/image3.jpg"));
        touch(&root.join("top10.jpg"));
        touch(&root.join("top2.png"));
        touch(&root.join("notes.txt"));
        let idx = PreviewIndex::in_memory().unwrap();
        let p = resolve(&root, SortSpec::default(), Limits::default(), &idx);
        assert_eq!(names(&p), ["top2.png", "top10.jpg", "image1.jpg", "image5.jpg"]);
        assert_eq!(p.count, 2);
    }

    #[test]
    fn respects_depth_hidden_packages_and_links() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("r");
        touch(&root.join(".hidden/a.jpg"));
        touch(&root.join("Lib.photoslibrary/b.jpg"));
        touch(&root.join("1/2/3/4/5/6/deep6.jpg"));
        touch(&root.join("1/2/3/4/5/6/7/deep7.jpg"));
        let other = t.path().join("other");
        touch(&other.join("linked.jpg"));
        std::os::unix::fs::symlink(&other, root.join("link")).unwrap();
        let idx = PreviewIndex::in_memory().unwrap();
        let p = resolve(&root, SortSpec::default(), Limits::default(), &idx);
        assert_eq!(names(&p), ["deep6.jpg"]);
        let p = resolve(&root, SortSpec::default(), Limits { depth: 5, max_entries: MAX_ENTRIES }, &idx);
        assert!(p.images.is_empty());
    }

    #[test]
    fn stops_at_entry_budget() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("r");
        for i in 0..30 {
            touch(&root.join(format!("a/f{i}.txt")));
        }
        touch(&root.join("b/img.jpg"));
        let idx = PreviewIndex::in_memory().unwrap();
        let p = resolve(&root, SortSpec::default(), Limits { depth: 6, max_entries: 20 }, &idx);
        assert!(p.images.is_empty());
        let p = resolve(&root, SortSpec::default(), Limits::default(), &idx);
        assert_eq!(names(&p), ["img.jpg"]);
    }
}
