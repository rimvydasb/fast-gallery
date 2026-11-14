//! Arrangement of a listing: the sorts that need an analysis (quality, similarity) and the groups
//! of the grid. A listing stays a flat list of entries; the groups are consecutive ranges of the
//! images, so the selection, the viewer, and the keyboard work on the same indexes with or without
//! groups. Analysis results come only from the `analysis` cache: images without a result are
//! counted in `missing`, and the frontend starts the analysis task.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::index::PreviewIndex;
use super::{ext_lower, fill_taken, fill_taken_by_path, natural_cmp, read_entries, sort_entries, Entry, ListOptions, SortKey, SortSpec};
use crate::pipeline::{AnalysisCache, Pair};
use crate::quality::{bad_photo_flags, QualityAnalyzer, QualityFlag, QualityMetrics, Thresholds};
use crate::screenshot::{ScreenshotAnalyzer, ScreenshotSignals};
use crate::similarity::{cluster_bursts, cluster_similar, similarity_order, BurstKind, Level, Signature, SimilarityAnalyzer};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum GroupKey {
    #[default]
    None,
    /// By day of the date taken (the date modified if the image has none).
    Taken,
    /// By day of the date modified.
    Modified,
    /// By day of the date created.
    Created,
    Size,
    /// By file type.
    Type,
    /// By the first flag of the bad photo rules.
    Quality,
    BurstDense,
    BurstSparse,
    Same,
    High,
    Medium,
    Low,
}

/// The analysis results that a sort and a grouping read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    pub quality: bool,
    pub similarity: bool,
    pub taken: bool,
}

impl Needs {
    pub fn of(sort: SortSpec, group: GroupKey) -> Self {
        use GroupKey::*;
        Self {
            quality: sort.key == SortKey::Quality || group == Quality,
            similarity: sort.key == SortKey::Similarity
                || matches!(group, BurstDense | BurstSparse | Same | High | Medium | Low),
            taken: sort.key == SortKey::Taken || matches!(group, Taken | BurstDense | BurstSparse),
        }
    }

    /// The `need` argument of `start_analyze_photos`.
    pub fn analysis(&self) -> Option<&'static str> {
        match (self.quality, self.similarity) {
            (true, true) => Some("both"),
            (true, false) => Some("quality"),
            (false, true) => Some("similarity"),
            (false, false) => None,
        }
    }
}

/// One group of the grid: `count` consecutive images of the listing.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Group {
    /// The header text; an ISO date ("2024-03-12") for `kind` = "date".
    pub label: String,
    pub count: usize,
    /// "date": the frontend formats the label. "rest": the images that are in no cluster.
    /// "missing": the images without an analysis result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
}

#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub groups: Vec<Group>,
    /// Images without a result of a needed analysis.
    pub missing: u32,
    /// The analysis that the arrangement reads: "quality", "similarity", or "both".
    pub need: Option<&'static str>,
}

struct Item {
    e: Entry,
    q: Option<(QualityMetrics, ScreenshotSignals)>,
    s: Option<Signature>,
}

/// The entries of 1 folder in the grid order, with groups.
pub fn list_disk(
    dir: &Path,
    sort: SortSpec,
    show_hidden: bool,
    group: GroupKey,
    index: &Arc<PreviewIndex>,
    th: &Thresholds,
) -> std::io::Result<Listing> {
    let (mut entries, _) = read_entries(dir, &ListOptions { show_hidden, max_entries: usize::MAX, follow_links: true })?;
    if Needs::of(sort, group).taken {
        fill_taken(dir, &mut entries, index);
    }
    sort_entries(&mut entries, sort);
    Ok(arrange(Some(dir), entries, sort, group, index, th))
}

/// The entries of a collection (each with `Entry.p`) in the grid order, with groups.
pub fn list_paths(mut entries: Vec<Entry>, sort: SortSpec, group: GroupKey, index: &Arc<PreviewIndex>, th: &Thresholds) -> Listing {
    if Needs::of(sort, group).taken {
        fill_taken_by_path(&mut entries, index);
    }
    sort_entries(&mut entries, sort);
    arrange(None, entries, sort, group, index, th)
}

