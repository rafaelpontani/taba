//! The upload to the bucket: the `outbox` becomes segments (`.seg`), mutable state becomes a journal (`.jrn`), the
//! segments that died whole are deleted, and the `flush` forces everything (doc 23, "S3 backup").
//!
//! Nothing here runs on the publication path: a message's commit only inserted a row into
//! `_backup.db`. An S3 failure leaves everything as it is (the `outbox` keeps the IDs) and the upload tries
//! again; only **after the PUT confirms** do the batch's rows leave the `outbox`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::{Backup, lock};
use crate::core::id;
use crate::storage::backup::journal::{self, Header, Record};
use crate::storage::backup::segment::{self, Footer, FooterRead, SegmentBuilder, TAIL_GUESS};
use crate::storage::backup::{SavedRef, SegmentRow, keys};
use crate::storage::messages::MessageMeta;

/// How many `outbox` messages a pass pulls at a time.
const PAGE: usize = 5_000;

/// How long the upload waits after an S3 failure, before trying again.
const RETRY_AFTER: Duration = Duration::from_secs(2);

/// The segment still open for a topic: messages that already passed the `backup.delay` and
/// wait for the segment to close (by size, count or age).
struct OpenSegment {
    topic_id: String,
    items: Vec<(MessageMeta, Vec<u8>)>,
    bytes: u64,
    opened: Instant,
}

#[derive(Default)]
pub(crate) struct Uploader {
    open: HashMap<String, OpenSegment>,
    /// The messages that are in an open segment: the next pass does not pull them again.
    in_open: HashSet<(String, String)>,
    /// S3 failed a moment ago: waits until this instant.
    retry_at: Option<Instant>,
}

/// What the `flush` did (the response of `POST /node/backup/flush`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlushReport {
    /// Step 1: messages that left memory for disk.
    pub memory_moved: u64,
    /// Messages that stayed in memory (disk error or deadline).
    pub memory_remaining: u64,
    /// Steps 2 and 3: segments uploaded and the messages in them.
    pub segments_uploaded: u64,
    pub messages_uploaded: u64,
    pub journals_uploaded: u64,
    /// What is still left (zero when complete).
    pub outbox_pending: u64,
    pub journal_pending: u64,
    /// The deadline (`backup.drain.timeout`) ran out before finishing.
    pub timed_out: bool,
    pub s3_reachable: bool,
}

impl FlushReport {
    /// The report as the API shows it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "memoryMoved": self.memory_moved,
            "memoryRemaining": self.memory_remaining,
            "segmentsUploaded": self.segments_uploaded,
            "messagesUploaded": self.messages_uploaded,
            "journalsUploaded": self.journals_uploaded,
            "outboxPending": self.outbox_pending,
            "journalPending": self.journal_pending,
            "timedOut": self.timed_out,
            "s3Reachable": self.s3_reachable,
            "complete": self.complete(),
        })
    }

    pub fn complete(&self) -> bool {
        !self.timed_out
            && self.memory_remaining == 0
            && self.outbox_pending == 0
            && self.journal_pending == 0
    }
}

