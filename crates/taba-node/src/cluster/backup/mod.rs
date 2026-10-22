//! The optional S3 backup (doc 23, "S3 backup"): the coordination between the message engine, the
//! local `_backup.db`, the object store and the cluster.
//!
//! S3 is **only a backup for total or serious failure**: SQLite and memory remain the primary
//! storage, and no publication, consumption or synchronization waits for S3. Without `backup.s3.bucket`
//! this module does not exist for the node. With S3 down the feature stands still and the cluster works
//! as without it; all that piles up is the `outbox` (IDs) on disk.
//!
//! | File | What it does |
//! |---|---|
//! | `upload` | The `outbox` becomes segments (`.seg`); mutable state becomes a journal (`.jrn`); the cleanup of dead segments; the flush |
//! | `evict` | With a full disk, deletes the body of already-saved messages and reads it back by `Range` |
//! | `definitions` | Topic and token definitions (`topics/`, `tokens/`), with retry and reconciliation |
//! | `heartbeat` | Each node's heartbeat (`nodes/<id>.json`) and the classification of the others (alive, alive in another partition, dead) |
//! | `restore` | The restore lock, the bucket restore and the cleanup of `nodes/` |
//! | `stop` | The list of live nodes at the cluster's planned stop |

mod definitions;
mod evict;
mod heartbeat;
mod restore;
mod stop;
mod topology;
mod upload;

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

pub use heartbeat::{Heartbeat, Liveness};
pub use restore::{RestoreReport, StartupDecision};
pub use stop::StopList;
pub use topology::{ClusterTopology, Topology};
pub use upload::FlushReport;

use crate::cluster::state::{DrainState, NodeShared};
use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::{BackupSink, Messages};
use crate::engine::tokens::Tokens;
use crate::engine::topics::Topics;
use crate::storage::backup::{BackupDb, ObjectStore, SavedRef};
use crate::storage::messages::MessageStore;

/// The `backup.*` values the service depends on.
#[derive(Debug, Clone)]
pub struct BackupRules {
    pub delay: Duration,
    pub meta_interval: Duration,
    pub segment_max_bytes: u64,
    pub segment_max_messages: usize,
    pub segment_max_age: Duration,
    pub heartbeat_interval: Duration,
    pub dead_after: Duration,
    pub drain_timeout: Duration,
    pub evict_above: u32,
    pub evict_to: u32,
    pub pause_at: u32,
    pub reconcile_interval: Duration,
    pub restore_lock_ttl: Duration,
}

impl BackupRules {
    pub fn from_config(config: &Config) -> Self {
        let b = &config.backup;
        Self {
            delay: b.delay,
            meta_interval: b.meta_interval,
            segment_max_bytes: b.segment_max_bytes,
            segment_max_messages: b.segment_max_messages as usize,
            segment_max_age: b.segment_max_age,
            heartbeat_interval: b.heartbeat_interval,
            dead_after: b.dead_after,
            drain_timeout: b.drain_timeout,
            evict_above: b.evict_above,
            evict_to: b.evict_to,
            pause_at: b.pause_at,
            reconcile_interval: b.reconcile_interval,
            restore_lock_ttl: b.restore_lock_ttl,
        }
    }
}

/// A definition that needs to go (or go again) to the bucket.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DefRef {
    Topic(String),
    /// The topic was removed: writes the tombstone and deletes the messages prefix (`name`, `topicId`).
    TopicDeleted(String, String),
    Token(String),
    TokenRevoked(String),
}

/// What the service keeps in memory and that changes unhurriedly.
#[derive(Default)]
pub(crate) struct Runtime {
    /// Did S3 answer on the last call?
    pub s3_ok: bool,
    pub last_error: Option<String>,
    pub last_ok_ms: Option<u64>,
    /// Whole dead segments to delete from the bucket.
    pub dead_segments: Vec<String>,
    /// Definitions to write (the write failed or the node just became the manager).
    pub pending_defs: BTreeSet<DefRef>,
    /// Removed topics whose message prefix still needs deleting: `(name, topicId)`.
    pub purges: Vec<(String, String)>,
}

