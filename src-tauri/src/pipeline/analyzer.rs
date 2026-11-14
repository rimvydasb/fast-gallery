//! The 2 extension points of the pipeline: what to compute for each image, and what to do with it.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

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
}

pub trait ResultSink<O>: Send {
    /// Called in walk order.
    fn accept(&mut self, path: &Path, output: &O);
    /// Decode or analysis failed.
    fn failed(&mut self, path: &Path);
    /// Becomes `TaskDone.result`.
    fn finish(self, summary: &PipelineSummary) -> serde_json::Value;
}
