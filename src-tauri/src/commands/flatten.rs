//! Flatten commands: the photos of a folder tree in 1 grid (FLAT-01..FLAT-16, D1..D5).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow};

use super::{blocking, is_ignored_path, CmdError, CmdResult, DirListing};
use crate::flatten::{FlatEvents, FlatStatus, WalkConfig, FLAT_PREFIX};
use crate::scan::arrange::{self, GroupKey};
use crate::scan::{Entry, SortSpec};
use crate::AppState;

/// Sends `flat-progress` to the window of the session, and records the location in Recents.
struct WindowEvents(AppHandle);

impl FlatEvents for WindowEvents {
    fn progress(&self, window: &str, status: &FlatStatus) {
        let _ = self.0.emit_to(EventTarget::webview_window(window), "flat-progress", status);
    }

    /// FLAT-15: like a disk folder, a flat location goes to Recents only when its tree has photos.
    fn first_photo(&self, root: &Path) {
        let app = self.0.clone();
        let key = format!("{FLAT_PREFIX}{}", root.to_string_lossy());
        crate::pools::background().spawn(move || {
            if let Some(s) = app.state::<AppState>().settings.add_recent(&key) {
                let _ = app.emit("settings-changed", &s);
            }
        });
    }
}

fn not_found() -> CmdError {
    CmdError { kind: "notFound", message: "The flattened folder is no longer open.".into() }
}

/// Starts the walk of `root` for the window and watches the root (FLAT-03, FLAT-14). A new start
/// cancels the previous session of the window; a start of the same root after a finished walk is a
/// refresh. Rejected on `/` (D3) and in an ignored tree (D6).
#[tauri::command]
pub async fn start_flatten(window: WebviewWindow, state: State<'_, AppState>, root: String) -> CmdResult<FlatStatus> {
    let root = if root.len() > 1 { root.trim_end_matches('/').to_string() } else { root };
    if root == "/" {
        return Err(CmdError { kind: "other", message: "Flatten a folder inside the disk.".into() });
    }
    let ignore = state.ignore_set();
    let p = PathBuf::from(&root);
    blocking({
        let (p, ignore) = (p.clone(), ignore.clone());
        move || {
            if is_ignored_path(&ignore, &p) {
                return Err(CmdError::ignored());
            }
            match std::fs::metadata(&p) {
                Ok(m) if m.is_dir() => Ok(()),
                Ok(_) => Err(CmdError { kind: "notDir", message: "A flatten needs a folder.".into() }),
                Err(e) => Err(CmdError::io(&p, &e)),
            }
        }
    })
    .await??;
    let cap = state.settings.get().flatten_cap as usize;
    let app = window.app_handle().clone();
    let status = state.flats.start(window.label(), &p, WalkConfig { cap, ignore }, Arc::new(WindowEvents(app.clone())));
    super::watch(&app, &state, window.label(), &p);
    // NAV-11: the flat location becomes the last folder; Recents waits for the first photo.
    let key = format!("{FLAT_PREFIX}{root}");
    crate::pools::background().spawn(move || {
        app.state::<AppState>().settings.visit(&key, false);
    });
    Ok(status)
}

/// The photos found so far, sorted and grouped like a collection, each with `Entry.p` (D4).
#[tauri::command]
pub async fn list_flat(
    state: State<'_, AppState>,
    id: String,
    sort: SortSpec,
    group: Option<GroupKey>,
) -> CmdResult<DirListing> {
    let th = crate::quality::Thresholds::for_sensitivity(state.settings.get().quality_sensitivity);
    let index = Arc::clone(&state.previews.index);
    let (root, items) = state.flats.snapshot(&id).ok_or_else(not_found)?;
    blocking(move || {
        let listing = arrange::list_paths(entries_of(items), sort, group.unwrap_or_default(), &index, &th, Some(&root));
        DirListing::new(format!("{FLAT_PREFIX}{}", root.to_string_lossy()), listing)
    })
    .await
}

fn entries_of(items: Vec<crate::pipeline::WalkItem>) -> Vec<Entry> {
    items
        .into_iter()
        .map(|it| Entry {
            n: it.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            d: false,
            s: it.size,
            m: it.mtime,
            c: it.created,
            p: Some(it.path.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .collect()
}

/// "Show More" (FLAT-09): continues a capped walk for `flatten_cap` more photos.
#[tauri::command]
pub fn continue_flatten(state: State<'_, AppState>, id: String) -> CmdResult<FlatStatus> {
    let more = state.settings.get().flatten_cap as usize;
    state.flats.continue_walk(&id, more).ok_or_else(not_found)
}

/// Stops the walk (FLAT-06, FLAT-07). The photos found so far stay in the listing.
#[tauri::command]
pub fn cancel_flatten(state: State<'_, AppState>, id: String) -> CmdResult<FlatStatus> {
    state.flats.cancel(&id).ok_or_else(not_found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flatten::FlatStore;
    use crate::quality::Thresholds;
    use crate::scan::index::PreviewIndex;
    use crate::scan::SortKey;

    struct Quiet;
    impl FlatEvents for Quiet {
        fn progress(&self, _: &str, _: &FlatStatus) {}
    }

    #[test]
    fn list_flat_sets_entry_path() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        for p in ["DCIM/100CANON/IMG_0001.JPG", "DCIM/101CANON/IMG_0001.JPG", "top.png"] {
            std::fs::create_dir_all(r.join(p).parent().unwrap()).unwrap();
            std::fs::write(r.join(p), b"x").unwrap();
        }
        let store = FlatStore::new(Arc::new(|f| f()));
        let st = store.start("main", r, WalkConfig { cap: 100, ignore: Default::default() }, Arc::new(Quiet));
        let (root, items) = store.snapshot(&st.id).unwrap();
        let index = Arc::new(PreviewIndex::in_memory().unwrap());
        let name = SortSpec { key: SortKey::Name, desc: false };
        let l = arrange::list_paths(entries_of(items), name, GroupKey::Folder, &index, &Thresholds::default(), Some(&root));
        let paths: Vec<_> = l.entries.iter().map(|e| (e.n.as_str(), e.p.clone().unwrap())).collect();
        let at = |s: &str| r.join(s).to_string_lossy().into_owned();
        assert_eq!(
            paths,
            [
                ("top.png", at("top.png")),
                ("IMG_0001.JPG", at("DCIM/100CANON/IMG_0001.JPG")),
                ("IMG_0001.JPG", at("DCIM/101CANON/IMG_0001.JPG")),
            ]
        );
        assert!(l.entries.iter().all(|e| !e.d && e.s == 1));
        let root_name = r.file_name().unwrap().to_string_lossy().into_owned();
        let groups: Vec<_> = l.groups.iter().map(|g| g.label.clone()).collect();
        assert_eq!(groups, [root_name, "DCIM/100CANON".into(), "DCIM/101CANON".into()]);
    }
}
