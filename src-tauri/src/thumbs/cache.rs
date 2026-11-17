//! ThumbCache: thumbnail files on disk, under `~/Library/Caches/<bundle id>/thumbs` (THM-07).
//!
//! Layout: `thumbs/<fnv64(path)>/<size>_<mtime ns>_<file size>`. One folder per source file makes
//! invalidation of one file a single folder removal (FSW-03). The file modification time of a cache
//! entry is its last use time, for the LRU removal (THM-09). The folder name is the `path_hash` of the
//! source file, which the `analysis` table also stores, so that eviction can remove the analysis data
//! of that source file without a read of the source path.
//!
//! Eviction: the use can go 10 % over the limit (the surplus). When the surplus is full, the oldest
//! 10 % of the cache are removed at once, so that writes do not wait for eviction on each new entry.
//!
//! Threads: `store` queues the write to 1 writer thread, so that the decode threads do not wait for
//! the disk (the memory cache of `ThumbnailService` serves the entry until the write lands). The
//! disk use accounting and eviction run on own threads at utility QoS. Measured (benches/core.rs):
//! background QoS made the eviction 2.5 times longer and slowed down the cache reads during it more
//! than utility QoS or the default QoS did.

use std::fs;
use std::path::{Path, PathBuf};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

use super::ThumbKey;
use crate::decode::Encoded;
use crate::pools::{set_thread_qos, Qos};

const TOUCH_AFTER: Duration = Duration::from_secs(3600);
/// Queued writes. A full queue makes `store` wait (back pressure) instead of losing entries; with
/// thumbnails of about 20 KB this holds at most about 20 MB.
const QUEUE: usize = 1024;
/// Called with the `path_hash` of each source file whose last thumbnail was evicted.
pub type EvictHook = Box<dyn Fn(Vec<u64>) + Send + Sync>;

#[derive(Clone)]
pub struct ThumbCache {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    used: AtomicU64,
    limit: AtomicU64,
    evicting: AtomicBool,
    on_evict: parking_lot::RwLock<Option<EvictHook>>,
    /// Incremented by `clear`: the writer drops writes queued before the clear.
    generation: AtomicU64,
    queue: SyncSender<Job>,
}

enum Job {
    Write(ThumbKey, Arc<Encoded>, u64),
    Flush(mpsc::Sender<()>),
}

/// The hash of a source path: the name of its cache folder, and the `hash` column of `analysis`.
pub fn path_hash(path: &Path) -> u64 {
    fnv64(path.as_os_str().as_encoded_bytes())
}

