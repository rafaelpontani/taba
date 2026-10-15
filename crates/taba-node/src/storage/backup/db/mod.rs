//! The `_backup.db` (doc 23, "S3 backup"): the backup's local SQLite database, next to `_node.db`.
//!
//! - **`outbox`**: the messages the primary has already acknowledged and that have not yet gone into a segment.
//!   Only the `id`: the body and metadata come from wherever the message is. A message's commit only inserts
//!   a row here; nothing waits for S3.
//! - **`saved`**: where each saved message is in the bucket (segment, offset and size).
//!   With this the node deletes the body from disk (eviction) and reads it back by `Range`.
//! - **`segments`**: for each segment **this node** wrote, how many messages are still alive. With
//!   this the primary knows a segment died whole **without querying S3**.
//! - **`journal`**: the messages with changed state (or removed) since the last journal.
//!
//! No cap on the `outbox` (Q9): if the disk fills up, the node goes to `failed`, as with any
//! other lack of space. The methods are synchronous: whoever calls them runs them in `spawn_blocking`.

use std::io;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::db::to_io;

pub const BACKUP_DB_FILE: &str = "_backup.db";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS outbox (
        topic    TEXT NOT NULL,
        id       TEXT NOT NULL,
        added_at INTEGER NOT NULL,
        PRIMARY KEY (topic, id)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS outbox_added_at ON outbox (added_at);
    CREATE TABLE IF NOT EXISTS saved (
        topic   TEXT NOT NULL,
        id      TEXT NOT NULL,
        segment TEXT NOT NULL,
        off     INTEGER NOT NULL,
        len     INTEGER NOT NULL,
        saved_at INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (topic, id)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS saved_segment ON saved (segment);
    CREATE TABLE IF NOT EXISTS segments (
        key        TEXT PRIMARY KEY NOT NULL,
        topic      TEXT NOT NULL,
        topic_id   TEXT NOT NULL,
        total      INTEGER NOT NULL,
        live       INTEGER NOT NULL,
        bytes      INTEGER NOT NULL,
        created_at INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS journal (
        topic   TEXT NOT NULL,
        id      TEXT NOT NULL,
        removed INTEGER NOT NULL,
        PRIMARY KEY (topic, id)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS counters (
        name  TEXT PRIMARY KEY NOT NULL,
        value INTEGER NOT NULL
    ) WITHOUT ROWID;
";

/// Where a saved message is in the bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRef {
    pub segment: String,
    pub offset: u64,
    pub len: u64,
    /// When the segment's PUT was confirmed, on the clock of the node that wrote it (information, not to
    /// compare with another node's clock). `0`: unknown (old notice).
    pub saved_at_ms: u64,
}

/// The result of [`BackupDb::commit_segment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// How many of the segment's messages are still alive.
    pub live: u64,
    /// Those that died during the upload.
    pub dead: Vec<String>,
}

/// A segment this node wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRow {
    pub key: String,
    pub topic: String,
    pub topic_id: String,
    pub total: u64,
    pub live: u64,
    pub bytes: u64,
    pub created_ms: u64,
}

/// A state change not yet in the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirty {
    pub topic: String,
    pub id: String,
    pub removed: bool,
}

#[derive(Debug)]
pub struct BackupDb {
    conn: Mutex<Connection>,
}

