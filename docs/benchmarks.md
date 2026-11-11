# Benchmarks and Profiles

This document records the measured performance budgets (PRIO-04, PERF-14).

## Test Machine

All results in this document come from an Apple M4 Pro (10 performance cores) with macOS 27.2 and an SSD. The
budgets in `requirements.md` apply to an Apple M1 with 8 GB RAM. The M1 has slower cores, so the M1 values are expected
to be higher. An M1 run is still necessary before a release.

## How to Run

| Measurement                       | Command or method                                                                 |
|-----------------------------------|-----------------------------------------------------------------------------------|
| PERF-05, PERF-07 (PERF-14)        | `npm run bench` (runs `cargo bench --bench core` in `src-tauri`)                  |
| Start-up profile (PERF-02)        | `FG_TRACE_STARTUP=1 "Fast Gallery.app/Contents/MacOS/fast-gallery"`               |
| Bundle size (PERF-01)             | `npm run build:universal`, then `du -sh` on the `.app`                            |
| Memory (PERF-03, PERF-04)         | `footprint` on the app process and its WebKit processes                           |
| PERF-06, PERF-08 to PERF-10       | Scripted runs in an optimized build with debug assertions (`FG_DEV_SCRIPT` hook)  |

The `FG_DEV_SCRIPT=<file>` hook exists only in builds with debug assertions. It evaluates the file in the main window
each time the file changes. Release builds do not contain it.

## Results

| ID      | Metric                                                    | Budget    | Result                                    | Status        |
|---------|-----------------------------------------------------------|-----------|-------------------------------------------|---------------|
| PERF-01 | Size of the universal `.app` bundle                       | ≤ 15 MB   | 12.92 MiB (13.6 MB)                       | Pass          |
| PERF-02 | Cold start to an interactive window                       | ≤ 500 ms  | 356–376 ms (5 runs)                       | Pass          |
| PERF-03 | Memory at idle, empty window                              | ≤ 80 MB   | App 31 MB; with WebKit processes 87 MB    | See note      |
| PERF-04 | Memory, 10,000 images open in grid view                   | ≤ 300 MB  | 113 MB (app and all WebKit processes)     | Pass          |
| PERF-05 | List a directory of 10,000 entries                        | ≤ 150 ms  | 12–14 ms                                  | Pass          |
| PERF-06 | First visible thumbnails from cache after folder open     | ≤ 100 ms  | 78–85 ms (10,000-image folder)            | Pass          |
| PERF-07 | One 256 px thumbnail from a 24 MP JPEG                    | ≤ 30 ms   | 17.6 ms (4.7 MB photo)                    | Pass          |
| PERF-08 | Grid scroll frame rate                                    | 60 fps    | 60 fps, max frame 19 ms, 0 frames > 25 ms | Pass          |
| PERF-09 | Viewer: next image that is prefetched                     | ≤ 50 ms   | 2–5 ms                                    | Pass          |
| PERF-10 | Viewer: next 24 MP JPEG that is not prefetched            | ≤ 200 ms  | 65–138 ms                                 | Pass          |

PERF-03 note: the app process uses 31 MB. The WKWebView processes (WebContent 31 MB, GPU 20 MB, Networking 6 MB) add
56 MB. The live heap of the app process is 10.6 MB, mostly AppKit and Core Foundation objects. If the budget includes the
WebKit processes, the result is 7 MB over the budget. This floor comes from the system WebView (PLAT-03).

## Benchmark Output

`cargo bench --bench core`:

| ID      | Metric                                               | Median    | Budget   | Result |
|---------|------------------------------------------------------|-----------|----------|--------|
| PERF-05 | list_dir, 10,000 entries, sort by name               | 13.54 ms  | ≤ 150 ms | PASS   |
| PERF-05 | list_dir, 10,000 entries, sort by date modified      | 13.95 ms  | ≤ 150 ms | PASS   |
| -       | list_dir + JSON serialization (IPC payload)          | 14.44 ms  | -        | info   |
| PERF-07 | thumbnail 256 px, 24 MP JPEG photo (4.7 MB)          | 17.61 ms  | ≤ 30 ms  | PASS   |
| -       | thumbnail 512 px, 24 MP JPEG photo                   | 18.49 ms  | -        | info   |
| -       | thumbnail 256 px, 24 MP worst case: noise (8.8 MB)   | 30.00 ms  | -        | info   |
| -       | folder preview resolve, images at depth 3            | 0.09 ms   | -        | info   |

The 24 MP photo is the example photo `IMG_2439.JPG`, scaled to 6000 × 4000 with `sips` at quality 95. The worst case is
per-pixel noise without chroma subsampling. JPEG decode time is bound by entropy decoding, so it grows with the file
size (about 3.5 ms per MB).

## Decisions From Profiles

### Thumbnail decode (PERF-07)

Median time for one 256 px thumbnail from a 24 MP JPEG photo (4.3 MB):

| Option                                                                  | Time     |
|-------------------------------------------------------------------------|----------|
| `CGImageSourceCreateThumbnailAtIndex`, `FromImageAlways`, max 256       | 24.5 ms  |
| `CGImageSourceCreateImageAtIndex`, full decode                          | 38.2 ms  |
| `kCGImageSourceSubsampleFactor` 8, CoreGraphics downscale, encode       | 24.4 ms  |
| `kCGImageSourceSubsampleFactor` 2, CoreGraphics downscale, encode       | 17.3 ms  |
| `kCGImageSourceSubsampleFactor` 4, CoreGraphics downscale, encode       | 16.2 ms  |
| `kCGImageSourceSubsampleFactor` 4, downscale in `CGImageDestination`    | 23.6 ms  |

The JPEG path uses subsample factor 4 (or 2) and a CoreGraphics downscale with the EXIF orientation (PRIO-03). A
thumbnail request "without a create-from-image key" is not a cheap check for an embedded thumbnail: ImageIO decodes the
main image. So a small EXIF parser reads IFD1 from the file header to find the embedded thumbnail and its size (THM-03).
Other formats use the ImageIO thumbnail API.

### Viewer display (PERF-10)

A full 24 MP decode in WKWebView took 195–234 ms. The viewer now loads a display-size version first
(`image://…&w=<device pixels>`): the Rust core does a subsampled ImageIO decode and encodes a JPEG. The viewer loads the
full image when a zoom needs more pixels. The prefetch starts after the current image is decoded, so that it does not
compete with the current decode. Result: 65–138 ms.

### Thumbnail cancel (PERF-12)

The thumbnail queue removes queued jobs of tiles that leave the viewport. In a fast scroll (about 9,000 px/s), rows stay
mounted long enough for their jobs to start: 465 of about 620 passed tiles got a thumbnail. The queue now pauses new
requests while the scroll speed is above 2,000 px/s and continues 120 ms after the fast scroll ends. Result: 36
thumbnails (the final screen and its buffer rows), and 60 fps.

### Start-up (PERF-02)

`FG_TRACE_STARTUP=1` profile of the release build:

| Step                                      | Time after `main` |
|-------------------------------------------|-------------------|
| Tauri and AppKit initialization           | 73–91 ms          |
| App setup (settings, SQLite, menu)        | +12 ms            |
| WKWebView window created                  | 180–200 ms        |
| Page loaded                               | 225–244 ms        |
| First IPC call from JavaScript            | 282–302 ms        |
| First folder listed                       | 294–309 ms        |

The window shows as soon as the app shell is rendered, and the start-up IPC calls run in parallel.
