/// App commands. With an app manifest, each command must be allowed in a capability (SEC-02).
const COMMANDS: &[&str] = &[
    "list_dir",
    "get_folder_previews",
    "refresh_folder_previews",
    "get_image_info",
    "list_volumes",
    "path_kind",
    "get_settings",
    "update_settings",
    "get_cache_info",
    "clear_cache",
    "set_edit_mode",
    "get_edit_mode",
    "trash_files",
    "undo_trash",
    "copy_files",
    "reveal_in_finder",
    "open_with_apps",
    "open_with",
    "open_privacy_settings",
    "start_drag",
    "pick_folder",
    "new_window",
    "open_settings",
    "take_pending_open",
    "toggle_favorite",
    "set_folder_prefs",
    "home_dir",
    // Debug builds only; granted at runtime (see lib.rs, mod dev).
    "dev_log",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