#[derive(Default)]
pub(crate) struct Counters {
    pub segments_uploaded: AtomicU64,
    pub messages_uploaded: AtomicU64,
    pub bytes_uploaded: AtomicU64,
    pub journals_uploaded: AtomicU64,
    pub segments_deleted: AtomicU64,
    pub errors: AtomicU64,
    pub bodies_evicted: AtomicU64,
    pub bodies_fetched: AtomicU64,
    pub definitions_written: AtomicU64,
}

/// How the backup sees itself: what `GET /node/backup` and the flush responses show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupStatus {
    pub s3_ok: bool,
    pub last_error: Option<String>,
    pub last_ok_ms: Option<u64>,
    /// Messages still without a segment in the bucket.
    pub outbox_pending: u64,
    /// State changes still outside the journal.
    pub journal_pending: u64,
    pub segments: u64,
    pub segment_bytes: u64,
    /// Messages with a reference in the bucket (those this node, owner or replica, can read from there).
    pub saved: u64,
    pub segments_uploaded: u64,
    pub messages_uploaded: u64,
    pub errors: u64,
    pub bodies_evicted: u64,
    pub definitions_pending: u64,
}

impl BackupStatus {
    /// The state as the APIs show it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "s3Reachable": self.s3_ok,
            "lastError": self.last_error,
            "lastSuccessAt": self.last_ok_ms.map(crate::core::time::format_ms),
            "outboxPending": self.outbox_pending,
            "journalPending": self.journal_pending,
            "definitionsPending": self.definitions_pending,
            "pending": self.pending(),
            "segments": self.segments,
            "segmentBytes": self.segment_bytes,
            "saved": self.saved,
            "segmentsUploaded": self.segments_uploaded,
            "messagesUploaded": self.messages_uploaded,
            "bodiesEvicted": self.bodies_evicted,
            "errors": self.errors,
        })
    }

    /// What is still left to send: what the operator checks before maintenance.
    pub fn pending(&self) -> u64 {
        self.outbox_pending + self.journal_pending + self.definitions_pending
    }
}

pub struct Backup {
    pub(crate) rules: BackupRules,
    pub(crate) store: Arc<dyn ObjectStore>,
    pub db: Arc<BackupDb>,
    pub(crate) messages: Arc<Messages>,
    pub(crate) disk: Arc<MessageStore>,
    pub(crate) topics: Arc<Topics>,
    pub(crate) tokens: Arc<Tokens>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) shared: Arc<NodeShared>,
    pub(crate) topology: Arc<dyn Topology>,
    pub(crate) runtime: Mutex<Runtime>,
    pub(crate) written: Mutex<definitions::Written>,
    pub(crate) counters: Counters,
    /// The upload to the bucket (open segments): one pass at a time.
    pub(crate) uploader: tokio::sync::Mutex<upload::Uploader>,
    pub(crate) beats: heartbeat::Beats,
    /// The node's heartbeat is on (turns off on a finished drain and on leaving).
    pub(crate) beating: AtomicBool,
    pub(crate) rt: tokio::runtime::Handle,
    pub(crate) restoring: AtomicBool,
    /// Who reports the disk usage (%); the tests swap it to simulate the disk filling up.
    pub(crate) disk_probe: Mutex<Option<Arc<dyn Fn() -> u32 + Send + Sync>>>,
    /// The last real reading of disk usage and when it was taken.
    pub(crate) disk_usage: Mutex<Option<(std::time::Instant, u32)>>,
    /// The table's members on the last round: if one vanished, the node looks in the bucket for what only it
    /// had (doc 23, section 1.7, example B).
    pub(crate) last_members: Mutex<std::collections::BTreeSet<String>>,
    /// The ETag of the restore lock that another node holds and since when we see it unchanged.
    pub(crate) lock_seen: Mutex<Option<(String, std::time::Instant)>>,
    /// Wakes the background loop when there is new work (flush, eviction).
    pub(crate) wake: tokio::sync::Notify,
    pub(crate) cancel: CancellationToken,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `backup.not-configured`: the feature is not on on this node.
pub fn not_configured() -> TabaError {
    TabaError::new(
        ErrorCode::BackupNotConfigured,
        "The S3 backup is not configured (backup.s3.bucket).",
    )
}

