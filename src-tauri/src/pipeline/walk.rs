//! TreeWalker: the images of a folder tree, depth first, in natural name order (GRID-11). The images
//! of a folder come before its subfolders. Symbolic links, hidden items, package folders, and
//! ignored folders are skipped (FPV-09, FPV-10, IGN-03).

use std::collections::VecDeque;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::scan::ignore::IgnoreSet;
use crate::scan::{read_entries, sort_entries, ListOptions, SortKey, SortSpec};
use crate::tasks::CancelToken;

#[derive(Clone, Copy, Debug, Default)]
pub struct WalkOptions {
    pub max_depth: Option<usize>,
    pub max_images: Option<usize>,
    /// Do not enter a folder on another volume than its parent folder (D5).
    pub same_volume: bool,
}

/// One image of the walk, with the values for the cache stale check.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct WalkItem {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: i64,
    /// Creation time, ms since epoch (the grouping by date created).
    pub created: i64,
}

/// The device ID of a folder, for the one-volume rule.
fn device_of(p: &Path) -> Option<u64> {
    std::fs::metadata(p).ok().map(|m| m.dev())
}

/// The images that `run_pipeline` reads: a tree walk or a fixed list.
pub trait ImageSource: Iterator<Item = WalkItem> {
    /// The number of images for the progress total, if known before the end.
    fn total(&self) -> Option<usize>;
    /// True if the source has more images than it gives.
    fn is_capped(&self) -> bool {
        false
    }
}

/// A fixed list of images, for example the images of 1 folder or of a collection.
pub struct ListSource {
    items: std::vec::IntoIter<WalkItem>,
    total: usize,
}

impl ListSource {
    pub fn new(items: Vec<WalkItem>) -> Self {
        Self { total: items.len(), items: items.into_iter() }
    }
}

impl Iterator for ListSource {
    type Item = WalkItem;
    fn next(&mut self) -> Option<WalkItem> {
        self.items.next()
    }
}

impl ImageSource for ListSource {
    fn total(&self) -> Option<usize> {
        Some(self.total)
    }
}

impl ImageSource for TreeWalker {
    fn total(&self) -> Option<usize> {
        self.max_images()
    }
    fn is_capped(&self) -> bool {
        self.capped()
    }
}

/// Called before each folder read, for example to send progress while the walk reads folders
/// without images.
pub type Tick = Box<dyn FnMut() + Send>;

/// A folder still to read: its path, its depth, and the device ID of its parent (D5).
struct Pending {
    dir: PathBuf,
    depth: usize,
    parent_dev: Option<u64>,
}

pub struct TreeWalker {
    opts: WalkOptions,
    /// Folders still to read; the top is the next one.
    stack: Vec<Pending>,
    queue: VecDeque<WalkItem>,
    yielded: usize,
    capped: bool,
    done: bool,
    cancel: Option<CancelToken>,
    ignore: IgnoreSet,
    tick: Option<Tick>,
    device_of: fn(&Path) -> Option<u64>,
}

impl TreeWalker {
    pub fn new(root: &Path, opts: WalkOptions) -> Self {
        Self {
            opts,
            stack: vec![Pending { dir: root.to_path_buf(), depth: 0, parent_dev: None }],
            queue: VecDeque::new(),
            yielded: 0,
            capped: false,
            done: false,
            cancel: None,
            ignore: IgnoreSet::default(),
            tick: None,
            device_of,
        }
    }

    /// The walk checks the token before each folder.
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// The walk does not enter these folders (IGN-03).
    pub fn with_ignore(mut self, ignore: IgnoreSet) -> Self {
        self.ignore = ignore;
        self
    }

    /// `tick` runs before each folder read.
    pub fn with_tick(mut self, tick: Tick) -> Self {
        self.tick = Some(tick);
        self
    }

    /// Tests: a fake device ID per folder, because a test cannot mount a volume.
    #[cfg(test)]
    fn with_device_of(mut self, f: fn(&Path) -> Option<u64>) -> Self {
        self.device_of = f;
        self
    }

