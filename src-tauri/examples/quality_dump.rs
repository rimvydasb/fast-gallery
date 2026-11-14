//! Calibration tool for the bad photo thresholds: prints the quality metrics of each image in a
//! folder tree as CSV. Run it on a labelled set (for example `sharp/` and `blurry/` subfolders) and
//! compare the separation of the metrics.
//!
//! Run: `cargo run --release --example quality_dump -- <folder> [edge]` (in src-tauri).

use std::path::Path;

use fast_gallery_lib::pipeline::{decode_input, ImageAnalyzer, InputSpec, TreeWalker, WalkOptions};
use fast_gallery_lib::quality::{QualityAnalyzer, Thresholds, ANALYSIS_EDGE};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: quality_dump <folder> [edge]");
        std::process::exit(2);
    };
    let edge: u32 = args.next().and_then(|e| e.parse().ok()).unwrap_or(ANALYSIS_EDGE);
    let qa = QualityAnalyzer;
    let th = Thresholds::default();
    println!("path,folder,lap_var,lap_var_local,tenengrad,brenner,mean_luma,std_luma,clip_dark,clip_bright,flags,ms");
    for item in TreeWalker::new(Path::new(&root), WalkOptions::default()) {
        let t = std::time::Instant::now();
        let m = decode_input(&item.path, InputSpec::gray(edge)).and_then(|i| qa.analyze(&i));
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let folder = item.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy()).unwrap_or_default();
        let path = item.path.to_string_lossy().replace('"', "\"\"");
        match m {
            Some(m) => {
                let flags: Vec<&str> = th.flags(&m).iter().map(|f| f.badge()).collect();
                println!(
                    "\"{path}\",\"{folder}\",{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.4},{:.4},{},{ms:.1}",
                    m.lap_var,
                    m.lap_var_local,
                    m.tenengrad,
                    m.brenner,
                    m.mean_luma,
                    m.std_luma,
                    m.clip_dark,
                    m.clip_bright,
                    flags.join(" "),
                );
            }
            None => println!("\"{path}\",\"{folder}\",,,,,,,,,failed,{ms:.1}"),
        }
    }
}
