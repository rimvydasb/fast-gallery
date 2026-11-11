//! PreviewIndex: persistent `FolderPreview` storage in SQLite (system libsqlite3),
//! plus a cache of EXIF "date taken" values for the date-taken sort.

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
                PRIMARY KEY(dir, name)) WITHOUT ROWID;",
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
        let _ = conn.execute_batch("DELETE FROM previews; DELETE FROM taken;");
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
}
