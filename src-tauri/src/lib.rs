//! Fast Gallery: a macOS image browser with recursive folder previews.

pub mod commands;
pub mod decode;
pub mod pools;
pub mod protocols;
pub mod scan;
pub mod thumbs;
pub mod watch;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, RunEvent, TitleBarStyle, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use commands::edit::EditModeState;
use commands::settings::SettingsStore;
use scan::index::PreviewIndex;
use scan::preview::FolderPreviewService;
use thumbs::{ThumbCache, ThumbnailService};
use watch::FsWatcher;

/// Debug builds only: a test hook. `FG_DEV_SCRIPT=<file>` evaluates the file in the main window
/// each time it changes; the script reports with `invoke("dev_log", {msg})`. The permission for
/// `dev_log` is granted at runtime, so release builds do not allow it.
#[cfg(debug_assertions)]
mod dev {
    use tauri::ipc::CapabilityBuilder;
    use tauri::{AppHandle, Manager};

    #[tauri::command]
    pub fn dev_log(msg: String) {
        println!("[js] {msg}");
    }

    pub fn setup(app: &AppHandle) -> tauri::Result<()> {
        app.add_capability(CapabilityBuilder::new("dev").windows(["*"]).permission("allow-dev-log"))?;
        let Ok(script) = std::env::var("FG_DEV_SCRIPT") else { return Ok(()) };
        let app = app.clone();
        std::thread::spawn(move || {
            let mut last = None;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let m = std::fs::metadata(&script).and_then(|m| m.modified()).ok();
                if m.is_some() && m != last {
                    last = m;
                    if let (Ok(js), Some(w)) = (std::fs::read_to_string(&script), app.get_webview_window("main")) {
                        let _ = w.eval(&js);
                    }
                }
            }
        });
        Ok(())
    }
}

pub struct AppState {
    pub previews: Arc<FolderPreviewService>,
    pub thumbs: Arc<ThumbnailService>,
    pub watcher: FsWatcher,
    pub edit: EditModeState,
    pub settings: SettingsStore,
    pub pending_open: Mutex<Option<String>>,
}

static WINDOW_SEQ: AtomicU32 = AtomicU32::new(1);

