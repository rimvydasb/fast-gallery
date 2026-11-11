//! ThumbnailService and ThumbCache.

mod cache;

pub use cache::ThumbCache;

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use lru::LruCache;
use parking_lot::Mutex;

use crate::decode::{self, Encoded};

/// THM-01: the 2 thumbnail sizes.
pub const SIZES: [u32; 2] = [256, 512];
/// THM-11: size of the memory cache.
pub const MEM_ENTRIES: usize = 500;

pub fn normalize_size(s: u32) -> u32 {
    if s <= SIZES[0] {
        SIZES[0]
    } else {
        SIZES[1]
    }
}

/// THM-08: the cache key contains the path, file size, modification time, and thumbnail size.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ThumbKey {
    pub path: PathBuf,
    pub file_size: u64,
    pub mtime_ns: i128,
    pub size: u32,
}

impl ThumbKey {
    pub fn for_file(path: &Path, size: u32) -> std::io::Result<Self> {
        let md = std::fs::metadata(path)?;
        if !md.is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "not a file"));
        }
        let mtime_ns = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i128)
            .unwrap_or(0);
        Ok(Self { path: path.to_path_buf(), file_size: md.len(), mtime_ns, size: normalize_size(size) })
    }
}

pub struct ThumbnailService {
    mem: Mutex<LruCache<ThumbKey, Arc<Encoded>>>,
    pub cache: ThumbCache,
}

pub enum Lookup {
    Hit(Arc<Encoded>),
    Miss(ThumbKey),
}

impl ThumbnailService {
    pub fn new(cache: ThumbCache) -> Self {
        Self { mem: Mutex::new(LruCache::new(NonZeroUsize::new(MEM_ENTRIES).unwrap())), cache }
    }

    /// Memory and disk cache lookup. Cheap; runs on the I/O pool.
    pub fn lookup(&self, path: &Path, size: u32) -> std::io::Result<Lookup> {
        let key = ThumbKey::for_file(path, size)?;
        if let Some(e) = self.mem.lock().get(&key) {
            return Ok(Lookup::Hit(Arc::clone(e)));
        }
        if let Some(bytes) = self.cache.read(&key) {
            let mime = if bytes.starts_with(&[0x89, b'P']) { "image/png" } else { "image/jpeg" };
            let e = Arc::new(Encoded { bytes, mime });
            self.mem.lock().put(key, Arc::clone(&e));
            return Ok(Lookup::Hit(e));
        }
        Ok(Lookup::Miss(key))
    }

    /// Decode, downscale, encode, and store. Runs on the decode pool.
    pub fn generate(&self, key: ThumbKey) -> Option<Arc<Encoded>> {
        let e = Arc::new(decode::make_thumbnail(&key.path, key.size)?);
        self.cache.write(&key, &e.bytes);
        self.mem.lock().put(key, Arc::clone(&e));
        Some(e)
    }

    /// Blocking get, for benchmarks and tests.
    pub fn get_blocking(&self, path: &Path, size: u32) -> Option<Arc<Encoded>> {
        match self.lookup(path, size).ok()? {
            Lookup::Hit(e) => Some(e),
            Lookup::Miss(k) => self.generate(k),
        }
    }

    /// FSW-03: a change to a file invalidates its thumbnails.
    pub fn invalidate(&self, path: &Path) {
        let mut mem = self.mem.lock();
        let stale: Vec<ThumbKey> = mem.iter().filter(|(k, _)| k.path == path).map(|(k, _)| k.clone()).collect();
        for k in stale {
            mem.pop(&k);
        }
        drop(mem);
        self.cache.invalidate(path);
    }

    pub fn clear(&self) {
        self.mem.lock().clear();
        self.cache.clear();
    }
}

/// Path of a cached thumbnail file, if one exists (used for the drag image).
pub fn cached_file(svc: &ThumbnailService, path: &Path) -> Option<PathBuf> {
    let key = ThumbKey::for_file(path, 256).ok()?;
    let f = svc.cache.file_for(&key);
    f.exists().then_some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(normalize_size(10), 256);
        assert_eq!(normalize_size(256), 256);
        assert_eq!(normalize_size(300), 512);
        assert_eq!(normalize_size(4000), 512);
    }
}
