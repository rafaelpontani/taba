//! The `definition_versions` table of `_node.db` (doc 07, section 10.2): the version of each topic and
//! token definition this node applied, and the **tombstones** (the removals), with the `topicId` of the
//! topic. It is what lets a node that starts from its own disk know which version of each definition it
//! holds and not resurrect what was removed. The cluster's `Definitions` writes a row at each
//! application and loads them when the node resumes the definitions of the disk.

use std::io;
use std::sync::Arc;

use rusqlite::params;

use super::db::NodeDb;

/// One definition's state: the version, whether it was removed and the fingerprint of the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    /// `topic` or `token`.
    pub kind: String,
    /// The topic name or the token `id`.
    pub key: String,
    pub version: u64,
    pub deleted: bool,
    pub fingerprint: [u8; 32],
    /// The topic's `id` (a recreated topic has another one); `None` for tokens and for a tombstone
    /// whose topic this node never knew.
    pub topic_id: Option<String>,
    /// When this node applied the version (ms), for the tombstone's expiry.
    pub at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct VersionStore {
    db: Arc<NodeDb>,
}

impl VersionStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }

    /// Writes (or replaces) the definition's row.
    pub fn save(&self, row: &VersionRow) -> io::Result<()> {
        self.db
            .conn()
            .execute(
                "INSERT INTO definition_versions
                     (kind, key, version, deleted, fingerprint, topic_id, at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(kind, key) DO UPDATE SET
                     version = excluded.version, deleted = excluded.deleted,
                     fingerprint = excluded.fingerprint, topic_id = excluded.topic_id,
                     at_ms = excluded.at_ms",
                params![
                    row.kind,
                    row.key,
                    row.version as i64,
                    row.deleted,
                    row.fingerprint.as_slice(),
                    row.topic_id,
                    row.at_ms as i64
                ],
            )
            .map(|_| ())
            .map_err(|e| self.db.fail(e))
    }

    /// Deletes the definition's row. Deleting what does not exist is no error.
    pub fn delete(&self, kind: &str, key: &str) -> io::Result<()> {
        self.db
            .conn()
            .execute(
                "DELETE FROM definition_versions WHERE kind = ?1 AND key = ?2",
                params![kind, key],
            )
            .map(|_| ())
            .map_err(|e| self.db.fail(e))
    }

    /// Deletes the tombstones applied before `before_ms`; returns how many rows it deleted.
    pub fn delete_tombstones_before(&self, before_ms: u64) -> io::Result<usize> {
        self.db
            .conn()
            .execute(
                "DELETE FROM definition_versions WHERE deleted = 1 AND at_ms < ?1",
                params![before_ms as i64],
            )
            .map_err(|e| self.db.fail(e))
    }

    /// Deletes every row (the node comes back as a new node).
    pub fn clear(&self) -> io::Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM definition_versions", [])
            .map(|_| ())
            .map_err(|e| self.db.fail(e))
    }

    /// All the rows, by kind and key. A row with a fingerprint of the wrong size is an error: the
    /// node does not start with a half-read table.
    pub fn load_all(&self) -> io::Result<Vec<VersionRow>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT kind, key, version, deleted, fingerprint, topic_id, at_ms
                 FROM definition_versions ORDER BY kind, key",
            )
            .map_err(|e| self.db.fail(e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .map_err(|e| self.db.fail(e))?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, key, version, deleted, fingerprint, topic_id, at_ms) =
                row.map_err(|e| self.db.fail(e))?;
            let fingerprint: [u8; 32] = fingerprint.try_into().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("definition version `{kind}:{key}` in _node.db: invalid fingerprint"),
                )
            })?;
            out.push(VersionRow {
                kind,
                key,
                version: version.max(0) as u64,
                deleted,
                fingerprint,
                topic_id,
                at_ms: at_ms.max(0) as u64,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
