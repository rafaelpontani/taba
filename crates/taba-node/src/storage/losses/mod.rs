//! The `losses` table of `_node.db` (doc 02, section 3.2; doc 04, section 6.6): the (topic, ID) pairs
//! that some synchronization declared lost. A node returning from an exclusion queries it on the
//! other nodes to recover the messages it still has. The records expire after
//! `cluster.message-loss.ttl`.

use std::io;
use std::sync::Arc;

use rusqlite::params;

use super::db::NodeDb;

/// A recorded loss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loss {
    pub topic: String,
    pub id: String,
    /// The ring block the message belongs to (16 hexadecimal digits).
    pub block: String,
    pub detected_ms: u64,
}

#[derive(Debug, Clone)]
pub struct LossStore {
    db: Arc<NodeDb>,
}

impl LossStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }

    /// Records the loss. The key is (topic, ID): repeating changes nothing. `true` if it was new.
    pub fn record(&self, loss: &Loss) -> io::Result<bool> {
        let changed = self
            .db
            .conn()
            .execute(
                "INSERT OR IGNORE INTO losses (topic, id, block, detected_at) VALUES (?1, ?2, ?3, ?4)",
                params![loss.topic, loss.id, loss.block, loss.detected_ms as i64],
            )
            .map_err(|e| self.db.fail(e))?;
        Ok(changed > 0)
    }

    /// Acknowledges the loss (the message was recovered). `true` if it existed.
    pub fn remove(&self, topic: &str, id: &str) -> io::Result<bool> {
        let changed = self
            .db
            .conn()
            .execute(
                "DELETE FROM losses WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(|e| self.db.fail(e))?;
        Ok(changed > 0)
    }

    /// The losses with `id` from `from` (inclusive), in `id` order, at most `limit`.
    pub fn list(&self, from: Option<&str>, limit: usize) -> io::Result<Vec<Loss>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT topic, id, block, detected_at FROM losses WHERE id >= ?1 ORDER BY id, topic LIMIT ?2",
            )
            .map_err(|e| self.db.fail(e))?;
        let rows = stmt
            .query_map(
                params![from.unwrap_or(""), i64::try_from(limit).unwrap_or(i64::MAX)],
                |row| {
                    Ok(Loss {
                        topic: row.get(0)?,
                        id: row.get(1)?,
                        block: row.get(2)?,
                        detected_ms: row.get::<_, i64>(3)? as u64,
                    })
                },
            )
            .map_err(|e| self.db.fail(e))?;
        rows.collect::<Result<_, _>>().map_err(|e| self.db.fail(e))
    }

    /// Deletes the records detected before `before_ms`; returns how many.
    pub fn expire(&self, before_ms: u64) -> io::Result<u64> {
        let removed = self
            .db
            .conn()
            .execute(
                "DELETE FROM losses WHERE detected_at < ?1",
                params![before_ms as i64],
            )
            .map_err(|e| self.db.fail(e))?;
        Ok(removed as u64)
    }

    pub fn count(&self) -> io::Result<u64> {
        let n: i64 = self
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM losses", [], |row| row.get(0))
            .map_err(|e| self.db.fail(e))?;
        Ok(n as u64)
    }
}

#[cfg(test)]
mod tests;
