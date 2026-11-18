//! Ignored folders (IGN-01..IGN-10): exact folder paths that the app never lists, previews, scans,
//! or flattens. Each path blocks its whole tree (D6, D7).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// The defaults of `Settings.ignored_folders` (IGN-07): the macOS system trees and the large app
/// data folders of `~/Library`.
pub const DEFAULT_IGNORED: &[&str] = &[
    "/System",
    "/Library",
    "/private",
    "/etc",
    "/tmp",
    "/var",
    "/usr",
    "/bin",
    "/sbin",
    "/opt",
    "/cores",
    "/dev",
    "/Network",
    "/home",
    "/Applications",
    "~/Library/Caches",
    "~/Library/Containers",
    "~/Library/Group Containers",
    "~/Library/Application Support",
    "~/Library/Developer",
    "~/Library/Logs",
];

/// The resolved ignore list: absolute, lower-case paths without a trailing `/` (IGN-04). Cheap to
/// clone, so that a walk or a preview job can keep its own copy.
#[derive(Clone, Debug, Default)]
pub struct IgnoreSet(Arc<HashSet<String>>);

/// The comparison form of a path: lower case, without a trailing `/` (IGN-04).
fn key(p: &str) -> String {
    let t = if p.len() > 1 { p.trim_end_matches('/') } else { p };
    let t = if t.is_empty() { "/" } else { t };
    t.to_lowercase()
}

impl IgnoreSet {
    /// Resolves the entries of `Settings.ignored_folders`: `~/` becomes `home` (IGN-02). Entries that
    /// are not absolute after that are dropped.
    pub fn new<S: AsRef<str>>(paths: &[S], home: &str) -> Self {
        let home = home.trim_end_matches('/');
        let set = paths
            .iter()
            .filter_map(|p| {
                let p = p.as_ref().trim();
                let abs = match p.strip_prefix("~/") {
                    Some(rest) => format!("{home}/{rest}"),
                    None if p == "~" => home.to_string(),
                    None => p.to_string(),
                };
                abs.starts_with('/').then(|| key(&abs))
            })
            .collect();
        Self(Arc::new(set))
    }

    /// The set of the home folder of the user, from `HOME`.
    pub fn for_home<S: AsRef<str>>(paths: &[S]) -> Self {
        Self::new(paths, &std::env::var("HOME").unwrap_or_else(|_| "/".into()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// True if `path` itself is in the list.
    pub fn contains(&self, path: &Path) -> bool {
        if self.0.is_empty() {
            return false;
        }
        path.to_str().is_some_and(|s| self.0.contains(&key(s)))
    }

    /// True if `path` or a folder that contains it is in the list (D6).
    pub fn in_tree(&self, path: &Path) -> bool {
        !self.0.is_empty() && path.ancestors().any(|a| self.contains(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_set_matches_without_case() {
        let s = IgnoreSet::new(&["/Library", "~/Library/Caches/", "relative/x", "/Volumes/Card/"], "/Users/me");
        assert!(s.contains(Path::new("/Library")));
        assert!(s.contains(Path::new("/library")));
        assert!(s.contains(Path::new("/LIBRARY/")));
        assert!(s.contains(Path::new("/Users/me/Library/Caches")));
        assert!(s.contains(Path::new("/users/ME/library/caches")));
        assert!(s.contains(Path::new("/Volumes/Card")));
        assert!(!s.contains(Path::new("/Users/me/Library")));
        assert!(!s.contains(Path::new("relative/x")));
        assert!(!s.contains(Path::new("/Library2")));
    }

    #[test]
    fn a_folder_inside_an_ignored_folder_is_in_its_tree() {
        let s = IgnoreSet::new(&["~/Library/Caches"], "/Users/me");
        assert!(s.in_tree(Path::new("/Users/me/Library/Caches/com.apple.x/y")));
        assert!(s.in_tree(Path::new("/Users/me/library/caches")));
        assert!(!s.in_tree(Path::new("/Users/me/Library")));
        assert!(!s.in_tree(Path::new("/Users/me/Library/Caches2")));
        assert!(!IgnoreSet::default().in_tree(Path::new("/x")));
    }
}
