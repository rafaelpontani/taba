//! The node state in the `state` table of `_node.db` (doc 04, section 6.10): the start of the recording of the
//! excluded log and whether this life already inherited it (doc 07, section 10.3).

use std::io;
use std::sync::Arc;

use rusqlite::{OptionalExtension, params};

use super::db::NodeDb;

const EXCLUDED_SINCE: &str = "excluded_since";
const EXCLUDED_INHERITED: &str = "excluded_inherited";

#[derive(Debug, Clone)]
pub struct NodeStateStore {
    db: Arc<NodeDb>,
}

impl NodeStateStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }

    /// The start of the continuous recording of the excluded log (doc 07, section 10.3), in ms.
    pub fn excluded_since(&self) -> io::Result<Option<u64>> {
        let value: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT value FROM state WHERE key = ?1",
                params![EXCLUDED_SINCE],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| self.db.fail(e))?;
        Ok(value.and_then(|v| v.parse().ok()))
    }

    pub fn set_excluded_since(&self, ms: u64) -> io::Result<()> {
        self.db
            .conn()
            .execute(
                "INSERT INTO state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![EXCLUDED_SINCE, ms.to_string()],
            )
            .map(|_| ())
            .map_err(|e| self.db.fail(e))
    }

    /// Did this life already inherit the start of the recording from the sources of its first synchronization?
    pub fn excluded_inherited(&self) -> io::Result<bool> {
        let value: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT value FROM state WHERE key = ?1",
                params![EXCLUDED_INHERITED],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| self.db.fail(e))?;
        Ok(value.is_some())
    }

    pub fn set_excluded_inherited(&self, inherited: bool) -> io::Result<()> {
        let conn = self.db.conn();
        let result = if inherited {
            conn.execute(
                "INSERT OR REPLACE INTO state (key, value) VALUES (?1, '1')",
                params![EXCLUDED_INHERITED],
            )
        } else {
            conn.execute(
                "DELETE FROM state WHERE key = ?1",
                params![EXCLUDED_INHERITED],
            )
        };
        result.map(|_| ()).map_err(|e| self.db.fail(e))
    }
}

#[cfg(test)]
mod tests;
