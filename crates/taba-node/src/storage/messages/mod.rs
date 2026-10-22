//! The messages on disk (doc 02, section 3.2): the body in `${topic}/${id}.body`, only the bytes
//! as they came, and the rest in a row of the `messages` table of the topic's database,
//! `${topic}/_topic.db`, strictly local to the node.
//!
//! **Write order:** on writing, first the file and then the row; on removal,
//! first the row and then the file. A crash leaves at most a file without a row (harmless
//! garbage) or a row without a file (the message was lost: it is discarded on read).
//!
//! **Single writer:** each database has a connection behind a `Mutex`; every write waits for
//! its turn. The methods are synchronous: the `engine` calls them outside the async executor
//! (`spawn_blocking`).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use super::db::to_io;
use crate::config::DbSynchronous;

mod excluded;
use excluded::ExcludedLog;
pub use excluded::{
    DEFAULT_MAX_AGE_MS, DEFAULT_MAX_BYTES, ExcludedLimits, ExcludedRow, merged_since,
};

pub const TOPIC_DB_FILE: &str = "_topic.db";
const BODY_EXT: &str = "body";

/// Everything known about a message except the body: it is what goes into the database row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageMeta {
    pub id: String,
    pub content_type: String,
    pub group_id: Option<String>,
    /// FIFO queue only: the deduplication key within the group (the given one, or the SHA-256 of the
    /// body in hexadecimal).
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    pub delivery_count: u32,
    /// Instant from which the message is deliverable. `0`: it was never delivered.
    pub invisible_until_ms: u64,
    pub receipt_token: Option<String>,
    /// When it entered the dead-letter (and why). From then on the `ttl` counts from this instant.
    pub dead_lettered_ms: Option<u64>,
    pub dead_letter_reason: Option<String>,
    /// The position on the hash ring (doc 05, section 7.1): it is how the message's block is found.
    /// Whoever stores the message computes it (a FIFO queue's is the group's).
    pub position: u64,
}

/// The position on the ring as the database stores it: the `u64` with the highest bit flipped, in a signed
/// integer that keeps the same order (SQLite has no unsigned integer).
fn position_to_db(position: u64) -> i64 {
    (position ^ (1 << 63)) as i64
}

fn position_from_db(stored: i64) -> u64 {
    (stored as u64) ^ (1 << 63)
}

impl MessageMeta {
    /// The instant the `ttl` counts from.
    pub fn age_base_ms(&self) -> u64 {
        self.dead_lettered_ms.unwrap_or(self.published_ms)
    }
}

/// A row of [`MessageStore::block_rows`]: what a block's ID listing says about each
/// message (doc 06c, `GET /cluster/blocks/{id}/ids`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRow {
    pub id: String,
    /// FIFO queue only, on the short row the ack leaves.
    pub delete_at_ms: Option<u64>,
}