pub struct BackupArgs {
    pub rules: BackupRules,
    pub store: Arc<dyn ObjectStore>,
    pub db: Arc<BackupDb>,
    pub messages: Arc<Messages>,
    pub disk: Arc<MessageStore>,
    pub topics: Arc<Topics>,
    pub tokens: Arc<Tokens>,
    pub clock: Arc<dyn Clock>,
    pub shared: Arc<NodeShared>,
    pub topology: Arc<dyn Topology>,
    pub cancel: CancellationToken,
}

impl Backup {
    /// Creates the service and attaches it to the engine (the messages), the tables (topics and tokens) and the node.
    /// Call from inside a Tokio runtime.
    pub fn new(args: BackupArgs) -> Arc<Self> {
        let backup = Arc::new(Self {
            rules: args.rules,
            store: args.store,
            db: args.db,
            messages: args.messages,
            disk: args.disk,
            topics: args.topics,
            tokens: args.tokens,
            clock: args.clock,
            shared: args.shared,
            topology: args.topology,
            runtime: Mutex::new(Runtime::default()),
            written: Mutex::new(definitions::Written::default()),
            counters: Counters::default(),
            uploader: tokio::sync::Mutex::new(upload::Uploader::default()),
            beats: heartbeat::Beats::default(),
            beating: AtomicBool::new(true),
            rt: tokio::runtime::Handle::current(),
            restoring: AtomicBool::new(false),
            lock_seen: Mutex::new(None),
            last_members: Mutex::new(std::collections::BTreeSet::new()),
            disk_probe: Mutex::new(None),
            disk_usage: Mutex::new(None),
            wake: tokio::sync::Notify::new(),
            cancel: args.cancel,
        });
        backup.messages.set_backup(backup.clone());
        definitions::attach(&backup);
        backup
    }

    /// Where the message is in the bucket and when it was saved, if this node (owner or replica) knows. It is the
    /// single query about the reference, which lives in `_backup.db` and not in `_topic.db`.
    pub fn saved_info(&self, topic: &str, id: &str) -> Option<SavedRef> {
        self.db.saved(topic, id).ok().flatten()
    }

    pub fn rules(&self) -> &BackupRules {
        &self.rules
    }

    /// Swaps the disk usage measure (in %): only for the tests to simulate the disk filling up.
    pub fn set_disk_probe(&self, probe: Arc<dyn Fn() -> u32 + Send + Sync>) {
        *lock(&self.disk_probe) = Some(probe);
    }

    pub fn node_id(&self) -> String {
        self.shared.node_id()
    }

