//! PreviewIndex: persistent `FolderPreview` storage in SQLite (system libsqlite3),
//! plus a cache of EXIF "date taken" values for the date-taken sort, and the generic `analysis`
//! table of the image scan pipeline (D3).

use std::collections::HashMap;
use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use super::preview::FolderPreview;

/// The `analysis` layout: 3 = key `(dir, name, analyzer)` plus a `hash` column (`thumbs::path_hash` of
/// the source file) without an index. Reads and writes cost the same as without the column; only the
/// thumbnail cache eviction uses `hash`, with 1 scan of the table (see `analysis_delete_hashes`).
const SCHEMA_VERSION: i64 = 3;

pub struct PreviewIndex {
    conn: Mutex<Connection>,
}

impl PreviewIndex {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path).or_else(|_| {
            // A corrupt index is only a cache: start again.
            let _ = std::fs::remove_file(path);
            Connection::open(path)
        })?;
        Self::init(conn)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        // An `analysis` table of an older layout: it is only a cache, so start it again.
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < SCHEMA_VERSION {
            conn.execute_batch(&format!("DROP TABLE IF EXISTS analysis; PRAGMA user_version = {SCHEMA_VERSION};"))?;
        }
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS previews(
                folder TEXT NOT NULL, sort TEXT NOT NULL, data TEXT NOT NULL,
                PRIMARY KEY(folder, sort)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS taken(
                dir TEXT NOT NULL, name TEXT NOT NULL, mtime INTEGER NOT NULL, taken INTEGER,
                PRIMARY KEY(dir, name)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS analysis(
                dir TEXT NOT NULL, name TEXT NOT NULL, analyzer TEXT NOT NULL, version INTEGER NOT NULL,
                mtime INTEGER NOT NULL, size INTEGER NOT NULL, data TEXT NOT NULL,
                hash INTEGER NOT NULL,
                PRIMARY KEY(dir, name, analyzer)) WITHOUT ROWID;",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn get_many(&self, folders: &[String], sort: &str) -> Vec<FolderPreview> {
        let conn = self.conn.lock();
        let Ok(mut st) = conn.prepare_cached("SELECT data FROM previews WHERE folder=?1 AND sort=?2") else {
            return Vec::new();
        };
        folders
            .iter()
            .filter_map(|f| {
                let s: String = st.query_row(params![f, sort], |r| r.get(0)).optional().ok()??;
                serde_json::from_str(&s).ok()
            })
            .collect()
    }

    pub fn get(&self, folder: &str, sort: &str) -> Option<FolderPreview> {
        self.get_many(&[folder.to_string()], sort).pop()
    }

    pub fn put(&self, p: &FolderPreview) {
        let Ok(data) = serde_json::to_string(p) else { return };
        let conn = self.conn.lock();
        let _ = conn
            .prepare_cached("INSERT OR REPLACE INTO previews(folder, sort, data) VALUES(?1, ?2, ?3)")
            .and_then(|mut st| st.execute(params![p.folder, p.sort, data]));
    }

    /// FSW-04 / FPV-17: a change at `path` invalidates the entries of the folder that contains it
    /// and of each parent folder.
    pub fn invalidate_ancestors(&self, path: &Path) -> usize {
        let conn = self.conn.lock();
        let Ok(mut st) = conn.prepare_cached("DELETE FROM previews WHERE folder=?1") else { return 0 };
        let mut n = 0;
        let mut cur = Some(path);
        while let Some(p) = cur {
            if let Some(s) = p.to_str() {
                n += st.execute(params![s]).unwrap_or(0);
            }
            cur = p.parent();
        }
        n
    }

    pub fn clear(&self) {
        let conn = self.conn.lock();
        let _ = conn.execute_batch("DELETE FROM previews; DELETE FROM taken; DELETE FROM analysis;");
    }

    /// name -> (mtime ms, taken)
    pub fn taken_many(&self, dir: &Path) -> HashMap<String, (i64, Option<i64>)> {
        let conn = self.conn.lock();
        let mut out = HashMap::new();
        let Some(d) = dir.to_str() else { return out };
        if let Ok(mut st) = conn.prepare_cached("SELECT name, mtime, taken FROM taken WHERE dir=?1") {
            if let Ok(rows) = st.query_map(params![d], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?))) {
                for (n, m, t) in rows.flatten() {
                    out.insert(n, (m, t));
                }
            }
        }
        out
    }

    pub fn put_taken(&self, dir: &Path, rows: &[(String, i64, Option<i64>)]) {
        let Some(d) = dir.to_str() else { return };
        let mut conn = self.conn.lock();
        let Ok(tx) = conn.transaction() else { return };
        {
            let Ok(mut st) =
                tx.prepare_cached("INSERT OR REPLACE INTO taken(dir, name, mtime, taken) VALUES(?1, ?2, ?3, ?4)")
            else {
                return;
            };
            for (n, m, t) in rows {
                let _ = st.execute(params![d, n, m, t]);
            }
        }
        let _ = tx.commit();
    }

    /// One `analysis` row: (version, mtime, size, data).
    pub fn analysis_get(&self, dir: &str, name: &str, analyzer: &str) -> Option<AnalysisRow> {
        let conn = self.conn.lock();
        let mut st = conn
            .prepare_cached("SELECT version, mtime, size, data FROM analysis WHERE dir=?1 AND name=?2 AND analyzer=?3")
            .ok()?;
        st.query_row(params![dir, name, analyzer], |r| {
            Ok(AnalysisRow { version: r.get(0)?, mtime: r.get(1)?, size: r.get(2)?, data: r.get(3)? })
        })
        .optional()
        .ok()?
    }

    /// Writes `analysis` rows in one transaction: (dir, name, path hash, analyzer, row).
    pub fn analysis_put_many(&self, rows: &[(String, String, u64, &str, AnalysisRow)]) {
        if rows.is_empty() {
            return;
        }
        let mut conn = self.conn.lock();
        let Ok(tx) = conn.transaction() else { return };
        {
            let Ok(mut st) = tx.prepare_cached(
                "INSERT OR REPLACE INTO analysis(dir, name, analyzer, version, mtime, size, data, hash)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            ) else {
                return;
            };
            for (d, n, h, a, r) in rows {
                let _ = st.execute(params![d, n, a, r.version, r.mtime, r.size, r.data, *h as i64]);
            }
        }
        let _ = tx.commit();
    }
}

