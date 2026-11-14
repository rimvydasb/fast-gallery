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
    /// Last move to Trash or rename per window: (original path, current path). FILE-05, FILE-17.
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

    /// FILE-14: renames a file in its folder. Rejected if edit mode is off (EDIT-05). Returns the new path.
    pub fn rename(&self, window: &str, path: &str, new_name: &str) -> Result<String, String> {
        if !self.is_on(window) {
            return Err(EDIT_OFF.into());
        }
        let from = PathBuf::from(path);
        let to = rename_file(&from, new_name)?;
        self.undo.lock().insert(window.to_string(), vec![(from, to.clone())]);
        Ok(to.to_string_lossy().into_owned())
    }

    /// The original paths that `undo` would restore, for the task lock check (D6).
    pub fn undo_targets(&self, window: &str) -> Vec<String> {
        self.undo
            .lock()
            .get(window)
            .map(|v| v.iter().map(|(orig, _)| orig.to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }

    /// FILE-05, FILE-17: moves the items of the last move to Trash or rename back to their original location.
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
            match move_no_overwrite(&trashed, &orig) {
                Ok(()) => restored.push(orig.to_string_lossy().into_owned()),
                Err(e) => errors.push(e),
            }
        }
        if errors.is_empty() {
            Ok(restored)
        } else {
            Err(format!("Could not restore: {}", errors.join("; ")))
        }
    }
}

/// FILE-03: NSFileManager.trashItem, so that Finder "Put Back" works. A volume without a Trash (a
/// network share) gets the fallback in `trash_on_volume` instead.
fn trash_item(path: &str) -> Result<Option<PathBuf>, String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let fm = NSFileManager::defaultManager();
    let mut out: Option<objc2::rc::Retained<NSURL>> = None;
    match fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut out)) {
        Ok(()) => Ok(out.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()))),
        Err(e) => {
            let msg = e.localizedDescription().to_string();
            // NSFeatureUnsupportedError: the volume has no Trash. Other errors (permissions) stay errors.
            if e.code() != NS_FEATURE_UNSUPPORTED {
                return Err(msg);
            }
            match trash_on_volume(std::path::Path::new(path)) {
                Ok(dest) => Ok(Some(dest)),
                Err(fallback) => Err(format!("{msg} (fallback failed: {fallback})")),
            }
        }
    }
}

/// `NSFeatureUnsupportedError` in `NSCocoaErrorDomain`.
const NS_FEATURE_UNSUPPORTED: isize = 3328;

/// Name of the folder at the volume root that receives files from a volume without a Trash.
const VOLUME_TRASH: &str = ".fast-gallery-trash";

/// Mount point of the volume that holds `path`.
fn mount_point(path: &std::path::Path) -> Result<PathBuf, String> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let on = unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) };
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(on.to_bytes())))
}

/// Fallback for a volume without a Trash: moves the file to `<volume root>/.fast-gallery-trash/`. This is a
/// rename on the same volume, so it is fast and undo can move the file back. Never replaces a file.
fn trash_on_volume(path: &std::path::Path) -> Result<PathBuf, String> {
    let root = mount_point(path)?;
    let dir = root.join(VOLUME_TRASH);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path.file_name().ok_or("The path has no file name.")?.to_string_lossy().into_owned();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    for n in 0..1000 {
        let candidate = if n == 0 { name.clone() } else { format!("{stem} ({n}){ext}") };
        let dest = dir.join(candidate);
        if std::fs::symlink_metadata(&dest).is_ok() {
            continue;
        }
        std::fs::rename(path, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        return Ok(dest);
    }
    Err("Too many files with the same name in the volume trash.".into())
}

/// Longest file name that APFS accepts, in bytes.
const MAX_NAME_BYTES: usize = 255;

/// FILE-15: checks a new file name. Returns the reason for a rejection.
pub fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("The name cannot be empty.".into());
    }
    if name == "." || name == ".." {
        return Err(format!("“{name}” is not a valid name."));
    }
    if name.contains(['/', ':', '\0']) {
        return Err("The name cannot contain “/” or “:”.".into());
    }
    if name.len() > MAX_NAME_BYTES {
        return Err("The name is too long.".into());
    }
    Ok(())
}

/// True if both paths are the same file (for example, a rename that only changes the letter case on
/// a case-insensitive volume).
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Moves `src` to `dst`. `std::fs::rename` replaces an existing file, so this refuses to do that.
fn move_no_overwrite(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if std::fs::symlink_metadata(dst).is_ok() && !same_file(src, dst) {
        return Err(format!("{} already exists", dst.display()));
    }
    std::fs::rename(src, dst).map_err(|e| format!("{}: {e}", dst.display()))
}