impl Backup {
    /// One pass of the upload. Without `force`, only the messages past `backup.delay` and the
    /// segments that closed go up; with `force` (flush, drain), everything goes up now. Returns how many segments
    /// went up.
    pub(crate) async fn upload_pass(&self, force: bool) -> u64 {
        let mut up = self.uploader.lock().await;
        if !force && up.retry_at.is_some_and(|at| Instant::now() < at) {
            return 0;
        }
        if !self.topology.is_active() {
            return 0;
        }
        up.retry_at = None;
        let mut uploaded = 0;
        loop {
            let before = if force {
                u64::MAX
            } else {
                self.clock
                    .now_millis()
                    .saturating_sub(self.rules.delay.as_millis() as u64)
            };
            let rows = match self.db.outbox(before, PAGE + up.in_open.len()) {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::error!(error = %e, "backup: could not read the outbox");
                    return uploaded;
                }
            };
            let fresh: Vec<(String, String)> = rows
                .into_iter()
                .filter(|row| !up.in_open.contains(row))
                .collect();
            let pulled = fresh.len();
            for (topic, id) in fresh {
                self.admit(&mut up, topic.clone(), id).await;
                // Reached a size or count limit: closes now, without waiting for the page.
                if up.open.get(&topic).is_some_and(|open| {
                    open.items.len() >= self.rules.segment_max_messages
                        || open.bytes >= self.rules.segment_max_bytes
                }) {
                    uploaded += self.close_topic(&mut up, &topic).await;
                    if up.retry_at.is_some() {
                        break;
                    }
                }
            }
            uploaded += self.close_ready(&mut up, force).await;
            // Without `force` one page is enough (the next pass continues); with `force` it repeats until the
            // outbox empties or S3 fails.
            if !force || pulled == 0 || up.retry_at.is_some() {
                break;
            }
        }
        uploaded
    }

    /// Puts the message in the topic's open segment. If it is no longer on the node (ack, `ttl`), it leaves the
    /// `outbox`.
    async fn admit(&self, up: &mut Uploader, topic: String, id: String) {
        let Ok(view) = self.topics.get(&topic) else {
            let _ = self.db.unqueue(&topic, &id);
            return;
        };
        let messages = self.messages.clone();
        let (t, i) = (topic.clone(), id.clone());
        let loaded = tokio::task::spawn_blocking(move || messages.backup_load(&t, &i))
            .await
            .ok()
            .flatten();
        let Some((meta, body)) = loaded else {
            let _ = self.db.unqueue(&topic, &id);
            return;
        };
        let open = up.open.entry(topic.clone()).or_insert_with(|| OpenSegment {
            topic_id: view.definition.id.clone(),
            items: Vec::new(),
            bytes: 0,
            opened: Instant::now(),
        });
        // The topic was deleted and recreated with the same name: the old segment closes first.
        if open.topic_id != view.definition.id {
            return;
        }
        open.bytes += body.len() as u64;
        open.items.push((meta, body));
        up.in_open.insert((topic, id));
    }

    /// Closes and uploads the segments that hit a limit (or all, with `force`).
    async fn close_ready(&self, up: &mut Uploader, force: bool) -> u64 {
        let ready: Vec<String> = up
            .open
            .iter()
            .filter(|(_, open)| {
                force
                    || open.items.len() >= self.rules.segment_max_messages
                    || open.bytes >= self.rules.segment_max_bytes
                    || open.opened.elapsed() >= self.rules.segment_max_age
            })
            .map(|(topic, _)| topic.clone())
            .collect();
        let mut uploaded = 0;
        for topic in ready {
            uploaded += self.close_topic(up, &topic).await;
            if up.retry_at.is_some() {
                break;
            }
        }
        uploaded
    }

    /// Closes and uploads the topic's open segment. Returns `1` if a segment went up. If S3
    /// fails, the rows stay in the `outbox` and the next pass tries again.
    async fn close_topic(&self, up: &mut Uploader, topic: &str) -> u64 {
        let Some(open) = up.open.remove(topic) else {
            return 0;
        };
        for (meta, _) in &open.items {
            up.in_open.remove(&(topic.to_owned(), meta.id.clone()));
        }
        match self.send_segment(topic, open).await {
            Ok(true) => 1,
            Ok(false) => 0,
            Err(()) => {
                up.retry_at = Some(Instant::now() + RETRY_AFTER);
                0
            }
        }
    }

    /// Builds the segment, uploads it and, **only after the PUT confirms**, records the references and takes the
    /// messages off the `outbox`. `Ok(false)`: nothing to send.
    async fn send_segment(&self, topic: &str, open: OpenSegment) -> Result<bool, ()> {
        // Only what still waits in the outbox goes in: what died while waiting does not go to the bucket.
        let items: Vec<_> = open
            .items
            .into_iter()
            .filter(|(meta, _)| self.db.queued(topic, &meta.id).unwrap_or(false))
            .collect();
        if items.is_empty() {
            return Ok(false);
        }
        let seq = self.db.next_seq().map_err(|_| ())?;
        let writer = self.node_id();
        let mut builder = SegmentBuilder::new(topic, &open.topic_id, &writer);
        let mut entries = Vec::with_capacity(items.len());
        for (meta, body) in &items {
            let entry = builder.push(meta, body);
            entries.push((entry.id, entry.offset, entry.len));
        }
        let key = keys::segment(
            topic,
            &open.topic_id,
            builder.first_id().unwrap_or_default(),
            &writer,
            seq,
        );
        let bytes = builder.finish();
        let size = bytes.len() as u64;
        let put = self.store.put(&key, bytes).await;
        self.note("PUT do segmento", &put);
        if put.is_err() {
            return Err(());
        }
        let row = SegmentRow {
            key: key.clone(),
            topic: topic.to_owned(),
            topic_id: open.topic_id.clone(),
            total: entries.len() as u64,
            live: 0,
            bytes: size,
            created_ms: self.clock.now_millis(),
        };
        let committed = match self.db.commit_segment(&row, &entries) {
            Ok(committed) => committed,
            Err(e) => {
                tracing::error!(error = %e, "backup: could not record the segment");
                return Err(());
            }
        };
        self.counters
            .segments_uploaded
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .messages_uploaded
            .fetch_add(committed.live, Ordering::Relaxed);
        self.counters
            .bytes_uploaded
            .fetch_add(size, Ordering::Relaxed);
        if committed.live == 0 {
            // All died during the upload: the segment is born dead.
            lock(&self.runtime).dead_segments.push(key.clone());
        }
        let saved: Vec<(String, SavedRef)> = entries
            .iter()
            .filter(|(id, ..)| !committed.dead.contains(id))
            .map(|(id, offset, len)| {
                (
                    id.clone(),
                    SavedRef {
                        segment: key.clone(),
                        offset: *offset,
                        len: *len,
                        saved_at_ms: row.created_ms,
                    },
                )
            })
            .collect();
        self.announce_saved(topic, &items, &saved);
        Ok(true)
    }

    /// Tells the replicas which messages are already in the bucket, in a batch (`BackupSaved`). Without a
    /// cluster, there is no one to tell.
    fn announce_saved(
        &self,
        topic: &str,
        items: &[(MessageMeta, Vec<u8>)],
        saved: &[(String, SavedRef)],
    ) {
        let positions: HashMap<&str, u64> = items
            .iter()
            .map(|(meta, _)| (meta.id.as_str(), meta.position))
            .collect();
        let mut by_node: HashMap<String, Vec<(String, SavedRef)>> = HashMap::new();
        let me = self.node_id();
        for (id, saved) in saved {
            let Some(position) = positions.get(id.as_str()) else {
                continue;
            };
            for owner in self.topology.owners(*position) {
                if owner != me {
                    by_node
                        .entry(owner)
                        .or_default()
                        .push((id.clone(), saved.clone()));
                }
            }
        }
        for (node, list) in by_node {
            self.topology.send_saved(&node, topic, list);
        }
    }

    /// Deletes from the bucket the segments in which all the messages died.
    pub(crate) async fn purge_dead_segments(&self) {
        let pending: Vec<String> = std::mem::take(&mut lock(&self.runtime).dead_segments);
        let mut failed = Vec::new();
        for key in pending {
            let result = self.store.delete(&key).await;
            self.note("DELETE do segmento", &result);
            match result {
                Ok(()) => {
                    let _ = self.db.drop_segment(&key);
                    self.counters
                        .segments_deleted
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => failed.push(key),
            }
        }
        if !failed.is_empty() {
            lock(&self.runtime).dead_segments.extend(failed);
        }
    }

    /// The journal: only the **last state** of each message changed since the last one, by topic. An
    /// ack goes in as a removal.
    pub(crate) async fn journal_pass(&self) -> u64 {
        if !self.topology.is_active() {
            return 0;
        }
        let dirty = match self.db.dirty(PAGE) {
            Ok(dirty) if !dirty.is_empty() => dirty,
            _ => return 0,
        };
        let mut by_topic: HashMap<String, Vec<Record>> = HashMap::new();
        let mut done = Vec::with_capacity(dirty.len());
        for item in dirty {
            let record = if item.removed {
                Some(Record::removal(&item.id))
            } else {
                match self.messages.message_info(&item.topic, &item.id) {
                    Ok(Some(info)) => Some(Record::state(&info.meta)),
                    // It is no longer on the node: the ack will come as a removal.
                    _ => None,
                }
            };
            if let Some(record) = record {
                by_topic.entry(item.topic.clone()).or_default().push(record);
            }
            done.push(item);
        }
        let mut written = 0;
        let mut ok_topics: HashSet<String> = HashSet::new();
        for (topic, records) in by_topic {
            let Ok(view) = self.topics.get(&topic) else {
                ok_topics.insert(topic);
                continue;
            };
            let Ok(seq) = self.db.next_seq() else {
                continue;
            };
            let writer = self.node_id();
            let header = Header {
                topic: topic.clone(),
                topic_id: view.definition.id.clone(),
                writer: writer.clone(),
                at_ms: self.clock.now_millis(),
            };
            let key = keys::journal(&topic, &view.definition.id, header.at_ms, &writer, seq);
            let put = self
                .store
                .put(&key, journal::encode(&header, &records))
                .await;
            self.note("PUT of the journal", &put);
            if put.is_ok() {
                ok_topics.insert(topic);
                written += 1;
                self.counters
                    .journals_uploaded
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        // Only what went to the bucket leaves the list: the rest tries on the next pass.
        let sent: Vec<_> = done
            .into_iter()
            .filter(|item| ok_topics.contains(&item.topic))
            .collect();
        let _ = self.db.clear_dirty(&sent);
        written
    }

    /// `POST /node/backup/flush` (doc 23, section 1.11): in three steps in sequence, without deregistering
    /// or stopping the node.
    ///
    /// 1. memory → disk, through the same path as the drain (the memory limit drops to zero);
    /// 2. disk → S3: the pending segments and journal, with priority;
    /// 3. `outbox` → S3: the messages that have not gone out yet.
    ///
    /// Ends when all three finish or when `backup.drain.timeout` runs out.
    pub async fn flush(&self) -> FlushReport {
        let deadline = Instant::now() + self.rules.drain_timeout;
        let mut report = FlushReport::default();

        // Etapa 1.
        let was_draining = self.messages.is_draining();
        if !was_draining {
            self.messages.begin_drain();
        }
        let messages = self.messages.clone();
        let moved = tokio::task::spawn_blocking(move || {
            messages.drain_until(|| Instant::now() >= deadline)
        })
        .await;
        // A real drain that started in the middle of the flush keeps the memory limit at zero.
        if !was_draining && self.shared.drain_state() == crate::cluster::state::DrainState::Active {
            self.messages.cancel_drain();
        }
        if let Ok(moved) = moved {
            report.memory_moved = moved.moved;
            report.memory_remaining = moved.remaining;
        }

        // Etapas 2 e 3.
        self.upload_everything(deadline, &mut report).await;
        report
    }

    /// The drain's upload (memory is already on disk): everything there is goes to S3, with priority,
    /// within `backup.drain.timeout`.
    pub async fn drain_upload(&self) -> FlushReport {
        let deadline = Instant::now() + self.rules.drain_timeout;
        let mut report = FlushReport::default();
        self.upload_everything(deadline, &mut report).await;
        report
    }

    /// Steps 2 and 3 of the flush: the segments, the journal and the `outbox`, until empty or the deadline.
    async fn upload_everything(&self, deadline: Instant, report: &mut FlushReport) {
        let before = self.counters_snapshot();
        loop {
            self.upload_pass(true).await;
            self.journal_pass().await;
            self.purge_dead_segments().await;
            self.flush_definitions().await;
            let status = self.status();
            if status.outbox_pending == 0
                && status.journal_pending == 0
                && status.definitions_pending == 0
            {
                break;
            }
            if Instant::now() >= deadline {
                report.timed_out = true;
                break;
            }
            // S3 failed: waits a bit and tries again, until the deadline.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let after = self.counters_snapshot();
        report.segments_uploaded = after.0 - before.0;
        report.messages_uploaded = after.1 - before.1;
        report.journals_uploaded = after.2 - before.2;
        let status = self.status();
        report.outbox_pending = status.outbox_pending;
        report.journal_pending = status.journal_pending;
        report.s3_reachable = status.s3_ok;
    }

    fn counters_snapshot(&self) -> (u64, u64, u64) {
        (
            self.counters.segments_uploaded.load(Ordering::Relaxed),
            self.counters.messages_uploaded.load(Ordering::Relaxed),
            self.counters.journals_uploaded.load(Ordering::Relaxed),
        )
    }

    /// Reads from the bucket the body of a saved message, by `Range` (the eviction took it off disk).
    pub(crate) async fn read_saved(&self, saved: &SavedRef) -> Option<Vec<u8>> {
        let raw = self
            .store
            .get_range(&saved.segment, saved.offset, saved.len)
            .await;
        self.note("GET do segmento", &raw);
        let raw = raw.ok().flatten()?;
        segment::decode_entry(&raw).ok().map(|(_, body)| body)
    }
}

impl Backup {
    /// The footer of a bucket segment, read from the end (without downloading the segment).
    pub(crate) async fn footer_of(&self, key: &str) -> Option<Footer> {
        let tail = self
            .store
            .get_suffix(key, TAIL_GUESS)
            .await
            .ok()
            .flatten()?;
        match segment::read_footer(&tail).ok()? {
            FooterRead::Done(footer) => Some(footer),
            FooterRead::NeedMore(n) => {
                let tail = self.store.get_suffix(key, n).await.ok().flatten()?;
                match segment::read_footer(&tail).ok()? {
                    FooterRead::Done(footer) => Some(footer),
                    FooterRead::NeedMore(_) => None,
                }
            }
        }
    }

    /// Deletes the segments in which all the messages are already past the topic's `ttl`: whoever wrote them
    /// may have stopped being the primary (or died), and no one else would count them as dead.
    /// Only the topic's manager sweeps, and the dead-letter stays out (its `ttl` counts from the entry
    /// into it, not from the publication).
    pub(crate) async fn sweep_expired_segments(&self) {
        if !self.topology.is_active() || self.restoring.load(Ordering::SeqCst) {
            return;
        }
        let now = self.clock.now_millis();
        let slack = 2 * self.rules.meta_interval.as_millis() as u64;
        for name in self.topics.names() {
            if !self.topology.manages_definition("topic", &name) {
                continue;
            }
            let Ok(view) = self.topics.get(&name) else {
                continue;
            };
            let Some(ttl_secs) = view.ttl_secs() else {
                continue;
            };
            let ttl_ms = ttl_secs.saturating_mul(1000);
            let listed = self
                .store
                .list(&keys::segments_prefix(&name, &view.definition.id))
                .await;
            self.note("LIST of segments", &listed);
            for object in listed.unwrap_or_default().into_iter().take(50) {
                let Some(footer) = self.footer_of(&object.key).await else {
                    continue;
                };
                let newest = footer
                    .entries
                    .iter()
                    .filter_map(|e| id::unix_millis_of(&e.id))
                    .max();
                if newest.is_some_and(|at| at.saturating_add(ttl_ms).saturating_add(slack) < now) {
                    let deleted = self.store.delete(&object.key).await;
                    self.note("DELETE do segmento vencido", &deleted);
                    if deleted.is_ok() {
                        let _ = self.db.drop_segment(&object.key);
                        self.counters
                            .segments_deleted
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}