    /// True only if the walk found 1 more image after `max_images`.
    pub fn capped(&self) -> bool {
        self.capped
    }

    pub fn max_images(&self) -> Option<usize> {
        self.opts.max_images
    }

    /// Continues a capped walk for `more` images, from the image after the last one (FLAT-09).
    pub fn raise_cap(&mut self, more: usize) {
        if let Some(m) = self.opts.max_images.as_mut() {
            *m = m.saturating_add(more);
        }
        if self.capped {
            self.capped = false;
            self.done = false;
        }
    }

    /// The next image, without the cap. Reads folders until it finds one.
    fn next_raw(&mut self) -> Option<WalkItem> {
        loop {
            if let Some(it) = self.queue.pop_front() {
                return Some(it);
            }
            if self.cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                return None;
            }
            let Pending { dir, depth, parent_dev } = self.stack.pop()?;
            if let Some(t) = self.tick.as_mut() {
                t();
            }
            let dev = if self.opts.same_volume { (self.device_of)(&dir) } else { None };
            // D5: a mount point of another volume is not entered.
            if parent_dev.is_some() && dev.is_some() && dev != parent_dev {
                continue;
            }
            let opts =
                ListOptions { show_hidden: false, max_entries: usize::MAX, follow_links: false, ignore: &self.ignore };
            // A folder that cannot be read is skipped.
            let Ok((mut entries, _)) = read_entries(&dir, &opts) else { continue };
            sort_entries(&mut entries, SortSpec { key: SortKey::Name, desc: false });
            let can_descend = self.opts.max_depth.is_none_or(|m| depth < m);
            let mut subdirs = Vec::new();
            for e in entries {
                if e.d {
                    if can_descend {
                        subdirs.push(dir.join(&e.n));
                    }
                } else {
                    self.queue.push_back(WalkItem { path: dir.join(&e.n), size: e.s, mtime: e.m, created: e.c });
                }
            }
            for d in subdirs.into_iter().rev() {
                self.stack.push(Pending { dir: d, depth: depth + 1, parent_dev: dev });
            }
        }
    }
}

impl Iterator for TreeWalker {
    type Item = WalkItem;

    fn next(&mut self) -> Option<WalkItem> {
        if self.done {
            return None;
        }
        if self.opts.max_images.is_some_and(|m| self.yielded >= m) {
            // Look ahead 1 image, so that a tree with exactly `max_images` images is not capped. The
            // image goes back to the queue for `raise_cap`.
            if let Some(it) = self.next_raw() {
                self.queue.push_front(it);
                self.capped = true;
            }
            self.done = true;
            return None;
        }
        match self.next_raw() {
            Some(it) => {
                self.yielded += 1;
                Some(it)
            }
            None => {
                self.done = true;
                None
            }
        }
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

    fn names(w: TreeWalker) -> Vec<String> {
        w.map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn cap_needs_one_more_image() {
        for (n, capped) in [(99, false), (100, false), (101, true)] {
            let t = tempfile::tempdir().unwrap();
            for i in 0..n {
                // Spread over folders, so that the look-ahead must read another folder.
                touch(&t.path().join(format!("d{}/p{i}.jpg", i / 10)));
            }
            let mut w = TreeWalker::new(t.path(), WalkOptions { max_images: Some(100), ..Default::default() });
            let count = w.by_ref().count();
            assert_eq!(count, n.min(100), "{n}");
            assert_eq!(w.capped(), capped, "{n}");
        }
    }

    #[test]
    fn depth_first_natural_order_images_before_subfolders() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        touch(&r.join("b10/x.jpg"));
        touch(&r.join("b2/deep/y.jpg"));
        touch(&r.join("b2/z.jpg"));
        touch(&r.join("top10.jpg"));
        touch(&r.join("top2.png"));
        touch(&r.join("notes.txt"));
        let w = TreeWalker::new(r, WalkOptions::default());
        assert_eq!(names(w), ["top2.png", "top10.jpg", "z.jpg", "y.jpg", "x.jpg"]);
        let w = TreeWalker::new(r, WalkOptions { max_depth: Some(1), ..Default::default() });
        assert_eq!(names(w), ["top2.png", "top10.jpg", "z.jpg", "x.jpg"]);
    }

    #[test]
    fn skips_hidden_packages_and_links() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("r");
        touch(&root.join(".hidden/a.jpg"));
        touch(&root.join(".b.jpg"));
        touch(&root.join("Lib.photoslibrary/c.jpg"));
        touch(&root.join("ok/d.jpg"));
        let other = t.path().join("other");
        touch(&other.join("e.jpg"));
        std::os::unix::fs::symlink(&other, root.join("link")).unwrap();
        std::os::unix::fs::symlink(other.join("e.jpg"), root.join("f.jpg")).unwrap();
        assert_eq!(names(TreeWalker::new(&root, WalkOptions::default())), ["d.jpg"]);
    }