/// A row of [`MessageStore::scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRow {
    pub id: String,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    /// The short row the FIFO queue's ack leaves for deduplication (`deleteAt`): no body, no
    /// delivery and outside the index.
    pub delete_at_ms: Option<u64>,
    pub age_ms: u64,
    pub delivery_count: u32,
    pub invisible_until_ms: u64,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS messages (
        id                 TEXT PRIMARY KEY NOT NULL,
        content_type       TEXT NOT NULL,
        group_id              TEXT,
        published_at       INTEGER NOT NULL,
        delivery_count     INTEGER NOT NULL DEFAULT 0,
        invisible_until    INTEGER NOT NULL DEFAULT 0,
        receipt_token      TEXT,
        dead_lettered_at   INTEGER,
        dead_letter_reason TEXT,
        age_base           INTEGER NOT NULL,
        deduplication_id   TEXT,
        delete_at          INTEGER,
        position           INTEGER NOT NULL DEFAULT 0
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS messages_invisible_until ON messages (invisible_until);
    CREATE INDEX IF NOT EXISTS messages_age_base ON messages (age_base);
    CREATE TABLE IF NOT EXISTS excluded (
        id       TEXT PRIMARY KEY NOT NULL,
        position INTEGER NOT NULL,
        at_ms    INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS excluded_at ON excluded (at_ms);
    CREATE INDEX IF NOT EXISTS excluded_position ON excluded (position);
";

/// The indexes of the FIFO queue columns, created after `migrate` (a database from before them
/// does not have them yet).
const FIFO_INDEXES: &str = "
    CREATE INDEX IF NOT EXISTS messages_group_dedup ON messages (group_id, deduplication_id);
    CREATE INDEX IF NOT EXISTS messages_delete_at ON messages (delete_at);
    CREATE INDEX IF NOT EXISTS messages_position ON messages (position);
";

/// Adds to the database of an earlier version the columns it lacks.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('messages')")?;
    let have: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    for (name, kind) in [
        ("deduplication_id", "TEXT"),
        ("delete_at", "INTEGER"),
        ("position", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !have.iter().any(|c| c == name) {
            conn.execute_batch(&format!("ALTER TABLE messages ADD COLUMN {name} {kind}"))?;
        }
    }
    Ok(())
}

const COLUMNS: &str = "id, content_type, group_id, published_at, delivery_count, invisible_until, \
                       receipt_token, dead_lettered_at, dead_letter_reason, deduplication_id, \
                       position";

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageMeta> {
    Ok(MessageMeta {
        id: row.get(0)?,
        content_type: row.get(1)?,
        group_id: row.get(2)?,
        published_ms: row.get::<_, i64>(3)? as u64,
        delivery_count: row.get::<_, i64>(4)? as u32,
        invisible_until_ms: row.get::<_, i64>(5)? as u64,
        receipt_token: row.get(6)?,
        dead_lettered_ms: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        dead_letter_reason: row.get(8)?,
        deduplication_id: row.get(9)?,
        position: position_from_db(row.get::<_, i64>(10)?),
    })
}

/// A topic's folder. Topic names are case-sensitive and the disk of Windows and macOS is not: each
/// capital becomes `~` plus the lowercase (`Orders` → `~orders`).
/// The `~` cannot be in a topic name (`A-Za-z0-9_-`), so the swap is not ambiguous.
pub fn dir_name(topic: &str) -> String {
    let mut stem = String::with_capacity(topic.len());
    for c in topic.chars() {
        if c.is_ascii_uppercase() {
            stem.push('~');
        }
        stem.push(c.to_ascii_lowercase());
    }
    stem
}

/// The topic name of a folder (the inverse of `dir_name`); `None` if it is not one of our folders.
fn topic_of_dir(dir: &str) -> Option<String> {
    let mut topic = String::with_capacity(dir.len());
    let mut chars = dir.chars();
    while let Some(c) = chars.next() {
        if c == '~' {
            let next = chars.next().filter(char::is_ascii_lowercase)?;
            topic.push(next.to_ascii_uppercase());
        } else if c.is_ascii_uppercase() {
            return None;
        } else {
            topic.push(c);
        }
    }
    Some(topic)
}

/// A topic's disk: the folder and the database.
#[derive(Debug)]
struct TopicDisk {
    dir: PathBuf,
    conn: Mutex<Connection>,
}

impl TopicDisk {
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn body_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{BODY_EXT}"))
    }
}

/// All the node's messages on disk, by topic. The databases open on demand: a topic that
/// never went to disk has no folder.
#[derive(Debug)]
pub struct MessageStore {
    root: PathBuf,
    synchronous: DbSynchronous,
    topics: Mutex<HashMap<String, Arc<TopicDisk>>>,
    /// The log of excluded messages (doc 37, section 4.4).
    excluded: ExcludedLog,
}

impl MessageStore {
    /// `root` is the `storage.dir`; each topic gets a folder inside it.
    pub fn new(root: &Path, synchronous: DbSynchronous) -> Self {
        Self {
            root: root.to_path_buf(),
            synchronous,
            topics: Mutex::new(HashMap::new()),
            excluded: ExcludedLog::default(),
        }
    }

    fn topics(&self) -> MutexGuard<'_, HashMap<String, Arc<TopicDisk>>> {
        self.topics.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The topic's disk. Without `create`, a topic without a folder is `None` (reads create nothing).
    fn disk(&self, topic: &str, create: bool) -> io::Result<Option<Arc<TopicDisk>>> {
        let mut topics = self.topics();
        if let Some(disk) = topics.get(topic) {
            return Ok(Some(disk.clone()));
        }
        let dir = self.root.join(dir_name(topic));
        if !create && !dir.join(TOPIC_DB_FILE).is_file() {
            return Ok(None);
        }
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join(TOPIC_DB_FILE)).map_err(to_io)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(to_io)?;
        conn.pragma_update(
            None,
            "synchronous",
            super::db::synchronous_name(self.synchronous),
        )
        .map_err(to_io)?;
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        migrate(&conn).map_err(to_io)?;
        conn.execute_batch(FIFO_INDEXES).map_err(to_io)?;
        let disk = Arc::new(TopicDisk {
            dir,
            conn: Mutex::new(conn),
        });
        topics.insert(topic.to_owned(), disk.clone());
        Ok(Some(disk))
    }

    /// Writes the message: first the body file, then the row.
    pub fn put(&self, topic: &str, meta: &MessageMeta, body: &[u8]) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("created");
        let target = disk.body_path(&meta.id);
        let tmp = target.with_extension(format!("{BODY_EXT}.tmp"));
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &target)?;
        Self::insert_row(&disk, meta)
    }

    fn insert_row(disk: &TopicDisk, meta: &MessageMeta) -> io::Result<()> {
        disk.conn()
            .execute(
                "INSERT OR REPLACE INTO messages (id, content_type, group_id, published_at, \
                 delivery_count, invisible_until, receipt_token, dead_lettered_at, \
                 dead_letter_reason, age_base, deduplication_id, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    meta.id,
                    meta.content_type,
                    meta.group_id,
                    meta.published_ms as i64,
                    meta.delivery_count,
                    meta.invisible_until_ms as i64,
                    meta.receipt_token,
                    meta.dead_lettered_ms.map(|v| v as i64),
                    meta.dead_letter_reason,
                    meta.age_base_ms() as i64,
                    meta.deduplication_id,
                    position_to_db(meta.position),
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    pub fn get(&self, topic: &str, id: &str) -> io::Result<Option<MessageMeta>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        disk.conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"),
                params![id],
                from_row,
            )
            .optional()
            .map_err(to_io)
    }

    /// Writes again the fields that change on delivery: counter, deadline and `receiptToken`.
    pub fn update_delivery(&self, topic: &str, meta: &MessageMeta) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        disk.conn()
            .execute(
                "UPDATE messages SET delivery_count = ?2, invisible_until = ?3, \
                 receipt_token = ?4 WHERE id = ?1",
                params![
                    meta.id,
                    meta.delivery_count,
                    meta.invisible_until_ms as i64,
                    meta.receipt_token
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// The body bytes, as they were written.
    pub fn body(&self, topic: &str, id: &str) -> io::Result<Vec<u8>> {
        match self.disk(topic, false)? {
            Some(disk) => std::fs::read(disk.body_path(id)),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    /// Removes the message: first the row, then the file.
    pub fn remove(&self, topic: &str, id: &str) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        disk.conn()
            .execute("DELETE FROM messages WHERE id = ?1", params![id])
            .map_err(to_io)?;
        match std::fs::remove_file(disk.body_path(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn ids(&self, topic: &str, sql: &str, args: &[i64]) -> io::Result<Vec<String>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        let conn = disk.conn();
        let mut stmt = conn.prepare_cached(sql).map_err(to_io)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |row| row.get(0))
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// What the topic's index needs from each message on disk (without the body), to rebuild it
    /// when the topic is opened: `id`, group, age, deliveries and end of invisibility.
    pub fn scan(&self, topic: &str) -> io::Result<Vec<ScanRow>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        let conn = disk.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, group_id, age_base, delivery_count, invisible_until, deduplication_id, \
                 published_at, delete_at FROM messages ORDER BY id",
            )
            .map_err(to_io)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ScanRow {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    age_ms: row.get::<_, i64>(2)? as u64,
                    delivery_count: row.get::<_, i64>(3)? as u32,
                    invisible_until_ms: row.get::<_, i64>(4)? as u64,
                    deduplication_id: row.get(5)?,
                    published_ms: row.get::<_, i64>(6)? as u64,
                    delete_at_ms: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                })
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// The topic's messages whose position falls in any of the `ranges` (a block's arcs), with
    /// `id` between `from` and `to` (inclusive), in `id` order, at most `limit`. Includes the FIFO queue's
    /// short row (`delete_at`).
    pub fn block_rows(
        &self,
        topic: &str,
        ranges: &[(u64, u64)],
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<BlockRow>> {
        use rusqlite::types::Value;
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        if ranges.is_empty() {
            return Ok(Vec::new());
        }
        let mut args: Vec<Value> = Vec::new();
        let mut sql = String::from("SELECT id, delete_at FROM messages WHERE (");
        for (i, (low, high)) in ranges.iter().enumerate() {
            if i > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str("position BETWEEN ? AND ?");
            args.push(Value::Integer(position_to_db(*low)));
            args.push(Value::Integer(position_to_db(*high)));
        }
        sql.push(')');
        if let Some(from) = from {
            sql.push_str(" AND id >= ?");
            args.push(Value::Text(from.to_owned()));
        }
        if let Some(to) = to {
            sql.push_str(" AND id <= ?");
            args.push(Value::Text(to.to_owned()));
        }
        sql.push_str(" ORDER BY id LIMIT ?");
        args.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
        let conn = disk.conn();
        let mut stmt = conn.prepare(&sql).map_err(to_io)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |row| {
                Ok(BlockRow {
                    id: row.get(0)?,
                    delete_at_ms: row.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                })
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// The first `limit` deliverable ids (deadline expired), in `id` order.
    pub fn ready_ids(&self, topic: &str, now_ms: u64, limit: usize) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM messages WHERE invisible_until <= ?1 ORDER BY id LIMIT ?2",
            &[now_ms as i64, limit as i64],
        )
    }

    /// The instant the next invisible one becomes deliverable again.
    pub fn next_invisible_until(&self, topic: &str, now_ms: u64) -> io::Result<Option<u64>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        let next: Option<i64> = disk
            .conn()
            .query_row(
                "SELECT MIN(invisible_until) FROM messages WHERE invisible_until > ?1",
                params![now_ms as i64],
                |row| row.get(0),
            )
            .map_err(to_io)?;
        Ok(next.map(|v| v as u64))
    }

    /// The messages whose `ttl` expired: `age_base + ttl ≤ now`.
    pub fn expired_ids(&self, topic: &str, now_ms: u64, ttl_ms: u64) -> io::Result<Vec<String>> {
        let limit = i64::try_from(now_ms.saturating_sub(ttl_ms)).unwrap_or(i64::MAX);
        if ttl_ms > now_ms {
            return Ok(Vec::new());
        }
        self.ids(
            topic,
            "SELECT id FROM messages WHERE age_base <= ?1",
            &[limit],
        )
    }

    /// The messages that used up their deliveries and whose invisibility ended.
    pub fn exhausted_ids(&self, topic: &str, now_ms: u64, max: u64) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM messages WHERE delivery_count > 0 AND delivery_count >= ?1 \
             AND invisible_until <= ?2",
            &[i64::try_from(max).unwrap_or(i64::MAX), now_ms as i64],
        )
    }

    /// Moves the message to another topic's folder (the dead-letter), swapping the metadata.
    /// The file is renamed, without copying the body.
    pub fn move_to(&self, from: &str, to: &str, meta: &MessageMeta) -> io::Result<()> {
        let source = self.disk(from, true)?.expect("created");
        let target = self.disk(to, true)?.expect("created");
        std::fs::rename(source.body_path(&meta.id), target.body_path(&meta.id))?;
        Self::insert_row(&target, meta)?;
        // It leaves the source dead: the exclusion is recorded in the transaction that deletes it.
        let mut conn = source.conn();
        let tx = conn.transaction().map_err(to_io)?;
        tx.execute(
            excluded::RECORD_FROM_ROW,
            params![meta.id, meta.dead_lettered_ms.unwrap_or_default() as i64],
        )
        .map_err(to_io)?;
        tx.execute("DELETE FROM messages WHERE id = ?1", params![meta.id])
            .map_err(to_io)?;
        tx.commit().map_err(to_io)
    }

    /// The FIFO queue's ack: the message becomes a short row with `delete_at` (doc 17, section 16.7).
    /// What stays is `id`, group, `deduplicationId` and the dates; the body and delivery metadata go. The
    /// row applies even if the message was only in memory. First the row, then the file.
    pub fn tombstone(&self, topic: &str, meta: &MessageMeta, delete_at_ms: u64) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("created");
        write_tombstone(&disk.conn(), meta, delete_at_ms)?;
        match std::fs::remove_file(disk.body_path(&meta.id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Deletes the short rows whose `delete_at` has passed, by the index, and returns how many.
    pub fn sweep_tombstones(&self, topic: &str, now_ms: u64) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let removed = disk
            .conn()
            .execute(
                "DELETE FROM messages WHERE delete_at IS NOT NULL AND delete_at <= ?1",
                params![now_ms as i64],
            )
            .map_err(to_io)?;
        Ok(removed as u64)
    }

    /// The free space, in bytes, on the `storage.dir` volume (what an unprivileged process can
    /// use).
    pub fn free_bytes(&self) -> io::Result<u64> {
        fs4::available_space(&self.root)
    }

    /// The usage of the `storage.dir` volume, as a percentage (of what an unprivileged process sees).
    pub fn disk_usage_percent(&self) -> io::Result<u32> {
        let total = fs4::total_space(&self.root)?;
        if total == 0 {
            return Ok(0);
        }
        let free = fs4::available_space(&self.root)?.min(total);
        Ok(((total - free) * 100 / total) as u32)
    }

    /// Deletes only the body file, keeping the row (the backup's eviction, doc 23, section 1.8): the
    /// "file first, row after" order still holds. Returns the bytes freed; `0` if the
    /// body was no longer on disk.
    pub fn evict_body(&self, topic: &str, id: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let path = disk.body_path(id);
        let size = match std::fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(size),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Is the message body in a file on disk?
    pub fn has_body(&self, topic: &str, id: &str) -> bool {
        self.disk(topic, false)
            .ok()
            .flatten()
            .is_some_and(|disk| disk.body_path(id).is_file())
    }

    /// The sum of the sizes of the topic's body files (reads only the files' metadata, in
    /// folder order: it costs one call per message on disk).
    pub fn body_bytes(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let mut total = 0;
        for entry in std::fs::read_dir(&disk.dir)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == BODY_EXT) {
                match entry.metadata() {
                    Ok(meta) => total += meta.len(),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(total)
    }

    pub fn count(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let n: i64 = disk
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE delete_at IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// The topics that have a folder with a database in `storage.dir`.
    pub fn topics_on_disk(&self) -> io::Result<Vec<String>> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if !path.join(TOPIC_DB_FILE).is_file() {
                continue;
            }
            if let Some(topic) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(topic_of_dir)
            {
                out.push(topic);
            }
        }
        Ok(out)
    }

    /// The folder of a disk with no manifest (a version before the quarantine per life): it waits for the restart
    /// recovery of the losses and is deleted at the end of it (PRB-048; doc 37, section 4.12).
    pub const PREVIOUS_LIFE: &str = "_previous-life";

    /// The folder that holds one subfolder per previous life, named with the node ID of that life (doc 37,
    /// section 4.5). Nothing here is deleted by the recovery.
    pub const LIVES: &str = "_lives";

    /// The manifest of a life, in the root of the disk of the running life and, afterwards, in its folder.
    pub const LIFE_MANIFEST: &str = "manifest.json";

    /// The `storage.dir`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Sets aside what was left on the previous life's disk: each topic folder (and the manifest) goes to
    /// `${root}/_lives/<life>`, and the node goes on with an empty disk. Without `life` (a disk with no manifest),
    /// the folders go to `${root}/_previous-life`. A topic folder that was already there (a restart that crashed
    /// halfway through the move) stays as it is and the new one is deleted: the first is the one that may have
    /// what only it kept; the new one only brings copies the cluster already has. Returns the previous life's
    /// disk, or `None` if there are no messages on it.
    pub fn set_aside_old_disk(&self, life: Option<&str>) -> io::Result<Option<MessageStore>> {
        if life.is_some_and(|id| id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.')) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the ID of the life is not a folder name",
            ));
        }
        let aside = match life {
            Some(id) => self.root.join(Self::LIVES).join(id),
            None => self.root.join(Self::PREVIOUS_LIFE),
        };
        for topic in self.topics_on_disk()? {
            let from = self.root.join(dir_name(&topic));
            let to = aside.join(dir_name(&topic));
            self.topics().remove(&topic);
            if to.exists() {
                std::fs::remove_dir_all(&from)?;
                continue;
            }
            std::fs::create_dir_all(&aside)?;
            move_dir(&from, &to)?;
        }
        let manifest = self.root.join(Self::LIFE_MANIFEST);
        if life.is_some() && manifest.is_file() {
            std::fs::create_dir_all(&aside)?;
            let to = aside.join(Self::LIFE_MANIFEST);
            // The one from the running disk is the latest: it replaces the one of a move that crashed.
            let _ = std::fs::remove_file(&to);
            std::fs::rename(&manifest, &to)?;
        }
        let previous = MessageStore::new(&aside, self.synchronous);
        Ok((!previous.topics_on_disk()?.is_empty()).then_some(previous))
    }

    /// Deletes the folder of a disk with no manifest (its recovery is over). The folders of the lives stay.
    pub fn drop_old_disk(&self) -> io::Result<()> {
        match std::fs::remove_dir_all(self.root.join(Self::PREVIOUS_LIFE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Deletes the topic's folder, with the database and the bodies.
    pub fn drop_topic(&self, topic: &str) -> io::Result<()> {
        self.topics().remove(topic);
        match std::fs::remove_dir_all(self.root.join(dir_name(topic))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Writes the FIFO queue's short row (the SQL of `tombstone`, shared with the one that also records the
/// exclusion in the same transaction).
fn write_tombstone(conn: &Connection, meta: &MessageMeta, delete_at_ms: u64) -> io::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO messages (id, content_type, group_id, published_at, \
         delivery_count, invisible_until, receipt_token, dead_lettered_at, \
         dead_letter_reason, age_base, deduplication_id, delete_at, position) \
         VALUES (?1, '', ?2, ?3, 0, 0, NULL, NULL, NULL, ?4, ?5, ?6, ?7)",
        params![
            meta.id,
            meta.group_id,
            meta.published_ms as i64,
            meta.published_ms as i64,
            meta.deduplication_id,
            delete_at_ms as i64,
            position_to_db(meta.position),
        ],
    )
    .map(|_| ())
    .map_err(to_io)
}

/// Moves a folder. Where the system does not allow renaming (on Windows, with a file of the folder still
/// open), it copies and deletes the original, repeating the delete for up to three seconds.
fn move_dir(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_dir(from, to)?;
    let mut result = std::fs::remove_dir_all(from);
    for _ in 0..30 {
        if result.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        result = std::fs::remove_dir_all(from);
    }
    result
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
