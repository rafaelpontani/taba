//! The log of **excluded** messages (doc 07, section 10.3): the `excluded` table of each topic's database,
//! one row per message that left the topic dead (ack, `ttl`, prune, or sent to the dead-letter), with the
//! ring position (the block is found by it) and the instant. The row is written **in the same transaction**
//! that deletes the message, so a crash leaves both or neither, and with `storage.db.synchronous=NORMAL` it
//! costs no extra fsync.
//!
//! The log has a limit of size and age; the oldest rows go first. The **start of the continuous recording**
//! (`excluded_since`) is the instant from which the node guarantees to have every exclusion: it begins when
//! the log is created and only moves forward when a row is discarded by a limit (a cluster without exclusions
//! for three days covers the three days).

use std::io;
use std::sync::Mutex;

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, params};

use super::{MessageMeta, MessageStore, position_from_db, position_to_db, to_io, write_tombstone};
use crate::storage::state::NodeStateStore;

/// Default limits of the log (doc 07, section 10.3): 100 MB and 24 hours.
pub const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const DEFAULT_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

/// What a row weighs besides the `id`, for the size limit (position, instant and the table's own cost).
const ROW_OVERHEAD: i64 = 24;

/// Records, from the message's own row, the exclusion (`?1` is the `id`, `?2` the instant).
pub(super) const RECORD_FROM_ROW: &str = "INSERT OR IGNORE INTO excluded (id, position, at_ms) \
     SELECT id, position, ?2 FROM messages WHERE id = ?1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExcludedLimits {
    pub max_bytes: u64,
    pub max_age_ms: u64,
}

impl Default for ExcludedLimits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_age_ms: DEFAULT_MAX_AGE_MS,
        }
    }
}

/// A row of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedRow {
    pub id: String,
    pub position: u64,
    pub at_ms: u64,
}

/// The log's node-level state: the limits and the start of the continuous recording.
#[derive(Debug, Default)]
pub(super) struct ExcludedLog {
    limits: ExcludedLimits,
    /// Where `since` is kept between starts; without it (tests) it lives only in memory.
    state: Option<NodeStateStore>,
    since: Mutex<Option<u64>>,
}

impl ExcludedLog {
    pub(super) fn new(limits: ExcludedLimits, state: Option<NodeStateStore>) -> Self {
        Self {
            limits,
            state,
            since: Mutex::new(None),
        }
    }

    fn since(&self) -> std::sync::MutexGuard<'_, Option<u64>> {
        self.since.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The start of the recording after incorporating the log of a **past life** (doc 07, section 10.3): the
/// old interval `[past_since, past_end]` and the current one `[own_since, ...)` must meet, with no hole;
/// then the start goes back to the old one. If the node was down for longer than the sources remember, it
/// stays where it is.
pub fn merged_since(own_since: u64, past_since: u64, past_end: u64) -> u64 {
    if past_end >= own_since {
        own_since.min(past_since)
    } else {
        own_since
    }
}

fn ranges_filter(
    ranges: &[(u64, u64)],
    from: Option<&str>,
    to: Option<&str>,
) -> (String, Vec<Value>) {
    let mut args: Vec<Value> = Vec::new();
    let mut sql = String::from("(");
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
    (sql, args)
}

impl MessageStore {
    /// Sets the limits of the log and where the start of the recording is kept (`_node.db`).
    pub fn with_excluded(mut self, limits: ExcludedLimits, state: Option<NodeStateStore>) -> Self {
        self.excluded = ExcludedLog::new(limits, state);
        self
    }

    pub fn excluded_limits(&self) -> ExcludedLimits {
        self.excluded.limits
    }