/// `FG_TRACE_STARTUP=1` prints start-up milestones (PERF-02 profiling).
pub(crate) fn trace(what: &str) {
    static T0: std::sync::OnceLock<Option<std::time::Instant>> = std::sync::OnceLock::new();
    if let Some(t0) = T0.get_or_init(|| std::env::var_os("FG_TRACE_STARTUP").map(|_| std::time::Instant::now())) {
        eprintln!("[startup] {:>7.1} ms  {what}", t0.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Opens a browser window. The first window has the label "main".
pub fn open_main_window(app: &AppHandle, folder: Option<&str>) -> tauri::Result<WebviewWindow> {
    let label = if app.get_webview_window("main").is_none() {
        "main".to_string()
    } else {
        format!("w{}", WINDOW_SEQ.fetch_add(1, Ordering::Relaxed))
    };
    let url = match folder {
        Some(f) => format!("index.html?folder={}", percent_encoding::utf8_percent_encode(f, percent_encoding::NON_ALPHANUMERIC)),
        None => "index.html".to_string(),
    };
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Fast Gallery")
        .inner_size(1200.0, 800.0)
        .min_inner_size(640.0, 420.0)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .visible(false);
    if let Some(w) = app.webview_windows().values().find(|w| w.label() != "settings") {
        if let Ok(pos) = w.outer_position() {
            let scale = w.scale_factor().unwrap_or(1.0);
            let p = pos.to_logical::<f64>(scale);
            b = b.position(p.x + 24.0, p.y + 24.0);
        }
    }
    b.build()
}

pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") {
        return w.set_focus();
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html#settings".into()))
        .title("Settings")
        .inner_size(520.0, 520.0)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .visible(false)
        .build()?;
    Ok(())
}

fn focused_browser(app: &AppHandle) -> Option<WebviewWindow> {
    let ws = app.webview_windows();
    ws.values()
        .find(|w| w.label() != "settings" && w.is_focused().unwrap_or(false))
        .or_else(|| ws.get("main"))
        .or_else(|| ws.values().find(|w| w.label() != "settings"))
        .cloned()
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let app_menu = Submenu::with_items(
        app,
        "Fast Gallery",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Fast Gallery"), None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &MenuItem::with_id(app, "new_window", "New Window", true, Some("CmdOrCtrl+N"))?,
            &MenuItem::with_id(app, "open_folder", "Open Folder…", true, Some("CmdOrCtrl+O"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(app, "View", true, &[&PredefinedMenuItem::fullscreen(app, None)?])?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[&PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::maximize(app, None)?],
    )?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window])
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "settings" => {
            let _ = open_settings_window(app);
        }
        "new_window" => {
            let _ = open_main_window(app, None);
        }
        "open_folder" => match focused_browser(app) {
            Some(w) => {
                let _ = w.emit_to(tauri::EventTarget::webview_window(w.label()), "menu-open-folder", ());
            }
            None => {
                let _ = open_main_window(app, None);
            }
        },
        _ => {}
    }
}

/// PLAT-06: a file or folder opened from Finder ("Open With").
fn on_opened(app: &AppHandle, urls: Vec<tauri::Url>) {
    let Some(path) = urls.into_iter().find_map(|u| u.to_file_path().ok()) else { return };
    let path = path.to_string_lossy().into_owned();
    match focused_browser(app) {
        Some(w) => {
            let _ = w.emit_to(tauri::EventTarget::webview_window(w.label()), "open-path", &path);
            let _ = w.set_focus();
        }
        None => *app.state::<AppState>().pending_open.lock() = Some(path),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    trace("main");
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("thumb", protocols::thumb)
        .register_asynchronous_uri_scheme_protocol("image", protocols::image)
        .on_page_load(|_, payload| trace(&format!("page load {:?}", payload.event())))
        .setup(|app| {
            trace("setup");
            let handle = app.handle();
            let cache_dir = app.path().app_cache_dir()?;
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&cache_dir)?;
            let settings = SettingsStore::load(config_dir.join("settings.json"));
            let s = settings.get();
            let index = Arc::new(
                PreviewIndex::open(&cache_dir.join("index.sqlite")).or_else(|_| PreviewIndex::in_memory())?,
            );
            let cache = ThumbCache::new(cache_dir.join("thumbs"), s.cache_limit_mb * 1024 * 1024);
            cache.start_accounting();
            app.manage(AppState {
                previews: Arc::new(FolderPreviewService::new(index)),
                thumbs: Arc::new(ThumbnailService::new(cache)),
                watcher: FsWatcher::default(),
                edit: EditModeState::default(),
                settings,
                pending_open: Mutex::new(None),
            });
            app.set_menu(build_menu(handle)?)?;
            app.on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()));
            trace("menu set");
            trace("state ready");
            open_main_window(handle, None)?;
            trace("window created");
            #[cfg(debug_assertions)]
            dev::setup(handle)?;
            Ok(())
        })
        .on_window_event(|window, ev| {
            if let WindowEvent::Destroyed = ev {
                let st = window.state::<AppState>();
                st.watcher.unwatch(window.label());
                st.edit.forget(window.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::get_folder_previews,
            commands::refresh_folder_previews,
            commands::get_image_info,
            commands::list_volumes,
            commands::path_kind,
            commands::get_settings,
            commands::update_settings,
            commands::get_cache_info,
            commands::clear_cache,
            commands::set_edit_mode,
            commands::get_edit_mode,
            commands::trash_files,
            commands::undo_trash,
            commands::copy_files,
            commands::reveal_in_finder,
            commands::open_with_apps,
            commands::open_with,
            commands::set_desktop_picture,
            commands::open_privacy_settings,
            commands::start_drag,
            commands::pick_folder,
            commands::new_window,
            commands::open_settings,
            commands::take_pending_open,
            commands::toggle_favorite,
            commands::set_folder_prefs,
            commands::home_dir,
            #[cfg(debug_assertions)]
            dev::dev_log,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Fast Gallery");

    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => on_opened(app, urls),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { has_visible_windows: false, .. } => {
            let _ = open_main_window(app, None);
        }
        _ => {}
    });
}
