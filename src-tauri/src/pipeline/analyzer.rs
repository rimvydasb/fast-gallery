//! The 2 extension points of the pipeline: what to compute for each image, and what to do with it.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::cache::AnalysisCache;
use super::input::{AnalysisInput, InputSpec};
use super::run::PipelineSummary;

pub trait ImageAnalyzer: Send + Sync + 'static {
    /// Cache key part, for example "quality".
    const ID: &'static str;
    /// A change makes the cached rows of this analyzer stale.
    const VERSION: u32;
    type Output: Serialize + DeserializeOwned + Send + 'static;
    /// The decoded input this analyzer needs.
    fn input(&self) -> InputSpec;
    /// None: the image cannot be analyzed.
    fn analyze(&self, input: &AnalysisInput) -> Option<Self::Output>;

    /// The cached output, if not stale. Only a composite analyzer such as `Pair` overrides this.
    fn cache_get(cache: &AnalysisCache, path: &Path, mtime: i64, size: u64) -> Option<Self::Output>
    where
        Self: Sized,
    {
        cache.get_row::<Self>(path, mtime, size)
    }

    /// Queues the output for the cache. Only a composite analyzer such as `Pair` overrides this.
    fn cache_put(cache: &mut AnalysisCache, path: &Path, mtime: i64, size: u64, out: &Self::Output)
    where
        Self: Sized,
    {
        cache.put_row::<Self>(path, mtime, size, out)
    }
}

/// Runs 2 analyzers on 1 decode of the merged `InputSpec`. The cache keeps 1 row for each part,
/// under the part's own `ID` and `VERSION`, so a single analyzer reuses the rows of a pair run and
/// the other way round. A pair is a cache hit only if both parts are.
pub struct Pair<A, B>(pub A, pub B);

impl<A: ImageAnalyzer, B: ImageAnalyzer> ImageAnalyzer for Pair<A, B> {
    /// Not used: `cache_get` and `cache_put` store the parts under their own IDs.
    const ID: &'static str = "";
    const VERSION: u32 = 0;
    type Output = (A::Output, B::Output);

    fn input(&self) -> InputSpec {
        self.0.input().merge(self.1.input())
    }

    fn analyze(&self, input: &AnalysisInput) -> Option<Self::Output> {
        Some((self.0.analyze(input)?, self.1.analyze(input)?))
    }

    fn cache_get(cache: &AnalysisCache, path: &Path, mtime: i64, size: u64) -> Option<Self::Output> {
        Some((A::cache_get(cache, path, mtime, size)?, B::cache_get(cache, path, mtime, size)?))
    }

    fn cache_put(cache: &mut AnalysisCache, path: &Path, mtime: i64, size: u64, out: &Self::Output) {
        A::cache_put(cache, path, mtime, size, &out.0);
        B::cache_put(cache, path, mtime, size, &out.1);
    }
}

pub trait ResultSink<O>: Send {
    /// Called in walk order.
    fn accept(&mut self, path: &Path, output: &O);
    /// Decode or analysis failed.
    fn failed(&mut self, path: &Path);
    /// Becomes `TaskDone.result`.
    fn finish(self, summary: &PipelineSummary) -> serde_json::Value;
}
