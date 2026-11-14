//! Tauri command handlers. Commands that touch the file system are `async` and run their work off
//! the main thread (PERF-11).

pub mod edit;
pub mod files;
pub mod settings;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::scan::{self, preview, preview::FolderPreview, Entry, SortSpec};
use crate::AppState;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CmdError {
    /// "permission", "notFound", "notDir", "editOff", or "other".
    pub kind: &'static str,
    pub message: String,
}

impl CmdError {
    fn other(m: impl ToString) -> Self {
        Self { kind: "other", message: m.to_string() }
    }
    fn io(path: &Path, e: &std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        match e.kind() {
            PermissionDenied => Self {
                kind: "permission",
                message: format!("You do not have permission to read the folder “{name}”."),
            },
            NotFound => Self { kind: "notFound", message: format!("The folder “{name}” does not exist.") },
            NotADirectory => Self { kind: "notDir", message: format!("“{name}” is not a folder.") },
            _ => Self::other(format!("Could not read “{name}”: {e}")),
        }
    }
}

type CmdResult<T> = Result<T, CmdError>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(CmdError::other)
}

#[derive(Serialize)]
pub struct DirListing {
    pub path: String,
    pub entries: Vec<Entry>,
}

/// Lists one directory in the grid order and starts to watch it (FSW-01).
#[tauri::command]
pub async fn list_dir(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    sort: SortSpec,
    show_hidden: bool,
) -> CmdResult<DirListing> {
    let index = Arc::clone(&state.previews.index);
    let p = PathBuf::from(&path);
    let entries = blocking({
        let p = p.clone();
        move || scan::list_dir(&p, sort, show_hidden, &index).map_err(|e| CmdError::io(&p, &e))
    })
    .await??;
    state.watcher.watch(window.app_handle(), window.label(), &p, Arc::clone(&state.thumbs), Arc::clone(&state.previews.index));
    state.settings.visit(&path);
    crate::trace("list_dir done");
    Ok(DirListing { path, entries })
}

fn limits(state: &AppState) -> preview::Limits {
    preview::Limits { depth: state.settings.get().preview_depth as usize, max_entries: preview::MAX_ENTRIES }
}

/// Returns the cached previews at once and resolves all folders in the background, in the given
/// order (visible folder tiles first).
#[tauri::command]
pub async fn get_folder_previews(
    window: WebviewWindow,
    state: State<'_, AppState>,
    folders: Vec<String>,
    sort: SortSpec,
) -> CmdResult<Vec<FolderPreview>> {
    let svc = Arc::clone(&state.previews);
    let l = limits(&state);
    let app = window.app_handle().clone();
    let label = window.label().to_string();
    blocking(move || svc.request(&app, &label, folders, sort, l)).await
}

/// Resolves previews again after a file system change, without a cancel of other jobs.
#[tauri::command]
pub fn refresh_folder_previews(window: WebviewWindow, state: State<'_, AppState>, folders: Vec<String>, sort: SortSpec) {
    let l = limits(&state);
    for f in folders {
        state.previews.refresh(window.app_handle(), window.label(), f, sort, l);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub modified: i64,
    #[serde(flatten)]
    pub meta: crate::decode::ImageMeta,
}

/// INFO-02..INFO-04.
#[tauri::command]
pub async fn get_image_info(path: String) -> CmdResult<ImageInfo> {
    blocking(move || {
        let p = PathBuf::from(&path);
        let md = std::fs::metadata(&p).map_err(|e| CmdError::io(&p, &e))?;
        let modified = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(ImageInfo {
            name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            size: md.len(),
            modified,
            meta: crate::decode::image_meta(&p),
            path,
        })
    })
    .await?
}

#[derive(Serialize)]
pub struct Volume {
    pub name: String,
    pub path: String,
}

/// Mounted volumes (NAV-08).
#[tauri::command]
pub async fn list_volumes() -> CmdResult<Vec<Volume>> {
    blocking(|| {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let p = e.path();
                // The boot volume appears as a link to "/".
                let path = match std::fs::read_link(&p) {
                    Ok(t) if t == Path::new("/") => "/".to_string(),
                    _ => p.to_string_lossy().into_owned(),
                };
                out.push(Volume { name, path });
            }
        }
        out.sort_by(|a, b| (a.path != "/").cmp(&(b.path != "/")).then_with(|| a.name.cmp(&b.name)));
        out
    })
    .await
}

/// "dir", "file", or "missing", for drag and drop onto the window.
#[tauri::command]
pub async fn path_kind(path: String) -> CmdResult<&'static str> {
    blocking(move || match std::fs::metadata(&path) {
        Ok(m) if m.is_dir() => "dir",
        Ok(_) => "file",
        Err(_) => "missing",
    })
    .await
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> settings::Settings {
    crate::trace("get_settings (JS running)");
    state.settings.get()
}

#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<'_, AppState>, patch: serde_json::Value) -> CmdResult<settings::Settings> {
    let old_limit = state.settings.get().cache_limit_mb;
    let s = state.settings.update(patch).map_err(CmdError::other)?;
    if s.cache_limit_mb != old_limit {
        state.thumbs.cache.set_limit(s.cache_limit_mb * 1024 * 1024);
    }
    let _ = app.emit("settings-changed", &s);
    Ok(s)
}