/// Applies the analysis sorts and the grouping to entries that `sort_entries` has sorted.
pub fn arrange(
    dir: Option<&Path>,
    mut entries: Vec<Entry>,
    sort: SortSpec,
    group: GroupKey,
    index: &Arc<PreviewIndex>,
    th: &Thresholds,
) -> Listing {
    let needs = Needs::of(sort, group);
    if group == GroupKey::None && needs.analysis().is_none() {
        return Listing { entries, ..Default::default() };
    }
    let folders = entries.iter().take_while(|e| e.d).count();
    let images = entries.split_off(folders);
    let cache = AnalysisCache::new(Arc::clone(index));
    let mut missing = 0u32;
    let mut items: Vec<Item> = images
        .into_iter()
        .map(|e| {
            let p: PathBuf = match (&e.p, dir) {
                (Some(p), _) => PathBuf::from(p),
                (None, Some(d)) => d.join(&e.n),
                (None, None) => PathBuf::from(&e.n),
            };
            let q = if needs.quality { cache.get::<Pair<QualityAnalyzer, ScreenshotAnalyzer>>(&p, e.m, e.s) } else { None };
            let s = if needs.similarity { cache.get::<SimilarityAnalyzer>(&p, e.m, e.s) } else { None };
            if (needs.quality && q.is_none()) || (needs.similarity && s.is_none()) {
                missing += 1;
            }
            Item { e, q, s }
        })
        .collect();

    // The bad photo badges, when the arrangement is about quality.
    if needs.quality {
        for it in items.iter_mut().filter(|it| it.e.b.is_none()) {
            if let Some((m, s)) = &it.q {
                let flags = bad_photo_flags(th, Path::new(&it.e.n), m, s);
                if !flags.is_empty() {
                    it.e.b = Some(flags.into_iter().map(|f| f.badge().to_string()).collect());
                }
            }
        }
    }

    match sort.key {
        SortKey::Quality => sort_by_quality(&mut items, sort.desc),
        SortKey::Similarity => sort_by_similarity(&mut items, sort.desc),
        _ => {}
    }

    let groups = match group {
        GroupKey::None => Vec::new(),
        GroupKey::Taken | GroupKey::Modified | GroupKey::Created => {
            keyed(&mut items, sort.desc, |it| {
                let secs = match group {
                    GroupKey::Taken => it.e.t.unwrap_or_else(|| to_local(it.e.m / 1000)),
                    GroupKey::Created => to_local(it.e.c / 1000),
                    _ => to_local(it.e.m / 1000),
                };
                let day = secs.div_euclid(86_400);
                ((day, String::new()), iso_date(day), Some("date"))
            })
        }
        GroupKey::Size => keyed(&mut items, sort.desc, |it| {
            let (i, label) = size_bucket(it.e.s);
            ((i, String::new()), label.to_string(), None)
        }),
        GroupKey::Type => keyed(&mut items, false, |it| {
            let label = type_label(&it.e.n);
            ((0, label.clone()), label, None)
        }),
        GroupKey::Quality => keyed(&mut items, false, |it| match &it.q {
            None => ((99, String::new()), "Not analyzed".to_string(), Some("missing")),
            Some((m, s)) => {
                let flag = bad_photo_flags(th, Path::new(&it.e.n), m, s).into_iter().next();
                let (i, label) = quality_bucket(flag);
                ((i, String::new()), label.to_string(), None)
            }
        }),
        GroupKey::BurstDense | GroupKey::BurstSparse => {
            let kind = if group == GroupKey::BurstDense { BurstKind::Dense } else { BurstKind::Sparse };
            // Bursts are found in time order, whatever the sort order of the grid is.
            let mut by_time: Vec<usize> = (0..items.len()).collect();
            let time = |it: &Item| it.e.t.unwrap_or(it.e.m / 1000);
            by_time.sort_by(|&a, &b| {
                time(&items[a]).cmp(&time(&items[b])).then_with(|| natural_cmp(&items[a].e.n, &items[b].e.n))
            });
            let seq: Vec<(i64, Option<&Signature>)> = by_time.iter().map(|&i| (time(&items[i]), items[i].s.as_ref())).collect();
            let clusters = cluster_bursts(&seq, kind).into_iter().map(|c| c.into_iter().map(|k| by_time[k]).collect()).collect();
            clustered(&mut items, clusters, "Burst", "Not in a burst")
        }
        GroupKey::Same | GroupKey::High | GroupKey::Medium | GroupKey::Low => {
            let (level, name, rest) = match group {
                GroupKey::Same => (Level::Same, "Duplicates", "No duplicates"),
                GroupKey::High => (Level::High, "Similar", "No similar photos"),
                GroupKey::Medium => (Level::Medium, "Similar", "No similar photos"),
                _ => (Level::Low, "Similar", "No similar photos"),
            };
            let sigs: Vec<Option<&Signature>> = items.iter().map(|it| it.s.as_ref()).collect();
            let clusters = cluster_similar(&sigs, level);
            clustered(&mut items, clusters, name, rest)
        }
    };

    entries.extend(items.into_iter().map(|it| it.e));
    Listing { entries, groups, missing, need: needs.analysis() }
}

