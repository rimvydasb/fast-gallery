//! Collection commands: the in-memory virtual folders (QUAL-19..QUAL-22).

use std::path::Path;
use std::sync::Arc;

use tauri::State;

use super::{blocking, CmdError, CmdResult, DirListing};
use crate::collections::CollectionSummary;
use crate::scan::arrange::{self, GroupKey};
use crate::scan::{self, Entry, SortSpec};
use crate::AppState;

#[tauri::command]
pub fn list_collections(state: State<'_, AppState>) -> Vec<CollectionSummary> {
    state.collections.list()
}

/// The images of a collection as a listing, with `Entry.p` and `Entry.b`. Removes paths that no
/// longer exist.
#[tauri::command]
pub async fn list_collection(
    state: State<'_, AppState>,
    id: String,
    sort: SortSpec,
    group: Option<GroupKey>,
) -> CmdResult<DirListing> {
    let th = crate::quality::Thresholds::for_sensitivity(state.settings.get().quality_sensitivity);
    let store = Arc::clone(&state.collections);
    let index = Arc::clone(&state.previews.index);
    blocking(move || {
        let c = store
            .prune(&id)
            .ok_or_else(|| CmdError { kind: "notFound", message: "The collection no longer exists.".into() })?;
        let entries: Vec<Entry> = c
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
                    c: scan::created_ms(&md),
                })
            })
            .collect();
        let listing = arrange::list_paths(entries, sort, group.unwrap_or_default(), &index, &th);
        Ok(DirListing::new(format!("collection:{id}"), listing))
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
