//! DirScanner: directory listing, image detection, and sort order.

pub mod arrange;
pub mod index;
pub mod preview;

use std::cmp::Ordering;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

/// Supported image extensions (lower case). FMT-01..FMT-04.
pub const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "webp", "gif", "tif", "tiff", "bmp", "avif", "cr2", "cr3", "nef",
    "arw", "dng", "raf", "orf", "svg",
];
const RAW_EXTS: &[&str] = &["cr2", "cr3", "nef", "arw", "dng", "raf", "orf"];

/// macOS package folder extensions that the scan treats as files (FPV-10).
const PACKAGE_EXTS: &[&str] = &[
    "app", "photoslibrary", "photolibrary", "migratedphotolibrary", "aplibrary", "bundle", "framework", "plugin",
    "kext", "xcodeproj", "xcworkspace", "playground", "rtfd", "pkg", "mpkg", "musiclibrary", "tvlibrary",
    "imovielibrary", "fcpbundle", "logicx", "band", "pages", "numbers", "key", "lrdata", "lrlibrary", "cocatalog",
    "photosdb", "sparsebundle", "xcarchive", "wdgt", "saver", "prefpane", "qlgenerator", "mdimporter",
];

pub fn ext_lower(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

fn ext_of_name(name: &str) -> Option<&str> {
    let i = name.rfind('.')?;
    if i == 0 {
        return None;
    }
    Some(&name[i + 1..])
}

/// FMT-04: image files are identified by extension, case-insensitive.
pub fn is_image_name(name: &str) -> bool {
    match ext_of_name(name) {
        Some(e) if e.len() <= 5 => IMAGE_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)),
        _ => false,
    }
}

pub fn is_image_path(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(is_image_name)
}

pub fn is_raw(path: &Path) -> bool {
    ext_lower(path).is_some_and(|e| RAW_EXTS.contains(&e.as_str()))
}

pub fn is_package_name(name: &str) -> bool {
    match ext_of_name(name) {
        Some(e) => PACKAGE_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)),
        None => false,
    }
}

pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Taken,
    Size,
    /// By the quality score of the bad photo metrics; needs an analysis (`arrange`).
    Quality,
    /// Similar photos side by side; needs an analysis (`arrange`).
    Similarity,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub key: SortKey,
    #[serde(default)]
    pub desc: bool,
}

impl SortSpec {
    /// Stable string form, used in the PreviewIndex key (FPV-04).
    pub fn as_key(&self) -> String {
        let k = match self.key {
            SortKey::Name => "name",
            SortKey::Modified => "modified",
            SortKey::Taken => "taken",
            SortKey::Size => "size",
            SortKey::Quality => "quality",
            SortKey::Similarity => "similarity",
        };
        format!("{k}:{}", if self.desc { "desc" } else { "asc" })
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Entry {
    /// File name.
    pub n: String,
    /// true for a folder.
    pub d: bool,
    /// Size in bytes (0 for folders).
    pub s: u64,
    /// Modification time, ms since epoch.
    pub m: i64,
    /// Date taken, s (naive local), only for sort by date taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<i64>,
    /// Absolute path, only in a collection listing (the file can be in any folder).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<String>,
    /// Badge codes, only in a collection listing, for example `quality.blurry`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<Vec<String>>,
    /// Creation time, ms since epoch. Only for the grouping in the Rust core.
    #[serde(skip)]
    pub c: i64,
}

/// Natural order comparison: digit runs compare as numbers, letters compare case-insensitively (GRID-11).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < ab.len() && j < bb.len() {
        let (ca, cb) = (ab[i], bb[j]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let si = i;
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < bb.len() && bb[j].is_ascii_digit() {
                j += 1;
            }
            let na = trim_zeros(&ab[si..i]);
            let nb = trim_zeros(&bb[sj..j]);
            let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if o != Ordering::Equal {
                return o;
            }
        } else if ca < 0x80 && cb < 0x80 {
            let o = ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase());
            if o != Ordering::Equal {
                return o;
            }
            i += 1;
            j += 1;
        } else {
            // Non-ASCII: compare the remaining chars with Unicode lower case.
            let ra = a[i..].chars().next().unwrap();
            let rb = b[j..].chars().next().unwrap();
            let o = ra.to_lowercase().cmp(rb.to_lowercase());
            if o != Ordering::Equal {
                return o;
            }
            i += ra.len_utf8();
            j += rb.len_utf8();
        }
    }
    (ab.len() - i).cmp(&(bb.len() - j)).then_with(|| a.cmp(b))
}