    /// Removes the message like `remove`, recording the exclusion in the same transaction. A message that is
    /// not on disk records nothing (`record_excluded` is for the one that was only in memory).
    pub fn remove_excluded(&self, topic: &str, id: &str, at_ms: u64) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        {
            let mut conn = disk.conn();
            let tx = conn.transaction().map_err(to_io)?;
            tx.execute(RECORD_FROM_ROW, params![id, at_ms as i64])
                .map_err(to_io)?;
            tx.execute("DELETE FROM messages WHERE id = ?1", params![id])
                .map_err(to_io)?;
            tx.commit().map_err(to_io)?;
        }
        match std::fs::remove_file(disk.body_path(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Records the exclusion of a message that was not on disk (it was only in memory).
    pub fn record_excluded(
        &self,
        topic: &str,
        id: &str,
        position: u64,
        at_ms: u64,
    ) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("created");
        disk.conn()
            .execute(
                "INSERT OR IGNORE INTO excluded (id, position, at_ms) VALUES (?1, ?2, ?3)",
                params![id, position_to_db(position), at_ms as i64],
            )
            .map(|_| ())
            .map_err(to_io)
    }

    /// The FIFO queue's ack like `tombstone`, recording the exclusion in the same transaction.
    pub fn tombstone_excluded(
        &self,
        topic: &str,
        meta: &MessageMeta,
        delete_at_ms: u64,
        at_ms: u64,
    ) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("created");
        {
            let mut conn = disk.conn();
            let tx = conn.transaction().map_err(to_io)?;
            write_tombstone(&tx, meta, delete_at_ms)?;
            tx.execute(
                "INSERT OR IGNORE INTO excluded (id, position, at_ms) VALUES (?1, ?2, ?3)",
                params![meta.id, position_to_db(meta.position), at_ms as i64],
            )
            .map_err(to_io)?;
            tx.commit().map_err(to_io)?;
        }
        match std::fs::remove_file(disk.body_path(&meta.id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Writes rows that came from elsewhere (the synchronization of a block, or the log of a past life).
    /// The oldest instant of each `id` stays.
    pub fn import_excluded(&self, topic: &str, rows: &[ExcludedRow]) -> io::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let disk = self.disk(topic, true)?.expect("created");
        let mut conn = disk.conn();
        let tx = conn.transaction().map_err(to_io)?;
        for row in rows {
            // (not an upsert: the table is called `excluded`, like the pseudo-table of `ON CONFLICT`)
            tx.execute(
                "INSERT OR IGNORE INTO excluded (id, position, at_ms) VALUES (?1, ?2, ?3)",
                params![row.id, position_to_db(row.position), row.at_ms as i64],
            )
            .map_err(to_io)?;
            tx.execute(
                "UPDATE excluded SET at_ms = MIN(at_ms, ?2) WHERE id = ?1",
                params![row.id, row.at_ms as i64],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    /// Is the message in the topic's log of excluded?
    pub fn is_excluded(&self, topic: &str, id: &str) -> io::Result<bool> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(false);
        };
        let found: Option<i64> = disk
            .conn()
            .query_row("SELECT 1 FROM excluded WHERE id = ?1", params![id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(to_io)?;
        Ok(found.is_some())
    }

    /// The topic's rows whose position falls in any of the `ranges` (a block), with `id` between `from` and
    /// `to` (inclusive), in `id` order, at most `limit`.
    pub fn excluded_rows(
        &self,
        topic: &str,
        ranges: &[(u64, u64)],
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<ExcludedRow>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        if ranges.is_empty() {
            return Ok(Vec::new());
        }
        let (filter, mut args) = ranges_filter(ranges, from, to);
        let sql =
            format!("SELECT id, position, at_ms FROM excluded WHERE {filter} ORDER BY id LIMIT ?");
        args.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
        let conn = disk.conn();
        let mut stmt = conn.prepare(&sql).map_err(to_io)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |row| {
                Ok(ExcludedRow {
                    id: row.get(0)?,
                    position: position_from_db(row.get::<_, i64>(1)?),
                    at_ms: row.get::<_, i64>(2)?.max(0) as u64,
                })
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// Every row of the topic's log, in `id` order.
    pub fn excluded_all(&self, topic: &str) -> io::Result<Vec<ExcludedRow>> {
        self.excluded_rows(topic, &[(0, u64::MAX)], None, None, usize::MAX >> 1)
    }

    /// The estimated size of the log (all topics), the same measure the size limit uses.
    pub fn excluded_bytes(&self) -> io::Result<u64> {
        let mut total = 0u64;
        for topic in self.topics_on_disk()? {
            total += self.excluded_bytes_since(&topic, 0)?;
        }
        Ok(total)
    }

    fn excluded_bytes_since(&self, topic: &str, from_ms: i64) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let bytes: i64 = disk
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(length(id) + ?1), 0) FROM excluded WHERE at_ms >= ?2",
                params![ROW_OVERHEAD, from_ms],
                |row| row.get(0),
            )
            .map_err(to_io)?;
        Ok(bytes.max(0) as u64)
    }

    /// Drops what the limits do not allow: first the rows older than the age limit, then the oldest ones
    /// until the log fits the size limit. When a row is discarded, the start of the recording moves
    /// forward to the instant of the newest one discarded. Returns how many rows it dropped.
    pub fn trim_excluded(&self, now_ms: u64) -> io::Result<u64> {
        let limits = self.excluded.limits;
        let topics = self.topics_on_disk()?;
        let mut removed = 0u64;
        let mut newest_dropped = 0u64;

        let age_cutoff = now_ms.saturating_sub(limits.max_age_ms) as i64;
        removed += self.drop_excluded_before(&topics, age_cutoff, &mut newest_dropped)?;

        let mut total = 0u64;
        for topic in &topics {
            total += self.excluded_bytes_since(topic, 0)?;
        }
        if total > limits.max_bytes {
            // The smallest instant from which what remains fits: a bisection over the instants.
            let (mut low, mut high) = (age_cutoff, now_ms as i64 + 1);
            while low < high {
                let middle = low + (high - low) / 2;
                let mut bytes = 0u64;
                for topic in &topics {
                    bytes += self.excluded_bytes_since(topic, middle)?;
                }
                if bytes <= limits.max_bytes {
                    high = middle;
                } else {
                    low = middle + 1;
                }
            }
            removed += self.drop_excluded_before(&topics, low, &mut newest_dropped)?;
        }
        if newest_dropped > 0 {
            self.advance_excluded_since(newest_dropped)?;
        }
        Ok(removed)
    }

    fn drop_excluded_before(
        &self,
        topics: &[String],
        before_ms: i64,
        newest_dropped: &mut u64,
    ) -> io::Result<u64> {
        let mut removed = 0;
        for topic in topics {
            let Some(disk) = self.disk(topic, false)? else {
                continue;
            };
            let conn = disk.conn();
            let newest: Option<i64> = conn
                .query_row(
                    "SELECT MAX(at_ms) FROM excluded WHERE at_ms < ?1",
                    params![before_ms],
                    |row| row.get(0),
                )
                .map_err(to_io)?;
            let Some(newest) = newest else { continue };
            *newest_dropped = (*newest_dropped).max(newest.max(0) as u64);
            removed += conn
                .execute("DELETE FROM excluded WHERE at_ms < ?1", params![before_ms])
                .map_err(to_io)? as u64;
        }
        Ok(removed)
    }

    /// The start of the continuous recording (ms): when the log was created, moved forward by every row
    /// discarded by a limit. The first call of a log that does not have one yet fixes it at `now_ms`.
    pub fn excluded_since(&self, now_ms: u64) -> io::Result<u64> {
        let mut cached = self.excluded.since();
        if let Some(since) = *cached {
            return Ok(since);
        }
        let stored = match &self.excluded.state {
            Some(state) => state.excluded_since()?,
            None => None,
        };
        let since = match stored {
            Some(since) => since,
            None => {
                if let Some(state) = &self.excluded.state {
                    state.set_excluded_since(now_ms)?;
                }
                now_ms
            }
        };
        *cached = Some(since);
        Ok(since)
    }

    /// The sources of the first synchronization tell from when they record: a node that has not inherited yet
    /// (a new one, or one that set its old disk aside) takes the **latest** of their dates, which is what the
    /// rows it received cover; after that, the date only moves forward. Returns whether it inherited now.
    pub fn inherit_excluded_since(&self, sources_ms: u64) -> io::Result<bool> {
        let Some(state) = &self.excluded.state else {
            self.set_excluded_since(sources_ms)?;
            return Ok(true);
        };
        if state.excluded_inherited()? {
            return Ok(false);
        }
        self.set_excluded_since(sources_ms)?;
        state.set_excluded_inherited(true)?;
        Ok(true)
    }

    /// A new life of the disk: the next synchronization inherits the date again.
    pub fn forget_excluded_inheritance(&self) -> io::Result<()> {
        match &self.excluded.state {
            Some(state) => state.set_excluded_inherited(false),
            None => Ok(()),
        }
    }

    /// Moves the start forward (never back).
    pub fn advance_excluded_since(&self, ms: u64) -> io::Result<()> {
        let current = self.excluded_since(ms)?;
        if ms > current {
            self.set_excluded_since(ms)?;
        }
        Ok(())
    }

    /// Sets the start of the recording, going back or forward: for a log that was just created or that
    /// inherited the date of its sources (doc 07, section 10.3).
    pub fn set_excluded_since(&self, ms: u64) -> io::Result<()> {
        if let Some(state) = &self.excluded.state {
            state.set_excluded_since(ms)?;
        }
        *self.excluded.since() = Some(ms);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