#[derive(Serialize)]
pub struct CacheInfo {
    pub used: u64,
    pub limit: u64,
    pub path: String,
}

#[tauri::command]
pub fn get_cache_info(state: State<'_, AppState>) -> CacheInfo {
    let c = &state.thumbs.cache;
    CacheInfo { used: c.used(), limit: c.limit(), path: c.root().to_string_lossy().into_owned() }
}

/// THM-10: clears the thumbnail cache and the preview index.
#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> CmdResult<()> {
    let thumbs = Arc::clone(&state.thumbs);
    let index = Arc::clone(&state.previews.index);
    blocking(move || {
        thumbs.clear();
        index.clear();
    })
    .await
}

#[tauri::command]
pub fn set_edit_mode(window: WebviewWindow, state: State<'_, AppState>, on: bool) -> bool {
    state.edit.set(window.label(), on);
    on
}

#[tauri::command]
pub fn get_edit_mode(window: WebviewWindow, state: State<'_, AppState>) -> bool {
    state.edit.is_on(window.label())
}

/// FILE-01, FILE-03: moves files to the Trash. Rejected if edit mode is off (EDIT-05).
#[tauri::command]
pub async fn trash_files(window: WebviewWindow, state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<edit::TrashResult> {
    if !state.edit.is_on(window.label()) {
        return Err(CmdError { kind: "editOff", message: edit::EDIT_OFF.into() });
    }
    let app = window.app_handle().clone();
    let label = window.label().to_string();
    let r = blocking(move || app.state::<AppState>().edit.trash(&label, &paths)).await?;
    r.map_err(|m| CmdError { kind: "editOff", message: m })
}

/// FILE-05: undo of the last move to Trash.
#[tauri::command]
pub async fn undo_trash(window: WebviewWindow) -> CmdResult<Vec<String>> {
    let app = window.app_handle().clone();
    let label = window.label().to_string();
    blocking(move || app.state::<AppState>().edit.undo(&label)).await?.map_err(CmdError::other)
}

fn on_main<T: Send + 'static>(app: &AppHandle, f: impl FnOnce() -> T + Send + 'static) -> CmdResult<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(CmdError::other)?;
    rx.recv().map_err(CmdError::other)
}

#[tauri::command]
pub async fn copy_files(app: AppHandle, paths: Vec<String>) -> CmdResult<bool> {
    on_main(&app, move || files::copy_files(&paths))
}

#[tauri::command]
pub async fn reveal_in_finder(app: AppHandle, paths: Vec<String>) -> CmdResult<()> {
    on_main(&app, move || files::reveal(&paths))
}

#[tauri::command]
pub async fn open_with_apps(path: String) -> CmdResult<Vec<files::AppInfo>> {
    blocking(move || files::open_with_apps(&path)).await
}

#[tauri::command]
pub async fn open_with(path: String, app: Option<String>) -> CmdResult<()> {
    blocking(move || files::open_with(&path, app.as_deref())).await?.map_err(CmdError::other)
}

#[tauri::command]
pub fn open_privacy_settings() {
    std::thread::spawn(files::open_privacy_settings);
}

/// Starts a native drag of the files out of the app, with copy semantics.
#[tauri::command]
pub async fn start_drag(window: WebviewWindow, state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<()> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).filter(|p| scan::is_image_path(p)).collect();
    let Some(first) = paths.first() else { return Ok(()) };
    let icon = crate::thumbs::cached_file(&state.thumbs, first).unwrap_or_else(|| first.clone());
    let w = window.clone();
    on_main(window.app_handle(), move || files::start_drag(&w, paths, drag::Image::File(icon)))?.map_err(CmdError::other)
}

/// NAV-01: system folder picker. Returns the folder or null.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, window: WebviewWindow) -> CmdResult<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().set_parent(&window).pick_folder(move |p| {
        let _ = tx.send(p.and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned()));
    });
    blocking(move || rx.recv().ok().flatten()).await
}

#[tauri::command]
pub fn new_window(app: AppHandle) -> CmdResult<()> {
    crate::open_main_window(&app, None).map(|_| ()).map_err(CmdError::other)
}

#[tauri::command]
pub fn open_settings(app: AppHandle) -> CmdResult<()> {
    crate::open_settings_window(&app).map_err(CmdError::other)
}

/// A file or folder that macOS asked the app to open before the window was ready (PLAT-06).
#[tauri::command]
pub fn take_pending_open(window: WebviewWindow, state: State<'_, AppState>) -> Option<String> {
    if window.label() != "main" {
        return None;
    }
    state.pending_open.lock().take()
}

#[tauri::command]
pub fn toggle_favorite(app: AppHandle, state: State<'_, AppState>, path: String) -> settings::Settings {
    let s = state.settings.mutate(|s| {
        if let Some(i) = s.favorites.iter().position(|f| f == &path) {
            s.favorites.remove(i);
        } else {
            s.favorites.push(path);
        }
    });
    let _ = app.emit("settings-changed", &s);
    s
}

#[tauri::command]
pub fn set_folder_prefs(state: State<'_, AppState>, path: String, prefs: settings::FolderPrefs) {
    state.settings.mutate(|s| {
        s.folder_prefs.insert(path, prefs);
    });
}

#[tauri::command]
pub fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}
