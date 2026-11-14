//! Collection commands: the in-memory virtual folders (QUAL-19..QUAL-22).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::State;

use super::{blocking, CmdError, CmdResult, DirListing};
use crate::collections::CollectionSummary;
use crate::scan::{self, Entry, SortKey, SortSpec};
use crate::AppState;

#[tauri::command]
pub fn list_collections(state: State<'_, AppState>) -> Vec<CollectionSummary> {
    state.collections.list()
}

/// The images of a collection as a listing, with `Entry.p` and `Entry.b`. Removes paths that no
/// longer exist.
#[tauri::command]
pub async fn list_collection(state: State<'_, AppState>, id: String, sort: SortSpec) -> CmdResult<DirListing> {
    let store = Arc::clone(&state.collections);
    let index = Arc::clone(&state.previews.index);
    blocking(move || {
        let c = store
            .prune(&id)
            .ok_or_else(|| CmdError { kind: "notFound", message: "The collection no longer exists.".into() })?;
        let mut entries: Vec<Entry> = c
            .items
            .into_iter()
            .filter_map(|it| {
                let p = Path::new(&it.path);
                let md = std::fs::metadata(p).ok()?;
                Some(Entry {
                    n: p.file_name()?.to_string_lossy().into_owned(),
                    d: false,
                    s: md.len(),
                    m: scan::mtime_ms(&md),
                    t: None,
                    b: (!it.badges.is_empty()).then_some(it.badges),
                    p: Some(it.path),
                })
            })
            .collect();
        if sort.key == SortKey::Taken {
            // The date taken cache is per folder.
            let mut by_dir: HashMap<PathBuf, Vec<Entry>> = HashMap::new();
            for e in entries.drain(..) {
                let dir = Path::new(e.p.as_deref().unwrap_or("/")).parent().unwrap_or(Path::new("/")).to_path_buf();
                by_dir.entry(dir).or_default().push(e);
            }
            for (dir, mut group) in by_dir {
                scan::fill_taken(&dir, &mut group, &index);
                entries.extend(group);
            }
        }
        scan::sort_entries(&mut entries, sort);
        Ok(DirListing { path: format!("collection:{id}"), entries })
    })
    .await?
}

/// Rejected with `busy` while a task runs (D6): a running scan may write the collection.
#[tauri::command]
pub fn remove_collection(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.tasks.guard()?;
    state.collections.remove(&id);
    Ok(())
}