impl PreviewIndex {
    /// Removes the `analysis` rows of the source files with these `thumbs::path_hash` values
    /// (thumbnail cache eviction). The hashes go to a temporary table, then 1 scan of `analysis`
    /// removes the rows: `hash` has no index, so that writes do not pay for one.
    pub fn analysis_delete_hashes(&self, hashes: &[u64]) {
        if hashes.is_empty() {
            return;
        }
        let mut conn = self.conn.lock();
        let Ok(tx) = conn.transaction() else { return };
        let ok = tx
            .execute_batch("CREATE TEMP TABLE IF NOT EXISTS evicted(h INTEGER PRIMARY KEY); DELETE FROM evicted;")
            .is_ok();
        if ok {
            if let Ok(mut st) = tx.prepare_cached("INSERT OR IGNORE INTO evicted(h) VALUES(?1)") {
                for h in hashes {
                    let _ = st.execute(params![*h as i64]);
                }
            }
            let _ = tx.execute_batch("DELETE FROM analysis WHERE hash IN (SELECT h FROM evicted); DELETE FROM evicted;");
        }
        let _ = tx.commit();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisRow {
    pub version: u32,
    pub mtime: i64,
    pub size: i64,
    pub data: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_delete_hashes_removes_all_analyzers_of_the_file() {
        let ix = PreviewIndex::in_memory().unwrap();
        let row = AnalysisRow { version: 1, mtime: 1, size: 1, data: "{}".into() };
        let h1 = crate::thumbs::path_hash(Path::new("/a/1.jpg"));
        let h2 = crate::thumbs::path_hash(Path::new("/a/2.jpg"));
        ix.analysis_put_many(&[
            ("/a".into(), "1.jpg".into(), h1, "quality", row.clone()),
            ("/a".into(), "1.jpg".into(), h1, "similarity", row.clone()),
            ("/a".into(), "2.jpg".into(), h2, "quality", row.clone()),
        ]);
        ix.analysis_delete_hashes(&[h1]);
        assert!(ix.analysis_get("/a", "1.jpg", "quality").is_none());
        assert!(ix.analysis_get("/a", "1.jpg", "similarity").is_none());
        assert_eq!(ix.analysis_get("/a", "2.jpg", "quality"), Some(row));
    }

    #[test]
    fn old_analysis_table_is_replaced() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("index.sqlite");
        Connection::open(&p)
            .unwrap()
            .execute_batch(
                "CREATE TABLE analysis(dir TEXT NOT NULL, name TEXT NOT NULL, analyzer TEXT NOT NULL,
                 version INTEGER NOT NULL, mtime INTEGER NOT NULL, size INTEGER NOT NULL, data TEXT NOT NULL,
                 PRIMARY KEY(dir, name, analyzer)) WITHOUT ROWID;
                 INSERT INTO analysis VALUES('/a', '1.jpg', 'quality', 1, 1, 1, '{}');",
            )
            .unwrap();
        let ix = PreviewIndex::open(&p).unwrap();
        assert!(ix.analysis_get("/a", "1.jpg", "quality").is_none());
        let row = AnalysisRow { version: 1, mtime: 1, size: 1, data: "{}".into() };
        ix.analysis_put_many(&[("/a".into(), "1.jpg".into(), 7, "quality", row.clone())]);
        assert_eq!(ix.analysis_get("/a", "1.jpg", "quality"), Some(row));
    }
}