fn fnv64(s: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in s {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl ThumbCache {
    pub fn new(root: PathBuf, limit: u64) -> Self {
        let _ = fs::create_dir_all(&root);
        let (queue, rx) = mpsc::sync_channel(QUEUE);
        let inner = Arc::new(Inner {
            root,
            used: AtomicU64::new(0),
            limit: AtomicU64::new(limit),
            evicting: AtomicBool::new(false),
            on_evict: parking_lot::RwLock::new(None),
            generation: AtomicU64::new(0),
            queue,
        });
        let weak = Arc::downgrade(&inner);
        let _ = std::thread::Builder::new().name("thumb-writer".into()).spawn(move || writer(weak, rx));
        Self { inner }
    }

    /// Computes the disk use in the background.
    pub fn start_accounting(&self) {
        let this = self.clone();
        std::thread::spawn(move || {
            set_thread_qos(Qos::Utility);
            let total: u64 = this.entries().iter().map(|e| e.1).sum();
            this.inner.used.store(total, Ordering::Relaxed);
            this.maybe_evict();
        });
    }

    /// Queues the write of a thumbnail to the writer thread.
    pub fn store(&self, key: ThumbKey, e: Arc<Encoded>) {
        let generation = self.inner.generation.load(Ordering::Acquire);
        let _ = self.inner.queue.send(Job::Write(key, e, generation));
    }

    /// Waits until the writes queued before this call are on disk (tests and benchmarks).
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.inner.queue.send(Job::Flush(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    /// Sets the hook that removes the index data of evicted source files.
    pub fn set_on_evict(&self, hook: EvictHook) {
        *self.inner.on_evict.write() = Some(hook);
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    fn dir_for(&self, path: &Path) -> PathBuf {
        self.inner.root.join(format!("{:016x}", path_hash(path)))
    }

    pub fn file_for(&self, key: &ThumbKey) -> PathBuf {
        self.dir_for(&key.path).join(format!("{}_{}_{}", key.size, key.mtime_ns, key.file_size))
    }

    /// Reads an entry with 1 open: the size for the buffer and the last use time come from the same
    /// file handle. Records the use for LRU removal at most once per hour per entry (`futimens` on
    /// the read-only handle: allowed for the file owner).
    pub fn read(&self, key: &ThumbKey) -> Option<Vec<u8>> {
        let mut file = fs::File::open(self.file_for(key)).ok()?;
        let md = file.metadata().ok()?;
        let mut bytes = Vec::with_capacity(md.len() as usize);
        file.read_to_end(&mut bytes).ok()?;
        let age = md.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok());
        if age.is_some_and(|d| d > TOUCH_AFTER) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(bytes)
    }

    /// Writes an entry on the calling thread. `store` is the queued form.
    pub fn write(&self, key: &ThumbKey, bytes: &[u8]) {
        let dir = self.dir_for(&key.path);
        // One mkdir on the common path (folder exists); `create_dir_all` costs a second call to check it.
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                if fs::create_dir_all(&dir).is_err() {
                    return;
                }
            }
        }
        let f = self.file_for(key);
        let tmp = f.with_extension("tmp");
        if fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, &f).is_ok() {
            self.inner.used.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            self.maybe_evict();
        }
    }

    pub fn invalidate(&self, path: &Path) {
        let dir = self.dir_for(path);
        let freed: u64 = fs::read_dir(&dir)
            .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
            .unwrap_or(0);
        if fs::remove_dir_all(&dir).is_ok() {
            sub(&self.inner.used, freed);
        }
    }

    pub fn clear(&self) {
        let root = &self.inner.root;
        // Move aside first, so that the clear is instant for the UI.
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        let trash = root.with_extension(format!("old-{}", std::process::id()));
        if fs::rename(root, &trash).is_ok() {
            std::thread::spawn(move || {
                let _ = fs::remove_dir_all(trash);
            });
        } else {
            let _ = fs::remove_dir_all(root);
        }
        let _ = fs::create_dir_all(root);
        self.inner.used.store(0, Ordering::Relaxed);
    }

    pub fn used(&self) -> u64 {
        self.inner.used.load(Ordering::Relaxed)
    }

    pub fn limit(&self) -> u64 {
        self.inner.limit.load(Ordering::Relaxed)
    }

    /// The use at which eviction starts: the limit plus the 10 % surplus.
    pub fn hard_limit(&self) -> u64 {
        self.limit().saturating_add(self.limit() / 10)
    }

    pub fn set_limit(&self, limit: u64) {
        self.inner.limit.store(limit, Ordering::Relaxed);
        self.maybe_evict();
    }

    /// (path, size, last use)
    fn entries(&self) -> Vec<(PathBuf, u64, SystemTime)> {
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(&self.inner.root) else { return out };
        for d in rd.flatten() {
            let Ok(rd2) = fs::read_dir(d.path()) else { continue };
            for f in rd2.flatten() {
                if let Ok(md) = f.metadata() {
                    out.push((f.path(), md.len(), md.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
                }
            }
        }
        out
    }

    fn maybe_evict(&self) {
        if self.used() <= self.hard_limit() || self.inner.evicting.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = self.clone();
        std::thread::spawn(move || {
            set_thread_qos(Qos::Utility);
            this.evict();
            this.inner.evicting.store(false, Ordering::Release);
        });
    }

    /// THM-09: removes the least recently used entries: the oldest 10 % of the use, and more when
    /// the use is still over the limit (after the user made the limit smaller). Removes the index
    /// data of each source file that has no thumbnail left.
    pub fn evict(&self) {
        let mut entries = self.entries();
        let mut total: u64 = entries.iter().map(|e| e.1).sum();
        let target = (total / 10 * 9).min(self.limit());
        entries.sort_unstable_by_key(|e| e.2);
        let mut gone = Vec::new();
        for (p, len, _) in entries {
            if total <= target {
                break;
            }
            if fs::remove_file(&p).is_ok() {
                total -= len;
                // `remove_dir` only succeeds when the folder is empty: the last thumbnail is gone.
                if let Some(parent) = p.parent() {
                    if fs::remove_dir(parent).is_ok() {
                        if let Some(h) = parent.file_name().and_then(|n| n.to_str()).and_then(|n| u64::from_str_radix(n, 16).ok()) {
                            gone.push(h);
                        }
                    }
                }
            }
        }
        self.inner.used.store(total, Ordering::Relaxed);
        if !gone.is_empty() {
            if let Some(hook) = self.inner.on_evict.read().as_ref() {
                hook(gone);
            }
        }
    }
}

/// The writer thread: ends when the last `ThumbCache` is dropped (the queue sender is in `Inner`).
fn writer(weak: Weak<Inner>, rx: Receiver<Job>) {
    set_thread_qos(Qos::Utility);
    for job in rx {
        match job {
            Job::Write(key, e, generation) => {
                let Some(inner) = weak.upgrade() else { return };
                if inner.generation.load(Ordering::Acquire) == generation {
                    ThumbCache { inner }.write(&key, &e.bytes);
                }
            }
            Job::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn sub(a: &AtomicU64, v: u64) {
    let mut cur = a.load(Ordering::Relaxed);
    while let Err(actual) = a.compare_exchange_weak(cur, cur.saturating_sub(v), Ordering::Relaxed, Ordering::Relaxed) {
        cur = actual;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_eviction_keeps_recent() {
        let t = tempfile::tempdir().unwrap();
        let c = ThumbCache::new(t.path().join("thumbs"), u64::MAX);
        let mk = |i: u32| ThumbKey { path: PathBuf::from(format!("/x/{i}.jpg")), file_size: 1, mtime_ns: 1, size: 256 };
        for i in 0..5 {
            c.write(&mk(i), &[0u8; 300]);
            let f = c.file_for(&mk(i));
            let file = fs::File::options().append(true).open(&f).unwrap();
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1000 + i as u64)).unwrap();
        }
        let evicted = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let e2 = Arc::clone(&evicted);
        c.set_on_evict(Box::new(move |p| e2.lock().extend(p)));
        c.inner.limit.store(1000, Ordering::Relaxed);
        c.evict();
        assert!(c.used() <= 1000);
        assert_eq!(evicted.lock()[..], [path_hash(Path::new("/x/0.jpg")), path_hash(Path::new("/x/1.jpg"))]);
        assert!(c.read(&mk(4)).is_some());
        assert!(c.read(&mk(0)).is_none());
        c.invalidate(Path::new("/x/4.jpg"));
        assert!(c.read(&mk(4)).is_none());
    }

    #[test]
    fn surplus_then_oldest_tenth_removed() {
        let t = tempfile::tempdir().unwrap();
        let c = ThumbCache::new(t.path().join("thumbs"), 10_000);
        let mk = |i: u32| ThumbKey { path: PathBuf::from(format!("/y/{i}.jpg")), file_size: 1, mtime_ns: 1, size: 256 };
        // 10 500 bytes: over the limit, in the surplus: no eviction.
        for i in 0..105 {
            c.write(&mk(i), &[0u8; 100]);
            let f = c.file_for(&mk(i));
            let file = fs::File::options().append(true).open(&f).unwrap();
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1000 + i as u64)).unwrap();
        }
        assert_eq!(c.used(), 10_500);
        c.evict();
        // The oldest 10 % go.
        assert!(c.used() <= 9_450 && c.used() > 9_000, "{}", c.used());
        assert!(c.read(&mk(0)).is_none());
        assert!(c.read(&mk(104)).is_some());
    }

    fn enc(n: usize) -> Arc<Encoded> {
        Arc::new(Encoded { bytes: vec![1u8; n], mime: "image/jpeg" })
    }

    #[test]
    fn store_writes_on_the_writer_thread() {
        let t = tempfile::tempdir().unwrap();
        let c = ThumbCache::new(t.path().join("thumbs"), u64::MAX);
        let k = ThumbKey { path: PathBuf::from("/z/1.jpg"), file_size: 1, mtime_ns: 1, size: 256 };
        c.store(k.clone(), enc(100));
        c.flush();
        assert_eq!(c.read(&k).unwrap().len(), 100);
        assert_eq!(c.used(), 100);
    }

    #[test]
    fn clear_drops_queued_writes() {
        let t = tempfile::tempdir().unwrap();
        let c = ThumbCache::new(t.path().join("thumbs"), u64::MAX);
        let k = ThumbKey { path: PathBuf::from("/z/1.jpg"), file_size: 1, mtime_ns: 1, size: 256 };
        // A write queued before the clear: its generation is old.
        let _ = c.inner.queue.send(Job::Write(k.clone(), enc(100), 0));
        c.inner.generation.fetch_add(1, Ordering::AcqRel);
        c.flush();
        assert!(c.read(&k).is_none());
    }

    #[test]
    fn read_records_use_after_an_hour() {
        let t = tempfile::tempdir().unwrap();
        let c = ThumbCache::new(t.path().join("thumbs"), u64::MAX);
        let k = ThumbKey { path: PathBuf::from("/z/2.jpg"), file_size: 1, mtime_ns: 1, size: 256 };
        c.write(&k, &[0u8; 10]);
        let f = c.file_for(&k);
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        fs::File::options().append(true).open(&f).unwrap().set_modified(old).unwrap();
        assert_eq!(c.read(&k).unwrap().len(), 10);
        let m = fs::metadata(&f).unwrap().modified().unwrap();
        assert!(m > old + Duration::from_secs(3600), "use time not recorded");
    }
}
