//! Calibration tool for the screenshot rules: prints the screenshot signals of each image in a folder
//! tree as CSV. Run it on a labelled set (for example `screenshots/` and `photos/` subfolders).
//!
//! Run: `cargo run --release --example screenshot_dump -- <folder>` (in src-tauri).

use std::path::Path;

use fast_gallery_lib::pipeline::{decode_input_for, ImageAnalyzer, TreeWalker, WalkOptions};
use fast_gallery_lib::screenshot::{is_screenshot, name_matches, screen_size, ScreenshotAnalyzer};

fn main() {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("usage: screenshot_dump <folder>");
        std::process::exit(2);
    };
    let a = ScreenshotAnalyzer;
    println!("path,folder,width,height,png,camera_exif,os_comment,name,screen_size,flat,top_colors,screenshot,ms");
    for item in TreeWalker::new(Path::new(&root), WalkOptions::default()) {
        let t = std::time::Instant::now();
        let s = decode_input_for(&a, &item.path).and_then(|i| a.analyze(&i));
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let folder = item.path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy()).unwrap_or_default();
        let path = item.path.to_string_lossy().replace('"', "\"\"");
        match s {
            Some(s) => println!(
                "\"{path}\",\"{folder}\",{},{},{},{},{},{},{},{:.4},{:.4},{},{ms:.1}",
                s.width,
                s.height,
                s.png,
                s.camera_exif,
                s.os_comment,
                name_matches(&item.path),
                screen_size(s.width, s.height),
                s.flat,
                s.top_colors,
                is_screenshot(&item.path, &s),
            ),
            None => println!("\"{path}\",\"{folder}\",,,,,,,,,,failed,{ms:.1}"),
        }
    }
}
