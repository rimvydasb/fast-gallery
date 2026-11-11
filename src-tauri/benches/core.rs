//! PERF-14 benchmark: measures PERF-05 (list a directory of 10,000 entries) and PERF-07 (generate one
//! 256 px thumbnail from a 24 MP JPEG). Also reports related numbers for the folder preview and the
//! thumbnail cache.
//!
//! Run: `cargo bench --bench core` (in src-tauri), or `npm run bench`.

use std::path::Path;
use std::time::{Duration, Instant};

use fast_gallery_lib::decode;
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