/// Analyzed images by score; images without a result stay at the end, in their order.
fn sort_by_quality(items: &mut [Item], desc: bool) {
    items.sort_by(|a, b| match (&a.q, &b.q) {
        (Some((ma, _)), Some((mb, _))) => {
            let o = ma.score().total_cmp(&mb.score());
            if desc { o.reverse() } else { o }
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

/// Analyzed images as a chain of nearest neighbors; images without a result stay at the end.
fn sort_by_similarity(items: &mut Vec<Item>, desc: bool) {
    let (with, without): (Vec<Item>, Vec<Item>) = std::mem::take(items).into_iter().partition(|it| it.s.is_some());
    let sigs: Vec<&Signature> = with.iter().filter_map(|it| it.s.as_ref()).collect();
    let mut order = similarity_order(&sigs);
    if desc {
        order.reverse();
    }
    let mut slots: Vec<Option<Item>> = with.into_iter().map(Some).collect();
    items.extend(order.into_iter().filter_map(|i| slots[i].take()));
    items.extend(without);
}

/// Groups by a key of each image. The groups are in key order, reversed for `desc`; the images of
/// a group keep their order. `key` returns (order key, label, kind).
fn keyed(
    items: &mut Vec<Item>,
    desc: bool,
    key: impl Fn(&Item) -> ((i64, String), String, Option<&'static str>),
) -> Vec<Group> {
    let mut groups: BTreeMap<(i64, String), (Group, Vec<Item>)> = BTreeMap::new();
    for it in std::mem::take(items) {
        let (k, label, kind) = key(&it);
        let g = groups.entry(k).or_insert_with(|| (Group { label, count: 0, kind }, Vec::new()));
        g.0.count += 1;
        g.1.push(it);
    }
    let mut ordered: Vec<(Group, Vec<Item>)> = groups.into_values().collect();
    if desc {
        ordered.reverse();
    }
    let mut out = Vec::with_capacity(ordered.len());
    for (g, members) in ordered {
        out.push(g);
        items.extend(members);
    }
    out
}

/// Groups by clusters of image indexes. The clusters are in the order of their first image; then
/// the images in no cluster (`rest`), then the images without a signature.
fn clustered(items: &mut Vec<Item>, mut clusters: Vec<Vec<usize>>, name: &str, rest: &str) -> Vec<Group> {
    for c in clusters.iter_mut() {
        c.sort_unstable();
    }
    clusters.sort_by_key(|c| c[0]);
    let mut slots: Vec<Option<Item>> = std::mem::take(items).into_iter().map(Some).collect();
    let mut out = Vec::new();
    for (n, c) in clusters.iter().enumerate() {
        out.push(Group { label: format!("{name} {}", n + 1), count: c.len(), kind: None });
        items.extend(c.iter().filter_map(|&i| slots[i].take()));
    }
    let (single, missing): (Vec<Item>, Vec<Item>) = slots.into_iter().flatten().partition(|it| it.s.is_some());
    if !single.is_empty() {
        out.push(Group { label: rest.to_string(), count: single.len(), kind: Some("rest") });
        items.extend(single);
    }
    if !missing.is_empty() {
        out.push(Group { label: "Not analyzed".to_string(), count: missing.len(), kind: Some("missing") });
        items.extend(missing);
    }
    out
}

fn size_bucket(bytes: u64) -> (i64, &'static str) {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    match bytes {
        b if b < 100 * KB => (0, "Tiny (under 100 KB)"),
        b if b < MB => (1, "Small (100 KB – 1 MB)"),
        b if b < 5 * MB => (2, "Medium (1 – 5 MB)"),
        b if b < 20 * MB => (3, "Large (5 – 20 MB)"),
        _ => (4, "Huge (over 20 MB)"),
    }
}

fn quality_bucket(flag: Option<QualityFlag>) -> (i64, &'static str) {
    match flag {
        None => (0, "Good"),
        Some(QualityFlag::Blurry) => (1, "Blurry"),
        Some(QualityFlag::Underexposed) => (2, "Underexposed"),
        Some(QualityFlag::Overexposed) => (3, "Overexposed"),
        Some(QualityFlag::LowContrast) => (4, "Low Contrast"),
        Some(QualityFlag::Screenshot) => (5, "Screenshot"),
    }
}

fn type_label(name: &str) -> String {
    match ext_lower(Path::new(name)).as_deref() {
        Some("jpg" | "jpeg") => "JPEG".into(),
        Some("heic" | "heif") => "HEIC".into(),
        Some("tif" | "tiff") => "TIFF".into(),
        Some(e) => e.to_ascii_uppercase(),
        None => "Other".into(),
    }
}

/// Seconds since the epoch (UTC) to naive local seconds, like the date taken.
fn to_local(secs: i64) -> i64 {
    // SAFETY: `localtime_r` writes only to `tm`, which is a valid, zeroed `libc::tm`.
    let off = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let t = secs as libc::time_t;
        if libc::localtime_r(&t, &mut tm).is_null() { 0 } else { tm.tm_gmtoff }
    };
    secs + off
}

/// "2024-03-12" for a day number since 1970-01-01 (proleptic Gregorian calendar).
fn iso_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::ImageAnalyzer;

    fn img(n: &str, s: u64, t: i64) -> Entry {
        Entry { n: n.into(), d: false, s, m: t * 1000, t: Some(t), ..Default::default() }
    }

    fn names(l: &Listing) -> Vec<&str> {
        l.entries.iter().map(|e| e.n.as_str()).collect()
    }

    fn labels(l: &Listing) -> Vec<(&str, usize)> {
        l.groups.iter().map(|g| (g.label.as_str(), g.count)).collect()
    }

    fn idx() -> Arc<PreviewIndex> {
        Arc::new(PreviewIndex::in_memory().unwrap())
    }

    const NAME: SortSpec = SortSpec { key: SortKey::Name, desc: false };
    const DAY: i64 = 86_400;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(19_782), "2024-02-29");
        assert_eq!(iso_date(-1), "1969-12-31");
    }

    #[test]
    fn no_grouping_leaves_the_listing_as_it_is() {
        let e = vec![img("b.jpg", 1, 0), img("a.jpg", 1, 0)];
        let l = arrange(None, e, NAME, GroupKey::None, &idx(), &Thresholds::default());
        assert_eq!(names(&l), ["b.jpg", "a.jpg"]);
        assert!(l.groups.is_empty() && l.need.is_none() && l.missing == 0);
    }

    #[test]
    fn groups_by_day_keep_folders_first_and_follow_the_direction() {
        let mk = || {
            vec![
                Entry { n: "dir".into(), d: true, ..Default::default() },
                img("a.jpg", 1, 3 * DAY + 10),
                img("b.jpg", 1, 5 * DAY + 10),
                img("c.jpg", 1, 3 * DAY + 20),
            ]
        };
        let l = arrange(None, mk(), NAME, GroupKey::Taken, &idx(), &Thresholds::default());
        assert_eq!(names(&l), ["dir", "a.jpg", "c.jpg", "b.jpg"]);
        assert_eq!(labels(&l), [("1970-01-04", 2), ("1970-01-06", 1)]);
        assert_eq!(l.groups[0].kind, Some("date"));
        let l = arrange(None, mk(), SortSpec { key: SortKey::Name, desc: true }, GroupKey::Taken, &idx(), &Thresholds::default());
        assert_eq!(labels(&l), [("1970-01-06", 1), ("1970-01-04", 2)]);
    }

    #[test]
    fn groups_by_size_and_type() {
        let e = vec![img("a.JPG", 10, 0), img("b.png", 3 << 20, 0), img("c.jpeg", 2 << 20, 0), img("d.heic", 50, 0)];
        let l = arrange(None, e.clone(), NAME, GroupKey::Size, &idx(), &Thresholds::default());
        assert_eq!(names(&l), ["a.JPG", "d.heic", "b.png", "c.jpeg"]);
        assert_eq!(labels(&l), [("Tiny (under 100 KB)", 2), ("Medium (1 – 5 MB)", 2)]);
        let l = arrange(None, e, NAME, GroupKey::Type, &idx(), &Thresholds::default());
        assert_eq!(labels(&l), [("HEIC", 1), ("JPEG", 2), ("PNG", 1)]);
        assert_eq!(names(&l), ["d.heic", "a.JPG", "c.jpeg", "b.png"]);
    }

    /// Writes analysis rows for "/x/<name>".
    fn put<A: ImageAnalyzer>(index: &Arc<PreviewIndex>, e: &Entry, out: &A::Output) {
        let mut c = AnalysisCache::new(Arc::clone(index));
        c.put::<A>(&Path::new("/x").join(&e.n), e.m, e.s, out);
    }

    fn sig(bits: u32) -> Signature {
        Signature { p: if bits == 0 { 0 } else { u64::MAX >> (64 - bits) }, d: 0, c: vec![9; 48], h: vec![] }
    }

    #[test]
    fn similarity_groups_report_missing_results() {
        let index = idx();
        let e = vec![img("a.jpg", 1, 0), img("b.jpg", 1, 0), img("c.jpg", 1, 0), img("d.jpg", 1, 0), img("e.jpg", 1, 0)];
        for (i, bits) in [(0, 0), (1, 60), (2, 2), (3, 30)] {
            put::<SimilarityAnalyzer>(&index, &e[i], &sig(bits));
        }
        let l = arrange(Some(Path::new("/x")), e.clone(), NAME, GroupKey::Same, &index, &Thresholds::default());
        assert_eq!(names(&l), ["a.jpg", "c.jpg", "b.jpg", "d.jpg", "e.jpg"]);
        assert_eq!(labels(&l), [("Duplicates 1", 2), ("No duplicates", 2), ("Not analyzed", 1)]);
        assert_eq!((l.missing, l.need), (1, Some("similarity")));
        // The sort by similarity makes a chain; the image without a result is last.
        let l = arrange(Some(Path::new("/x")), e, SortSpec { key: SortKey::Similarity, desc: false }, GroupKey::None, &index, &Thresholds::default());
        assert_eq!(names(&l), ["a.jpg", "c.jpg", "d.jpg", "b.jpg", "e.jpg"]);
        assert!(l.groups.is_empty());
    }

    #[test]
    fn bursts_are_found_in_time_order_and_shown_in_sort_order() {
        let index = idx();
        // Name order differs from time order.
        let e = vec![img("a.jpg", 1, 100), img("b.jpg", 1, 500), img("c.jpg", 1, 101), img("d.jpg", 1, 102)];
        for x in &e {
            put::<SimilarityAnalyzer>(&index, x, &sig(0));
        }
        let l = arrange(Some(Path::new("/x")), e, NAME, GroupKey::BurstDense, &index, &Thresholds::default());
        assert_eq!(names(&l), ["a.jpg", "c.jpg", "d.jpg", "b.jpg"]);
        assert_eq!(labels(&l), [("Burst 1", 3), ("Not in a burst", 1)]);
    }

    #[test]
    fn quality_sort_and_groups_use_the_cached_metrics() {
        let index = idx();
        let e = vec![img("a.jpg", 1, 0), img("b.jpg", 1, 0), img("c.jpg", 1, 0)];
        let good = QualityMetrics { lap_var_local: 900.0, mean_luma: 120.0, std_luma: 50.0, ..Default::default() };
        let blurry = QualityMetrics { lap_var_local: 1.0, ..good };
        let photo = ScreenshotSignals { camera_exif: true, ..Default::default() };
        put::<Pair<QualityAnalyzer, ScreenshotAnalyzer>>(&index, &e[0], &(blurry, photo));
        put::<Pair<QualityAnalyzer, ScreenshotAnalyzer>>(&index, &e[1], &(good, photo));
        let th = Thresholds::default();
        let l = arrange(Some(Path::new("/x")), e.clone(), NAME, GroupKey::Quality, &index, &th);
        assert_eq!(labels(&l), [("Good", 1), ("Blurry", 1), ("Not analyzed", 1)]);
        assert_eq!(names(&l), ["b.jpg", "a.jpg", "c.jpg"]);
        assert_eq!(l.entries[1].b.as_deref(), Some(&["quality.blurry".to_string()][..]));
        assert_eq!((l.missing, l.need), (1, Some("quality")));
        let best_first = SortSpec { key: SortKey::Quality, desc: true };
        let l = arrange(Some(Path::new("/x")), e.clone(), best_first, GroupKey::None, &index, &th);
        assert_eq!(names(&l), ["b.jpg", "a.jpg", "c.jpg"]);
        let l = arrange(Some(Path::new("/x")), e, SortSpec { key: SortKey::Quality, desc: false }, GroupKey::None, &index, &th);
        assert_eq!(names(&l), ["a.jpg", "b.jpg", "c.jpg"]);
    }
}
