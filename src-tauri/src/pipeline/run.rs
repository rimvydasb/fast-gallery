//! run_pipeline: connects the walker, the cache, the analyzer, and the sink.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

use super::analyzer::{ImageAnalyzer, ResultSink};
use super::cache::AnalysisCache;
use super::input::decode_input;
use super::walk::{ImageSource, WalkItem};

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct PipelineSummary {
    /// Images taken from the walk (cached, analyzed, and failed).
    pub scanned: u32,
    pub analyzed: u32,
    pub cached: u32,
    pub failed: u32,
    /// The tree has more images than `max_images`.
    pub capped: bool,
    pub cancelled: bool,
}

/// Cancel and progress for the pipeline. `TaskContext` implements it; tests use their own.
pub trait Progress: Sync {
    fn is_cancelled(&self) -> bool;
    fn report_progress(&self, done: u32, total: u32);
}

impl Progress for crate::tasks::TaskContext {
    fn is_cancelled(&self) -> bool {
        crate::tasks::TaskContext::is_cancelled(self)
    }
    fn report_progress(&self, done: u32, total: u32) {
        crate::tasks::TaskContext::report_progress(self, done, total)
    }
}

/// Maximum analysis jobs in the decode pool (QUAL-18). The decode pool takes jobs in FIFO order, so
/// with this limit a thumbnail request waits for at most 1 analysis job (GRID-09, PERF-06).
pub fn max_in_flight() -> usize {
    (crate::pools::perf_cores() / 2).max(1)
}

type Outcome<O> = Option<O>;

struct Done<O> {
    seq: usize,
    item: WalkItem,
    out: Outcome<O>,
    /// True if the result is new and must go to the cache.
    fresh: bool,
}

