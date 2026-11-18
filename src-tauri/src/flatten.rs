//! FlatStore: the flat listings of folder trees (FLAT-01..FLAT-16). A flatten is a per-window
//! listing, not a `TaskRunner` task (D1): it only reads, so it does not block a task and a task does
//! not block it. The walk streams the photos it finds, stops at a cap, and stops on cancel (D2).
//! The store keeps the paths found so far; `list_flat` sorts them on each refresh (D4).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use crate::pipeline::{TreeWalker, WalkItem, WalkOptions};
use crate::scan::ignore::IgnoreSet;
use crate::tasks::CancelToken;

/// Minimum interval between 2 `flat-progress` events of a session (D4).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// The key prefix of a flat location (FLAT-15).
pub const FLAT_PREFIX: &str = "flat:";

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlatStatus {
    /// The session id from `start_flatten`.
    pub id: String,
    /// The absolute path of the flattened folder.
    pub root: String,
    /// The photos found so far by the current walk.
    pub count: u32,
    /// The walk finished the whole tree.
    pub done: bool,
    /// The walk stopped at the cap and the tree has more photos.
    pub capped: bool,
    /// The user or a location change stopped the walk.
    pub cancelled: bool,
    /// A new walk after a file system change runs; the listing shows the result of the last walk
    /// until it ends (FLAT-14).
    pub refreshing: bool,
}

impl FlatStatus {
    pub fn walking(&self) -> bool {
        !self.done && !self.capped && !self.cancelled
    }
}

/// Sends the events of a session. `AppHandle` implements it in the command layer; tests use a recorder.
pub trait FlatEvents: Send + Sync + 'static {
    /// `flat-progress` to the window that owns the session.
    fn progress(&self, window: &str, status: &FlatStatus);
    /// The first photo of a session: the flat location goes to Recents (FLAT-15).
    fn first_photo(&self, _root: &Path) {}
}

/// Runs a walk. The app uses the scan pool; tests run the walk when they choose to.
pub type Spawner = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

struct Inner {
    /// The photos that `list_flat` shows.
    items: Vec<WalkItem>,
    /// A refresh walk collects here; `items` stays visible until the walk ends (FLAT-14).
    next: Option<Vec<WalkItem>>,
    status: FlatStatus,
    /// The walker of a capped walk, for "Show More" (FLAT-09).
    walker: Option<TreeWalker>,
    cancel: CancelToken,
    /// The walk generation: a walk of an older generation changes nothing.
    gen: u64,
    /// The current limit of the walk: the cap, plus the cap for each "Show More".
    limit: usize,
    last_emit: Option<Instant>,
    dirty: bool,
    /// `first_photo` was sent.
    recorded: bool,
}

struct Session {
    id: String,
    window: String,
    root: PathBuf,
    events: Arc<dyn FlatEvents>,
    inner: Mutex<Inner>,
}

impl Session {
    /// Sends the progress if there are new photos and the last event is old enough.
    fn tick(&self, gen: u64) {
        let (status, first) = {
            let mut i = self.inner.lock();
            if i.gen != gen || !i.dirty || i.last_emit.is_some_and(|t| t.elapsed() < PROGRESS_INTERVAL) {
                return;
            }
            i.last_emit = Some(Instant::now());
            i.dirty = false;
            let first = take_first(&mut i);
            (i.status.clone(), first)
        };
        self.emit(&status, first);
    }

    fn emit(&self, status: &FlatStatus, first: bool) {
        if first {
            self.events.first_photo(&self.root);
        }
        self.events.progress(&self.window, status);
    }

    fn walker(self: &Arc<Self>, limit: usize, cancel: CancelToken, gen: u64, ignore: IgnoreSet) -> TreeWalker {
        let s = Arc::clone(self);
        let opts = WalkOptions { max_images: Some(limit), same_volume: true, ..Default::default() };
        TreeWalker::new(&self.root, opts)
            .with_cancel(cancel)
            .with_ignore(ignore)
            .with_tick(Box::new(move || s.tick(gen)))
    }
}

/// True once per session, for the first event with photos.
fn take_first(i: &mut Inner) -> bool {
    let first = !i.recorded && i.status.count > 0;
    i.recorded |= first;
    first
}