/// FILE-14, FILE-16: renames a file in its folder. Never replaces another file.
pub fn rename_file(from: &std::path::Path, new_name: &str) -> Result<PathBuf, String> {
    validate_file_name(new_name)?;
    let md = std::fs::symlink_metadata(from).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "The file does not exist.".to_string(),
        _ => e.to_string(),
    })?;
    if md.is_dir() {
        return Err("Only files can be renamed.".into());
    }
    let dir = from.parent().ok_or("The file has no folder.")?;
    let to = dir.join(new_name);
    if to == from {
        return Err("The name did not change.".into());
    }
    move_no_overwrite(from, &to).map_err(|e| {
        if e.ends_with("already exists") {
            format!("A file named “{new_name}” already exists.")
        } else {
            e
        }
    })?;
    Ok(to)
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

    fn setup() -> (EditModeState, tempfile::TempDir) {
        let st = EditModeState::default();
        st.set("main", true);
        (st, tempfile::tempdir().unwrap())
    }

    fn touch(dir: &std::path::Path, name: &str, body: &[u8]) -> String {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn rename_rejected_when_edit_mode_off() {
        let st = EditModeState::default();
        let t = tempfile::tempdir().unwrap();
        let f = touch(t.path(), "a.jpg", b"x");
        assert_eq!(st.rename("main", &f, "b.jpg").unwrap_err(), EDIT_OFF);
        assert!(t.path().join("a.jpg").exists());
        assert!(!t.path().join("b.jpg").exists());
    }

    #[test]
    fn rename_edit_mode_is_per_window() {
        let (st, t) = setup();
        let f = touch(t.path(), "a.jpg", b"x");
        assert_eq!(st.rename("other", &f, "b.jpg").unwrap_err(), EDIT_OFF);
        assert!(st.rename("main", &f, "b.jpg").is_ok());
    }

    #[test]
    fn rename_moves_file_and_keeps_content() {
        let (st, t) = setup();
        let f = touch(t.path(), "a.jpg", b"pixels");
        let to = st.rename("main", &f, "b.jpeg").unwrap();
        assert_eq!(to, t.path().join("b.jpeg").to_string_lossy());
        assert!(!t.path().join("a.jpg").exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"pixels");
    }

    #[test]
    fn rename_allows_unicode_and_spaces() {
        let (st, t) = setup();
        let f = touch(t.path(), "a.jpg", b"x");
        let to = st.rename("main", &f, "Atostogos 2024 – žąsis.jpg").unwrap();
        assert!(std::path::Path::new(&to).exists());
    }

    #[test]
    fn rename_never_overwrites() {
        let (st, t) = setup();
        let a = touch(t.path(), "a.jpg", b"A");
        touch(t.path(), "b.jpg", b"B");
        let e = st.rename("main", &a, "b.jpg").unwrap_err();
        assert!(e.contains("already exists"), "{e}");
        assert_eq!(std::fs::read(t.path().join("a.jpg")).unwrap(), b"A");
        assert_eq!(std::fs::read(t.path().join("b.jpg")).unwrap(), b"B");
    }

    #[test]
    fn rename_rejects_invalid_names() {
        let (st, t) = setup();
        let f = touch(t.path(), "a.jpg", b"x");
        let long = "x".repeat(MAX_NAME_BYTES + 1);
        for bad in ["", "   ", ".", "..", "x/y.jpg", "../x.jpg", "x:y.jpg", "x\0y", long.as_str()] {
            assert!(st.rename("main", &f, bad).is_err(), "{bad:?}");
        }
        assert!(t.path().join("a.jpg").exists());
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 1);
    }

    #[test]
    fn rename_rejects_unchanged_name_missing_file_and_folder() {
        let (st, t) = setup();
        let f = touch(t.path(), "a.jpg", b"x");
        assert!(st.rename("main", &f, "a.jpg").is_err());
        let missing = t.path().join("nope.jpg").to_string_lossy().into_owned();
        assert!(st.rename("main", &missing, "b.jpg").unwrap_err().contains("does not exist"));
        let d = t.path().join("dir");
        std::fs::create_dir(&d).unwrap();
        assert!(st.rename("main", &d.to_string_lossy(), "d2").is_err());
        assert!(d.exists());
    }

    #[test]
    fn rename_only_changes_letter_case() {
        let (st, t) = setup();
        let f = touch(t.path(), "photo.jpg", b"x");
        st.rename("main", &f, "Photo.jpg").unwrap();
        let names: Vec<String> =
            std::fs::read_dir(t.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["Photo.jpg"]);
    }

    #[test]
    fn undo_restores_the_old_name() {
        let (st, t) = setup();
        let f = touch(t.path(), "a.jpg", b"x");
        let to = st.rename("main", &f, "b.jpg").unwrap();
        let restored = st.undo("main").unwrap();
        assert_eq!(restored, vec![f.clone()]);
        assert!(std::path::Path::new(&f).exists());
        assert!(!std::path::Path::new(&to).exists());
        // Only one step of undo.
        assert!(st.undo("main").unwrap().is_empty());
    }

    #[test]
    fn undo_of_case_only_rename_works() {
        let (st, t) = setup();
        let f = touch(t.path(), "photo.jpg", b"x");
        st.rename("main", &f, "Photo.jpg").unwrap();
        assert_eq!(st.undo("main").unwrap(), vec![f]);
        let names: Vec<String> =
            std::fs::read_dir(t.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["photo.jpg"]);
    }

    #[test]
    fn undo_does_not_overwrite_a_new_file_with_the_old_name() {
        let (st, t) = setup();
        let f = touch(t.path(), "a.jpg", b"old");
        st.rename("main", &f, "b.jpg").unwrap();
        touch(t.path(), "a.jpg", b"new");
        assert!(st.undo("main").unwrap_err().contains("already exists"));
        assert_eq!(std::fs::read(&f).unwrap(), b"new");
        assert_eq!(std::fs::read(t.path().join("b.jpg")).unwrap(), b"old");
    }

    #[test]
    fn failed_rename_keeps_the_previous_undo_step() {
        let (st, t) = setup();
        let a = touch(t.path(), "a.jpg", b"x");
        let b = st.rename("main", &a, "b.jpg").unwrap();
        assert!(st.rename("main", &b, "../c.jpg").is_err());
        assert_eq!(st.undo("main").unwrap(), vec![a]);
    }
}
