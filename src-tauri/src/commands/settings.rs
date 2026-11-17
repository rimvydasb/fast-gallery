//! Settings, stored as JSON in the Tauri app config directory (SET-04).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::scan::SortSpec;

pub const MIN_TILE: u32 = 96;
pub const MIN_SCAN_CAP: u32 = 100;
pub const MAX_SCAN_CAP: u32 = 1000;
const SCAN_CAP_STEP: u32 = 100;
pub const MAX_TILE: u32 = 512;
const MAX_RECENTS: usize = 10;
const MAX_FOLDER_PREFS: usize = 2000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderPrefs {
    pub sort: SortSpec,
    pub tile_size: u32,
    #[serde(default)]
    pub group: crate::scan::arrange::GroupKey,
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
    /// Multiplies the blur threshold of the bad photo scan.
    pub quality_sensitivity: crate::quality::Sensitivity,
    /// Maximum number of photos in one bad photo scan.
    pub scan_cap: u32,
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
            quality_sensitivity: crate::quality::Sensitivity::Medium,
            scan_cap: 200,
        }
    }
}

impl Settings {
    fn clamp(&mut self) {
        self.default_tile_size = self.default_tile_size.clamp(MIN_TILE, MAX_TILE);
        self.preview_depth = self.preview_depth.clamp(1, 10);
        self.cache_limit_mb = self.cache_limit_mb.clamp(64, 1024 * 1024);
        self.slideshow_seconds = self.slideshow_seconds.clamp(1, 600);
        let cap = (self.scan_cap + SCAN_CAP_STEP / 2) / SCAN_CAP_STEP * SCAN_CAP_STEP;
        self.scan_cap = cap.clamp(MIN_SCAN_CAP, MAX_SCAN_CAP);
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

    /// Call these 2 off the command thread: they write the settings file.
    ///
    /// Records an opened folder as the last folder (NAV-11) and, when `recent` is true, in Recent
    /// folders (NAV-08). Returns the new settings when Recent folders changed, so the caller can
    /// tell the windows; the last folder is read only at start-up and needs no event.
    pub fn visit(&self, folder: &str, recent: bool) -> Option<Settings> {
        self.record(Some(folder), recent.then_some(folder))
    }

    /// Adds a folder to Recent folders (NAV-08) without a change to the last folder.
    pub fn add_recent(&self, folder: &str) -> Option<Settings> {
        self.record(None, Some(folder))
    }

    /// A folder that is already in Recent folders keeps its place: a reorder on each visit makes
    /// the sidebar list jump, for example when the user opens a folder from that list.
    fn record(&self, last: Option<&str>, recent: Option<&str>) -> Option<Settings> {
        let mut d = self.data.lock();
        let new_last = last.filter(|f| d.last_folder.as_deref() != Some(*f));
        let new_recent = recent.filter(|f| !d.recents.iter().any(|r| r == f));
        if new_last.is_none() && new_recent.is_none() {
            return None;
        }
        if let Some(f) = new_last {
            d.last_folder = Some(f.to_string());
        }
        if let Some(f) = new_recent {
            d.recents.insert(0, f.to_string());
            d.recents.truncate(MAX_RECENTS);
        }
        let s = d.clone();
        drop(d);
        // The callers run on the background pool, so the write is done here.
        self.save(&s);
        new_recent.map(|_| s)
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

    #[test]
    fn scan_cap_is_clamped_to_steps_of_100() {
        let t = tempfile::tempdir().unwrap();
        let st = SettingsStore::load(t.path().join("settings.json"));
        assert_eq!(st.get().scan_cap, 200);
        for (v, want) in [(5, 100), (149, 100), (250, 300), (999, 1000), (50_000, 1000)] {
            assert_eq!(st.update(serde_json::json!({"scanCap": v})).unwrap().scan_cap, want, "{v}");
        }
        let s = st.update(serde_json::json!({"qualitySensitivity": "high"})).unwrap();
        assert_eq!(s.quality_sensitivity, crate::quality::Sensitivity::High);
    }

    #[test]
    fn visit_without_recent_only_sets_last_folder() {
        let t = tempfile::tempdir().unwrap();
        let st = SettingsStore::load(t.path().join("settings.json"));
        assert!(st.visit("/photos", true).is_some());
        assert!(st.visit("/empty", false).is_none(), "no Recent change, no event");
        assert_eq!(st.get().last_folder.as_deref(), Some("/empty"));
        assert_eq!(st.get().recents, vec!["/photos".to_string()]);
        let s = st.add_recent("/later").unwrap();
        assert_eq!(s.last_folder.as_deref(), Some("/empty"));
        assert_eq!(s.recents, vec!["/later".to_string(), "/photos".to_string()]);
    }

    #[test]
    fn known_recent_keeps_its_place() {
        let t = tempfile::tempdir().unwrap();
        let st = SettingsStore::load(t.path().join("settings.json"));
        st.visit("/a", true);
        st.visit("/b", true);
        assert!(st.visit("/a", true).is_none());
        let s = st.get();
        assert_eq!(s.recents, vec!["/b".to_string(), "/a".to_string()]);
        assert_eq!(s.last_folder.as_deref(), Some("/a"));
    }
}