/// Runs a walk to its end, cap, or cancel, and sends the final status.
fn run_walk(s: Arc<Session>, gen: u64, mut walker: TreeWalker) {
    for item in walker.by_ref() {
        let mut i = s.inner.lock();
        if i.gen != gen {
            return;
        }
        match i.next.as_mut() {
            Some(n) => n.push(item),
            None => i.items.push(item),
        }
        i.status.count += 1;
        i.dirty = true;
    }
    let (status, first) = {
        let mut i = s.inner.lock();
        if i.gen != gen {
            return;
        }
        let cancelled = i.cancel.is_cancelled();
        let capped = walker.capped() && !cancelled;
        if let Some(n) = i.next.take() {
            // A cancelled refresh keeps the result of the last walk.
            if !cancelled {
                i.items = n;
            }
        }
        if capped {
            i.walker = Some(walker);
        }
        i.status.count = i.items.len() as u32;
        i.status.done = !capped && !cancelled;
        i.status.capped = capped;
        i.status.cancelled = cancelled;
        i.status.refreshing = false;
        i.last_emit = Some(Instant::now());
        i.dirty = false;
        let first = take_first(&mut i);
        (i.status.clone(), first)
    };
    s.emit(&status, first);
}

/// The settings of a new walk.
#[derive(Clone)]
pub struct WalkConfig {
    pub cap: usize,
    pub ignore: IgnoreSet,
}

/// The flat sessions: at most 1 for each window label.
pub struct FlatStore {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    seq: AtomicU64,
    spawn: Spawner,
}

impl Default for FlatStore {
    /// The walks run on the scan pool.
    fn default() -> Self {
        Self::new(Arc::new(|f| crate::pools::scan().spawn(f)))
    }
}

impl FlatStore {
    pub fn new(spawn: Spawner) -> Self {
        Self { sessions: Mutex::new(HashMap::new()), seq: AtomicU64::new(1), spawn }
    }

    fn by_id(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.lock().values().find(|s| s.id == id).cloned()
    }

    fn spawn_walk(&self, s: Arc<Session>, gen: u64, walker: TreeWalker) {
        (self.spawn)(Box::new(move || run_walk(s, gen, walker)));
    }

