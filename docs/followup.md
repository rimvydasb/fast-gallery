# Photo Quality Follow-up

These tasks remain open after the implementation of `photo-quality-spec.md`. Each task is a **human task**: an agent
cannot complete it alone.

## Human Tasks

- [ ] **HUMAN** Verify QUAL-28: PERF-06 and PERF-08 pass while a scan runs on a tree with 10,000 photos.
- [ ] **HUMAN** Calibrate the bad photo thresholds on a hand-labelled set of photos.

## QUAL-28: Performance While a Scan Runs

QUAL-28 is not verified. The scan shares the decode thread pool with thumbnail generation. The pipeline limits the
scan to `perf_cores() / 2` jobs in flight, so a thumbnail request waits for at most 1 analysis job. No measurement
confirms that this limit keeps the grid fast.

> Rationale for a human task: PERF-06 and PERF-08 need a visible window on a real display, a scroll by hand or by
> script, and a frame-rate measurement. The budgets apply to the reference machine (Apple M1, 8 GB RAM). The agent
> session ran only on an M4 Pro and checked the UI through the page's elements, not on screen.

| Budget  | Metric                                                | Target   |
|---------|-------------------------------------------------------|----------|
| PERF-06 | First visible thumbnails from cache after folder open | ≤ 100 ms |
| PERF-08 | Grid scroll frame rate                                | 60 fps   |

> Context: the requirement text of QUAL-28 names PERF-06 and PERF-07. The Verification section of the spec names
> PERF-06 and PERF-08. PERF-07 (one thumbnail from a 24 MP JPEG) is a benchmark without a scan. Measure PERF-08 as
> the Verification section says, and correct the requirement text if PERF-08 is the intended budget.

### Steps

1. Create a tree with 10,000 photos in many subfolders. Use real photos of 12 MP or more, not small test files.
2. Set the scan cap in Settings to 1,000, so that the scan runs long enough to measure.
3. Build an optimized app with debug assertions, so that the `FG_DEV_SCRIPT` hook exists (see `benchmarks.md`).
4. Open a large folder outside the scan tree, then start "Scan for Bad Photos" on the tree.
5. While the scan runs, open a folder with cached thumbnails and measure PERF-06.
6. While the scan runs, scroll the grid and measure PERF-08 (frame rate, maximum frame time).
7. Repeat steps 4 to 6 on the M1.
8. Record the results in `benchmarks.md`. Mark QUAL-28 as done in `requirements.md` if both budgets pass.

> Risk: if PERF-06 or PERF-08 fails, lower the job limit in `max_in_flight()` (`src-tauri/src/pipeline/run.rs`), for
> example to 1, and measure again. A separate thread pool for analysis is rejected (THM-12 thread limit).

## Threshold Calibration

The thresholds are not calibrated. All default values in `Thresholds::default()`
(`src-tauri/src/quality/sink.rs`) are first estimates. `blur_threshold` = 100 is the common value from OpenCV
tutorials; the exposure and contrast values are guesses.

> Rationale for a human task: calibration needs photos that a person marks as sharp or blurry, well or badly exposed.
> The agent has only the 6 example photos in `examples/example gallery/`, which are too few and not labelled. The
> labelled set also comes from your own library, which the agent must not read without your request.

The calibration tool prints the metrics of each image in a folder tree as CSV:

```sh
cd src-tauri
cargo run --release --example quality_dump -- <folder> [edge] > metrics.csv
```

- The `folder` column holds the name of the parent folder of each image. Put the labelled photos in folders named by
  their label, for example `sharp/`, `blurry/`, `dark/`, `bright/`, `flat/`.
- The optional `edge` argument sets the long edge of the analysis buffer (default 1024).

### Steps

1. Collect 50 or more sharp photos and 50 or more blurry photos. Include portraits with a blurred background, night
   photos, snow scenes, and high-ISO photos, because these cause the known false results.
2. Add folders of underexposed, overexposed, and low-contrast photos for the exposure thresholds.
3. Run `quality_dump` on the set at the edges 768, 1024, and 1536.
4. Compare the separation of `sharp` and `blurry` for `lap_var_local`, `lap_var`, `tenengrad`, and `brenner`.
5. Set `blur` and the exposure values in `Thresholds::default()` from the result.
6. Update the threshold table in `photo-quality-spec.md`.
7. Add 2 or 3 small sharp and blurry samples to `examples/` for the unit tests.

> Context: a threshold change needs no new analysis, because the cache stores only the metrics. A change to the
> metrics (a different edge, a different primary metric, or a Gaussian blur before the Laplacian) changes the
> algorithm. Then increase `ALGO_VERSION` in `src-tauri/src/quality/analyzer.rs`, so that the cached rows become stale.