    /// Runs at once what the service does in the background, until the node stops.
    pub async fn run(self: Arc<Self>) {
        let mut upload = tokio::time::interval(Duration::from_millis(500));
        let mut meta = tokio::time::interval(self.rules.meta_interval);
        let mut beat = tokio::time::interval(self.rules.heartbeat_interval);
        let mut evict = tokio::time::interval(Duration::from_secs(2));
        let mut reconcile = tokio::time::interval(self.rules.reconcile_interval);
        for ticker in [
            &mut upload,
            &mut meta,
            &mut beat,
            &mut evict,
            &mut reconcile,
        ] {
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        }
        // The first `tick` of an `interval` is immediate: the journal and the reconciliation wait their turn.
        meta.reset();
        reconcile.reset();
        // Messages that were already on the node before the backup turned on (or after a crash) go into the
        // outbox: the node adopts what is its own.
        self.adopt_all();
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => return,
                _ = upload.tick() => {
                    self.upload_pass(false).await;
                    self.purge_dead_segments().await;
                    self.flush_definitions().await;
                    self.purge_removed_topics().await;
                }
                () = self.wake.notified() => {
                    self.evict_pass();
                }
                _ = meta.tick() => { self.journal_pass().await; }
                _ = beat.tick() => {
                    self.heartbeat_pass().await;
                    self.check_membership_loss().await;
                }
                _ = evict.tick() => { self.evict_pass(); }
                _ = reconcile.tick() => {
                    self.reconcile_definitions().await;
                    self.sweep_expired_segments().await;
                    self.cleanup_nodes().await;
                    self.adopt_all();
                }
            }
        }
    }

    /// The node enters the outbox as the owner of everything the engine says is its own and is not yet
    /// saved.
    pub fn adopt_all(&self) {
        if !self.topology.is_active() {
            return;
        }
        for topic in self.messages.topic_names() {
            for id in self.messages.owned_ids(&topic) {
                BackupSink::adopted(self, &topic, &id);
            }
        }
    }

    pub fn status(&self) -> BackupStatus {
        let (segments, segment_bytes) = self.db.segment_totals().unwrap_or((0, 0));
        let runtime = lock(&self.runtime);
        BackupStatus {
            s3_ok: runtime.s3_ok,
            last_error: runtime.last_error.clone(),
            last_ok_ms: runtime.last_ok_ms,
            outbox_pending: self.db.outbox_len().unwrap_or(0),
            journal_pending: self.db.dirty_len().unwrap_or(0),
            segments,
            segment_bytes,
            saved: self.db.saved_len().unwrap_or(0),
            segments_uploaded: self.counters.segments_uploaded.load(Ordering::Relaxed),
            messages_uploaded: self.counters.messages_uploaded.load(Ordering::Relaxed),
            errors: self.counters.errors.load(Ordering::Relaxed),
            bodies_evicted: self.counters.bodies_evicted.load(Ordering::Relaxed),
            definitions_pending: runtime.pending_defs.len() as u64,
        }
    }

    /// Notes the result of a call to S3: the feature lives or is down.
    pub(crate) fn note<T, E: std::fmt::Display>(&self, what: &str, result: &Result<T, E>) {
        let mut runtime = lock(&self.runtime);
        match result {
            Ok(_) => {
                runtime.s3_ok = true;
                runtime.last_error = None;
                runtime.last_ok_ms = Some(self.clock.now_millis());
            }
            Err(e) => {
                if runtime.s3_ok || runtime.last_error.is_none() {
                    tracing::warn!(error = %e, "backup: {what} failed; is S3 down? The cluster goes on without it");
                }
                runtime.s3_ok = false;
                runtime.last_error = Some(format!("{what}: {e}"));
                self.counters.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Runs a future to the end on a separate thread (the engine is synchronous and calls the backup with
    /// the topic lock; see `BackupSink::fetch_body`).
    pub(crate) fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future + Send,
        F::Output: Send,
    {
        let handle = self.rt.clone();
        std::thread::scope(|scope| {
            scope
                .spawn(move || handle.block_on(future))
                .join()
                .expect("the backup thread must not panic")
        })
    }

    /// The node is in drain or drained: the heartbeat and the upload go on only until the drain finishes.
    pub(crate) fn drained(&self) -> bool {
        self.shared.drain_state() == DrainState::Drained
    }
}

impl BackupSink for Backup {
    fn stored(&self, topic: &str, id: &str, owned: bool) {
        if !owned {
            return;
        }
        if self.db.saved(topic, id).ok().flatten().is_some() {
            return;
        }
        if let Err(e) = self.db.enqueue(topic, id, self.clock.now_millis()) {
            self.disk_failure("could not write to _backup.db", &e);
        }
    }

    fn removed(&self, topic: &str, id: &str, owned: bool) {
        let result = if owned {
            self.db.mark_dead(topic, id).map(|dead| {
                if let Some(segment) = dead {
                    lock(&self.runtime).dead_segments.push(segment);
                }
            })
        } else {
            self.db.forget(topic, id)
        };
        if let Err(e) = result {
            self.disk_failure("could not write to _backup.db", &e);
        }
    }

    fn changed(&self, topic: &str, id: &str, owned: bool) {
        if owned && let Err(e) = self.db.touch(topic, id) {
            self.disk_failure("could not write to _backup.db", &e);
        }
    }

    fn adopted(&self, topic: &str, id: &str) {
        BackupSink::stored(self, topic, id, true);
    }

    fn released(&self, topic: &str, id: &str) {
        if let Err(e) = self.db.unqueue(topic, id) {
            self.disk_failure("could not write to _backup.db", &e);
        }
    }

    fn fetch_body(&self, topic: &str, id: &str) -> Option<Vec<u8>> {
        let saved: SavedRef = self.db.saved(topic, id).ok().flatten()?;
        let body = self.block_on(self.read_saved(&saved));
        if body.is_some() {
            self.counters.bodies_fetched.fetch_add(1, Ordering::Relaxed);
        }
        body
    }

    fn before_disk_write(&self) -> bool {
        self.wait_for_disk()
    }
}

impl Backup {
    /// An `_backup.db` error does not bring the publication down: it becomes a log. A full disk or corrupt database
    /// bring the node down, as with any other database of its own (doc 23: Q9).
    fn disk_failure(&self, what: &str, e: &std::io::Error) {
        tracing::error!(error = %e, "{what}");
        if crate::storage::is_fatal(e) {
            self.shared.set_failed();
        }
    }
}

/// How the cluster talks to the backup without knowing the rest of the node: the service and the planned stop
/// (which needs the drain, a piece of the node). The `ClusterService` only keeps a weak reference.
pub trait BackupPort: Send + Sync + 'static {
    fn backup(&self) -> Arc<Backup>;

    /// This node's drain for the cluster stop (`POST /cluster/stop`): leaves rotation, sends
    /// everything to S3 and answers when the node is done, with the report in JSON.
    fn stop_node(
        &self,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<serde_json::Value, TabaError>> + Send + '_>>;
}

impl Backup {
    /// The replica receives the primary's notice: of the messages it keeps, which are already in the
    /// bucket. What it does not know is ignored.
    pub fn accept_saved(&self, saved: &crate::cluster::protocol::BackupSaved) -> usize {
        let mut kept = 0;
        for item in &saved.items {
            if !self.messages.holds(&saved.topic, &item.id) {
                continue;
            }
            let reference = SavedRef {
                segment: item.segment.clone(),
                offset: item.offset,
                len: item.len,
                saved_at_ms: item.saved_at_ms,
            };
            if self
                .db
                .set_saved(&saved.topic, &item.id, &reference)
                .is_ok()
            {
                kept += 1;
            }
        }
        kept
    }

    /// Before excluding `candidate` by vote: waits for its heartbeat to expire. Returns `true` if
    /// it can go on with the vote (the heartbeat expired, or S3 says nothing: the behavior is the
    /// usual one) and `false` if the channel came back in the middle of the wait.
    pub async fn confirm_dead(&self, candidate: &str, reached: impl Fn() -> bool) -> bool {
        let mut announced = false;
        // The channel of a node that dies flickers (reconnects and drops again): it only counts as "came back" if
        // it is open on several checks in a row.
        let mut steady = 0;
        loop {
            steady = if reached() { steady + 1 } else { 0 };
            if steady >= 3 {
                return false;
            }
            match self.liveness(candidate) {
                Liveness::Dead | Liveness::Unknown => return true,
                Liveness::Fresh => {}
            }
            if !announced {
                tracing::warn!(
                    candidate,
                    "backup: the node does not answer, but its heartbeat in S3 is fresh (alive in another partition?); the exclusion waits for the heartbeat to expire"
                );
                announced = true;
            }
            if self.cancel.is_cancelled() {
                return false;
            }
            tokio::time::sleep((self.rules.heartbeat_interval / 2).max(Duration::from_millis(30)))
                .await;
        }
    }

    /// Waits, for up to `2 × backup.dead-after`, for all the other members to become dead to S3.
    /// Whoever excludes the dead one by one only sees the last one after its heartbeat expires.
    pub async fn wait_all_others_dead(&self) -> bool {
        let deadline = std::time::Instant::now() + self.rules.dead_after * 2;
        loop {
            if self.all_others_dead() {
                return true;
            }
            if std::time::Instant::now() >= deadline || self.cancel.is_cancelled() {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Are the other members all dead (to S3)? It is the sole survivor, which may exclude
    /// the dead even below `cluster.vote.min-voters`.
    pub fn all_others_dead(&self) -> bool {
        let me = self.node_id();
        let members = self.topology.members();
        members
            .iter()
            .filter(|id| **id != me)
            .all(|id| self.liveness(id) == Liveness::Dead)
    }
}

/// The nodes the cluster knows, with each one's heartbeat state.
pub type Observed = HashMap<String, Liveness>;

#[cfg(test)]
mod tests;