fn trim_zeros(s: &[u8]) -> &[u8] {
    let k = s.iter().position(|&c| c != b'0').unwrap_or(s.len());
    &s[k..]
}

pub fn created_ms(md: &fs::Metadata) -> i64 {
    md.created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(|| mtime_ms(md))
}

pub fn mtime_ms(md: &fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Sorts entries: folders first (GRID-01), then images, each group in the sort order.
/// Folders sort by name, or by date for the date sorts.
pub fn sort_entries(entries: &mut [Entry], sort: SortSpec) {
    entries.sort_unstable_by(|a, b| {
        b.d.cmp(&a.d).then_with(|| {
            let o = match (sort.key, a.d) {
                // Quality and Similarity: `arrange` orders the images after this sort by name.
                (SortKey::Name | SortKey::Quality | SortKey::Similarity, _) | (SortKey::Size, true) => {
                    natural_cmp(&a.n, &b.n)
                }
                (SortKey::Modified, _) | (SortKey::Taken, true) => a.m.cmp(&b.m),
                (SortKey::Taken, false) => a.t.unwrap_or(a.m / 1000).cmp(&b.t.unwrap_or(b.m / 1000)),
                (SortKey::Size, false) => a.s.cmp(&b.s),
            };
            let o = if sort.desc { o.reverse() } else { o };
            // Collection entries can share a name; the path makes the order stable.
            o.then_with(|| natural_cmp(&a.n, &b.n)).then_with(|| a.p.cmp(&b.p))
        })
    });
}

pub struct ListOptions {
    pub show_hidden: bool,
    /// Stop after this many raw directory entries (FPV-08). usize::MAX for no limit.
    pub max_entries: usize,
    /// Follow symbolic links (grid: yes; recursive preview search: no, FPV-09).
    pub follow_links: bool,
}

/// Lists the folders and images of one directory, unsorted. Non-image files are skipped
/// without a stat call (d_type), which keeps a 10,000-entry listing fast (PERF-05).
/// Returns the entries and the number of raw entries read.
pub fn read_entries(dir: &Path, opts: &ListOptions) -> std::io::Result<(Vec<Entry>, usize)> {
    let rd = fs::read_dir(dir)?;
    let mut out = Vec::new();
    let mut read = 0usize;
    for de in rd {
        if read >= opts.max_entries {
            break;
        }
        read += 1;
        let Ok(de) = de else { continue };
        let name = de.file_name();
        let Some(name) = name.to_str() else { continue };
        if !opts.show_hidden && is_hidden_name(name) {
            continue;
        }
        let Ok(ft) = de.file_type() else { continue };
        let is_link = ft.is_symlink();
        if is_link && !opts.follow_links {
            continue;
        }
        let image = is_image_name(name);
        let maybe_dir = ft.is_dir() || is_link;
        if !image && !maybe_dir {
            continue;
        }
        let md = if is_link { fs::metadata(de.path()) } else { de.metadata() };
        let Ok(md) = md else { continue };
        if md.is_dir() {
            if is_package_name(name) {
                continue;
            }
            out.push(Entry { n: name.to_string(), d: true, s: 0, m: mtime_ms(&md), ..Default::default() });
        } else if image && md.is_file() {
            out.push(Entry { n: name.to_string(), d: false, s: md.len(), m: mtime_ms(&md), c: created_ms(&md), ..Default::default() });
        }
    }
    Ok((out, read))
}

/// Fills the date taken of image entries, in parallel, through the PreviewIndex cache.
pub fn fill_taken(dir: &Path, entries: &mut [Entry], index: &index::PreviewIndex) {
    use rayon::prelude::*;
    let cached = index.taken_many(dir);
    let mut missing = Vec::new();
    for (i, e) in entries.iter_mut().enumerate().filter(|(_, e)| !e.d) {
        match cached.get(&e.n) {
            Some(&(m, t)) if m == e.m => e.t = t,
            _ => missing.push(i),
        }
    }
    if missing.is_empty() {
        return;
    }
    let found: Vec<(usize, Option<i64>)> = crate::pools::scan().install(|| {
        missing.par_iter().map(|&i| (i, crate::decode::date_taken(&dir.join(&entries[i].n)))).collect()
    });
    let mut rows = Vec::with_capacity(found.len());
    for (i, t) in found {
        entries[i].t = t;
        rows.push((entries[i].n.clone(), entries[i].m, t));
    }
    index.put_taken(dir, &rows);
}

/// Fills the date taken of entries that carry their own path (`Entry.p`), from any folders.
pub fn fill_taken_by_path(entries: &mut Vec<Entry>, index: &index::PreviewIndex) {
    use std::collections::HashMap;
    use std::path::PathBuf;
    // The date taken cache is per folder.
    let mut by_dir: HashMap<PathBuf, Vec<Entry>> = HashMap::new();
    for e in entries.drain(..) {
        let dir = Path::new(e.p.as_deref().unwrap_or("/")).parent().unwrap_or(Path::new("/")).to_path_buf();
        by_dir.entry(dir).or_default().push(e);
    }
    for (dir, mut group) in by_dir {
        fill_taken(&dir, &mut group, index);
        entries.extend(group);
    }
}

/// list_dir: entries of one directory in the grid order.
pub fn list_dir(
    dir: &Path,
    sort: SortSpec,
    show_hidden: bool,
    index: &index::PreviewIndex,
) -> std::io::Result<Vec<Entry>> {
    let (mut entries, _) =
        read_entries(dir, &ListOptions { show_hidden, max_entries: usize::MAX, follow_links: true })?;
    if sort.key == SortKey::Taken {
        fill_taken(dir, &mut entries, index);
    }
    sort_entries(&mut entries, sort);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["image10.jpg", "image2.jpg", "Image1.jpg", "image02b.jpg", "a.jpg"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a.jpg", "Image1.jpg", "image2.jpg", "image02b.jpg", "image10.jpg"]);
    }

    #[test]
    fn detects_images_case_insensitive() {
        assert!(is_image_name("IMG_1.JPG"));
        assert!(is_image_name("x.HeIc"));
        assert!(!is_image_name("x.txt"));
        assert!(!is_image_name(".jpg"));
        assert!(is_package_name("Photos Library.photoslibrary"));
    }

    #[test]
    fn folders_first() {
        let mut v = vec![
            Entry { n: "b.jpg".into(), d: false, s: 1, ..Default::default() },
            Entry { n: "z".into(), d: true, ..Default::default() },
            Entry { n: "a.jpg".into(), d: false, s: 2, ..Default::default() },
        ];
        sort_entries(&mut v, SortSpec { key: SortKey::Name, desc: false });
        let names: Vec<_> = v.iter().map(|e| e.n.as_str()).collect();
        assert_eq!(names, ["z", "a.jpg", "b.jpg"]);
        sort_entries(&mut v, SortSpec { key: SortKey::Size, desc: true });
        let names: Vec<_> = v.iter().map(|e| e.n.as_str()).collect();
        assert_eq!(names, ["z", "a.jpg", "b.jpg"]);
    }
}
