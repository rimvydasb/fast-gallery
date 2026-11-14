//! TreeWalker: the images of a folder tree, depth first, in natural name order (GRID-11). The images
//! of a folder come before its subfolders. Symbolic links, hidden items, and package folders are
//! skipped (FPV-09, FPV-10).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::scan::{read_entries, sort_entries, ListOptions, SortKey, SortSpec};
use crate::tasks::CancelToken;

#[derive(Clone, Copy, Debug, Default)]
pub struct WalkOptions {
    pub max_depth: Option<usize>,
    pub max_images: Option<usize>,
}

/// One image of the walk, with the values for the cache stale check.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkItem {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: i64,
}

pub struct TreeWalker {
    opts: WalkOptions,
    /// Folders still to read, with their depth; the top is the next one.
    stack: Vec<(PathBuf, usize)>,
    queue: VecDeque<WalkItem>,
    yielded: usize,
    capped: bool,
    done: bool,
    cancel: Option<CancelToken>,
}

impl TreeWalker {
    pub fn new(root: &Path, opts: WalkOptions) -> Self {
        Self {
            opts,
            stack: vec![(root.to_path_buf(), 0)],
            queue: VecDeque::new(),
            yielded: 0,
            capped: false,
            done: false,
            cancel: None,
        }
    }

    /// The walk checks the token before each folder.
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// True only if the walk found 1 more image after `max_images`.
    pub fn capped(&self) -> bool {
        self.capped
    }

    pub fn max_images(&self) -> Option<usize> {
        self.opts.max_images
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
            let (dir, depth) = self.stack.pop()?;
            let opts = ListOptions { show_hidden: false, max_entries: usize::MAX, follow_links: false };
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
                    self.queue.push_back(WalkItem { path: dir.join(&e.n), size: e.s, mtime: e.m });
                }
            }
            for d in subdirs.into_iter().rev() {
                self.stack.push((d, depth + 1));
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
            // Look ahead 1 image, so that a tree with exactly `max_images` images is not capped.
            self.capped = self.next_raw().is_some();
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
            let mut w = TreeWalker::new(t.path(), WalkOptions { max_depth: None, max_images: Some(100) });
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
        let w = TreeWalker::new(r, WalkOptions { max_depth: Some(1), max_images: None });
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
