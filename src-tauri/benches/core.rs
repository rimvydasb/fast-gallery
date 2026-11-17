//! PERF-14 benchmark: measures PERF-05 (list a directory of 10,000 entries) and PERF-07 (generate one
//! 256 px thumbnail from a 24 MP JPEG). Also reports related numbers for the folder preview and the
//! thumbnail cache.
//!
//! Run: `cargo bench --bench core` (in src-tauri), or `npm run bench`.

use std::path::Path;
use std::time::{Duration, Instant};

use fast_gallery_lib::decode;
use fast_gallery_lib::pipeline::{decode_input_for, AnalysisCache, ImageAnalyzer, Pair};
use fast_gallery_lib::quality::QualityAnalyzer;
use fast_gallery_lib::screenshot::ScreenshotAnalyzer;
use fast_gallery_lib::similarity::SimilarityAnalyzer;
use fast_gallery_lib::scan::index::PreviewIndex;
use fast_gallery_lib::scan::preview::{resolve, Limits};
use fast_gallery_lib::scan::{list_dir, SortKey, SortSpec};
use fast_gallery_lib::thumbs::{ThumbCache, ThumbnailService};

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> Duration {
    let _ = f(); // warm up
    median((0..runs).map(|_| {
        let t = Instant::now();
        std::hint::black_box(f());
        t.elapsed()
    }).collect())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn row(id: &str, metric: &str, d: Duration, budget_ms: Option<f64>) -> bool {
    let (b, verdict, ok) = match budget_ms {
        Some(b) => {
            let ok = ms(d) <= b;
            (format!("≤ {b} ms"), if ok { "PASS" } else { "FAIL" }, ok)
        }
        None => ("-".into(), "info", true),
    };
    println!("| {id:<8} | {metric:<52} | {:>9.2} ms | {b:>10} | {verdict} |", ms(d));
    ok
}

/// A 6000 × 4000 (24 MP) photo: the example iPhone photo scaled up with `sips` at quality 95.
/// Returns false if the example is missing.
fn make_24mp_photo(path: &Path) -> bool {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery/IMG_2439.JPG");
    src.exists()
        && std::process::Command::new("/usr/bin/sips")
            .args(["-z", "4000", "6000", "-s", "formatOptions", "95"])
            .arg(&src)
            .arg("--out")
            .arg(path)
            .output()
            .is_ok_and(|o| o.status.success())
}

/// A 6000 × 4000 (24 MP) worst-case JPEG: per-pixel noise, no chroma subsampling, 8.8 MB.
/// Decode time is bound by entropy decoding, which is proportional to the file size.
fn make_24mp_noise(path: &Path) {
    let (w, h) = (6000u32, 4000u32);
    let mut seed: u32 = 0x1234_5678;
    let mut img = image::RgbImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let n = (seed & 31) as i32 - 16;
        let r = ((x * 255 / w) as i32 + n).clamp(0, 255) as u8;
        let g = ((y * 255 / h) as i32 + n).clamp(0, 255) as u8;
        let b = ((((x / 40 + y / 40) % 2) * 120 + 60) as i32 + n).clamp(0, 255) as u8;
        *p = image::Rgb([r, g, b]);
    }
    let f = std::fs::File::create(path).unwrap();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 90);
    enc.encode_image(&img).unwrap();
}