    #[test]
    fn walk_stays_on_volume() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        touch(&r.join("a.jpg"));
        touch(&r.join("card/b.jpg"));
        touch(&r.join("card/inner/c.jpg"));
        touch(&r.join("local/d.jpg"));
        // "card" is a mount point of another volume; its subfolders are on that volume too.
        fn fake_dev(p: &Path) -> Option<u64> {
            Some(if p.components().any(|c| c.as_os_str() == "card") { 2 } else { 1 })
        }
        let same = WalkOptions { same_volume: true, ..Default::default() };
        assert_eq!(names(TreeWalker::new(r, same).with_device_of(fake_dev)), ["a.jpg", "d.jpg"]);
        // A walk from inside the other volume stays on it.
        assert_eq!(names(TreeWalker::new(&r.join("card"), same).with_device_of(fake_dev)), ["b.jpg", "c.jpg"]);
        // Without the rule, the walk enters the other volume.
        let any = WalkOptions::default();
        assert_eq!(names(TreeWalker::new(r, any).with_device_of(fake_dev)), ["a.jpg", "b.jpg", "c.jpg", "d.jpg"]);
    }

    #[test]
    fn raise_cap_continues_after_the_last_image() {
        let t = tempfile::tempdir().unwrap();
        for i in 0..7 {
            touch(&t.path().join(format!("d{}/p{i}.jpg", i / 3)));
        }
        let mut w = TreeWalker::new(t.path(), WalkOptions { max_images: Some(3), ..Default::default() });
        let first: Vec<_> = w.by_ref().collect();
        assert_eq!(first.len(), 3);
        assert!(w.capped());
        w.raise_cap(3);
        assert!(!w.capped());
        let second: Vec<_> = w.by_ref().collect();
        assert_eq!(second.len(), 3);
        assert!(w.capped());
        w.raise_cap(3);
        let third: Vec<_> = w.by_ref().collect();
        assert_eq!(third.len(), 1);
        assert!(!w.capped());
        let all: std::collections::BTreeSet<_> = first.into_iter().chain(second).chain(third).map(|i| i.path).collect();
        assert_eq!(all.len(), 7, "no image twice, none lost");
    }

    #[test]
    fn skips_ignored_folders_and_ticks_per_folder() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        touch(&r.join("a/1.jpg"));
        touch(&r.join("Skip/2.jpg"));
        touch(&r.join("Skip/deeper/3.jpg"));
        let ignore = IgnoreSet::new(&[r.join("skip").to_string_lossy()], "/");
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = std::sync::Arc::clone(&n);
        let w = TreeWalker::new(r, WalkOptions::default())
            .with_ignore(ignore)
            .with_tick(Box::new(move || {
                n2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }));
        assert_eq!(names(w), ["1.jpg"]);
        // The root and "a".
        assert_eq!(n.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn cancel_stops_before_the_next_folder() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("a/1.jpg"));
        touch(&t.path().join("b/2.jpg"));
        let c = CancelToken::default();
        let mut w = TreeWalker::new(t.path(), WalkOptions::default()).with_cancel(c.clone());
        assert!(w.next().is_some());
        c.cancel();
        assert!(w.next().is_none());
    }
}
