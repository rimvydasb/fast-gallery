//! ThumbCache: thumbnail files on disk, under `~/Library/Caches/<bundle id>/thumbs` (THM-07).
//!
//! Layout: `thumbs/<fnv64(path)>/<size>_<mtime ns>_<file size>`. One folder per source file makes
//! invalidation of one file a single folder removal (FSW-03). The file modification time of a cache
//! entry is its last use time, for the LRU removal (THM-09).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::ThumbKey;

const TOUCH_AFTER: Duration = Duration::from_secs(3600);

#[derive(Clone)]
pub struct ThumbCache {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    used: AtomicU64,
    limit: AtomicU64,
    evicting: AtomicBool,
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
        Self {
            inner: Arc::new(Inner {
                root,
                used: AtomicU64::new(0),
                limit: AtomicU64::new(limit),
                evicting: AtomicBool::new(false),
            }),
        }
    }

    /// Computes the disk use in the background.
    pub fn start_accounting(&self) {
        let this = self.clone();
        crate::pools::io().spawn(move || {
            let total: u64 = this.entries().iter().map(|e| e.1).sum();
            this.inner.used.store(total, Ordering::Relaxed);
            this.maybe_evict();
        });
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    fn dir_for(&self, path: &Path) -> PathBuf {
        self.inner.root.join(format!("{:016x}", fnv64(path.as_os_str().as_encoded_bytes())))
    }

    pub fn file_for(&self, key: &ThumbKey) -> PathBuf {
        self.dir_for(&key.path).join(format!("{}_{}_{}", key.size, key.mtime_ns, key.file_size))
    }

    pub fn read(&self, key: &ThumbKey) -> Option<Vec<u8>> {
        let f = self.file_for(key);
        let bytes = fs::read(&f).ok()?;
        // Record the use for LRU removal, at most once per hour per entry.
        if let Ok(file) = fs::File::options().append(true).open(&f) {
            if let Ok(md) = file.metadata() {
                let old = md.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok());
                if old.is_some_and(|d| d > TOUCH_AFTER) {
                    let _ = file.set_modified(SystemTime::now());
                }
            }
        }
        Some(bytes)
    }

    pub fn write(&self, key: &ThumbKey, bytes: &[u8]) {
        let dir = self.dir_for(&key.path);
        if fs::create_dir_all(&dir).is_err() {
            return;
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
        if self.used() <= self.limit() || self.inner.evicting.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = self.clone();
        std::thread::spawn(move || {
            this.evict();
            this.inner.evicting.store(false, Ordering::Release);
        });
    }

    /// THM-09: removes the least recently used entries until the use is 90 % of the limit.
    pub fn evict(&self) {
        let mut entries = self.entries();
        let mut total: u64 = entries.iter().map(|e| e.1).sum();
        let target = self.limit() / 10 * 9;
        entries.sort_unstable_by_key(|e| e.2);
        for (p, len, _) in entries {
            if total <= target {
                break;
            }
            if fs::remove_file(&p).is_ok() {
                total -= len;
                if let Some(parent) = p.parent() {
                    let _ = fs::remove_dir(parent); // only succeeds when empty
                }
            }
        }
        self.inner.used.store(total, Ordering::Relaxed);
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
        c.inner.limit.store(1000, Ordering::Relaxed);
        c.evict();
        assert!(c.used() <= 900);
        assert!(c.read(&mk(4)).is_some());
        assert!(c.read(&mk(0)).is_none());
        c.invalidate(Path::new("/x/4.jpg"));
        assert!(c.read(&mk(4)).is_none());
    }
}