/// Runs `analyzer` on each image of the walk. Cached results are reused; other images are decoded
/// and analyzed in the decode pool, with at most `in_flight` jobs at a time. `sink.accept` is called
/// in walk order. The total for the progress is `ImageSource::total` (or 0 if not known).
pub fn run_pipeline<A, S>(
    mut walker: impl ImageSource,
    analyzer: Arc<A>,
    mut sink: S,
    cache: &mut AnalysisCache,
    ctx: &dyn Progress,
    in_flight: usize,
) -> serde_json::Value
where
    A: ImageAnalyzer,
    S: ResultSink<A::Output>,
{
    let total = walker.total().unwrap_or(0) as u32;
    let limit = in_flight.max(1);
    let spec = analyzer.input();
    let (tx, rx) = crossbeam_channel::unbounded::<Done<A::Output>>();
    let mut sum = PipelineSummary::default();
    // Results that wait for an earlier result, by walk position.
    let mut ready: BTreeMap<usize, Done<A::Output>> = BTreeMap::new();
    let mut next_seq = 0usize;
    let mut next_out = 0usize;
    let mut running = 0usize;

    let deliver = |d: Done<A::Output>,
                       ready: &mut BTreeMap<usize, Done<A::Output>>,
                       next_out: &mut usize,
                       sum: &mut PipelineSummary,
                       sink: &mut S,
                       cache: &mut AnalysisCache| {
        ready.insert(d.seq, d);
        while let Some(d) = ready.remove(next_out) {
            *next_out += 1;
            match &d.out {
                Some(o) => {
                    if d.fresh {
                        cache.put::<A>(&d.item.path, d.item.mtime, d.item.size, o);
                        sum.analyzed += 1;
                    }
                    sink.accept(&d.item.path, o);
                }
                None => {
                    sum.failed += 1;
                    sink.failed(&d.item.path);
                }
            }
            ctx.report_progress(*next_out as u32, total.max(*next_out as u32));
        }
    };

    loop {
        if ctx.is_cancelled() {
            sum.cancelled = true;
            break;
        }
        let Some(item) = walker.next() else { break };
        let seq = next_seq;
        next_seq += 1;
        sum.scanned += 1;
        if let Some(out) = cache.get::<A>(&item.path, item.mtime, item.size) {
            sum.cached += 1;
            deliver(Done { seq, item, out: Some(out), fresh: false }, &mut ready, &mut next_out, &mut sum, &mut sink, cache);
            continue;
        }
        while running >= limit {
            let Ok(d) = rx.recv() else { break };
            running -= 1;
            deliver(d, &mut ready, &mut next_out, &mut sum, &mut sink, cache);
        }
        let tx = tx.clone();
        let a = Arc::clone(&analyzer);
        running += 1;
        crate::pools::decode().spawn(move || {
            let out = decode_input(&item.path, spec).and_then(|inp| a.analyze(&inp));
            let _ = tx.send(Done { seq, item, out, fresh: true });
        });
    }
    // Wait for the jobs in flight, also after a cancel.
    while running > 0 {
        let Ok(d) = rx.recv() else { break };
        running -= 1;
        deliver(d, &mut ready, &mut next_out, &mut sum, &mut sink, cache);
    }
    cache.flush();
    sum.capped = walker.is_capped();
    sum.cancelled |= ctx.is_cancelled();
    sink.finish(&sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::input::{AnalysisInput, InputSpec};
    use crate::pipeline::walk::{TreeWalker, WalkOptions};
    use crate::scan::index::PreviewIndex;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Mean luminance; counts its calls.
    struct MeanAnalyzer {
        calls: AtomicUsize,
    }

    impl ImageAnalyzer for MeanAnalyzer {
        const ID: &'static str = "test-mean";
        const VERSION: u32 = 1;
        type Output = u32;
        fn input(&self) -> InputSpec {
            InputSpec::gray(64)
        }
        fn analyze(&self, i: &AnalysisInput) -> Option<u32> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let g = &i.gray.as_ref()?.data;
            Some((g.iter().map(|&v| v as u64).sum::<u64>() / g.len() as u64) as u32)
        }
    }

    struct V2(MeanAnalyzer);
    impl ImageAnalyzer for V2 {
        const ID: &'static str = "test-mean";
        const VERSION: u32 = 2;
        type Output = u32;
        fn input(&self) -> InputSpec {
            self.0.input()
        }
        fn analyze(&self, i: &AnalysisInput) -> Option<u32> {
            self.0.analyze(i)
        }
    }

    #[derive(Default)]
    struct Collect {
        seen: Vec<String>,
        failed: Vec<String>,
    }

    impl ResultSink<u32> for Collect {
        fn accept(&mut self, p: &Path, _: &u32) {
            self.seen.push(p.file_name().unwrap().to_string_lossy().into_owned());
        }
        fn failed(&mut self, p: &Path) {
            self.failed.push(p.file_name().unwrap().to_string_lossy().into_owned());
        }
        fn finish(self, s: &PipelineSummary) -> serde_json::Value {
            serde_json::json!({ "seen": self.seen, "failed": self.failed, "summary": s })
        }
    }

    struct Ctx {
        cancel: AtomicBool,
        cancel_after: Option<u32>,
        last: AtomicUsize,
    }

    impl Ctx {
        fn new(cancel_after: Option<u32>) -> Self {
            Self { cancel: AtomicBool::new(false), cancel_after, last: AtomicUsize::new(0) }
        }
    }

    impl Progress for Ctx {
        fn is_cancelled(&self) -> bool {
            self.cancel.load(Ordering::Relaxed)
        }
        fn report_progress(&self, done: u32, _: u32) {
            self.last.store(done as usize, Ordering::Relaxed);
            if self.cancel_after.is_some_and(|n| done >= n) {
                self.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Small PNG files with different gray levels, so that decode times differ little but results
    /// can arrive in any order.
    fn make_tree(root: &Path, n: usize) -> Vec<String> {
        let mut names = Vec::new();
        for i in 0..n {
            let dir = root.join(format!("d{}", i / 4));
            std::fs::create_dir_all(&dir).unwrap();
            let name = format!("img{i}.png");
            let size = 16 + (i % 5) as u32 * 40;
            let img = image::GrayImage::from_pixel(size, size, image::Luma([(i * 7 % 255) as u8]));
            img.save(dir.join(&name)).unwrap();
            names.push(name);
        }
        std::fs::write(root.join("d0/broken.jpg"), b"not an image").unwrap();
        names
    }

    fn run<A: ImageAnalyzer<Output = u32>>(
        root: &Path,
        a: Arc<A>,
        idx: &Arc<PreviewIndex>,
        ctx: &Ctx,
        cap: Option<usize>,
    ) -> serde_json::Value {
        let w = TreeWalker::new(root, WalkOptions { max_images: cap, ..Default::default() });
        let mut cache = AnalysisCache::new(Arc::clone(idx));
        run_pipeline(w, a, Collect::default(), &mut cache, ctx, 3)
    }

    #[test]
    fn walk_order_cache_reuse_and_version_change() {
        let t = tempfile::tempdir().unwrap();
        // "broken.jpg" fails to decode; the sink gets it in `failed`.
        let expect = make_tree(t.path(), 13);
        let idx = Arc::new(PreviewIndex::in_memory().unwrap());
        let a = Arc::new(MeanAnalyzer { calls: AtomicUsize::new(0) });

        let r = run(t.path(), Arc::clone(&a), &idx, &Ctx::new(None), None);
        assert_eq!(r["seen"], serde_json::json!(expect));
        assert_eq!(r["failed"], serde_json::json!(["broken.jpg"]));
        assert_eq!(r["summary"]["analyzed"], 13);
        assert_eq!(r["summary"]["scanned"], 14);
        assert_eq!(a.calls.load(Ordering::Relaxed), 13);

        // The second run reads only the cache; the failed image is tried again.
        let r = run(t.path(), Arc::clone(&a), &idx, &Ctx::new(None), None);
        assert_eq!(r["seen"], serde_json::json!(expect));
        assert_eq!(r["summary"]["cached"], 13);
        assert_eq!(r["summary"]["analyzed"], 0);
        assert_eq!(a.calls.load(Ordering::Relaxed), 13);

        // A version change makes the rows stale.
        let v2 = Arc::new(V2(MeanAnalyzer { calls: AtomicUsize::new(0) }));
        let r = run(t.path(), Arc::clone(&v2), &idx, &Ctx::new(None), None);
        assert_eq!(r["summary"]["analyzed"], 13);
        assert_eq!(v2.0.calls.load(Ordering::Relaxed), 13);
    }

    /// Max luminance, as a second analyzer with its own cache rows.
    struct MaxAnalyzer;
    impl ImageAnalyzer for MaxAnalyzer {
        const ID: &'static str = "test-max";
        const VERSION: u32 = 1;
        type Output = u32;
        fn input(&self) -> InputSpec {
            InputSpec::gray(64)
        }
        fn analyze(&self, i: &AnalysisInput) -> Option<u32> {
            i.gray.as_ref()?.data.iter().max().map(|&v| v as u32)
        }
    }

    #[test]
    fn a_pair_shares_the_cache_rows_of_its_parts() {
        let t = tempfile::tempdir().unwrap();
        make_tree(t.path(), 5);
        let idx = Arc::new(PreviewIndex::in_memory().unwrap());
        let a = Arc::new(MeanAnalyzer { calls: AtomicUsize::new(0) });
        run(t.path(), Arc::clone(&a), &idx, &Ctx::new(None), None);
        struct Outputs(Vec<(u32, u32)>);
        impl ResultSink<(u32, u32)> for Outputs {
            fn accept(&mut self, _: &Path, o: &(u32, u32)) {
                self.0.push(*o);
            }
            fn failed(&mut self, _: &Path) {}
            fn finish(self, s: &PipelineSummary) -> serde_json::Value {
                serde_json::json!({ "outputs": self.0, "summary": s })
            }
        }
        let pair = || {
            let p = Arc::new(crate::pipeline::Pair(MeanAnalyzer { calls: AtomicUsize::new(0) }, MaxAnalyzer));
            let w = TreeWalker::new(t.path(), WalkOptions::default());
            let mut cache = AnalysisCache::new(Arc::clone(&idx));
            run_pipeline(w, p, Outputs(Vec::new()), &mut cache, &Ctx::new(None), 3)
        };
        // The first pair run analyzes again: the rows of MaxAnalyzer are missing.
        let first = pair();
        assert_eq!(first["summary"]["analyzed"], 5);
        let second = pair();
        assert_eq!(second["summary"]["cached"], 5);
        assert_eq!(first["outputs"], second["outputs"]);
        // The single analyzer still finds its rows.
        let r = run(t.path(), Arc::clone(&a), &idx, &Ctx::new(None), None);
        assert_eq!(r["summary"]["cached"], 5);
    }

    #[test]
    fn a_changed_file_is_analyzed_again() {
        let t = tempfile::tempdir().unwrap();
        make_tree(t.path(), 3);
        let idx = Arc::new(PreviewIndex::in_memory().unwrap());
        let a = Arc::new(MeanAnalyzer { calls: AtomicUsize::new(0) });
        run(t.path(), Arc::clone(&a), &idx, &Ctx::new(None), None);
        image::GrayImage::from_pixel(30, 30, image::Luma([1])).save(t.path().join("d0/img1.png")).unwrap();
        let r = run(t.path(), Arc::clone(&a), &idx, &Ctx::new(None), None);
        assert_eq!(r["summary"]["analyzed"], 1);
        assert_eq!(r["summary"]["cached"], 2);
    }

    #[test]
    fn cancel_stops_the_walk_and_cap_counts_failed_images() {
        let t = tempfile::tempdir().unwrap();
        make_tree(t.path(), 40);
        let idx = Arc::new(PreviewIndex::in_memory().unwrap());
        let a = Arc::new(MeanAnalyzer { calls: AtomicUsize::new(0) });
        let ctx = Ctx::new(Some(5));
        let r = run(t.path(), Arc::clone(&a), &idx, &ctx, None);
        assert_eq!(r["summary"]["cancelled"], true);
        // At most the jobs in flight finish after the cancel.
        let scanned = r["summary"]["scanned"].as_u64().unwrap();
        assert!(scanned < 12, "{scanned}");

        let r = run(t.path(), Arc::clone(&a), &idx, &Ctx::new(None), Some(10));
        assert_eq!(r["summary"]["scanned"], 10);
        assert_eq!(r["summary"]["failed"], 1);
        assert_eq!(r["summary"]["capped"], true);
    }
}
