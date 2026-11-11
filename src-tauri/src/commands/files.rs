//! File operations that do not change files (FILE-09..FILE-14).

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSPasteboard, NSPasteboardWriting, NSScreen, NSWorkspace};
use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
use serde::Serialize;

fn file_url(p: &str) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(p))
}

/// FILE-09: copies files to the general pasteboard as file references.
pub fn copy_files(paths: &[String]) -> bool {
    let objs: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
        paths.iter().map(|p| ProtocolObject::from_retained(file_url(p))).collect();
    let arr = NSArray::from_retained_slice(&objs);
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    pb.writeObjects(&arr)
}

/// FILE-12: reveals files in Finder.
pub fn reveal(paths: &[String]) {
    let urls: Vec<Retained<NSURL>> = paths.iter().map(|p| file_url(p)).collect();
    let arr = NSArray::from_retained_slice(&urls);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&arr);
}

#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub default: bool,
}

/// FILE-13: applications that can open the file, the default application first.
pub fn open_with_apps(path: &str) -> Vec<AppInfo> {
    let ws = NSWorkspace::sharedWorkspace();
    let url = file_url(path);
    let default = ws.URLForApplicationToOpenURL(&url).and_then(|u| u.path()).map(|p| p.to_string());
    let apps = ws.URLsForApplicationsToOpenURL(&url);
    let mut out: Vec<AppInfo> = apps
        .iter()
        .filter_map(|u| u.path())
        .map(|p| p.to_string())
        .filter(|p| !p.contains("/Fast Gallery.app"))
        .map(|p| AppInfo {
            name: Path::new(&p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            default: default.as_deref() == Some(p.as_str()),
            path: p,
        })
        .collect();
    out.sort_by(|a, b| b.default.cmp(&a.default).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

pub fn open_with(path: &str, app: Option<&str>) -> Result<(), String> {
    let mut cmd = std::process::Command::new("/usr/bin/open");
    if let Some(a) = app {
        cmd.arg("-a").arg(a);
    }
    cmd.arg("--").arg(path);
    let st = cmd.status().map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("Could not open {path}"))
    }
}

/// FILE-14: sets the image as the desktop picture of each screen. Main thread only.
pub fn set_desktop_picture(mtm: MainThreadMarker, path: &str) -> Result<(), String> {
    let ws = NSWorkspace::sharedWorkspace();
    let url = file_url(path);
    let opts = NSDictionary::new();
    for screen in NSScreen::screens(mtm).iter() {
        unsafe { ws.setDesktopImageURL_forScreen_options_error(&url, &screen, &opts) }
            .map_err(|e| e.localizedDescription().to_string())?;
    }
    Ok(())
}

/// SEC-05: opens System Settings at Full Disk Access. Called only after a read fails with a
/// permission error.
pub fn open_privacy_settings() {
    let _ = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .status();
}

/// FILE-10, FILE-11: starts a native drag of files out of the window, with the copy operation.
pub fn start_drag<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, paths: Vec<PathBuf>, icon: drag::Image) -> Result<(), String> {
    drag::start_drag(
        window,
        drag::DragItem::Files(paths),
        icon,
        |_r, _p| {},
        drag::Options { skip_animatation_on_cancel_or_failure: false, mode: drag::DragMode::Copy },
    )
    .map_err(|e| e.to_string())
}