fn main() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let index = PreviewIndex::in_memory().unwrap();
    let mut ok = true;

    println!("| ID       | Metric                                               |    Median    |   Budget   | Result |");
    println!("|----------|------------------------------------------------------|--------------|------------|--------|");

    // PERF-05: 10,000 entries: 9,000 images, 500 folders, 500 other files.
    let big = root.join("big");
    std::fs::create_dir_all(&big).unwrap();
    for i in 0..9000 {
        std::fs::write(big.join(format!("IMG_{i:05}.jpg")), b"").unwrap();
    }
    for i in 0..500 {
        std::fs::create_dir(big.join(format!("folder {i}"))).unwrap();
        std::fs::write(big.join(format!("notes {i}.txt")), b"").unwrap();
    }
    let name = SortSpec { key: SortKey::Name, desc: false };
    let d = time(20, || list_dir(&big, name, false, &index).unwrap().len());
    ok &= row("PERF-05", "list_dir, 10,000 entries, sort by name", d, Some(150.0));
    let d = time(20, || list_dir(&big, SortSpec { key: SortKey::Modified, desc: true }, false, &index).unwrap().len());
    ok &= row("PERF-05", "list_dir, 10,000 entries, sort by date modified", d, Some(150.0));
    let json = time(20, || serde_json::to_vec(&list_dir(&big, name, false, &index).unwrap()).unwrap().len());
    row("-", "list_dir + JSON serialization (IPC payload)", json, None);

    // PERF-07: 256 px thumbnail of a 24 MP JPEG.
    let noise = root.join("24mp-noise.jpg");
    make_24mp_noise(&noise);
    let photo = root.join("24mp-photo.jpg");
    let jpeg = if make_24mp_photo(&photo) { photo } else { noise.clone() };
    let mb = |p: &Path| std::fs::metadata(p).unwrap().len() as f64 / 1e6;
    let d = time(15, || decode::make_thumbnail(&jpeg, 256).unwrap().bytes.len());
    ok &= row("PERF-07", &format!("thumbnail 256 px, 24 MP JPEG photo ({:.1} MB)", mb(&jpeg)), d, Some(30.0));
    let d = time(15, || decode::make_thumbnail(&jpeg, 512).unwrap().bytes.len());
    row("-", "thumbnail 512 px, 24 MP JPEG photo", d, None);
    let d = time(9, || decode::make_thumbnail(&noise, 256).unwrap().bytes.len());
    row("-", &format!("thumbnail 256 px, 24 MP worst case: noise ({:.1} MB)", mb(&noise)), d, None);

    // QUAL-24: analysis of one 24 MP JPEG (1024 px gray decode and the kernels).
    let qa = QualityAnalyzer;
    let d = time(15, || qa.analyze(&decode_input_for(&qa, &jpeg).unwrap()).unwrap().lap_var);
    ok &= row("QUAL-24", "quality analysis, 24 MP JPEG photo", d, Some(40.0));
    let input = decode_input_for(&qa, &jpeg).unwrap();
    let g = input.gray.as_ref().unwrap();
    let d = time(15, || qa.analyze(&input).unwrap().lap_var);
    row("-", &format!("quality kernels only, {} × {} gray", g.width, g.height), d, None);
    // The bad photo scan: quality and screenshot signals from 1 decode.
    let pair = Pair(QualityAnalyzer, ScreenshotAnalyzer);
    let d = time(15, || pair.analyze(&decode_input_for(&pair, &jpeg).unwrap()).unwrap().1.flat);
    row("-", "quality + screenshot analysis, 24 MP JPEG photo", d, None);

    // Cached thumbnail read (disk cache, then memory cache), related to PERF-06.
    let svc = ThumbnailService::new(ThumbCache::new(root.join("cache"), u64::MAX));
    svc.get_blocking(&jpeg, 256).unwrap();
    let d = time(50, || svc.get_blocking(&jpeg, 256).unwrap().bytes.len());
    row("-", "cached thumbnail lookup (memory)", d, None);
    let d = time(20, || {
        // A new service has an empty memory cache, so this reads the disk cache.
        let fresh = ThumbnailService::new(ThumbCache::new(root.join("cache"), u64::MAX));
        fresh.get_blocking(&jpeg, 256).unwrap().bytes.len()
    });
    row("-", "cached thumbnail lookup (disk)", d, None);

    // ThumbCache writes and eviction (THM-09), with real thumbnail bytes.
    let thumb = std::sync::Arc::new(decode::make_thumbnail(&jpeg, 256).unwrap());
    let key = |i: usize, size: u32| fast_gallery_lib::thumbs::ThumbKey {
        path: root.join(format!("src/IMG_{i:05}.jpg")),
        file_size: 1,
        mtime_ns: 1,
        size,
    };
    const N: usize = 2000;
    // `store` queues the write (after) or writes at once (before); `flush` waits for the disk.
    let mut blocked = Vec::new();
    let mut writes_new = Vec::new();
    let mut writes_existing = Vec::new();
    for run in 0..5 {
        let c = ThumbCache::new(root.join(format!("wcache{run}")), u64::MAX);
        let t = Instant::now();
        for i in 0..N {
            c.store(key(i, 256), std::sync::Arc::clone(&thumb));
        }
        blocked.push(t.elapsed());
        c.flush();
        writes_new.push(t.elapsed());
        let t = Instant::now();
        for i in 0..N {
            c.store(key(i, 512), std::sync::Arc::clone(&thumb));
        }
        c.flush();
        writes_existing.push(t.elapsed());
        c.clear();
    }
    row("-", &format!("cache store, time the decode thread waits, {N}"), median(blocked), None);
    row("-", &format!("cache store until on disk, new photos, {N}"), median(writes_new), None);
    row("-", &format!("cache store until on disk, folder exists, {N}"), median(writes_existing), None);

    const E: usize = 5000;
    let mut evicts = Vec::new();
    for run in 0..5 {
        let c = ThumbCache::new(root.join(format!("ecache{run}")), u64::MAX);
        for i in 0..E {
            c.store(key(i, 256), std::sync::Arc::clone(&thumb));
        }
        c.flush();
        // Limit = use: both the old rule (90 % of the limit) and the new rule (90 % of the use)
        // remove the same 10 %.
        c.set_limit(c.used());
        let t = Instant::now();
        c.evict();
        evicts.push(t.elapsed());
        c.clear();
    }
    row("-", &format!("cache eviction of 10 %, {E} entries"), median(evicts), None);

    // Disk cache reads (the folder open from cache, PERF-06), alone and while an eviction runs on
    // its own thread (before: normal QoS; after: background QoS).
    const R: usize = 20_000;
    let c = ThumbCache::new(root.join("rcache"), u64::MAX);
    for i in 0..R {
        c.store(key(i, 256), std::sync::Arc::clone(&thumb));
    }
    c.flush();
    let newest: Vec<_> = (R - N..R).map(|i| key(i, 256)).collect();
    let read_all = || newest.iter().filter(|k| c.read(k).is_some()).count();
    assert_eq!(read_all(), N);
    row("-", &format!("cache read from disk, {N} entries"), time(20, read_all), None);
    let mut during = Vec::new();
    let mut evict_wall = Vec::new();
    for run in 0..3 {
        // A new cache of R entries, then the use is made 130 % of the limit: the eviction thread starts.
        let c = ThumbCache::new(root.join(format!("rcache{run}")), u64::MAX);
        for i in 0..R {
            c.store(key(i, 256), std::sync::Arc::clone(&thumb));
        }
        c.flush();
        let read_all = || newest.iter().filter(|k| c.read(k).is_some()).count();
        let used = c.used();
        let t = Instant::now();
        c.set_limit(used / 13 * 10);
        while c.used() > c.limit() {
            let t = Instant::now();
            std::hint::black_box(read_all());
            during.push(t.elapsed());
        }
        evict_wall.push(t.elapsed());
    }
    row("-", &format!("cache read from disk, {N} entries, during eviction"), median(during), None);
    row("-", &format!("eviction thread wall time, {R} entries (not same amount)"), median(evict_wall), None);
    // Analysis cache writes to an on-disk index (QUAL-17): the cost of the `analysis` table indexes.
    let qout = qa.analyze(&input).unwrap();
    let mut awrites = Vec::new();
    for run in 0..5 {
        let ix = std::sync::Arc::new(PreviewIndex::open(&root.join(format!("ix{run}.sqlite"))).unwrap());
        let t = Instant::now();
        {
            let mut c = AnalysisCache::new(ix);
            for i in 0..N {
                c.put::<QualityAnalyzer>(&root.join(format!("src/IMG_{i:05}.jpg")), 1, 1, &qout);
            }
        } // drop flushes the last batch
        awrites.push(t.elapsed());
    }
    row("-", &format!("analysis cache write, {N} rows"), median(awrites), None);
    // Reads (a rescan of an analysed folder), in an order other than the write order.
    let c = AnalysisCache::new(std::sync::Arc::new(PreviewIndex::open(&root.join("ix0.sqlite")).unwrap()));
    let hits = (0..N).filter(|i| c.get::<QualityAnalyzer>(&root.join(format!("src/IMG_{i:05}.jpg")), 1, 1).is_some()).count();
    assert_eq!(hits, N, "the read benchmark must hit the cache");
    let d = time(5, || {
        (0..N).rev().filter(|i| c.get::<QualityAnalyzer>(&root.join(format!("src/IMG_{i:05}.jpg")), 1, 1).is_some()).count()
    });
    row("-", &format!("analysis cache read, {N} rows"), d, None);

    // Eviction removes the analysis rows of evicted photos: 1 scan of `analysis` (`hash` has no
    // index, so that writes stay as fast as without the column). 20,000 photos with quality and
    // similarity rows; 10 % evicted.
    const P: usize = 20_000;
    let sa = SimilarityAnalyzer;
    let sout = sa.analyze(&decode_input_for(&sa, &jpeg).unwrap()).unwrap();
    let src = |i: usize| root.join(format!("big-src/IMG_{i:05}.jpg"));
    let mut deletes = Vec::new();
    for run in 0..5 {
        let ix = std::sync::Arc::new(PreviewIndex::open(&root.join(format!("ixd{run}.sqlite"))).unwrap());
        {
            let mut c = AnalysisCache::new(std::sync::Arc::clone(&ix));
            for i in 0..P {
                c.put::<QualityAnalyzer>(&src(i), 1, 1, &qout);
                c.put::<SimilarityAnalyzer>(&src(i), 1, 1, &sout);
            }
        }
        let hashes: Vec<u64> = (0..P / 10).map(|i| fast_gallery_lib::thumbs::path_hash(&src(i))).collect();
        let t = Instant::now();
        ix.analysis_delete_hashes(&hashes);
        deletes.push(t.elapsed());
        let c = AnalysisCache::new(ix);
        assert!(c.get::<QualityAnalyzer>(&src(0), 1, 1).is_none() && c.get::<QualityAnalyzer>(&src(P - 1), 1, 1).is_some());
    }
    row("-", &format!("eviction: analysis delete, {} of {P} photos", P / 10), median(deletes), None);

    // Folder preview resolution: 20 nested folders, images at depth 3.
    let tree = root.join("tree");
    for i in 0..20 {
        let d = tree.join(format!("{i}/a/b"));
        std::fs::create_dir_all(&d).unwrap();
        for j in 0..10 {
            std::fs::write(d.join(format!("p{j}.jpg")), b"").unwrap();
        }
    }
    let d = time(20, || resolve(&tree, name, Limits::default(), &index).images.len());
    row("-", "folder preview resolve, images at depth 3", d, None);

    // Examples from the repository, if present.
    let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery");
    if let Ok(rd) = std::fs::read_dir(&ex) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| fast_gallery_lib::scan::is_image_path(p)).collect();
        files.sort();
        for f in files {
            let d = time(5, || decode::make_thumbnail(&f, 256).map(|e| e.bytes.len()));
            row("-", &format!("thumbnail 256 px: {}", f.file_name().unwrap().to_string_lossy()), d, None);
        }
    }

    println!();
    println!("threads: decode pool = {} (performance cores)", fast_gallery_lib::pools::perf_cores());
    if !ok {
        eprintln!("A performance budget failed.");
        std::process::exit(1);
    }
}
