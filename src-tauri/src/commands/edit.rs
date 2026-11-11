//! EditModeState and the commands that change files (EDIT-01..EDIT-07, FILE-01..FILE-08).

use std::collections::HashMap;
use std::path::PathBuf;

use objc2_foundation::{NSFileManager, NSString, NSURL};
use parking_lot::Mutex;
use serde::Serialize;

/// EDIT-04: edit mode state for each window. A window that is not in the map is in read-only mode,
/// so edit mode is off at each app start (EDIT-02).
#[derive(Default)]
pub struct EditModeState {
    on: Mutex<HashMap<String, bool>>,
    /// Last move to Trash per window: (original path, path in the Trash). FILE-05.
    undo: Mutex<HashMap<String, Vec<(PathBuf, PathBuf)>>>,
}

pub const EDIT_OFF: &str = "Edit mode is off. Turn on edit mode (⌘E) to change files.";

#[derive(Serialize, Debug, Default)]
pub struct TrashFailure {
    pub path: String,
    pub error: String,
}

#[derive(Serialize, Debug, Default)]
pub struct TrashResult {
    pub trashed: Vec<String>,
    pub failed: Vec<TrashFailure>,
}

impl EditModeState {
    pub fn is_on(&self, window: &str) -> bool {
        self.on.lock().get(window).copied().unwrap_or(false)
    }

    pub fn set(&self, window: &str, on: bool) {
        self.on.lock().insert(window.to_string(), on);
    }

    pub fn forget(&self, window: &str) {
        self.on.lock().remove(window);
        self.undo.lock().remove(window);
    }

    /// EDIT-05: rejects the request if edit mode is off.
    pub fn trash(&self, window: &str, paths: &[String]) -> Result<TrashResult, String> {
        if !self.is_on(window) {
            return Err(EDIT_OFF.into());
        }
        let mut res = TrashResult::default();
        let mut undo = Vec::new();
        for p in paths {
            match trash_item(p) {
                Ok(dest) => {
                    res.trashed.push(p.clone());
                    if let Some(d) = dest {
                        undo.push((PathBuf::from(p), d));
                    }
                }
                Err(e) => res.failed.push(TrashFailure { path: p.clone(), error: e }),
            }
        }
        if !undo.is_empty() {
            self.undo.lock().insert(window.to_string(), undo);
        }
        Ok(res)
    }

    /// FILE-05: moves the items of the last move to Trash back to their original location.
    pub fn undo(&self, window: &str) -> Result<Vec<String>, String> {
        if !self.is_on(window) {
            return Err(EDIT_OFF.into());
        }
        let Some(items) = self.undo.lock().remove(window) else {
            return Ok(Vec::new());
        };
        let mut restored = Vec::new();
        let mut errors = Vec::new();
        for (orig, trashed) in items {
            if orig.exists() {
                errors.push(format!("{} already exists", orig.display()));
                continue;
            }
            match std::fs::rename(&trashed, &orig) {
                Ok(()) => restored.push(orig.to_string_lossy().into_owned()),
                Err(e) => errors.push(format!("{}: {e}", orig.display())),
            }
        }
        if errors.is_empty() {
            Ok(restored)
        } else {
            Err(format!("Could not restore: {}", errors.join("; ")))
        }
    }
}

/// FILE-03: NSFileManager.trashItem, so that Finder "Put Back" works.
fn trash_item(path: &str) -> Result<Option<PathBuf>, String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let fm = NSFileManager::defaultManager();
    let mut out: Option<objc2::rc::Retained<NSURL>> = None;
    fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut out))
        .map_err(|e| e.localizedDescription().to_string())?;
    Ok(out.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_rejected_when_edit_mode_off() {
        let st = EditModeState::default();
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("a.jpg");
        std::fs::write(&f, b"x").unwrap();
        let r = st.trash("main", &[f.to_string_lossy().into_owned()]);
        assert_eq!(r.unwrap_err(), EDIT_OFF);
        assert!(f.exists());
        assert!(st.undo("main").is_err());
    }
}
