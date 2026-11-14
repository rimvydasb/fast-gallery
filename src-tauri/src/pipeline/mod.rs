//! Image scan pipeline (D2): walks a folder tree and runs an `ImageAnalyzer` on each image, with a
//! persistent cache, a bounded number of decode jobs, cancel, and progress. It contains no code for a
//! specific analyzer (QUAL-14).

pub mod analyzer;
pub mod cache;
pub mod input;
pub mod run;
pub mod walk;

pub use analyzer::{ImageAnalyzer, Pair, ResultSink};
pub use cache::AnalysisCache;
pub use input::{decode_input, decode_input_for, AnalysisInput, InputSpec, Plane};
pub use run::{max_in_flight, run_pipeline, PipelineSummary, Progress};
pub use walk::{ImageSource, ListSource, TreeWalker, WalkItem, WalkOptions};