impl BackupDb {
    pub fn open(dir: &Path) -> io::Result<Self> {
        let conn = Connection::open(dir.join(BACKUP_DB_FILE)).map_err(to_io)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(to_io)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(to_io)?;
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        // A `_backup.db` from before `saved_at` gets the column.
        let has: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('saved') WHERE name = 'saved_at'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(to_io)?
            > 0;
        if !has {
            conn.execute_batch("ALTER TABLE saved ADD COLUMN saved_at INTEGER NOT NULL DEFAULT 0")
                .map_err(to_io)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- outbox ----

    /// Puts the message in the send queue. Repeating does nothing.
    pub fn enqueue(&self, topic: &str, id: &str, now_ms: u64) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT OR IGNORE INTO outbox (topic, id, added_at) VALUES (?1, ?2, ?3)",
                params![topic, id, now_ms as i64],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// The queue's messages added up to `before_ms` (inclusive), the oldest first, at most
    /// `limit`. `before_ms = u64::MAX`: all of them.
    pub fn outbox(&self, before_ms: u64, limit: usize) -> io::Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare_cached(
                "SELECT topic, id FROM outbox WHERE added_at <= ?1 ORDER BY topic, id LIMIT ?2",
            )
            .map_err(to_io)?;
        let rows = stmt
            .query_map(
                params![i64::try_from(before_ms).unwrap_or(i64::MAX), limit as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    pub fn outbox_len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// Takes it off the queue (the message was consumed before the window closed, or stopped being this
    /// node's). `true` if it was there.
    pub fn unqueue(&self, topic: &str, id: &str) -> io::Result<bool> {
        let n = self
            .conn()
            .execute(
                "DELETE FROM outbox WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(to_io)?;
        Ok(n > 0)
    }

    // ---- segments and references ----

    /// Records a segment the PUT confirmed: the segment, each message's reference and their
    /// exit from the `outbox`, in a single transaction.
    ///
    /// A message that was no longer in the `outbox` died while the segment was going up (ack or `ttl`):
    /// it does not count as alive and goes into the journal as a removal, so the restore does not
    /// resurrect it. `segment.live` is recalculated here.
    pub fn commit_segment(
        &self,
        segment: &SegmentRow,
        entries: &[(String, u64, u64)],
    ) -> io::Result<Committed> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        let mut live = 0u64;
        let mut dead = Vec::new();
        for (id, offset, len) in entries {
            let queued = tx
                .execute(
                    "DELETE FROM outbox WHERE topic = ?1 AND id = ?2",
                    params![segment.topic, id],
                )
                .map_err(to_io)?;
            if queued == 0 {
                tx.execute(
                    "INSERT OR REPLACE INTO journal (topic, id, removed) VALUES (?1, ?2, 1)",
                    params![segment.topic, id],
                )
                .map_err(to_io)?;
                dead.push(id.clone());
                continue;
            }
            tx.execute(
                "INSERT OR REPLACE INTO saved (topic, id, segment, off, len, saved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    segment.topic,
                    id,
                    segment.key,
                    *offset as i64,
                    *len as i64,
                    segment.created_ms as i64
                ],
            )
            .map_err(to_io)?;
            live += 1;
        }
        tx.execute(
            "INSERT OR REPLACE INTO segments (key, topic, topic_id, total, live, bytes, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                segment.key,
                segment.topic,
                segment.topic_id,
                entries.len() as i64,
                live as i64,
                segment.bytes as i64,
                segment.created_ms as i64
            ],
        )
        .map_err(to_io)?;
        tx.commit().map_err(to_io)?;
        Ok(Committed { live, dead })
    }

    /// Records a segment this node took over administering (the restore read it from the bucket).
    pub fn put_segment(&self, segment: &SegmentRow) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO segments (key, topic, topic_id, total, live, bytes, created_at)                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    segment.key,
                    segment.topic,
                    segment.topic_id,
                    segment.total as i64,
                    segment.live as i64,
                    segment.bytes as i64,
                    segment.created_ms as i64
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// How many messages have a reference in the bucket.
    pub fn saved_len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM saved", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// Is the message still waiting in the `outbox`?
    pub fn queued(&self, topic: &str, id: &str) -> io::Result<bool> {
        self.conn()
            .query_row(
                "SELECT 1 FROM outbox WHERE topic = ?1 AND id = ?2",
                params![topic, id],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(to_io)
    }

    /// Stores the reference announced by the primary (the replica and the heir use it to read from the bucket
    /// what they deleted from disk).
    pub fn set_saved(&self, topic: &str, id: &str, saved: &SavedRef) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO saved (topic, id, segment, off, len, saved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    topic,
                    id,
                    saved.segment,
                    saved.offset as i64,
                    saved.len as i64,
                    saved.saved_at_ms as i64
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    pub fn saved(&self, topic: &str, id: &str) -> io::Result<Option<SavedRef>> {
        self.conn()
            .query_row(
                "SELECT segment, off, len, saved_at FROM saved WHERE topic = ?1 AND id = ?2",
                params![topic, id],
                |row| {
                    Ok(SavedRef {
                        segment: row.get(0)?,
                        offset: row.get::<_, i64>(1)? as u64,
                        len: row.get::<_, i64>(2)? as u64,
                        saved_at_ms: row.get::<_, i64>(3)? as u64,
                    })
                },
            )
            .optional()
            .map_err(to_io)
    }

    /// The topic's messages that already have a reference in the bucket, with it.
    pub fn saved_of_topic(&self, topic: &str) -> io::Result<Vec<(String, SavedRef)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id, segment, off, len, saved_at FROM saved WHERE topic = ?1")
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![topic], |row| {
                Ok((
                    row.get(0)?,
                    SavedRef {
                        segment: row.get(1)?,
                        offset: row.get::<_, i64>(2)? as u64,
                        len: row.get::<_, i64>(3)? as u64,
                        saved_at_ms: row.get::<_, i64>(4)? as u64,
                    },
                ))
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// The message died (ack, `ttl`, dead-letter): it leaves the `outbox` and the saved list, and its
    /// segment loses a live one. Returns the segment if all its messages died (the
    /// primary deletes it from the bucket) and whether the message was saved. The removal goes into the journal.
    pub fn mark_dead(&self, topic: &str, id: &str) -> io::Result<Option<String>> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        let was_queued = tx
            .execute(
                "DELETE FROM outbox WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(to_io)?
            > 0;
        let segment: Option<String> = tx
            .query_row(
                "SELECT segment FROM saved WHERE topic = ?1 AND id = ?2",
                params![topic, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(to_io)?;
        let mut empty = None;
        if let Some(segment) = segment {
            tx.execute(
                "DELETE FROM saved WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(to_io)?;
            tx.execute(
                "UPDATE segments SET live = MAX(live - 1, 0) WHERE key = ?1",
                params![segment],
            )
            .map_err(to_io)?;
            let live: Option<i64> = tx
                .query_row(
                    "SELECT live FROM segments WHERE key = ?1",
                    params![segment],
                    |row| row.get(0),
                )
                .optional()
                .map_err(to_io)?;
            if live == Some(0) {
                empty = Some(segment);
            }
            tx.execute(
                "INSERT OR REPLACE INTO journal (topic, id, removed) VALUES (?1, ?2, 1)",
                params![topic, id],
            )
            .map_err(to_io)?;
        } else if was_queued {
            // It was still waiting to be sent: it never left the node, there is nothing to record.
            tx.execute(
                "DELETE FROM journal WHERE topic = ?1 AND id = ?2 AND removed = 0",
                params![topic, id],
            )
            .map_err(to_io)?;
        } else {
            // Neither saved nor queued: the message was inherited from another primary (which may have
            // saved it) or its segment is in flight. The removal goes into the journal to be safe:
            // without it the restore would resurrect an acknowledged message.
            tx.execute(
                "INSERT OR REPLACE INTO journal (topic, id, removed) VALUES (?1, ?2, 1)",
                params![topic, id],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)?;
        Ok(empty)
    }

    /// The message died on a node that is not the owner (replica): it only forgets the reference and the queue.
    /// The journal and the segment count belong to the owner.
    pub fn forget(&self, topic: &str, id: &str) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        for table in ["outbox", "saved"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE topic = ?1 AND id = ?2"),
                params![topic, id],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    /// The segment was deleted from the bucket: forget its row.
    pub fn drop_segment(&self, key: &str) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        tx.execute("DELETE FROM segments WHERE key = ?1", params![key])
            .map_err(to_io)?;
        tx.execute("DELETE FROM saved WHERE segment = ?1", params![key])
            .map_err(to_io)?;
        tx.commit().map_err(to_io)
    }

    /// The segments this node wrote, for the topic.
    pub fn segments_of(&self, topic: &str) -> io::Result<Vec<SegmentRow>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT key, topic, topic_id, total, live, bytes, created_at FROM segments \
                 WHERE topic = ?1 ORDER BY key",
            )
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![topic], row_to_segment)
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    pub fn segment(&self, key: &str) -> io::Result<Option<SegmentRow>> {
        self.conn()
            .query_row(
                "SELECT key, topic, topic_id, total, live, bytes, created_at FROM segments \
                 WHERE key = ?1",
                params![key],
                row_to_segment,
            )
            .optional()
            .map_err(to_io)
    }

    /// How many segments and how many bytes this node keeps in the bucket.
    pub fn segment_totals(&self) -> io::Result<(u64, u64)> {
        self.conn()
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(bytes), 0) FROM segments",
                [],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
            )
            .map_err(to_io)
    }

    // ---- journal ----

    /// The message changed state (delivery, invisibility): it goes into the next journal. Only applies
    /// to a message already saved in a segment; pending ones go with the current state.
    pub fn touch(&self, topic: &str, id: &str) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT INTO journal (topic, id, removed) \
                 SELECT ?1, ?2, 0 WHERE EXISTS \
                 (SELECT 1 FROM saved WHERE topic = ?1 AND id = ?2) \
                 ON CONFLICT(topic, id) DO NOTHING",
                params![topic, id],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// The changes not yet in the journal.
    pub fn dirty(&self, limit: usize) -> io::Result<Vec<Dirty>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT topic, id, removed FROM journal ORDER BY topic, id LIMIT ?1")
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Dirty {
                    topic: row.get(0)?,
                    id: row.get(1)?,
                    removed: row.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// The journal was written: removes its changes from the list (those that changed again in the meantime stay,
    /// because `touch` does not duplicate them: the written state was already the last one read).
    pub fn clear_dirty(&self, items: &[Dirty]) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        for item in items {
            tx.execute(
                "DELETE FROM journal WHERE topic = ?1 AND id = ?2",
                params![item.topic, item.id],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    pub fn dirty_len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM journal", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    // ---- general ----

    /// The next sequence number (to name segments and journals without repeating).
    pub fn next_seq(&self) -> io::Result<u64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO counters (name, value) VALUES ('seq', 1) \
             ON CONFLICT(name) DO UPDATE SET value = value + 1",
            [],
        )
        .map_err(to_io)?;
        let n: i64 = conn
            .query_row("SELECT value FROM counters WHERE name = 'seq'", [], |row| {
                row.get(0)
            })
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// The topic was deleted: forget everything about it.
    pub fn drop_topic(&self, topic: &str) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        for table in ["outbox", "saved", "segments", "journal"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE topic = ?1"),
                params![topic],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    /// Deletes everything (the node comes back as a new node).
    pub fn reset(&self) -> io::Result<()> {
        self.conn()
            .execute_batch(
                "DELETE FROM outbox; DELETE FROM saved; DELETE FROM segments; DELETE FROM journal;",
            )
            .map_err(to_io)
    }
}

fn row_to_segment(row: &rusqlite::Row<'_>) -> rusqlite::Result<SegmentRow> {
    Ok(SegmentRow {
        key: row.get(0)?,
        topic: row.get(1)?,
        topic_id: row.get(2)?,
        total: row.get::<_, i64>(3)? as u64,
        live: row.get::<_, i64>(4)? as u64,
        bytes: row.get::<_, i64>(5)? as u64,
        created_ms: row.get::<_, i64>(6)? as u64,
    })
}

#[cfg(test)]
mod tests;
