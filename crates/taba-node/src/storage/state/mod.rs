//! The node state in the `state` table of `_node.db` (doc 04, section 6.10): today the
//! `save_completed` mark, the instant the drain ended. It is what separates a clean stop (the save
//! finished) from a crash (the next start warns that what was only in memory was lost).

use std::io;
use std::sync::Arc;

use rusqlite::{OptionalExtension, params};

use super::db::NodeDb;

const SAVE_COMPLETED: &str = "save_completed";

#[derive(Debug, Clone)]
pub struct NodeStateStore {
    db: Arc<NodeDb>,
}

impl NodeStateStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }

    /// Writes the mark with the instant (ms) the save ended.
    pub fn mark_save_completed(&self, at_ms: u64) -> io::Result<()> {
        self.db
            .conn()
            .execute(
                "INSERT INTO state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![SAVE_COMPLETED, at_ms.to_string()],
            )
            .map(|_| ())
            .map_err(|e| self.db.fail(e))
    }

    /// Reads the mark **and deletes it**: whoever starts consumes it, so a crash after the start does not
    /// count as a clean save. `None`: there was no completed save.
    pub fn take_save_mark(&self) -> io::Result<Option<u64>> {
        let conn = self.db.conn();
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM state WHERE key = ?1",
                params![SAVE_COMPLETED],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| self.db.fail(e))?;
        conn.execute("DELETE FROM state WHERE key = ?1", params![SAVE_COMPLETED])
            .map_err(|e| self.db.fail(e))?;
        Ok(value.and_then(|v| v.parse().ok()))
    }

    /// Deletes the mark without reading (the drain was cancelled: it went back to writing in memory).
    pub fn clear_save_mark(&self) -> io::Result<()> {
        self.take_save_mark().map(|_| ())
    }
}

#[cfg(test)]
mod tests;