    /// Starts a flatten of `root` for `window` and cancels the previous session of the window.
    /// When the previous session has the same root and its walk ended at its end or at the cap,
    /// this is a refresh (FLAT-14): the session keeps its id and its listing until the new walk ends.
    /// A refresh walks with the ignore set of `cfg`, so that a change of the list applies (IGN-06).
    pub fn start(&self, window: &str, root: &Path, cfg: WalkConfig, events: Arc<dyn FlatEvents>) -> FlatStatus {
        let mut sessions = self.sessions.lock();
        if let Some(old) = sessions.get(window).cloned() {
            let mut i = old.inner.lock();
            let complete = (i.status.done || i.status.capped) && !i.status.cancelled;
            if old.root == root && complete {
                i.cancel.cancel();
                i.cancel = CancelToken::default();
                i.gen += 1;
                i.next = Some(Vec::new());
                i.walker = None;
                i.dirty = false;
                i.last_emit = None;
                i.status = FlatStatus { id: old.id.clone(), root: i.status.root.clone(), refreshing: true, ..Default::default() };
                let (gen, limit, cancel, status) = (i.gen, i.limit, i.cancel.clone(), i.status.clone());
                drop(i);
                drop(sessions);
                let walker = old.walker(limit, cancel, gen, cfg.ignore);
                self.spawn_walk(old, gen, walker);
                return status;
            }
            i.cancel.cancel();
            i.gen += 1;
            // The walker of a capped walk holds the session through its tick: drop the cycle.
            i.walker = None;
        }
        let id = format!("{window}-{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let status = FlatStatus { id: id.clone(), root: root.to_string_lossy().into_owned(), ..Default::default() };
        let cancel = CancelToken::default();
        let limit = cfg.cap.max(1);
        let s = Arc::new(Session {
            id,
            window: window.to_string(),
            root: root.to_path_buf(),
            events,
            inner: Mutex::new(Inner {
                items: Vec::new(),
                next: None,
                status: status.clone(),
                walker: None,
                cancel: cancel.clone(),
                gen: 0,
                limit,
                last_emit: None,
                dirty: false,
                recorded: false,
            }),
        });
        sessions.insert(window.to_string(), Arc::clone(&s));
        drop(sessions);
        let walker = s.walker(limit, cancel, 0, cfg.ignore);
        self.spawn_walk(s, 0, walker);
        status
    }

    /// The status of a session.
    pub fn status(&self, id: &str) -> Option<FlatStatus> {
        self.by_id(id).map(|s| s.inner.lock().status.clone())
    }

    /// The root and the photos of a session, for `list_flat`.
    pub fn snapshot(&self, id: &str) -> Option<(PathBuf, Vec<WalkItem>)> {
        let s = self.by_id(id)?;
        let items = s.inner.lock().items.clone();
        Some((s.root.clone(), items))
    }

    /// The photos of the flat location of `window`, for the analysis task; None if the window has
    /// no session for `root`.
    pub fn items_of(&self, window: &str, root: &Path) -> Option<Vec<WalkItem>> {
        let s = self.sessions.lock().get(window).cloned()?;
        (s.root == root).then(|| s.inner.lock().items.clone())
    }

    /// "Show More" (FLAT-09): continues a capped walk for `more` photos, without a new walk of the
    /// folders that it has read.
    pub fn continue_walk(&self, id: &str, more: usize) -> Option<FlatStatus> {
        let s = self.by_id(id)?;
        let mut i = s.inner.lock();
        let Some(mut walker) = i.walker.take() else { return Some(i.status.clone()) };
        walker.raise_cap(more.max(1));
        i.limit = i.limit.saturating_add(more.max(1));
        i.status.capped = false;
        let (gen, status) = (i.gen, i.status.clone());
        drop(i);
        self.spawn_walk(s, gen, walker);
        Some(status)
    }

    /// Stops the walk of a session (FLAT-06, FLAT-07). The photos found so far stay listed.
    pub fn cancel(&self, id: &str) -> Option<FlatStatus> {
        let s = self.by_id(id)?;
        let mut i = s.inner.lock();
        i.cancel.cancel();
        if !i.status.done {
            i.status.cancelled = true;
            i.status.capped = false;
            i.walker = None;
        }
        Some(i.status.clone())
    }

    /// Stops and forgets the session of a closed window (FLAT-07).
    pub fn remove_window(&self, window: &str) {
        if let Some(s) = self.sessions.lock().remove(window) {
            let mut i = s.inner.lock();
            i.cancel.cancel();
            i.gen += 1;
            i.walker = None;
        }
    }

    fn each(&self, mut f: impl FnMut(&Session, &mut Inner)) {
        let sessions: Vec<Arc<Session>> = self.sessions.lock().values().cloned().collect();
        for s in sessions {
            let mut i = s.inner.lock();
            f(&s, &mut i);
        }
    }

    /// Files and folders moved to the Trash leave the listings (FILE-01 in a flat location).
    pub fn remove_paths<S: AsRef<str>>(&self, paths: &[S]) {
        let gone: Vec<&Path> = paths.iter().map(|p| Path::new(p.as_ref())).collect();
        self.each(|_, i| {
            let keep = |it: &WalkItem| !gone.iter().any(|g| it.path.starts_with(g));
            i.items.retain(keep);
            if let Some(n) = i.next.as_mut() {
                n.retain(keep);
            }
        });
    }

    /// A renamed file keeps its place in the listings (FILE-14 in a flat location).
    pub fn rename_path(&self, from: &str, to: &str) {
        let (from, to) = (Path::new(from), PathBuf::from(to));
        self.each(|_, i| {
            for it in i.items.iter_mut().chain(i.next.iter_mut().flatten()) {
                if it.path == from {
                    it.path = to.clone();
                }
            }
        });
    }

    /// After an undo: restored files come back, and files that the undo renamed back leave.
    pub fn restore_paths<S: AsRef<str>>(&self, paths: &[S]) {
        let back: Vec<&Path> = paths.iter().map(|p| Path::new(p.as_ref())).collect();
        self.each(|s, i| {
            let mine: Vec<&Path> = back.iter().copied().filter(|p| p.starts_with(&s.root)).collect();
            if mine.is_empty() {
                return;
            }
            let dirs: Vec<&Path> = mine.iter().filter_map(|p| p.parent()).collect();
            i.items.retain(|it| !it.path.parent().is_some_and(|d| dirs.contains(&d)) || it.path.exists());
            for p in mine {
                if !crate::scan::is_image_path(p) || i.items.iter().any(|it| it.path == p) {
                    continue;
                }
                if let Ok(md) = std::fs::metadata(p) {
                    i.items.push(WalkItem {
                        path: p.to_path_buf(),
                        size: md.len(),
                        mtime: crate::scan::mtime_ms(&md),
                        created: crate::scan::created_ms(&md),
                    });
                }
            }
            if !i.status.refreshing {
                i.status.count = i.items.len() as u32;
            }
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

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<FlatStatus>>,
        firsts: Mutex<Vec<PathBuf>>,
    }

    impl FlatEvents for Recorder {
        fn progress(&self, window: &str, status: &FlatStatus) {
            assert_eq!(window, "main");
            self.events.lock().push(status.clone());
        }
        fn first_photo(&self, root: &Path) {
            self.firsts.lock().push(root.to_path_buf());
        }
    }

    type Jobs = Arc<Mutex<Vec<Box<dyn FnOnce() + Send>>>>;

    /// A store whose walks run only when the test calls `run`.
    fn deferred() -> (FlatStore, Jobs) {
        let q: Jobs = Arc::default();
        let q2 = Arc::clone(&q);
        (FlatStore::new(Arc::new(move |f| q2.lock().push(f))), q)
    }

    fn run(q: &Mutex<Vec<Box<dyn FnOnce() + Send>>>) {
        let jobs: Vec<_> = q.lock().drain(..).collect();
        for j in jobs {
            j();
        }
    }

    fn cfg(cap: usize) -> WalkConfig {
        WalkConfig { cap, ignore: IgnoreSet::default() }
    }

    fn tree(n: usize) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for i in 0..n {
            touch(&t.path().join(format!("d{}/p{i}.jpg", i / 4)));
        }
        t
    }

    #[test]
    fn flatten_stops_at_cap_and_continues() {
        let t = tree(10);
        let (store, q) = deferred();
        let ev = Arc::new(Recorder::default());
        let st = store.start("main", t.path(), cfg(4), ev.clone());
        assert!(st.walking());
        run(&q);
        let s = store.status(&st.id).unwrap();
        assert_eq!((s.count, s.capped, s.done), (4, true, false));
        assert_eq!(ev.events.lock().last(), Some(&s));
        assert_eq!(*ev.firsts.lock(), vec![t.path().to_path_buf()]);

        let s = store.continue_walk(&st.id, 4).unwrap();
        assert!(!s.capped);
        run(&q);
        let s = store.status(&st.id).unwrap();
        assert_eq!((s.count, s.capped, s.done), (8, true, false));

        store.continue_walk(&st.id, 4);
        run(&q);
        let s = store.status(&st.id).unwrap();
        assert_eq!((s.count, s.capped, s.done), (10, false, true));
        let (_, items) = store.snapshot(&st.id).unwrap();
        let unique: std::collections::BTreeSet<_> = items.iter().map(|i| i.path.clone()).collect();
        assert_eq!(unique.len(), 10, "the walk did not read a folder twice");
        assert_eq!(ev.firsts.lock().len(), 1, "Recents once per session");
    }

    #[test]
    fn flatten_cancel_stops_walk() {
        let t = tree(12);
        let (store, q) = deferred();
        let ev = Arc::new(Recorder::default());
        let st = store.start("main", t.path(), cfg(100), ev.clone());
        let s = store.cancel(&st.id).unwrap();
        assert!(s.cancelled && !s.done);
        run(&q);
        let s = store.status(&st.id).unwrap();
        assert_eq!((s.count, s.done, s.cancelled), (0, false, true));

        // A new start of the window cancels the walk of its previous session.
        let a = store.start("main", t.path(), cfg(100), ev.clone());
        let b = store.start("main", t.path(), cfg(100), ev.clone());
        assert_ne!(a.id, b.id);
        assert!(store.status(&a.id).is_none(), "1 session per window");
        run(&q);
        assert_eq!(store.status(&b.id).unwrap().count, 12);

        store.remove_window("main");
        assert!(store.status(&b.id).is_none());
    }

    #[test]
    fn refresh_keeps_the_listing_until_the_new_walk_ends() {
        let t = tree(3);
        let (store, q) = deferred();
        let ev = Arc::new(Recorder::default());
        let st = store.start("main", t.path(), cfg(100), ev.clone());
        run(&q);
        touch(&t.path().join("new/x.jpg"));
        let r = store.start("main", t.path(), cfg(100), ev.clone());
        assert_eq!(r.id, st.id, "a refresh keeps the session");
        assert!(r.refreshing);
        assert_eq!(store.snapshot(&st.id).unwrap().1.len(), 3);
        run(&q);
        let s = store.status(&st.id).unwrap();
        assert_eq!((s.count, s.done, s.refreshing), (4, true, false));
        assert_eq!(store.snapshot(&st.id).unwrap().1.len(), 4);
    }

    #[test]
    fn file_changes_update_the_listing() {
        let t = tree(2);
        let (store, q) = deferred();
        let st = store.start("main", t.path(), cfg(100), Arc::new(Recorder::default()));
        run(&q);
        let (_, items) = store.snapshot(&st.id).unwrap();
        let a = items[0].path.to_string_lossy().into_owned();
        let b = items[1].path.to_string_lossy().into_owned();
        store.remove_paths(std::slice::from_ref(&a));
        let renamed = t.path().join("d0/renamed.jpg").to_string_lossy().into_owned();
        fs::rename(&b, &renamed).unwrap();
        store.rename_path(&b, &renamed);
        let paths = |s: &FlatStore| s.snapshot(&st.id).unwrap().1.into_iter().map(|i| i.path).collect::<Vec<_>>();
        assert_eq!(paths(&store), vec![PathBuf::from(&renamed)]);
        // Undo of the rename: the old name comes back, the new name leaves.
        fs::rename(&renamed, &b).unwrap();
        store.restore_paths(std::slice::from_ref(&b));
        assert_eq!(paths(&store), vec![PathBuf::from(&b)]);
    }
}
