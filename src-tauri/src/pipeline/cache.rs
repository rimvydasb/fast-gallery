//! AnalysisCache: results of all analyzers in the `analysis` table (D3). A row is stale when its
//! mtime, size, or analyzer version differs (QUAL-17).

use std::path::Path;
use std::sync::Arc;

use crate::scan::index::{AnalysisRow, PreviewIndex};

use super::analyzer::ImageAnalyzer;

/// Rows per write transaction.
pub const BATCH: usize = 32;

pub struct AnalysisCache {
    index: Arc<PreviewIndex>,
    pending: Vec<(String, String, &'static str, AnalysisRow)>,
}

fn split(path: &Path) -> Option<(String, String)> {
    Some((path.parent()?.to_str()?.to_string(), path.file_name()?.to_str()?.to_string()))
}

impl AnalysisCache {
    pub fn new(index: Arc<PreviewIndex>) -> Self {
        Self { index, pending: Vec::new() }
    }

    /// The cached output, if the row is not stale.
    pub fn get<A: ImageAnalyzer>(&self, path: &Path, mtime: i64, size: u64) -> Option<A::Output> {
        let (dir, name) = split(path)?;
        let row = self.index.analysis_get(&dir, &name, A::ID)?;
        if row.version != A::VERSION || row.mtime != mtime || row.size != size as i64 {
            return None;
        }
        serde_json::from_str(&row.data).ok()
    }

    /// Queues a row; writes a batch of `BATCH` rows.
    pub fn put<A: ImageAnalyzer>(&mut self, path: &Path, mtime: i64, size: u64, out: &A::Output) {
        let (Some((dir, name)), Ok(data)) = (split(path), serde_json::to_string(out)) else { return };
        self.pending.push((dir, name, A::ID, AnalysisRow { version: A::VERSION, mtime, size: size as i64, data }));
        if self.pending.len() >= BATCH {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        self.index.analysis_put_many(&self.pending);
        self.pending.clear();
    }
}

impl Drop for AnalysisCache {
    fn drop(&mut self) {
        self.flush();
    }
}
