//! CollectionStore: in-memory lists of image paths from any folders (QUAL-20). The store does not
//! know the features that fill it; items carry feature-defined badge codes such as `quality.blurry`.

use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::Serialize;

use crate::tasks::TaskKind;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionOrigin {
    pub task: TaskKind,
    pub root: String,
    pub scanned: u32,
    pub capped: bool,
    pub created_ms: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CollectionItem {
    pub path: String,
    pub badges: Vec<String>,
    /// Position at upsert, so that a restore puts the item back where it was.
    #[serde(skip)]
    ord: usize,
}

impl CollectionItem {
    pub fn new(path: String, badges: Vec<String>) -> Self {
        Self { path, badges, ord: 0 }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub origin: Option<CollectionOrigin>,
    pub items: Vec<CollectionItem>,
    /// Items removed by a move to the Trash, kept for `restore_paths`.
    #[serde(skip)]
    removed: Vec<CollectionItem>,
}

impl Collection {
    pub fn new(id: &str, name: &str, icon: &str, origin: Option<CollectionOrigin>, items: Vec<CollectionItem>) -> Self {
        Self { id: id.into(), name: name.into(), icon: icon.into(), origin, items, removed: Vec::new() }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub count: usize,
}

/// Receives the summary list after each change (`collections-changed` to all windows).
pub type ChangeListener = Arc<dyn Fn(&[CollectionSummary]) + Send + Sync>;

#[derive(Default)]
pub struct CollectionStore {
    collections: Mutex<Vec<Collection>>,
    listener: OnceLock<ChangeListener>,
}

impl CollectionStore {
    pub fn set_listener(&self, l: ChangeListener) {
        let _ = self.listener.set(l);
    }

    fn summaries(v: &[Collection]) -> Vec<CollectionSummary> {
        v.iter()
            .map(|c| CollectionSummary { id: c.id.clone(), name: c.name.clone(), icon: c.icon.clone(), count: c.items.len() })
            .collect()
    }

    fn changed(&self, v: &[Collection]) {
        if let Some(l) = self.listener.get() {
            l(&Self::summaries(v));
        }
    }

    pub fn list(&self) -> Vec<CollectionSummary> {
        Self::summaries(&self.collections.lock())
    }

    pub fn get(&self, id: &str) -> Option<Collection> {
        self.collections.lock().iter().find(|c| c.id == id).cloned()
    }

    /// Adds the collection, or replaces the collection with the same id (QUAL-19).
    pub fn upsert(&self, mut c: Collection) {
        for (i, it) in c.items.iter_mut().enumerate() {
            it.ord = i;
        }
        c.removed.clear();
        let mut v = self.collections.lock();
        match v.iter_mut().find(|x| x.id == c.id) {
            Some(x) => *x = c,
            None => v.push(c),
        }
        let snapshot = v.clone();
        drop(v);
        self.changed(&snapshot);
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut v = self.collections.lock();
        let n = v.len();
        v.retain(|c| c.id != id);
        let changed = v.len() != n;
        let snapshot = v.clone();
        drop(v);
        if changed {
            self.changed(&snapshot);
        }
        changed
    }

    fn edit(&self, f: impl Fn(&mut Collection) -> bool) {
        let mut v = self.collections.lock();
        let mut changed = false;
        for c in v.iter_mut() {
            changed |= f(c);
        }
        let snapshot = changed.then(|| v.clone());
        drop(v);
        if let Some(s) = snapshot {
            self.changed(&s);
        }
    }

    /// Removes paths from all collections, after a move to the Trash. A path removes also the items
    /// inside it, for a trashed folder.
    pub fn remove_paths<S: AsRef<str>>(&self, paths: &[S]) {
        let gone: Vec<&std::path::Path> = paths.iter().map(|p| std::path::Path::new(p.as_ref())).collect();
        self.edit(|c| {
            let (out, keep): (Vec<_>, Vec<_>) =
                c.items.drain(..).partition(|it| gone.iter().any(|g| std::path::Path::new(&it.path).starts_with(g)));
            c.items = keep;
            let changed = !out.is_empty();
            c.removed.extend(out);
            changed
        });
    }

    /// Puts removed paths back at their original position, after an undo.
    pub fn restore_paths<S: AsRef<str>>(&self, paths: &[S]) {
        let back: Vec<&std::path::Path> = paths.iter().map(|p| std::path::Path::new(p.as_ref())).collect();
        self.edit(|c| {
            let (restore, keep): (Vec<_>, Vec<_>) =
                c.removed.drain(..).partition(|it| back.iter().any(|b| std::path::Path::new(&it.path).starts_with(b)));
            c.removed = keep;
            if restore.is_empty() {
                return false;
            }
            c.items.extend(restore);
            c.items.sort_by_key(|it| it.ord);
            true
        });
    }

    /// Drops items whose file no longer exists. Returns the current collection.
    pub fn prune(&self, id: &str) -> Option<Collection> {
        let mut v = self.collections.lock();
        let c = v.iter_mut().find(|c| c.id == id)?;
        let n = c.items.len();
        c.items.retain(|it| std::path::Path::new(&it.path).exists());
        let out = c.clone();
        let snapshot = (c.items.len() != n).then(|| v.clone());
        drop(v);
        if let Some(s) = snapshot {
            self.changed(&s);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(paths: &[&str]) -> CollectionStore {
        let st = CollectionStore::default();
        let items = paths.iter().map(|p| CollectionItem::new(p.to_string(), vec!["quality.blurry".into()])).collect();
        st.upsert(Collection::new("bad-photos", "Bad photos", "quality", None, items));
        st
    }

    fn paths(st: &CollectionStore) -> Vec<String> {
        st.get("bad-photos").unwrap().items.into_iter().map(|i| i.path).collect()
    }

    #[test]
    fn remove_and_restore_keep_the_order() {
        let st = store_with(&["/a/1.jpg", "/a/2.jpg", "/b/3.jpg", "/b/4.jpg"]);
        st.remove_paths(&["/a/2.jpg", "/b/4.jpg"]);
        assert_eq!(paths(&st), ["/a/1.jpg", "/b/3.jpg"]);
        st.restore_paths(&["/b/4.jpg", "/a/2.jpg"]);
        assert_eq!(paths(&st), ["/a/1.jpg", "/a/2.jpg", "/b/3.jpg", "/b/4.jpg"]);
    }

    #[test]
    fn a_trashed_folder_removes_its_items() {
        let st = store_with(&["/a/1.jpg", "/a/sub/2.jpg", "/ab/3.jpg"]);
        st.remove_paths(&["/a"]);
        assert_eq!(paths(&st), ["/ab/3.jpg"]);
        st.restore_paths(&["/a"]);
        assert_eq!(paths(&st), ["/a/1.jpg", "/a/sub/2.jpg", "/ab/3.jpg"]);
    }

    #[test]
    fn upsert_replaces_and_emits() {
        let st = store_with(&["/a/1.jpg"]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = Arc::clone(&seen);
        st.set_listener(Arc::new(move |l: &[CollectionSummary]| s2.lock().push(l.to_vec())));
        st.upsert(Collection::new("bad-photos", "Bad photos", "quality", None, vec![]));
        assert_eq!(st.list().len(), 1);
        assert_eq!(st.list()[0].count, 0);
        assert_eq!(seen.lock().len(), 1);
        assert!(st.remove("bad-photos"));
        assert!(st.list().is_empty());
        assert_eq!(seen.lock().len(), 2);
    }
}
