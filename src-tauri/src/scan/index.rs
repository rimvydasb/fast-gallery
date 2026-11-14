//! PreviewIndex: persistent `FolderPreview` storage in SQLite (system libsqlite3),
//! plus a cache of EXIF "date taken" values for the date-taken sort, and the generic `analysis`
//! table of the image scan pipeline (D3).

use std::collections::HashMap;
use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use super::preview::FolderPreview;

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

    /// Writes `analysis` rows in one transaction: (dir, name, analyzer, row).
    pub fn analysis_put_many(&self, rows: &[(String, String, &str, AnalysisRow)]) {
        if rows.is_empty() {
            return;
        }
        let mut conn = self.conn.lock();
        let Ok(tx) = conn.transaction() else { return };
        {
            let Ok(mut st) = tx.prepare_cached(
                "INSERT OR REPLACE INTO analysis(dir, name, analyzer, version, mtime, size, data)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            ) else {
                return;
            };
            for (d, n, a, r) in rows {
                let _ = st.execute(params![d, n, a, r.version, r.mtime, r.size, r.data]);
            }
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
