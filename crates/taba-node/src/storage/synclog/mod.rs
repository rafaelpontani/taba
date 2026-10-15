//! The event database of a node in synchronization: `${storage.dir}/_sync/sync.db` (doc 05, section
//! 7.3, step 4). While the node loads the blocks it is already in the member table and receives the
//! normal cluster protocol, but cannot process the events: it stores each one here, in
//! arrival order (increasing sequence number), and processes them on top of the load.
//!
//! The event is the HTTP call as it arrived (method, path, headers and body): whoever processes it
//! reads it through the same path it would read a network call. The `loaded` table keeps the IDs the
//! load brought, so the final check knows what to verify. The file only exists during
//! the synchronization and is deleted at the end.
//!
//! A single connection behind a `Mutex` is the doc's single write thread.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use super::db::to_io;

pub const SYNC_DIR: &str = "_sync";
pub const SYNC_DB_FILE: &str = "sync.db";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS events (
        seq     INTEGER PRIMARY KEY AUTOINCREMENT,
        method  TEXT NOT NULL,
        path    TEXT NOT NULL,
        headers TEXT NOT NULL,
        body    BLOB NOT NULL
    );
    CREATE TABLE IF NOT EXISTS loaded (
        id    TEXT PRIMARY KEY NOT NULL,
        topic TEXT NOT NULL,
        block TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS loaded_block ON loaded (block, id);
";

/// A stored event: the call as it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: i64,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct SyncLog {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl SyncLog {
    /// Creates the database, deleting what an earlier synchronization left.
    pub fn create(storage_dir: &Path) -> io::Result<Self> {
        let dir = storage_dir.join(SYNC_DIR);
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(SYNC_DB_FILE);
        let conn = Connection::open(&path).map_err(to_io)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(to_io)?;
        // A database that only exists until the synchronization ends: durability is not worth the cost.
        conn.pragma_update(None, "synchronous", "OFF")
            .map_err(to_io)?;
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        Ok(Self {
            path,
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Appends the event to the end and returns the sequence number.
    pub fn append(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> io::Result<i64> {
        let headers = serde_json::to_string(headers).map_err(io::Error::other)?;
        let conn = self.conn();
        conn.execute(
            "INSERT INTO events (method, path, headers, body) VALUES (?1, ?2, ?3, ?4)",
            params![method, path, headers, body],
        )
        .map_err(to_io)?;
        Ok(conn.last_insert_rowid())
    }

    /// The oldest event (lowest sequence number).
    pub fn oldest(&self) -> io::Result<Option<StoredEvent>> {
        let row = self
            .conn()
            .query_row(
                "SELECT seq, method, path, headers, body FROM events ORDER BY seq LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(to_io)?;
        row.map(|(seq, method, path, headers, body)| {
            Ok(StoredEvent {
                seq,
                method,
                path,
                headers: serde_json::from_str(&headers).map_err(io::Error::other)?,
                body,
            })
        })
        .transpose()
    }

    /// Deletes the already-processed event.
    pub fn delete(&self, seq: i64) -> io::Result<()> {
        self.conn()
            .execute("DELETE FROM events WHERE seq = ?1", params![seq])
            .map(|_| ())
            .map_err(to_io)
    }

    /// How many events are left to process.
    pub fn len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    pub fn is_empty(&self) -> io::Result<bool> {
        self.len().map(|n| n == 0)
    }

    /// The database size on disk (pages × page size), for `cluster.sync.log.max-bytes`.
    pub fn size_bytes(&self) -> io::Result<u64> {
        let conn = self.conn();
        let pages: i64 = conn
            .pragma_query_value(None, "page_count", |row| row.get(0))
            .map_err(to_io)?;
        let size: i64 = conn
            .pragma_query_value(None, "page_size", |row| row.get(0))
            .map_err(to_io)?;
        Ok((pages.max(0) * size.max(0)) as u64)
    }

    /// Records that the load brought the message (of the block), for the final check.
    pub fn mark_loaded(&self, block: &str, items: &[(String, String)]) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO loaded (id, topic, block) VALUES (?1, ?2, ?3)",
                )
                .map_err(to_io)?;
            for (topic, id) in items {
                stmt.execute(params![id, topic, block]).map_err(to_io)?;
            }
        }
        tx.commit().map_err(to_io)
    }

    /// The messages the load brought from the block, `(topic, id)` in `id` order.
    pub fn loaded_in(&self, block: &str) -> io::Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT topic, id FROM loaded WHERE block = ?1 ORDER BY id")
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![block], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// Closes the database and deletes the `_sync` folder (the synchronization ended).
    pub fn discard(&self) -> io::Result<()> {
        // Swaps the connection for an in-memory one: the file's closes (Windows does not delete an open file).
        *self.conn() = Connection::open_in_memory().map_err(to_io)?;
        match self.path.parent().map(std::fs::remove_dir_all) {
            Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
