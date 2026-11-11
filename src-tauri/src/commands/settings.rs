//! Settings, stored as JSON in the Tauri app config directory (SET-04).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::scan::SortSpec;

pub const MIN_TILE: u32 = 96;
pub const MAX_TILE: u32 = 512;
const MAX_RECENTS: usize = 10;
const MAX_FOLDER_PREFS: usize = 2000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderPrefs {
    pub sort: SortSpec,
    pub tile_size: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub default_sort: SortSpec,
    pub default_tile_size: u32,
    pub show_hidden: bool,
    pub cache_limit_mb: u64,
    pub preview_depth: u32,
    pub slideshow_seconds: u32,
    pub show_filmstrip: bool,
    pub sidebar_collapsed: bool,
    pub favorites: Vec<String>,
    pub recents: Vec<String>,
    pub last_folder: Option<String>,
    pub folder_prefs: HashMap<String, FolderPrefs>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_sort: SortSpec::default(),
            default_tile_size: 180,
            show_hidden: false,
            cache_limit_mb: 2048,
            preview_depth: crate::scan::preview::DEFAULT_DEPTH as u32,
            slideshow_seconds: 3,
            show_filmstrip: true,
            sidebar_collapsed: false,
            favorites: Vec::new(),
            recents: Vec::new(),
            last_folder: None,
            folder_prefs: HashMap::new(),
        }
    }
}

impl Settings {
    fn clamp(&mut self) {
        self.default_tile_size = self.default_tile_size.clamp(MIN_TILE, MAX_TILE);
        self.preview_depth = self.preview_depth.clamp(1, 10);
        self.cache_limit_mb = self.cache_limit_mb.clamp(64, 1024 * 1024);
        self.slideshow_seconds = self.slideshow_seconds.clamp(1, 600);
        for p in self.folder_prefs.values_mut() {
            p.tile_size = p.tile_size.clamp(MIN_TILE, MAX_TILE);
        }
        self.recents.truncate(MAX_RECENTS);
        if self.folder_prefs.len() > MAX_FOLDER_PREFS {
            self.folder_prefs.clear();
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    data: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let mut s: Settings =
            std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.clamp();
        Self { path, data: Mutex::new(s) }
    }

    pub fn get(&self) -> Settings {
        self.data.lock().clone()
    }

    fn save(&self, s: &Settings) {
        write_json(&self.path, s);
    }

    /// Merges the top-level keys of `patch` into the settings and saves them.
    pub fn update(&self, patch: serde_json::Value) -> Result<Settings, String> {
        let mut d = self.data.lock();
        let mut cur = serde_json::to_value(&*d).map_err(|e| e.to_string())?;
        if let (Some(obj), serde_json::Value::Object(p)) = (cur.as_object_mut(), patch) {
            for (k, v) in p {
                obj.insert(k, v);
            }
        }
        let mut s: Settings = serde_json::from_value(cur).map_err(|e| e.to_string())?;
        s.clamp();
        *d = s.clone();
        drop(d);
        self.save(&s);
        Ok(s)
    }

    pub fn mutate(&self, f: impl FnOnce(&mut Settings)) -> Settings {
        let mut d = self.data.lock();
        f(&mut d);
        d.clamp();
        let s = d.clone();
        drop(d);
        self.save(&s);
        s
    }

    /// Records an opened folder as the last folder (NAV-11) and in Recent folders (NAV-08).
    pub fn visit(&self, folder: &str) {
        let mut d = self.data.lock();
        if d.last_folder.as_deref() == Some(folder) && d.recents.first().map(String::as_str) == Some(folder) {
            return;
        }
        d.last_folder = Some(folder.to_string());
        d.recents.retain(|r| r != folder);
        d.recents.insert(0, folder.to_string());
        d.recents.truncate(MAX_RECENTS);
        let s = d.clone();
        drop(d);
        // Write off the command thread; a lost write only loses a recent entry.
        let path = self.path.clone();
        crate::pools::io().spawn(move || write_json(&path, &s));
    }
}

fn write_json(path: &Path, s: &Settings) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(s) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_merges_and_clamps() {
        let t = tempfile::tempdir().unwrap();
        let st = SettingsStore::load(t.path().join("settings.json"));
        let s = st.update(serde_json::json!({"previewDepth": 50, "defaultTileSize": 10, "showHidden": true})).unwrap();
        assert_eq!(s.preview_depth, 10);
        assert_eq!(s.default_tile_size, MIN_TILE);
        assert!(s.show_hidden);
        let again = SettingsStore::load(t.path().join("settings.json"));
        assert!(again.get().show_hidden);
    }
}
