//! Publishing and consuming messages (doc 06a, "Messages"; doc 13).
//!
//! **Where the message lives.** In memory, while it fits in `memory.limit`; what goes past the
//! limit (or arrives during the drain) goes to disk: the body in `${topic}/${id}.body` and the
//! rest in a row of the topic's database (doc 02, section 3.2, `storage::messages`). Each
//! message is in **one** place only, and the operations (delivery, ack, invisibility, `ttl`,
//! dead-letter) work the same in both. When memory fills up, the split between topics
//! follows each one's read rate, with a reserve per topic (`rates`, `reserve_memory`).
//!
//! **One owner per topic.** Each topic has its state behind a lock of its own: an
//! operation on one topic (including a disk one) never waits for another's. It is the single writer per
//! topic of doc 02 and doc 18 (section 17.4), as a lock instead of a task with a command queue.
//! The only operation that looks at two topics at once is the trip to the dead-letter, always from the source
//! queue to the dead-letter; the memory demotion only uses `try_lock` on the others.
//!
//! **Index.** Each topic keeps a derived index (`index`): the ready list, the
//! invisible one, the age (for the `ttl`) and the per-group counters of fair delivery (doc 13,
//! section 12.11). It is rebuilt from the metadata (memory and database rows) when the topic is
//! opened, so losing it never loses a message.
//!
//! **Delivery.** `GET` takes the ready messages through the index (fairness between groups and the
//! accordion effect), increments `deliveryCount`, swaps the `receiptToken` and leaves them invisible for
//! `visibilityTimeout`. Once the deadline passes the message becomes deliverable again (*at-least-once*). The
//! ack and `PUT visibility` check the last delivery's `receiptToken`. The `ttl` and
//! `maxDeliveries` (with the dead-letter) are applied by `settle`, before each operation and
//! by the background sweep. Waiting `GET`s (long polling) are served in order of arrival.
//!
//! **FIFO queue** (doc 17). Delivery serializes by group (`index`): one message in flight per group,
//! in `id` order. Publication is deduplicated by `(group, deduplicationId)` within
//! `fifo.dedup.window`, and the ack does not delete the row: it leaves a short row with `deleteAt` so
//! deduplication holds for the whole window.
//!
//! The methods are synchronous and block on disk:
//! whoever calls from async code uses `spawn_blocking`.

mod backup;
mod blocks;
mod index;
mod rates;
mod replicas;
pub mod stats;

pub use backup::BackupSink;
pub use blocks::{BlockEntry, BlockMessage, Loaded};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::content::{Body, ContentType, DEFAULT_CONTENT_TYPE};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id::IdGenerator;
use crate::core::ring::{fifo_position, message_position};
use crate::core::validation;
use crate::engine::topics::{CreateTopic, Kind, TopicView, Topics};
use crate::storage;
use crate::storage::messages::{MessageMeta, MessageStore};
use index::{GroupKey, IndexRules, TopicIndex};
use rates::Rates;
use replicas::ReplicaSet;
use sha2::{Digest, Sha256};
use stats::{
    DistributorCounters, GroupSnapshot, NodeCounters, NodeObservation, Percentiles, StatsRules,
    TopicCounters, TopicSnapshot,
};

/// `waitTime` of `GET`: from 0 to 20 seconds (doc 12).
const WAIT_TIME_SECS: (u64, u64) = (0, 20);

/// How many waiting `GET`s, at most, an event wakes at once.
const MAX_WAKE: usize = 16;

/// Above this, the FIFO publication prunes from the deduplication map what already left the window.
const DEDUP_PRUNE_ABOVE: usize = 10_000;

/// The SHA-256 of the body bytes, in hexadecimal (the default deduplication key, doc 17).
fn body_hash(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The configuration values that consuming and publishing depend on (doc 10).
#[derive(Debug, Clone)]
pub struct MessageRules {
    /// `message.max-bytes`: applies on a distributor and on queues without `maxMessageBytes`.
    pub max_bytes: u64,
    /// `delivery.max-messages.default` and `.max` (no ceiling, if there is no `.max`).
    pub max_messages_default: u64,
    pub max_messages_max: Option<u64>,
    /// `delivery.visibility-timeout.default` and `.max`, in seconds.
    pub visibility_default_secs: u64,
    pub visibility_max_secs: Option<u64>,
    /// `topics.distributor.max-targets`.
    pub distributor_max_targets: Option<u64>,
    /// `retention.sweep.interval`: how often the sweep deletes what has expired.
    pub sweep_interval: Duration,
    /// `memory.limit`: bytes of body in memory; without it, nothing goes to disk by overflow.
    pub memory_limit: Option<u64>,
    /// `memory.rate.window`, in seconds: the read-rate window.
    pub rate_window_secs: u64,
    /// `memory.topic.reserve`: messages in memory guaranteed to each topic.
    pub topic_reserve: u64,
    /// `delivery.fair.ready-weight`, `.max-groups` and `.normal-every` (doc 13, section 12.11).
    pub fair_ready_weight: f64,
    pub fair_max_groups: Option<usize>,
    pub fair_normal_every: u32,
    /// `fifo.dedup.window`, in milliseconds.
    pub fifo_dedup_window_ms: u64,
}

impl MessageRules {
    pub fn from_config(config: &Config) -> Self {
        Self {
            max_bytes: config.messages.max_bytes,
            max_messages_default: u64::from(config.delivery.max_messages_default),
            max_messages_max: config.delivery.max_messages_max.map(u64::from),
            visibility_default_secs: config.delivery.visibility_timeout_default.as_secs(),
            visibility_max_secs: config.delivery.visibility_timeout_max.map(|d| d.as_secs()),
            distributor_max_targets: config.topics.distributor_max_targets.map(u64::from),
            sweep_interval: config.retention.sweep_interval,
            memory_limit: config.storage.memory_limit,
            rate_window_secs: config.storage.rate_window.as_secs().max(1),
            topic_reserve: config.storage.topic_reserve,
            fair_ready_weight: config.delivery.fair_ready_weight,
            fair_max_groups: config.delivery.fair_max_groups.map(|n| n as usize),
            fair_normal_every: config.delivery.fair_normal_every,
            fifo_dedup_window_ms: u64::try_from(config.fifo.dedup_window.as_millis())
                .unwrap_or(u64::MAX),
        }
    }

    fn index_rules(&self, fifo: bool) -> IndexRules {
        IndexRules {
            ready_weight: self.fair_ready_weight,
            max_groups: self.fair_max_groups,
            normal_every: self.fair_normal_every,
            fifo,
        }
    }
}

/// `POST /topics/{topic}/messages`: the body is the message's bytes; the rest comes from the
/// `Content-Type` and the query.
#[derive(Debug, Clone, Default)]
pub struct PublishRequest {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
}

/// The result of a publication.
#[derive(Debug)]
pub enum Published {
    /// On a queue: the `id` of the created message (or, on a duplicate, of the original). `duplicate` only
    /// exists on the FIFO queue.
    Queue { id: String, duplicate: Option<bool> },
    /// On a distributor: one copy per queue that matched.
    Distributor {
        deliveries: Vec<DistributorDelivery>,
    },
}

#[derive(Debug)]
pub struct DistributorDelivery {
    pub topic: String,
    /// The copy's `id`, or that queue's error.
    pub result: Result<String, TabaError>,
}

/// The query parameters of `GET /topics/{topic}/messages`, as they came, for the `engine` to
/// validate.
#[derive(Debug, Clone, Default)]
pub struct ConsumeQuery {
    pub max_messages: Option<String>,
    pub visibility_timeout: Option<String>,
    pub wait_time: Option<String>,
    /// `autoCreate`: creates the queue if it does not exist (doc 15, section 14.4). The next four
    /// fields only apply on creation.
    pub auto_create: Option<String>,
    pub volatile: Option<String>,
    pub idle_timeout: Option<String>,
    pub fifo: Option<String>,
    /// `tag.<key>=<value>`, without the `tag.` prefix.
    pub tags: BTreeMap<String, String>,
    /// The caller has the `create` permission (without authentication, always): the `autoCreate` that
    /// would create the queue without it is `403`.
    pub can_create: bool,
}

/// A delivered message, as the API's `Message` (doc 06a, "DTOs").
#[derive(Debug)]
pub struct Delivered {
    pub id: String,
    pub body: Body,
    pub content_type: String,
    pub group_id: Option<String>,
    /// FIFO queue only: the deduplication key (the given one, or the body's hash).
    pub deduplication_id: Option<String>,
    pub receipt_token: String,
    pub delivery_count: u32,
    pub published_ms: u64,
    pub invisible_until_ms: u64,
    /// Dead-letter only: when the message entered it and why.
    pub dead_lettered_ms: Option<u64>,
    pub dead_letter_reason: Option<String>,
}

/// The result of `PUT .../visibility`.
#[derive(Debug)]
pub struct VisibilityUpdated {
    pub id: String,
    pub invisible_until_ms: u64,
}

/// What the drain did (doc 04, section 6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    /// Messages that left memory for disk.
    pub moved: u64,
    /// Messages that stayed in memory because of a disk error.
    pub failed: u64,
    /// Messages still in memory at the end (the `failed` ones and those the early stop left).
    pub remaining: u64,
}

/// What `NodeStatus.storage` shows of disk and memory (doc 06b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageStats {
    pub memory_bytes: u64,
    /// `memory.limit`; `None` without a limit.
    pub memory_limit_bytes: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
}

/// A message in memory: the metadata is the same as the database row's.
struct Stored {
    meta: MessageMeta,
    body: Vec<u8>,
}

/// Where a message is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Memory,
    Disk,
}

/// A topic's state, behind the topic lock.
struct TopicState {
    /// In `id` order: the lowercase GUID v7 sorts by publication instant.
    memory: BTreeMap<String, Stored>,
    /// Everything the topic has (memory and disk), derived from the metadata.
    index: TopicIndex,
    rates: Rates,
    /// FIFO queue only: `(group, deduplicationId)` → `(id of the original, instant of the id)`. Valid until
    /// the window passes, including after the ack.
    dedup: HashMap<(String, String), (String, u64)>,
    /// FIFO queue only: the instant (ms) of the most recent `id` of each group this node knows, in
    /// any role. A group's new primary starts from here so the `id` keeps growing
    /// (doc 17, section 16.4, step 3).
    group_ms: HashMap<String, u64>,
    /// The `maxDeliveries` the ready ones were last checked against: if the `PATCH` changes it,
    /// those already past the new limit go to the dead-letter.
    checked_max_deliveries: Option<Option<u64>>,
    /// What the topic counts since the node opened it (doc 11).
    stats: TopicCounters,
    /// In the cluster: the messages kept here that the node is **not** the delivery owner of. Only the
    /// block's primary puts them in the index (doc 13, section 12.7).
    replicas: ReplicaSet,
    /// In the cluster: the messages the owner already sent to the dead-letter and that await the commit.
    dead_letter_inflight: HashSet<String>,
    /// The last "has ready messages" notified to the other nodes.
    ready_signaled: bool,
}

fn message_not_found() -> TabaError {
    TabaError::new(ErrorCode::MessageNotFound, "The message does not exist.")
}

fn stale_receipt() -> TabaError {
    TabaError::new(
        ErrorCode::MessageStaleReceipt,
        "The receiptToken is not that of this message's last delivery.",
    )
}

/// Called when a disk error brings the node down (full disk, corrupt database).
pub type FailureHook = Arc<dyn Fn() + Send + Sync>;

/// `validation.required`, with each missing field in `details`.
fn required(fields: &[&str]) -> TabaError {
    fields.iter().fold(
        TabaError::new(
            ErrorCode::ValidationRequired,
            "Required fields are missing.",
        ),
        |error, field| error.with_detail(*field, ErrorCode::ValidationRequired, "Required field."),
    )
}

/// How many `pruneId`s a node remembers to answer a repeated call.
const MAX_REMEMBERED_PRUNES: usize = 64;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `autoCreate=true` in the query? A value that is neither `true` nor `false` is `400`.
fn wants_auto_create(query: &ConsumeQuery) -> Result<bool, TabaError> {
    Ok(optional_bool("autoCreate", query.auto_create.as_deref())?.unwrap_or(false))
}

fn query_failed(field: &str, message: &str) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationFailed,
        "The request has invalid fields.",
    )
    .with_detail(field, ErrorCode::ValidationFailed, message)
}

fn optional_bool(field: &str, text: Option<&str>) -> Result<Option<bool>, TabaError> {
    match text {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(_) => Err(query_failed(field, "Use true or false.")),
    }
}

fn optional_int(field: &str, text: Option<&str>) -> Result<Option<i64>, TabaError> {
    match text {
        None => Ok(None),
        Some(t) => t
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| query_failed(field, "Expected an integer.")),
    }
}

fn wrong_type() -> TabaError {
    TabaError::new(
        ErrorCode::TopicWrongType,
        "A distributor topic is not consumed.",
    )
}

fn too_large(limit: u64) -> TabaError {
    TabaError::new(
        ErrorCode::MessageTooLarge,
        format!("The body exceeds the topic's limit of {limit} bytes."),
    )
}

/// A query integer within the range: text that is not an integer is `validation.failed`, and an
/// integer outside the range (including negative) is `validation.out-of-range`.
fn query_number(field: &str, text: &str, range: (u64, u64)) -> Result<u64, TabaError> {
    let Ok(value) = text.trim().parse::<i64>() else {
        let message = "Expected an integer.";
        return Err(TabaError::new(
            ErrorCode::ValidationFailed,
            "The request has invalid fields.",
        )
        .with_detail(field, ErrorCode::ValidationFailed, message));
    };
    validation::in_range_signed(field, value, range)
}

/// The parameters of a `GET`, already checked.
#[derive(Debug, Clone, Copy)]
pub struct ConsumeParams {
    pub max_messages: u64,
    pub visibility_secs: u64,
    pub wait_secs: u64,
}

/// What the group's primary decides on receiving a FIFO publication.
#[derive(Debug, Clone)]
pub enum Draft {
    /// A new message, with `id` and position already defined.
    New { meta: MessageMeta, body: Vec<u8> },
    /// The key already exists in the group within the window: the original's `id` applies.
    Duplicate { id: String },
}

/// Who says whether this node is the **delivery owner** of a message (doc 13, section 12.7): the primary of its
/// block on the ring. Without it (`mode=development`) the node owns everything.
pub trait Placement: Send + Sync + 'static {
    /// `position` is the message's placement position on the ring: the hash of the `id` or, on the FIFO queue,
    /// of the group (doc 17, section 16.3).
    fn is_owner(&self, topic: &str, id: &str, group: Option<&str>, position: u64) -> bool;

    /// Is the node one of the nodes responsible (primary or replica) for the position's block? The copy of a
    /// message the node stopped being responsible for can be deleted (doc 05, section 7.3, step 7).
    fn is_responsible(&self, _position: u64) -> bool {
        true
    }
}

/// A message the owner sent to the dead-letter and that the responsible nodes still have to move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadLetterJob {
    pub topic: String,
    pub id: String,
}

/// Notifies the cluster when the topic starts (or stops) having ready messages.
pub type ReadyHook = Arc<dyn Fn(&str, bool) + Send + Sync>;

/// What the cluster hands to the message engine.
pub struct ClusterHooks {
    pub placement: Arc<dyn Placement>,
    /// The queue of moves to the dead-letter, emptied by whoever coordinates the write.
    pub dead_letters: tokio::sync::mpsc::UnboundedSender<DeadLetterJob>,
    /// Called when the topic starts (or stops) having ready messages on this node.
    pub ready: ReadyHook,
}

/// What the node keeps of a message: the metadata and whether it is the owner.
#[derive(Debug, Clone)]
pub struct MessageInfo {
    pub meta: MessageMeta,
    pub owned: bool,
}

/// How an invisibility record is checked (doc 14, section 13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityCheck {
    /// Delivery: the message cannot be invisible.
    Delivery,
    /// Change (and ack): the `receiptToken` must match.
    Receipt,
}

/// A message chosen for delivery, already with the new counter, deadline and token, but not yet
/// confirmed by the quorum: until the commit it is reserved (in processing) in the owner's index.
pub struct Candidate {
    /// The message as it will be after the delivery.
    pub meta: MessageMeta,
    pub body: Vec<u8>,
    previous_deliveries: u32,
    band: u32,
}

impl Candidate {
    /// The message as the client API shows it.
    pub fn into_delivered(self) -> Delivered {
        let content_type = ContentType::parse(&self.meta.content_type)
            .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("valid"));
        Delivered {
            id: self.meta.id,
            body: Body::render(&content_type, &self.body),
            content_type: self.meta.content_type,
            group_id: self.meta.group_id,
            deduplication_id: self.meta.deduplication_id,
            receipt_token: self.meta.receipt_token.unwrap_or_default(),
            delivery_count: self.meta.delivery_count,
            published_ms: self.meta.published_ms,
            invisible_until_ms: self.meta.invisible_until_ms,
            dead_lettered_ms: self.meta.dead_lettered_ms,
            dead_letter_reason: self.meta.dead_letter_reason,
        }
    }
}

/// What a `take` returns.
struct Taken {
    delivered: Vec<Delivered>,
    /// When the next invisible one becomes deliverable again, so long polling knows how long to
    /// sleep.
    next_visible_ms: Option<u64>,
    /// There is still a ready message: the next waiting listener should wake up.
    more_ready: bool,
}

/// How often the overflow summary goes to the log (provisional, D118).
pub const OVERFLOW_SUMMARY_EVERY: Duration = Duration::from_secs(60);

/// How often the limits of the log of excluded messages are applied.
const EXCLUDED_TRIM_EVERY: Duration = Duration::from_secs(30);

pub struct Messages {
    topics: Arc<Topics>,
    clock: Arc<dyn Clock>,
    rules: MessageRules,
    ids: IdGenerator,
    store: Arc<MessageStore>,
    /// The state of each topic, created (and the index rebuilt from disk) on first use.
    states: RwLock<HashMap<String, Arc<Mutex<TopicState>>>>,
    /// Bytes of body in memory, of all topics, against `memory.limit`.
    memory_bytes: AtomicU64,
    /// In the drain the effective memory limit is zero: every new message goes to disk.
    draining: AtomicBool,
    /// The previous life's disk awaits the restart recovery: the sweep does not delete it.
    keep_old_disk: AtomicBool,
    /// The previous life's disk, set aside until the restart recovery ends (PRB-048).
    previous_life: Mutex<Option<Arc<MessageStore>>>,
    /// Wakes the waiting `GET`s (long polling) when a message arrives or the topic goes away.
    notifiers: Mutex<HashMap<String, Arc<Notify>>>,
    /// Notifies the node that the disk failed in a way that brings it down (`failed` state).
    on_failure: OnceLock<FailureHook>,
    /// The instant of each queue's last `GET`, by topic `id` (the name can be reused by
    /// a recreated queue). Applies to the volatile queues' idleness clock.
    last_get: Mutex<HashMap<String, u64>>,
    /// The latest prunes this node did (`pruneId` → messages removed), so that a repeated call does not
    /// prune again and answers the same.
    prunes: Mutex<VecDeque<(String, u64)>>,
    /// The node's start, in milliseconds: the idleness clock never counts the stopped time.
    started_ms: u64,
    /// What the whole node counts (doc 11): read by `GET /metrics`.
    counters: NodeCounters,
    /// What each distributor counts, by name.
    distributors: Mutex<HashMap<String, DistributorCounters>>,
    /// Only in `mode=production`: who says whether the node is the owner, where the dead-letters go and the signals.
    cluster: OnceLock<ClusterHooks>,
    /// The optional S3 backup: the engine only notifies it of what happens (doc 23, "S3 backup").
    backup: OnceLock<Arc<dyn BackupSink>>,
}

/// Closes the wait of a long-polling `GET`: if the caller gave up before the end (the connection
/// dropped and the future was dropped), it counts as `disconnected`.
pub struct LongPoll<'a> {
    counters: &'a NodeCounters,
    /// `Some(true)`: ended delivering; `Some(false)`: ended empty; `None`: was dropped.
    pub delivered: Option<bool>,
}

impl<'a> LongPoll<'a> {
    pub fn start(counters: &'a NodeCounters) -> Self {
        counters.longpoll_waiting.fetch_add(1, Ordering::Relaxed);
        Self {
            counters,
            delivered: None,
        }
    }
}

impl Drop for LongPoll<'_> {
    fn drop(&mut self) {
        self.counters
            .longpoll_waiting
            .fetch_sub(1, Ordering::Relaxed);
        let counter = match self.delivered {
            Some(true) => &self.counters.longpoll_delivered,
            Some(false) => &self.counters.longpoll_empty,
            None => &self.counters.longpoll_disconnected,
        };
        NodeCounters::add(counter, 1);
    }
}

impl Messages {
    /// Registers who is warned of a fatal disk error. Only the first call counts.
    pub fn on_failure(&self, hook: FailureHook) {
        let _ = self.on_failure.set(hook);
    }

    /// Disk error: becomes `500`, and the detail goes to the log. A full disk or a corrupt database
    /// also warn the node, which goes to `failed` (doc 05, section 7.3).
    fn disk_error(&self, what: &str, e: &io::Error) -> TabaError {
        tracing::error!(error = %e, "{what}");
        NodeCounters::add(&self.counters.disk_errors, 1);
        if storage::is_fatal(e) {
            self.fail();
        }
        TabaError::internal()
    }

    fn fail(&self) {
        if let Some(hook) = self.on_failure.get() {
            hook();
        }
    }

    /// `resume`: resumes the messages that are on disk (development with
    /// `development.resume-on-start`). Without it, what is left on disk from an earlier
    /// run is deleted: the node starts empty. Folders of topics that no longer exist are also
    /// deleted, so a topic recreated with the same name does not resurrect messages.
    pub fn new(
        topics: Arc<Topics>,
        clock: Arc<dyn Clock>,
        rules: MessageRules,
        store: Arc<MessageStore>,
        resume: bool,
    ) -> Arc<Self> {
        Self::build(topics, clock, rules, store, resume, false)
    }

    /// Like `new`, for the production node in a cluster: what is left on the previous life's disk is **not**
    /// deleted at startup or by the sweep. The node queries it first, in the bootstrap, to
    /// return what the others declared lost (doc 04, section 6.6; D111), and only then deletes it
    /// ([`Messages::release_old_disk`]).
    pub fn new_keeping_disk(
        topics: Arc<Topics>,
        clock: Arc<dyn Clock>,
        rules: MessageRules,
        store: Arc<MessageStore>,
    ) -> Arc<Self> {
        Self::build(topics, clock, rules, store, false, true)
    }

    /// Sets aside the previous life's disk and leaves the node with an empty disk, so it can join without waiting for
    /// the recovery (PRB-048). Returns whether there is anything to recover.
    pub fn set_aside_old_disk(&self) -> io::Result<bool> {
        // A disk with a manifest becomes the folder of that life; one without it (an earlier version) keeps
        // the path of the loss list only (doc 37, section 4.12).
        let life = crate::cluster::restore::Manifest::read(self.store.root()).map(|m| m.node_id);
        let previous = self
            .store
            .set_aside_old_disk(life.as_deref())?
            .map(Arc::new);
        // The disk is new, and so is its log of excluded messages: the recording starts now.
        self.store.set_excluded_since(self.clock.now_millis())?;
        self.store.forget_excluded_inheritance()?;
        let some = previous.is_some();
        *lock(&self.previous_life) = previous;
        Ok(some)
    }

    /// The `storage.dir` (where the manifest of the running life is written).
    pub fn storage_root(&self) -> &std::path::Path {
        self.store.root()
    }

    /// The restart recovery is over: the disk with no manifest is deleted. The folder of a life stays until the
    /// end of the restoration (doc 37, section 4.7).
    pub fn finish_previous_life(&self) {
        *lock(&self.previous_life) = None;
        if let Err(e) = self.store.drop_old_disk() {
            tracing::warn!(error = %e, "could not delete the previous life's disk");
        }
    }

    /// The restart recovery is over: the sweep goes back to deleting what belongs to an unknown topic.
    pub fn release_old_disk(&self) {
        self.keep_old_disk.store(false, Ordering::SeqCst);
    }

    fn build(
        topics: Arc<Topics>,
        clock: Arc<dyn Clock>,
        rules: MessageRules,
        store: Arc<MessageStore>,
        resume: bool,
        keep_old_disk: bool,
    ) -> Arc<Self> {
        match store.topics_on_disk() {
            Ok(on_disk) if !keep_old_disk => {
                for name in on_disk {
                    if !resume || topics.get(&name).is_err() {
                        tracing::info!(topic = %name, "deleting old messages from disk");
                        if let Err(e) = store.drop_topic(&name) {
                            tracing::warn!(topic = %name, error = %e, "could not delete");
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "could not list the messages on disk"),
        }
        let started_ms = clock.now_millis();
        let messages = Arc::new(Self {
            topics: topics.clone(),
            clock,
            rules,
            ids: IdGenerator::new(),
            store,
            states: RwLock::new(HashMap::new()),
            memory_bytes: AtomicU64::new(0),
            draining: AtomicBool::new(false),
            keep_old_disk: AtomicBool::new(keep_old_disk),
            previous_life: Mutex::new(None),
            notifiers: Mutex::new(HashMap::new()),
            on_failure: OnceLock::new(),
            last_get: Mutex::new(HashMap::new()),
            prunes: Mutex::new(VecDeque::new()),
            started_ms,
            counters: NodeCounters::new(),
            distributors: Mutex::new(HashMap::new()),
            cluster: OnceLock::new(),
            backup: OnceLock::new(),
        });
        // The messages go with the topic (doc 02, section 3.3).
        let weak = Arc::downgrade(&messages);
        topics.on_removed(Arc::new(move |names| {
            if let Some(messages) = weak.upgrade() {
                for name in names {
                    messages.drop_topic(name);
                }
            }
        }));
        messages
    }

    // ---- Optional S3 backup ----

    /// Attaches the backup. Only the first call counts.
    pub fn set_backup(&self, sink: Arc<dyn BackupSink>) {
        let _ = self.backup.set(sink);
    }

    fn backup_stored(&self, topic: &str, id: &str, owned: bool) {
        if let Some(sink) = self.backup.get() {
            sink.stored(topic, id, owned);
        }
    }

    fn backup_removed(&self, topic: &str, id: &str, owned: bool) {
        if let Some(sink) = self.backup.get() {
            sink.removed(topic, id, owned);
        }
    }

    fn backup_changed(&self, topic: &str, id: &str, owned: bool) {
        if let Some(sink) = self.backup.get() {
            sink.changed(topic, id, owned);
        }
    }

    /// The body of a message the eviction took off disk, fetched from S3.
    fn backup_body(&self, topic: &str, id: &str) -> Option<Vec<u8>> {
        self.backup
            .get()
            .and_then(|sink| sink.fetch_body(topic, id))
    }

    /// The topics the node knows, in memory or on disk.
    pub fn topic_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .existing_states()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        if let Ok(on_disk) = self.store.topics_on_disk() {
            names.extend(on_disk);
        }
        names.sort();
        names.dedup();
        names
    }

    /// The `id`s of the messages the node owns in the topic (those the primary sends to S3).
    pub fn owned_ids(&self, topic: &str) -> Vec<String> {
        let state = self.state_of(topic);
        lock(&state).index.ids()
    }

    /// The metadata and body of a message the node keeps, for the backup. `None` if it
    /// no longer exists or if the body is not on disk.
    pub fn backup_load(&self, topic: &str, id: &str) -> Option<(MessageMeta, Vec<u8>)> {
        let state = self.state_of(topic);
        let state = lock(&state);
        if let Some(stored) = state.memory.get(id) {
            return Some((stored.meta.clone(), stored.body.clone()));
        }
        if !state.index.contains(id) && !state.replicas.contains(id) {
            return None;
        }
        let meta = self.store.get(topic, id).ok().flatten()?;
        let body = self.store.body(topic, id).ok()?;
        Some((meta, body))
    }

    /// Gives the node a message read from the backup (restore): it is an already-decided publication.
    pub fn restore_message(
        &self,
        topic: &str,
        meta: MessageMeta,
        body: Vec<u8>,
    ) -> Result<(), TabaError> {
        self.apply_publish(topic, meta, body)
    }

    // ---- Topic state ----

    /// The topic's state. On first use the index is rebuilt from the topic database rows
    /// (restart or topic resumed from disk).
    fn state_of(&self, name: &str) -> Arc<Mutex<TopicState>> {
        if let Some(state) = self
            .states
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
        {
            return state.clone();
        }
        let mut states = self.states.write().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = states.get(name) {
            return state.clone();
        }
        let now = self.clock.now_millis();
        let fifo = self
            .topics
            .get(name)
            .is_ok_and(|view| view.definition.queue().is_some_and(|q| q.fifo));
        let mut index = TopicIndex::new(self.rules.index_rules(fifo));
        let mut dedup = HashMap::new();
        let mut group_ms: HashMap<String, u64> = HashMap::new();
        match self.store.scan(name) {
            Ok(rows) => {
                for row in rows {
                    if fifo && let Some(group) = &row.group_id {
                        let last = group_ms.entry(group.clone()).or_default();
                        *last = (*last).max(row.published_ms);
                    }
                    if fifo
                        && row
                            .published_ms
                            .saturating_add(self.rules.fifo_dedup_window_ms)
                            > now
                        && let (Some(group), Some(key)) = (&row.group_id, &row.deduplication_id)
                    {
                        dedup.insert(
                            (group.clone(), key.clone()),
                            (row.id.clone(), row.published_ms),
                        );
                    }
                    // The ack's short row only serves deduplication.
                    if row.delete_at_ms.is_some() {
                        continue;
                    }
                    index.insert(
                        &row.id,
                        row.group_id.as_deref(),
                        row.age_ms,
                        row.invisible_until_ms,
                        row.delivery_count,
                        now,
                    );
                }
            }
            Err(e) => {
                self.disk_error("failed to read the topic index on disk", &e);
            }
        }
        let state = Arc::new(Mutex::new(TopicState {
            memory: BTreeMap::new(),
            index,
            rates: Rates::default(),
            dedup,
            group_ms,
            checked_max_deliveries: None,
            stats: TopicCounters::new(),
            replicas: ReplicaSet::default(),
            dead_letter_inflight: HashSet::new(),
            ready_signaled: false,
        }));
        states.insert(name.to_owned(), state.clone());
        state
    }

    fn existing_states(&self) -> Vec<(String, Arc<Mutex<TopicState>>)> {
        self.states
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(name, state)| (name.clone(), state.clone()))
            .collect()
    }

    fn drop_topic(&self, name: &str) {
        let removed = self
            .states
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        if let Some(state) = removed {
            let bytes: u64 = lock(&state)
                .memory
                .values()
                .map(|s| s.body.len() as u64)
                .sum();
            self.memory_bytes.fetch_sub(bytes, Ordering::SeqCst);
        }
        if let Err(e) = self.store.drop_topic(name) {
            tracing::warn!(topic = %name, error = %e, "could not delete the topic's disk");
        }
        if let Some(notify) = lock(&self.notifiers).get(name) {
            notify.notify_waiters();
        }
    }

    fn notifier(&self, name: &str) -> Arc<Notify> {
        lock(&self.notifiers)
            .entry(name.to_owned())
            .or_default()
            .clone()
    }

    /// Wakes up to `count` waiting `GET`s, in the order they arrived (each `notify_one` takes the
    /// first in the listener queue, doc 13, section 12.8).
    fn wake(&self, name: &str, count: usize) {
        let notify = self.notifier(name);
        for _ in 0..count.clamp(1, MAX_WAKE) {
            notify.notify_one();
        }
    }

    // ---- Memory ----

    fn add_memory(&self, size: u64) {
        self.memory_bytes.fetch_add(size, Ordering::SeqCst);
    }

    fn sub_memory(&self, size: u64) {
        self.memory_bytes.fetch_sub(size, Ordering::SeqCst);
    }

    /// Reserves `size` bytes without exceeding the limit. `false` if it does not fit.
    fn try_reserve(&self, size: u64, limit: u64) -> bool {
        self.memory_bytes
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
                used.checked_add(size).filter(|total| *total <= limit)
            })
            .is_ok()
    }

    /// Takes the message out of the topic's memory, giving the bytes back to the total.
    fn take_from_memory(&self, state: &mut TopicState, id: &str) -> Option<Stored> {
        let stored = state.memory.remove(id)?;
        self.sub_memory(stored.body.len() as u64);
        Some(stored)
    }

    fn put_in_memory(&self, state: &mut TopicState, stored: Stored) {
        self.add_memory(stored.body.len() as u64);
        if let Some(previous) = state.memory.insert(stored.meta.id.clone(), stored) {
            self.sub_memory(previous.body.len() as u64);
        }
    }

    /// Decides whether the new message of `size` bytes stays in memory (and already reserves the space).
    ///
    /// With memory full (doc 02, section 3.2): the topic within its **reserve** makes room by
    /// demoting the messages the others keep above their reserve, starting with the one with the lowest
    /// read rate; the topic that already passed its reserve only demotes those with a **lower** rate
    /// than its own. Not finding room, the message goes to disk.
    fn reserve_memory(&self, state: &TopicState, name: &str, size: u64, now: u64) -> bool {
        if self.draining.load(Ordering::SeqCst) {
            return false;
        }
        let Some(limit) = self.rules.memory_limit else {
            self.add_memory(size);
            return true;
        };
        if self.try_reserve(size, limit) {
            return true;
        }
        let within_reserve = (state.memory.len() as u64) < self.rules.topic_reserve;
        let below_rate = (!within_reserve).then(|| self.consume_rate(state, now));
        let used = self.memory_bytes.load(Ordering::SeqCst);
        let need = (used + size).saturating_sub(limit);
        self.demote(name, need, below_rate, now);
        self.try_reserve(size, limit)
    }

    fn consume_rate(&self, state: &TopicState, now: u64) -> f64 {
        state
            .rates
            .consumed
            .per_second(now, self.rules.rate_window_secs)
    }

    /// Demotes to disk at least `need` bytes of other topics, above their reserve. It only
    /// uses `try_lock`: a busy topic is skipped, which avoids locking one against another.
    fn demote(&self, exclude: &str, need: u64, below_rate: Option<f64>, now: u64) {
        let mut victims: Vec<(f64, String, Arc<Mutex<TopicState>>)> = Vec::new();
        for (name, state) in self.existing_states() {
            if name == exclude {
                continue;
            }
            let Ok(guard) = state.try_lock() else {
                continue;
            };
            if (guard.memory.len() as u64) <= self.rules.topic_reserve {
                continue;
            }
            let rate = self.consume_rate(&guard, now);
            if below_rate.is_some_and(|mine| rate >= mine) {
                continue;
            }
            drop(guard);
            victims.push((rate, name, state));
        }
        victims.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let mut freed = 0;
        for (_, name, state) in victims {
            let Ok(mut guard) = state.try_lock() else {
                continue;
            };
            while freed < need && (guard.memory.len() as u64) > self.rules.topic_reserve {
                let Some(id) = Self::demotion_candidate(&guard) else {
                    break;
                };
                let Some(stored) = self.take_from_memory(&mut guard, &id) else {
                    break;
                };
                match self.store.put(&name, &stored.meta, &stored.body) {
                    Ok(()) => {
                        freed += stored.body.len() as u64;
                        NodeCounters::add(&self.counters.overflow, 1);
                    }
                    Err(e) => {
                        self.disk_error("failed to demote the message to disk", &e);
                        self.put_in_memory(&mut guard, stored);
                        break;
                    }
                }
            }
            if freed >= need {
                return;
            }
        }
    }

    /// Who leaves memory first: what is in processing (only read again after the
    /// deadline) and, then, the **middle** of the ready list, the last one the accordion effect
    /// would reach. The ends stay: they are the next to be delivered.
    fn demotion_candidate(state: &TopicState) -> Option<String> {
        state
            .memory
            .keys()
            .find(|id| state.index.is_processing(id))
            .or_else(|| state.memory.keys().nth(state.memory.len() / 2))
            .cloned()
    }

    // ---- Publishing ----

    /// The largest body the publish route reads: the queue's `maxMessageBytes`, or the general one
    /// on a distributor. Answers `404` if the topic does not exist before reading the body.
    pub fn publish_limit(&self, topic: &str) -> Result<u64, TabaError> {
        let view = self.topics.get(topic)?;
        Ok(view.max_message_bytes().unwrap_or(self.rules.max_bytes))
    }

    pub fn publish(&self, topic: &str, request: PublishRequest) -> Result<Published, TabaError> {
        let started = Instant::now();
        let view = self.topics.get(topic)?;
        let result = self.publish_checked(&view, &request, started);
        if result.is_err() {
            self.count_publish_error(&view);
        }
        result
    }

    fn publish_checked(
        &self,
        view: &TopicView,
        request: &PublishRequest,
        started: Instant,
    ) -> Result<Published, TabaError> {
        let prepared = Prepared::new(request)?;
        if let Some(prefixes) = view.definition.prefixes() {
            if request.body.len() as u64 > self.rules.max_bytes {
                return Err(too_large(self.rules.max_bytes));
            }
            return self.publish_to_distributor(
                &view.definition.name,
                prefixes,
                request,
                &prepared,
            );
        }
        let limit = view.max_message_bytes().unwrap_or(self.rules.max_bytes);
        if request.body.len() as u64 > limit {
            return Err(too_large(limit));
        }
        self.store(view, request, &prepared, started)
            .map(|(id, duplicate)| Published::Queue { id, duplicate })
    }

    /// A publication that returned an error to the client (validation, size, disk). The distributor
    /// does not count: it counts accepted publications and copies.
    fn count_publish_error(&self, view: &TopicView) {
        if view.definition.queue().is_some() {
            lock(&self.state_of(&view.definition.name))
                .stats
                .publish_errors += 1;
        }
    }

    /// Validates the publication fields that depend on the queue type and stores the message, in
    /// memory or, if it does not fit, on disk (first the body, then the row).
    fn store(
        &self,
        view: &TopicView,
        request: &PublishRequest,
        prepared: &Prepared,
        started: Instant,
    ) -> Result<(String, Option<bool>), TabaError> {
        let fifo = view.definition.queue().is_some_and(|q| q.fifo);
        if fifo && request.group_id.is_none() {
            return Err(TabaError::new(
                ErrorCode::ValidationGroupIdRequired,
                "A FIFO queue requires `groupId`.",
            )
            .with_detail(
                "groupId",
                ErrorCode::ValidationGroupIdRequired,
                "Required on a FIFO queue.",
            ));
        }
        if !fifo && request.deduplication_id.is_some() {
            return Err(TabaError::new(
                ErrorCode::ValidationNotApplicable,
                "`deduplicationId` only applies on a FIFO queue.",
            )
            .with_detail(
                "deduplicationId",
                ErrorCode::ValidationNotApplicable,
                "Only applies on a FIFO queue.",
            ));
        }
        let now = self.clock.now_millis();
        // On FIFO every message has a deduplication key: the given one, or the body's hash.
        let dedup_key = fifo.then(|| {
            (
                request.group_id.clone().unwrap_or_default(),
                request
                    .deduplication_id
                    .clone()
                    .unwrap_or_else(|| body_hash(&request.body)),
            )
        });
        let id = self.ids.new_at(now);
        let name = view.definition.name.clone();
        let meta = MessageMeta {
            id: id.clone(),
            content_type: prepared.content_type.as_str().to_owned(),
            group_id: request.group_id.clone(),
            deduplication_id: dedup_key.as_ref().map(|(_, key)| key.clone()),
            published_ms: now,
            delivery_count: 0,
            invisible_until_ms: 0,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
            position: self.position_of(view, &id, request.group_id.as_deref()),
        };
        let size = request.body.len() as u64;
        {
            let state = self.state_of(&name);
            let mut state = lock(&state);
            // The group's primary (here, this node) checks for a duplicate before creating anything.
            if let Some(key) = &dedup_key {
                if let Some(original) = self.duplicate_of(&mut state, &name, key, now) {
                    return Ok((original, Some(true)));
                }
                if state.dedup.len() > DEDUP_PRUNE_ABOVE {
                    let window = self.rules.fifo_dedup_window_ms;
                    state
                        .dedup
                        .retain(|_, (_, at)| at.saturating_add(window) > now);
                }
            }
            if self.reserve_memory(&state, &name, size, now) {
                state.memory.insert(
                    id.clone(),
                    Stored {
                        meta,
                        body: request.body.clone(),
                    },
                );
            } else {
                self.store
                    .put(&name, &meta, &request.body)
                    .map_err(|e| self.disk_error("failed to write the message to disk", &e))?;
                // In the drain every message goes to disk: that is not a lack of memory.
                if !self.draining.load(Ordering::SeqCst) {
                    NodeCounters::add(&self.counters.overflow, 1);
                }
            }
            state
                .index
                .insert(&id, request.group_id.as_deref(), now, 0, 0, now);
            self.backup_stored(&name, &id, true);
            if let Some(key) = dedup_key {
                state.dedup.insert(key, (id.clone(), now));
            }
            state
                .rates
                .published
                .record(now, 1, self.rules.rate_window_secs);
            state.stats.published += 1;
            state.stats.published_bytes += size;
            state.stats.last_published_ms = Some(now);
            state
                .stats
                .publish_latency
                .record(started.elapsed().as_secs_f64());
        }
        self.wake(&name, 1);
        Ok((id, fifo.then_some(false)))
    }

    /// If the deduplication key already exists in the group within the window, it counts the duplicate and returns
    /// the original's `id` (doc 17, section 16.7).
    fn duplicate_of(
        &self,
        state: &mut TopicState,
        name: &str,
        key: &(String, String),
        now: u64,
    ) -> Option<String> {
        let (original, at) = state.dedup.get(key)?;
        if at.saturating_add(self.rules.fifo_dedup_window_ms) <= now {
            return None;
        }
        let original = original.clone();
        state.stats.deduplicated += 1;
        NodeCounters::add(&self.counters.deduplicated, 1);
        tracing::debug!(
            event.name = "taba.message.deduplicated",
            messaging.destination.name = %name,
            group_id = %key.0,
            deduplication_id = %key.1,
            message.id = %original,
            "publication dropped by deduplication"
        );
        Some(original)
    }

    /// An independent copy, with its own `id`, in each queue matching a prefix. Zero
    /// queues is not an error: the message is discarded (doc 15, section 14.2).
    fn publish_to_distributor(
        &self,
        name: &str,
        prefixes: &[String],
        request: &PublishRequest,
        prepared: &Prepared,
    ) -> Result<Published, TabaError> {
        let targets = self.topics.queues_with_prefix(prefixes);
        if let Some(max) = self.rules.distributor_max_targets
            && targets.len() as u64 > max
        {
            return Err(TabaError::new(
                ErrorCode::DistributorTooManyTargets,
                format!("The publication matches more than {max} queues."),
            ));
        }
        let deliveries = targets
            .iter()
            .map(|view| {
                // The deduplication key only applies to FIFO queues: on plain ones the copy ignores it.
                let is_fifo = view.definition.queue().is_some_and(|q| q.fifo);
                let copy;
                let request = if is_fifo || request.deduplication_id.is_none() {
                    request
                } else {
                    copy = PublishRequest {
                        deduplication_id: None,
                        ..request.clone()
                    };
                    &copy
                };
                DistributorDelivery {
                    topic: view.definition.name.clone(),
                    result: self
                        .store(view, request, prepared, Instant::now())
                        .map(|(id, _)| id),
                }
            })
            .collect::<Vec<_>>();
        let failed = deliveries.iter().filter(|d| d.result.is_err()).count() as u64;
        {
            let mut distributors = lock(&self.distributors);
            let counters = distributors.entry(name.to_owned()).or_default();
            counters.received += 1;
            counters.copies += deliveries.len() as u64 - failed;
            counters.copy_errors += failed;
        }
        if deliveries.is_empty() {
            tracing::info!(
                event.name = "taba.distributor.no_target",
                messaging.destination.name = %name,
                "publication with no matching queue: the message was discarded"
            );
        } else if failed > 0 {
            tracing::warn!(
                event.name = "taba.distributor.partial",
                messaging.destination.name = %name,
                failed,
                copies = deliveries.len(),
                "at least one copy of the publication failed"
            );
        }
        Ok(Published::Distributor { deliveries })
    }

    // ---- Ack e invisibilidade ----

    /// A topic's queue that accepts ack and `PUT visibility`: `404` if it does not exist, `409` if
    /// it is a distributor.
    pub fn queue_view(&self, topic: &str) -> Result<TopicView, TabaError> {
        let view = self.topics.get(topic)?;
        if view.definition.queue().is_none() {
            return Err(wrong_type());
        }
        Ok(view)
    }

    /// Finds the message, in memory or on disk, and checks the `receiptToken`: a token that
    /// is not the last delivery's is `409`.
    fn locate(
        &self,
        state: &TopicState,
        topic: &str,
        id: &str,
        receipt_token: &str,
    ) -> Result<(MessageMeta, Place), TabaError> {
        if !state.index.contains(id) {
            return Err(message_not_found());
        }
        let found = match state.memory.get(id) {
            Some(stored) => Some((stored.meta.clone(), Place::Memory)),
            None => self
                .store
                .get(topic, id)
                .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
                .map(|meta| (meta, Place::Disk)),
        };
        let (meta, place) = found.ok_or_else(message_not_found)?;
        if meta.receipt_token.as_deref() != Some(receipt_token) {
            return Err(stale_receipt());
        }
        Ok((meta, place))
    }

    /// Confirms the consumption and removes the message (doc 13, section 12.4). The `receiptToken` is the
    /// **last** delivery's; an old token is `409` and nothing changes. Once the invisibility
    /// deadline has passed, but before a new delivery, the token still applies. On disk it removes the
    /// row and then the file.
    pub fn ack(&self, topic: &str, id: &str, receipt_token: Option<&str>) -> Result<(), TabaError> {
        let view = self.queue_view(topic)?;
        let receipt_token = receipt_token.ok_or_else(|| required(&["receiptToken"]))?;
        let started = Instant::now();
        let now = self.clock.now_millis();
        self.settle(&view, now);
        let name = &view.definition.name;
        let state = self.state_of(name);
        let mut state = lock(&state);
        let (meta, place) = self.locate(&state, name, id, receipt_token)?;
        if view.definition.queue().is_some_and(|q| q.fifo) {
            self.fifo_ack(&mut state, name, meta, place, now)?;
        } else {
            self.remove_message(&mut state, name, id, place)?;
        }
        state.stats.acked += 1;
        state.stats.last_consumed_ms = Some(now);
        state
            .rates
            .acked
            .record(now, 1, self.rules.rate_window_secs);
        state
            .stats
            .consume_latency
            .record(started.elapsed().as_secs_f64());
        Ok(())
    }

    /// The FIFO queue's ack does not delete the row: it writes `deleteAt` (the id's instant plus the
    /// deduplication window) and deletes only the body and delivery metadata, so deduplication holds for the
    /// whole window (doc 17, section 16.7). If `deleteAt` already passed, it deletes right away, as on the
    /// plain queue.
    fn fifo_ack(
        &self,
        state: &mut TopicState,
        topic: &str,
        meta: MessageMeta,
        place: Place,
        now: u64,
    ) -> Result<(), TabaError> {
        let delete_at = meta
            .published_ms
            .saturating_add(self.rules.fifo_dedup_window_ms);
        if delete_at <= now {
            return self.remove_message(state, topic, &meta.id, place);
        }
        self.store
            .tombstone_excluded(topic, &meta, delete_at, now)
            .map_err(|e| self.disk_error("failed to write the FIFO queue's ack", &e))?;
        let owned = state.index.contains(&meta.id);
        self.take_from_memory(state, &meta.id);
        state.index.remove(&meta.id);
        self.backup_removed(topic, &meta.id, owned);
        Ok(())
    }

    /// Takes the message from wherever it is and from the index, recording its **exclusion** in the log
    /// (doc 37, section 4.4): ack, prune.
    fn remove_message(
        &self,
        state: &mut TopicState,
        topic: &str,
        id: &str,
        place: Place,
    ) -> Result<(), TabaError> {
        self.remove_message_as(state, topic, id, place, true)
    }

    /// Like `remove_message`; with `exclude` false (the `ttl` expiry) nothing is recorded: a copy that comes
    /// back from an old life expires again by the same rule.
    fn remove_message_as(
        &self,
        state: &mut TopicState,
        topic: &str,
        id: &str,
        place: Place,
        exclude: bool,
    ) -> Result<(), TabaError> {
        let owned = state.index.contains(id);
        let now = self.clock.now_millis();
        match place {
            Place::Memory => {
                if let Some(stored) = self.take_from_memory(state, id)
                    && exclude
                {
                    self.store
                        .record_excluded(topic, id, stored.meta.position, now)
                        .map_err(|e| self.disk_error("failed to record the exclusion", &e))?;
                }
            }
            Place::Disk if exclude => self
                .store
                .remove_excluded(topic, id, now)
                .map_err(|e| self.disk_error("failed to remove the message from disk", &e))?,
            Place::Disk => self
                .store
                .remove(topic, id)
                .map_err(|e| self.disk_error("failed to remove the message from disk", &e))?,
        }
        state.index.remove(id);
        state.replicas.remove(id);
        self.backup_removed(topic, id, owned);
        Ok(())
    }

    /// Writes the message back where it is, after changing the delivery metadata.
    fn save_meta(
        &self,
        state: &mut TopicState,
        topic: &str,
        meta: &MessageMeta,
        place: Place,
    ) -> Result<(), TabaError> {
        let owned = state.index.contains(&meta.id);
        let saved = match place {
            Place::Memory => {
                if let Some(stored) = state.memory.get_mut(&meta.id) {
                    stored.meta = meta.clone();
                }
                Ok(())
            }
            Place::Disk => self
                .store
                .update_delivery(topic, meta)
                .map_err(|e| self.disk_error("failed to write the delivery to disk", &e)),
        };
        if saved.is_ok() {
            self.backup_changed(topic, &meta.id, owned);
        }
        saved
    }

    /// Sets the invisibility from now on (doc 06a, `PUT .../visibility`): replaces the
    /// deadline, does not add, and does not touch `deliveryCount` or the `receiptToken`. `0` releases the
    /// message right away.
    pub fn set_visibility(
        &self,
        topic: &str,
        id: &str,
        receipt_token: Option<&str>,
        visibility_timeout: Option<i64>,
    ) -> Result<VisibilityUpdated, TabaError> {
        let view = self.queue_view(topic)?;
        let mut missing = Vec::new();
        if receipt_token.is_none() {
            missing.push("receiptToken");
        }
        if visibility_timeout.is_none() {
            missing.push("visibilityTimeout");
        }
        let (Some(receipt_token), Some(visibility_timeout)) = (receipt_token, visibility_timeout)
        else {
            return Err(required(&missing));
        };
        let secs = validation::in_range_signed(
            "visibilityTimeout",
            visibility_timeout,
            (0, self.rules.visibility_max_secs.unwrap_or(u64::MAX)),
        )?;
        let name = &view.definition.name;
        let now = self.clock.now_millis();
        self.settle(&view, now);
        let mut exhausted = false;
        let meta = {
            let state = self.state_of(name);
            let mut state = lock(&state);
            let (mut meta, place) = self.locate(&state, name, id, receipt_token)?;
            meta.invisible_until_ms = now + secs * 1000;
            self.save_meta(&mut state, name, &meta, place)?;
            state.index.set_until(id, meta.invisible_until_ms, now);
            // Releasing a message that already used up its deliveries sends it to the dead-letter, instead of
            // returning it to the queue (doc 13, section 12.10).
            if secs == 0 && self.exhausted(&view, meta.delivery_count) {
                exhausted = true;
                self.dead_letter(&mut state, &view, vec![id.to_owned()], now);
            }
            meta
        };
        if secs == 0 && !exhausted {
            self.wake(name, 1);
        }
        Ok(VisibilityUpdated {
            id: id.to_owned(),
            invisible_until_ms: meta.invisible_until_ms,
        })
    }

    /// The parameters of `PUT .../visibility`, already checked: the `receiptToken` and the seconds.
    pub fn visibility_params(
        &self,
        receipt_token: Option<&str>,
        visibility_timeout: Option<i64>,
    ) -> Result<(String, u64), TabaError> {
        let mut missing = Vec::new();
        if receipt_token.is_none() {
            missing.push("receiptToken");
        }
        if visibility_timeout.is_none() {
            missing.push("visibilityTimeout");
        }
        let (Some(receipt_token), Some(visibility_timeout)) = (receipt_token, visibility_timeout)
        else {
            return Err(required(&missing));
        };
        let secs = validation::in_range_signed(
            "visibilityTimeout",
            visibility_timeout,
            (0, self.rules.visibility_max_secs.unwrap_or(u64::MAX)),
        )?;
        Ok((receipt_token.to_owned(), secs))
    }

    /// Is something missing from the ack's query? (the `receiptToken`).
    pub fn ack_params(&self, receipt_token: Option<&str>) -> Result<String, TabaError> {
        receipt_token
            .map(str::to_owned)
            .ok_or_else(|| required(&["receiptToken"]))
    }

    /// The waiting `GET` is counted as long polling while this guard exists.
    pub fn begin_long_poll(&self) -> LongPoll<'_> {
        LongPoll::start(&self.counters)
    }

    /// So long polling knows how long to sleep and whether a ready one is left: when the next invisible one
    /// becomes deliverable again and whether the owner still has a ready message.
    pub fn wait_hints(&self, topic: &str) -> (Option<u64>, bool) {
        let state = self.state_of(topic);
        let state = lock(&state);
        (state.index.next_visible(), state.index.has_ready())
    }

    /// The present instant on the node's clock.
    pub fn now_millis(&self) -> u64 {
        self.clock.now_millis()
    }

    // ---- Consumo ----

    /// Delivers up to `maxMessages` ready messages. With a `waitTime` greater than zero and nothing
    /// ready, it waits for a new message (long polling) and returns the first one that arrives.
    pub async fn consume(
        self: &Arc<Self>,
        topic: &str,
        query: ConsumeQuery,
    ) -> Result<Vec<Delivered>, TabaError> {
        let (view, params) = self.prepare_consume(topic, &query).await?;
        let ConsumeParams {
            max_messages,
            visibility_secs: visibility,
            wait_secs: wait,
        } = params;
        let name = view.definition.name.clone();
        let notify = self.notifier(&name);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(wait);
        // Only the call that ends up sleeping is a long polling for the metrics.
        let mut polling: Option<LongPoll<'_>> = None;
        loop {
            // Registers interest before looking at the queue, so as not to miss a message that
            // arrives between the look and the wait. The order of registration is the order of service.
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let view = self.topics.get(&name)?;
            let this = self.clone();
            let taken = tokio::task::spawn_blocking(move || {
                this.take(&view, max_messages as usize, visibility)
            })
            .await
            .map_err(|_| TabaError::internal())??;
            // A ready message is left: the next waiting listener can also be served.
            if !taken.delivered.is_empty() && taken.more_ready {
                notify.notify_one();
            }
            // In the drain the waiting `GET` answers empty right away, for the client to reconnect.
            if !taken.delivered.is_empty()
                || tokio::time::Instant::now() >= deadline
                || self.is_draining()
            {
                if let Some(poll) = polling.as_mut() {
                    poll.delivered = Some(!taken.delivered.is_empty());
                }
                return Ok(taken.delivered);
            }
            polling.get_or_insert_with(|| LongPoll::start(&self.counters));
            // Sleeps until a message arrives, the deadline ends or the next invisible one expires.
            let mut until = deadline;
            if let Some(at) = taken.next_visible_ms {
                let in_ms = at.saturating_sub(self.clock.now_millis()).max(10);
                until = until.min(tokio::time::Instant::now() + Duration::from_millis(in_ms));
            }
            let _ = tokio::time::timeout_at(until, notified).await;
        }
    }

    /// The start of every `GET`: finds (or creates, with `autoCreate`) the queue, counts the consumer and reads the
    /// query parameters. Serves single-node consuming and the cluster's.
    pub async fn prepare_consume(
        &self,
        topic: &str,
        query: &ConsumeQuery,
    ) -> Result<(TopicView, ConsumeParams), TabaError> {
        let view = match self.topics.get(topic) {
            Err(e) if e.code == ErrorCode::TopicNotFound && wants_auto_create(query)? => {
                self.auto_create(topic, query).await?
            }
            other => other?,
        };
        if view.definition.queue().is_none() {
            return Err(wrong_type());
        }
        // Any `GET` counts as a consumer, at the start of the call, with or without a message. The
        // dead-letter's does not count (doc 15, section 14.3).
        if view.dead_letter_of.is_none() {
            lock(&self.last_get).insert(view.definition.id.clone(), self.clock.now_millis());
        }
        let max_messages = match &query.max_messages {
            Some(text) => query_number(
                "maxMessages",
                text,
                (1, self.rules.max_messages_max.unwrap_or(u64::MAX)),
            )?,
            None => self.rules.max_messages_default,
        };
        let visibility = match &query.visibility_timeout {
            Some(text) => query_number(
                "visibilityTimeout",
                text,
                (0, self.rules.visibility_max_secs.unwrap_or(u64::MAX)),
            )?,
            None => self.rules.visibility_default_secs,
        };
        let wait = match &query.wait_time {
            Some(text) => query_number("waitTime", text, WAIT_TIME_SECS)?,
            None => 0,
        };

        Ok((
            view,
            ConsumeParams {
                max_messages,
                visibility_secs: visibility,
                wait_secs: wait,
            },
        ))
    }

    /// The `autoCreate`: creates the queue (volatile by default) and returns it right away. Whoever loses the race for
    /// creation treats it as "already exists" and goes on as a normal consume.
    async fn auto_create(&self, topic: &str, query: &ConsumeQuery) -> Result<TopicView, TabaError> {
        if !query.can_create {
            return Err(TabaError::new(
                ErrorCode::AuthForbidden,
                "Creating the queue through autoCreate requires the create permission.",
            ));
        }
        let request = CreateTopic {
            name: topic.to_owned(),
            kind: Kind::Queue,
            tags: (!query.tags.is_empty()).then(|| query.tags.clone()),
            prefixes: None,
            volatile: Some(optional_bool("volatile", query.volatile.as_deref())?.unwrap_or(true)),
            idle_timeout: optional_int("idleTimeout", query.idle_timeout.as_deref())?,
            fifo: optional_bool("fifo", query.fifo.as_deref())?,
            max_message_bytes: None,
            ttl: None,
            max_deliveries: None,
        };
        match self.topics.create_checked(request, true).await {
            Ok(view) => Ok(view),
            Err(e) if e.code == ErrorCode::TopicAlreadyExists => self.topics.get(topic),
            Err(e) => Err(e),
        }
    }

    /// The task that deletes idle volatile queues, until `cancel`.
    pub async fn run_idle_sweeper(self: Arc<Self>, cancel: tokio_util::sync::CancellationToken) {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => { self.expire_idle().await; }
            }
        }
    }

    /// Deletes the volatile queues with no consumer for `idleTimeout`, with the messages and the
    /// dead-letter. The deadline is `max(creation, start, last GET) + idleTimeout`: the start comes in
    /// so a node that resumed from disk does not delete the queue for the time it was stopped.
    /// Returns the names deleted.
    pub async fn expire_idle(&self) -> Vec<String> {
        let now = self.clock.now_millis();
        let queues = self.topics.volatile_queues();
        let live: HashSet<String> = queues.iter().map(|q| q.id.clone()).collect();
        let mut deleted = Vec::new();
        for queue in queues {
            let last = lock(&self.last_get)
                .get(&queue.id)
                .copied()
                .unwrap_or(0)
                .max(queue.created_ms)
                .max(self.started_ms);
            if now < last.saturating_add(queue.idle_timeout_secs.saturating_mul(1000)) {
                continue;
            }
            // Only deletes the `id` that was verified: a queue recreated along the way stays.
            match self.topics.delete_expected(&queue.name, &queue.id).await {
                Ok(()) => {
                    tracing::info!(
                        event.name = "taba.volatile.deleted",
                        messaging.destination.name = %queue.name,
                        topic.id = %queue.id,
                        "volatile queue deleted for idleness"
                    );
                    deleted.push(queue.name);
                }
                Err(e) if e.code == ErrorCode::TopicNotFound => {}
                Err(e) => tracing::warn!(
                    topic = %queue.name,
                    error = %e,
                    "could not delete the idle queue"
                ),
            }
        }
        lock(&self.last_get).retain(|id, _| live.contains(id));
        deleted
    }

    // ---- Retention ----

    /// `true` if the message already used up the allowed deliveries (`deliveryCount ≥ maxDeliveries`).
    /// The dead-letter has no limit: there is no dead-letter of the dead-letter.
    fn exhausted(&self, view: &TopicView, delivery_count: u32) -> bool {
        view.dead_letter_of.is_none()
            && view
                .max_deliveries()
                .is_some_and(|max| u64::from(delivery_count) >= max)
    }

    /// Applies the `ttl` and the delivery limit to what is stored, at instant `now`:
    /// deletes what expired (in any state, including invisible), returns to ready what
    /// had its invisibility expire and sends to the dead-letter the message that had it expire after
    /// spending the deliveries. A queue's dead-letter is settled together, because that is where the
    /// messages show up (doc 13, section 12.10, and doc 14, section 13.6). Each topic is settled
    /// with its own lock, one at a time.
    fn settle(&self, view: &TopicView, now: u64) {
        if let Some(source) = &view.dead_letter_of
            && let Ok(source_view) = self.topics.get(source)
        {
            self.settle_one(&source_view, now);
        }
        self.settle_one(view, now);
    }

    fn settle_one(&self, view: &TopicView, now: u64) {
        let name = &view.definition.name;
        let state = self.state_of(name);
        let mut state = lock(&state);
        let ttl_ms = view.ttl_secs().unwrap_or(u64::MAX).saturating_mul(1000);
        for id in state.index.expired(now, ttl_ms) {
            let place = if state.memory.contains_key(&id) {
                Place::Memory
            } else {
                Place::Disk
            };
            match self.remove_message_as(&mut state, name, &id, place, false) {
                Ok(()) => {
                    state.stats.expired += 1;
                    NodeCounters::add(&self.counters.expired, 1);
                    tracing::info!(
                        event.name = "taba.message.expired",
                        messaging.destination.name = %name,
                        message.id = %id,
                        "message discarded by ttl"
                    );
                }
                Err(_) => {
                    tracing::error!(topic = %name, id = %id, "failed to delete the expired one")
                }
            }
        }
        // Each node deletes its own copies by the same rule (doc 13, section 12.10); the replica does not
        // count in the statistics, which are the owner's only.
        for id in state.replicas.expired(now, ttl_ms) {
            let place = if state.memory.contains_key(&id) {
                Place::Memory
            } else {
                Place::Disk
            };
            if self
                .remove_message_as(&mut state, name, &id, place, false)
                .is_err()
            {
                tracing::error!(topic = %name, id = %id, "failed to delete the expired replica");
            }
        }
        // Invisibility that expired returns the message to ready; one that had already used up the
        // deliveries goes to the dead-letter instead of going back to the queue.
        let released = state.index.release_due(now);
        let mut exhausted: Vec<String> = released
            .iter()
            .filter(|id| {
                state
                    .index
                    .deliveries(id)
                    .is_some_and(|n| n > 0 && self.exhausted(view, n))
            })
            .cloned()
            .collect();
        // First time the topic is seen, or `maxDeliveries` changed through `PATCH`: the ready ones
        // that already exceed the limit also leave.
        let max = view.max_deliveries();
        if state.checked_max_deliveries != Some(max) {
            state.checked_max_deliveries = Some(max);
            for (id, deliveries) in state.index.ready_delivered() {
                if self.exhausted(view, deliveries) && !exhausted.contains(&id) {
                    exhausted.push(id);
                }
            }
        }
        let back_to_ready =
            released.len() - released.iter().filter(|id| exhausted.contains(id)).count();
        if !exhausted.is_empty() {
            self.dead_letter(&mut state, view, exhausted, now);
        }
        self.ready_changed(name, &mut state);
        drop(state);
        if back_to_ready > 0 {
            self.wake(name, back_to_ready);
        }
    }

    /// Sends the messages to the dead-letter. On a single node the move is local and immediate; in the cluster the
    /// owner asks the responsible nodes for the move (register + commit, doc 13, section 12.10): here they
    /// only enter the work queue, and the message leaves the ready list until the move
    /// finishes, so nobody delivers it again.
    fn dead_letter(&self, state: &mut TopicState, view: &TopicView, ids: Vec<String>, now: u64) {
        let Some(hooks) = self.cluster.get() else {
            self.move_to_dead_letter(state, view, ids, now);
            return;
        };
        for id in ids {
            if state.dead_letter_inflight.insert(id.clone()) {
                state.index.set_until(&id, u64::MAX / 2, now);
                let _ = hooks.dead_letters.send(DeadLetterJob {
                    topic: view.definition.name.clone(),
                    id,
                });
            }
        }
    }

    /// Moves the messages to the queue's dead-letter: the same `id`, `deliveryCount` and deadline
    /// zeroed, and `deadLetteredAt` and `deadLetterReason` filled in (doc 13, section 12.10). The
    /// message stays where it was (memory or disk); on disk the file is only renamed
    /// into the dead-letter's folder. The lock order is always source queue, then
    /// dead-letter.
    fn move_to_dead_letter(
        &self,
        source_state: &mut TopicState,
        view: &TopicView,
        ids: Vec<String>,
        now: u64,
    ) {
        let source = &view.definition.name;
        let target = format!("{source}{}", self.topics.rules().dead_letter_suffix);
        let target_state = self.state_of(&target);
        let mut target_state = lock(&target_state);
        let mut moved = 0;
        let fifo = view.definition.queue().is_some_and(|q| q.fifo);
        for id in ids {
            // In the cluster the replica also does the move, but only the owner counts and indexes.
            let owned = source_state.index.contains(&id);
            source_state.dead_letter_inflight.remove(&id);
            let original: Option<MessageMeta>;
            let reset = |mut meta: MessageMeta| {
                meta.delivery_count = 0;
                meta.invisible_until_ms = 0;
                meta.receipt_token = None;
                meta.dead_lettered_ms = Some(now);
                meta.dead_letter_reason = Some("max-deliveries".to_owned());
                // The dead-letter is a plain queue: it does not deduplicate.
                meta.deduplication_id = None;
                meta
            };
            let group;
            if let Some(mut stored) = self.take_from_memory(source_state, &id) {
                original = Some(stored.meta.clone());
                if let Err(e) = self
                    .store
                    .record_excluded(source, &id, stored.meta.position, now)
                {
                    tracing::error!(error = %e, topic = %source, id = %id, "failed to record the exclusion");
                }
                stored.meta = reset(stored.meta);
                group = stored.meta.group_id.clone();
                self.put_in_memory(&mut target_state, stored);
            } else {
                match self.store.get(source, &id) {
                    Ok(Some(meta)) => {
                        original = Some(meta.clone());
                        let meta = reset(meta);
                        group = meta.group_id.clone();
                        if let Err(e) = self.store.move_to(source, &target, &meta) {
                            tracing::error!(
                                error = %e, topic = %source, id = %id,
                                "failed to move to the dead-letter"
                            );
                            continue;
                        }
                    }
                    Ok(None) => {
                        source_state.index.remove(&id);
                        source_state.replicas.remove(&id);
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, topic = %source, "failed to read the message");
                        continue;
                    }
                }
            }
            source_state.index.remove(&id);
            source_state.replicas.remove(&id);
            self.backup_removed(source, &id, owned);
            self.backup_stored(&target, &id, owned);
            if owned {
                source_state.stats.dead_lettered += 1;
                NodeCounters::add(&self.counters.dead_lettered, 1);
                tracing::warn!(
                    event.name = "taba.message.dead_lettered",
                    messaging.destination.name = %source,
                    message.id = %id,
                    delivery_count = original.as_ref().map_or(0, |m| m.delivery_count),
                    "message sent to the dead-letter for reaching maxDeliveries"
                );
            }
            // On FIFO deduplication keeps applying after the message leaves (doc 17, 16.7).
            if fifo
                && let Some(original) = original
                && let delete_at = original
                    .published_ms
                    .saturating_add(self.rules.fifo_dedup_window_ms)
                && delete_at > now
                && let Err(e) = self.store.tombstone(source, &original, delete_at)
            {
                tracing::error!(error = %e, topic = %source, id = %id, "failed to write the deduplication row");
            }
            if owned {
                target_state
                    .index
                    .insert(&id, group.as_deref(), now, 0, 0, now);
                moved += 1;
            } else {
                target_state.replicas.insert(&id, now);
            }
        }
        self.ready_changed(&target, &mut target_state);
        drop(target_state);
        if moved > 0 {
            self.wake(&target, moved);
        }
    }

    /// One pass of the sweep over all the queues: deletes what expired by `ttl` and sends
    /// to the dead-letter what used up its deliveries. Each node does its own, propagating nothing.
    pub fn sweep(&self) {
        let now = self.clock.now_millis();
        let mut names: Vec<String> = self
            .existing_states()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        match self.store.topics_on_disk() {
            Ok(on_disk) => names.extend(on_disk),
            Err(e) => tracing::error!(error = %e, "failed to list the disk in the sweep"),
        }
        names.sort();
        names.dedup();
        for name in names {
            match self.topics.get(&name) {
                Ok(view) => {
                    self.settle(&view, now);
                    if view.definition.queue().is_some_and(|q| q.fifo) {
                        self.sweep_dedup(&name, now);
                    }
                }
                Err(_) if self.keep_old_disk.load(Ordering::SeqCst) => {}
                Err(_) => self.drop_topic(&name),
            }
        }
    }

    /// On the FIFO queue: deletes the deduplication keys that left the window and the short rows with an
    /// expired `deleteAt`, by the index, propagating nothing (doc 17, section 16.7).
    fn sweep_dedup(&self, name: &str, now: u64) {
        let window = self.rules.fifo_dedup_window_ms;
        lock(&self.state_of(name))
            .dedup
            .retain(|_, (_, at)| at.saturating_add(window) > now);
        if let Err(e) = self.store.sweep_tombstones(name, now) {
            tracing::error!(error = %e, topic = %name, "failed to sweep the deduplication rows");
        }
    }

    /// Applies the limits of the log of excluded messages (doc 37, section 4.4). The log is trimmed on its own
    /// schedule, not at each pass of the sweep.
    pub fn trim_excluded(&self) {
        let now = self.clock.now_millis();
        match self.store.trim_excluded(now) {
            Ok(0) => {}
            Ok(removed) => tracing::info!(
                removed,
                "the log of excluded messages dropped what the limits do not allow"
            ),
            Err(e) => tracing::error!(error = %e, "failed to trim the log of excluded messages"),
        }
    }

    /// Does the node's log say the message left the topic dead (ack, prune, dead-letter)?
    pub fn is_excluded(&self, topic: &str, id: &str) -> bool {
        self.store.is_excluded(topic, id).unwrap_or(false)
    }

    /// The sources of the first synchronization tell from when they record (see
    /// `MessageStore::inherit_excluded_since`).
    pub fn inherit_excluded_since(&self, sources_ms: u64) -> io::Result<bool> {
        self.store.inherit_excluded_since(sources_ms)
    }

    /// The start of the continuous recording of the log of excluded messages (ms).
    pub fn excluded_since(&self) -> u64 {
        let now = self.clock.now_millis();
        self.store.excluded_since(now).unwrap_or(now)
    }

    /// The sweep task, until `cancel`. The interval is `retention.sweep.interval`, but
    /// it never exceeds 1 s: the same pass takes to the dead-letter the message whose invisibility
    /// expired, and whoever consumes the dead-letter should not wait for the whole interval.
    pub async fn run_sweeper(self: Arc<Self>, cancel: tokio_util::sync::CancellationToken) {
        let period = self.rules.sweep_interval.min(Duration::from_secs(1));
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_summary = tokio::time::Instant::now();
        let mut last_trim = tokio::time::Instant::now();
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => {
                    let this = self.clone();
                    let _ = tokio::task::spawn_blocking(move || this.sweep()).await;
                    if last_trim.elapsed() >= EXCLUDED_TRIM_EVERY {
                        let this = self.clone();
                        let _ = tokio::task::spawn_blocking(move || this.trim_excluded()).await;
                        last_trim = tokio::time::Instant::now();
                    }
                    if last_summary.elapsed() >= OVERFLOW_SUMMARY_EVERY {
                        self.log_overflow_summary(OVERFLOW_SUMMARY_EVERY);
                        last_summary = tokio::time::Instant::now();
                    }
                }
            }
        }
    }

    /// The periodic summary of the memory-to-disk overflow (doc 11, "Logs"): an `info` log with
    /// how many messages went to disk since the previous summary, instead of one log per
    /// message. Without overflow in the window there is no log. Returns the summarized amount.
    pub fn log_overflow_summary(&self, window: Duration) -> u64 {
        let total = self.counters.overflow.load(Ordering::Relaxed);
        let reported = self
            .counters
            .overflow_reported
            .swap(total, Ordering::Relaxed);
        let moved = total.saturating_sub(reported);
        if moved > 0 {
            tracing::info!(
                event.name = "taba.storage.overflow",
                messages = moved,
                window_secs = window.as_secs(),
                "memory-to-disk overflow: {moved} messages in the last window"
            );
        }
        moved
    }

    // ---- Drain ----

    /// The local drain, which in development serves as the *save* (doc 04, section 6.10): the memory
    /// limit drops to zero, so every new message goes straight to disk, and those
    /// in memory are written with the metadata (`deliveryCount`, `invisibleUntil`,
    /// `receiptToken`, `groupId`, `contentType`) and leave memory. A disk error on one
    /// message does not interrupt the rest: it stays in memory and goes into the report.
    pub fn drain(&self) -> DrainReport {
        self.begin_drain();
        self.drain_until(|| false)
    }

    /// Enters drain: the effective memory limit drops to zero (every new message goes to
    /// disk) and the waiting `GET`s are woken, so they answer empty and the client
    /// reconnects (doc 04, section 6.10, step 1).
    pub fn begin_drain(&self) {
        self.draining.store(true, Ordering::SeqCst);
        for notify in lock(&self.notifiers).values() {
            notify.notify_waiters();
        }
    }

    /// Cancels the drain: the node goes back to writing in memory (`DELETE /node/drain`).
    pub fn cancel_drain(&self) {
        self.draining.store(false, Ordering::SeqCst);
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Moves memory to disk, one message at a time (the topic lock is released between them),
    /// until done or `stop` returns `true` (cancellation or `drain.timeout`). Topics go out
    /// in overflow order: first the one with the **lowest read rate**, and, on a tie, by name.
    pub fn drain_until(&self, stop: impl Fn() -> bool) -> DrainReport {
        let mut report = DrainReport {
            moved: 0,
            failed: 0,
            remaining: 0,
        };
        let now = self.clock.now_millis();
        let mut plan: Vec<(f64, String, Vec<String>)> = self
            .existing_states()
            .into_iter()
            .map(|(name, state)| {
                let state = lock(&state);
                (
                    self.consume_rate(&state, now),
                    name,
                    state.memory.keys().cloned().collect(),
                )
            })
            .collect();
        plan.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        'topics: for (_, topic, ids) in plan {
            let state = self.state_of(&topic);
            for id in ids {
                if stop() {
                    break 'topics;
                }
                // Disk close to full: waits for the eviction (S3) to free space (doc 23, 1.8).
                if let Some(sink) = self.backup.get() {
                    sink.before_disk_write();
                }
                let mut state = lock(&state);
                let Some(stored) = self.take_from_memory(&mut state, &id) else {
                    continue;
                };
                match self.store.put(&topic, &stored.meta, &stored.body) {
                    Ok(()) => report.moved += 1,
                    Err(e) => {
                        tracing::error!(error = %e, topic = %topic, id = %id, "drain failure");
                        report.failed += 1;
                        self.put_in_memory(&mut state, stored);
                        if storage::is_fatal(&e) {
                            self.fail();
                        }
                    }
                }
            }
        }
        report.remaining = self.pending();
        report
    }

    /// Messages still in memory (`pendingMessages` of `DrainStatus`).
    pub fn pending(&self) -> u64 {
        self.existing_states()
            .iter()
            .map(|(_, state)| lock(state).memory.len() as u64)
            .sum()
    }

    /// The node's storage (`NodeStatus.storage`): bodies and messages in memory, and
    /// messages on disk.
    pub fn storage_stats(&self) -> StorageStats {
        let disk_messages = self
            .store
            .topics_on_disk()
            .unwrap_or_default()
            .iter()
            .map(|topic| self.store.count(topic).unwrap_or(0))
            .sum();
        StorageStats {
            memory_bytes: self.memory_bytes.load(Ordering::SeqCst),
            memory_limit_bytes: self.rules.memory_limit,
            memory_messages: self.pending(),
            disk_messages,
        }
    }

    /// Bytes of body in memory and messages on disk of a topic (for tests and metrics).
    pub fn usage(&self, topic: &str) -> (u64, u64) {
        let state = self.state_of(topic);
        let memory = lock(&state)
            .memory
            .values()
            .map(|s| s.body.len() as u64)
            .sum();
        (memory, self.store.count(topic).unwrap_or(0))
    }

    /// The topic's publish rate and read rate, in messages per second in the
    /// `memory.rate.window` window (doc 11).
    pub fn rates(&self, topic: &str) -> (f64, f64) {
        let state = self.state_of(topic);
        let state = lock(&state);
        let now = self.clock.now_millis();
        (
            state
                .rates
                .published
                .per_second(now, self.rules.rate_window_secs),
            state
                .rates
                .consumed
                .per_second(now, self.rules.rate_window_secs),
        )
    }

    // ---- Statistics (doc 11) ----

    /// The snapshot of a queue for `GET /metrics/topics*`. First settles the `ttl` and the expired invisibilities,
    /// so the count of ready and in-processing is that of now. `404` if the topic
    /// does not exist and `409` on a distributor (which has [`Messages::distributor_counters`]).
    pub fn topic_snapshot(
        &self,
        topic: &str,
        rules: &StatsRules,
    ) -> Result<TopicSnapshot, TabaError> {
        let view = self.queue_view(topic)?;
        let now = self.clock.now_millis();
        self.settle(&view, now);
        let name = &view.definition.name;
        let window = self.rules.rate_window_secs;
        let mut snapshot = {
            let handle = self.state_of(name);
            let state = lock(&handle);
            let fifo = state.index.is_fifo();
            let totals = state.index.group_totals();
            let memory_messages = state.memory.len() as u64;
            TopicSnapshot {
                counters: state.stats.clone(),
                publish_rate: state.rates.published.per_second(now, window),
                consume_rate: state.rates.consumed.per_second(now, window),
                ack_rate: state.rates.acked.per_second(now, window),
                visible: state.index.ready_count() as u64,
                invisible: state.index.processing_count() as u64,
                oldest_ms: state.index.oldest_age_ms(),
                memory_messages,
                disk_messages: ((state.index.message_count() + state.replicas.len()) as u64)
                    .saturating_sub(memory_messages),
                memory_bytes: state.memory.values().map(|s| s.body.len() as u64).sum(),
                disk_bytes: 0,
                groups: totals.groups,
                largest_group: totals.largest,
                busy_groups: if fifo { totals.busy } else { 0 },
                stalled_group: rules
                    .group_stalled_deliveries
                    .filter(|_| fifo)
                    .and_then(|over| state.index.stalled_group(over)),
                publish_latency: Percentiles::of(&state.stats.publish_latency),
                consume_latency: Percentiles::of(&state.stats.consume_latency),
            }
        };
        // The bytes on disk come from the files' sizes, outside the topic lock.
        if snapshot.disk_messages > 0 {
            snapshot.disk_bytes = self.store.body_bytes(name).unwrap_or(0);
        }
        Ok(snapshot)
    }

    /// A queue's groups, with the counts and today's band (unordered). A group without
    /// messages does not exist in the index, so it does not show up.
    pub fn group_snapshots(&self, topic: &str) -> Result<Vec<GroupSnapshot>, TabaError> {
        let view = self.queue_view(topic)?;
        self.settle(&view, self.clock.now_millis());
        let handle = self.state_of(&view.definition.name);
        let state = lock(&handle);
        Ok(state
            .index
            .group_infos()
            .into_iter()
            .map(|info| {
                let (group_id, overflow) = match info.key {
                    GroupKey::Named(name) => (Some(name), false),
                    GroupKey::Overflow => (None, true),
                };
                GroupSnapshot {
                    group_id,
                    overflow,
                    visible: info.ready,
                    invisible: info.processing,
                    band: info.band,
                    oldest_ms: info.oldest_age_ms,
                }
            })
            .collect())
    }

    /// What the distributor counted since the node started.
    pub fn distributor_counters(&self, name: &str) -> DistributorCounters {
        lock(&self.distributors)
            .get(name)
            .copied()
            .unwrap_or_default()
    }

    /// What `GET /metrics` shows of the node: memory, disk, ready, busy groups and the
    /// counters. Counts by the indexes of the open topics (every topic with a message on disk is
    /// opened in the first sweep after the start).
    pub fn observe_node(&self) -> NodeObservation {
        let (mut ready, mut busy_groups, mut memory_messages, mut disk_messages) = (0, 0, 0, 0);
        for (_, state) in self.existing_states() {
            let state = lock(&state);
            ready += state.index.ready_count() as u64;
            if state.index.is_fifo() {
                busy_groups += state.index.group_totals().busy;
            }
            let in_memory = state.memory.len() as u64;
            memory_messages += in_memory;
            disk_messages += ((state.index.message_count() + state.replicas.len()) as u64)
                .saturating_sub(in_memory);
        }
        let c = &self.counters;
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        NodeObservation {
            memory_bytes: self.memory_bytes.load(Ordering::SeqCst),
            memory_limit_bytes: self.rules.memory_limit,
            memory_messages,
            disk_messages,
            ready,
            busy_groups,
            overflow: load(&c.overflow),
            reads_memory: load(&c.reads_memory),
            reads_disk: load(&c.reads_disk),
            disk_errors: load(&c.disk_errors),
            redeliveries: load(&c.redeliveries),
            dead_lettered: load(&c.dead_lettered),
            expired: load(&c.expired),
            deduplicated: load(&c.deduplicated),
            longpoll_waiting: c.longpoll_waiting.load(Ordering::Relaxed),
            longpoll_delivered: load(&c.longpoll_delivered),
            longpoll_empty: load(&c.longpoll_empty),
            longpoll_disconnected: load(&c.longpoll_disconnected),
            fair_wait: c.fair_wait(),
        }
    }

    /// The free space on the `storage.dir` volume, in bytes; `None` if the system did not say.
    pub fn disk_free_bytes(&self) -> Option<u64> {
        self.store.free_bytes().ok()
    }

    /// How long ago the topic's last `GET` was **on this node**, in whole seconds, and whether the count
    /// comes from the node's start (no `GET` record: the node just came up or restarted), so that a
    /// restart can only postpone the deletion of a volatile queue (doc 15, section 14.5).
    pub fn idle_of(&self, topic_id: &str) -> Result<(u64, bool), TabaError> {
        if !self.topics.has_id(topic_id) {
            return Err(TabaError::new(
                ErrorCode::TopicNotFound,
                "The topic does not exist.",
            ));
        }
        let now = self.clock.now_millis();
        let (since, from_start) = match lock(&self.last_get).get(topic_id) {
            Some(at) => (*at, false),
            None => (self.started_ms, true),
        };
        Ok((now.saturating_sub(since) / 1000, from_start))
    }

    /// The node's start, in milliseconds: where the idleness count of a topic with no activity
    /// begins when it is older than the node.
    pub fn started_ms(&self) -> u64 {
        self.started_ms
    }

    // ---- Entrega ----

    /// Takes the ready messages through the index (fairness between groups and the accordion effect),
    /// joining memory and disk.
    fn take(&self, view: &TopicView, max: usize, visibility_secs: u64) -> Result<Taken, TabaError> {
        let started = Instant::now();
        let name = &view.definition.name;
        let now = self.clock.now_millis();
        self.settle(view, now);
        let state = self.state_of(name);
        let mut state = lock(&state);
        let until = now + visibility_secs * 1000;

        let mut delivered = Vec::new();
        while delivered.len() < max {
            let Some(id) = state.index.pick() else {
                break;
            };
            let place = if state.memory.contains_key(&id) {
                Place::Memory
            } else {
                Place::Disk
            };
            let (mut meta, body) = match place {
                Place::Memory => {
                    let stored = &state.memory[&id];
                    (stored.meta.clone(), stored.body.clone())
                }
                Place::Disk => {
                    let Some(meta) = self
                        .store
                        .get(name, &id)
                        .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
                    else {
                        state.index.remove(&id);
                        continue;
                    };
                    match self.store.body(name, &id) {
                        Ok(body) => (meta, body),
                        // The body the eviction took off disk comes back from S3 (doc 23).
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            match self.backup_body(name, &id) {
                                Some(body) => (meta, body),
                                None => {
                                    // Row without a file (a crash in the middle of writing): lost.
                                    tracing::error!(topic = %name, id = %id, "corpo sumiu do disco");
                                    let _ = self.store.remove(name, &id);
                                    state.index.remove(&id);
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            return Err(self.disk_error("failed to read the body on disk", &e));
                        }
                    }
                }
            };
            match place {
                Place::Memory => NodeCounters::add(&self.counters.reads_memory, 1),
                Place::Disk => NodeCounters::add(&self.counters.reads_disk, 1),
            }
            if meta.delivery_count == 0 {
                // The wait until the first delivery, by the group's band at delivery time.
                let band = state.index.band_of_entry(&id).unwrap_or(0);
                let waited_ms = now.saturating_sub(meta.age_base_ms());
                self.counters.observe_wait(band, waited_ms as f64 / 1000.0);
            } else {
                state.stats.redelivered += 1;
                NodeCounters::add(&self.counters.redeliveries, 1);
            }
            meta.delivery_count += 1;
            meta.invisible_until_ms = until;
            let receipt_token = self.ids.new_at(now);
            meta.receipt_token = Some(receipt_token.clone());
            self.save_meta(&mut state, name, &meta, place)?;
            // Inside the loop the message stays in processing at least until the next instant,
            // so the same `GET` does not choose it again (with `visibilityTimeout=0`).
            state
                .index
                .deliver(&id, until.max(now + 1), now, meta.delivery_count);
            let content_type = ContentType::parse(&meta.content_type)
                .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("valid"));
            delivered.push(Delivered {
                id,
                body: Body::render(&content_type, &body),
                content_type: meta.content_type,
                group_id: meta.group_id,
                deduplication_id: meta.deduplication_id,
                receipt_token,
                delivery_count: meta.delivery_count,
                published_ms: meta.published_ms,
                invisible_until_ms: meta.invisible_until_ms,
                dead_lettered_ms: meta.dead_lettered_ms,
                dead_letter_reason: meta.dead_letter_reason,
            });
        }
        if until <= now {
            for message in &delivered {
                state.index.set_until(&message.id, until, now);
            }
        }
        state
            .rates
            .consumed
            .record(now, delivered.len() as u64, self.rules.rate_window_secs);
        if !delivered.is_empty() {
            state.stats.consumed += delivered.len() as u64;
            state.stats.last_consumed_ms = Some(now);
            state
                .stats
                .consume_latency
                .record(started.elapsed().as_secs_f64());
        }
        Ok(Taken {
            delivered,
            next_visible_ms: state.index.next_visible(),
            more_ready: state.index.has_ready(),
        })
    }
}

/// The cluster's building blocks (doc 05, section 7.2; doc 13, section 12.2; doc 14, section 13.3).
/// The engine remains synchronous and local: whoever talks to the other nodes is `cluster/replication`, which
/// calls these methods on the coordinating node and on the nodes that receive the register and the commit.
impl Messages {
    /// Attaches the node to the cluster. Only the first call counts.
    pub fn enable_cluster(&self, hooks: ClusterHooks) {
        let _ = self.cluster.set(hooks);
    }

    /// The ring position of a topic's message (doc 05, section 7.1): the hash of the `id`; on the
    /// FIFO queue, the group's (all of a group's messages fall in the same block).
    fn position_of(&self, view: &TopicView, id: &str, group: Option<&str>) -> u64 {
        if view.definition.queue().is_some_and(|q| q.fifo) {
            fifo_position(&view.definition.id, group.unwrap_or(""))
        } else {
            message_position(id)
        }
    }

    /// The same, by topic name. A topic that does not exist (any more) counts as a plain queue.
    pub fn position_for(&self, topic: &str, id: &str, group: Option<&str>) -> u64 {
        match self.topics.get(topic) {
            Ok(view) => self.position_of(&view, id, group),
            Err(_) => message_position(id),
        }
    }

    /// Is the node the message's delivery owner? Without a cluster, always.
    fn owns(&self, topic: &str, id: &str, group: Option<&str>) -> bool {
        self.cluster.get().is_none_or(|hooks| {
            hooks
                .placement
                .is_owner(topic, id, group, self.position_for(topic, id, group))
        })
    }

    /// Notifies the cluster when the topic starts (or stops) having ready messages here.
    fn ready_changed(&self, name: &str, state: &mut TopicState) {
        let has = state.index.has_ready();
        if state.ready_signaled != has {
            state.ready_signaled = has;
            if let Some(hooks) = self.cluster.get() {
                (hooks.ready)(name, has);
            }
        }
    }

    /// The topics with open state and, in each, whether there is a ready message (the snapshot a new
    /// node receives from the signals).
    pub fn ready_topics(&self) -> Vec<String> {
        self.existing_states()
            .into_iter()
            .filter(|(_, state)| lock(state).index.has_ready())
            .map(|(name, _)| name)
            .collect()
    }

    /// The topics with ready messages and how many (what the heartbeat carries).
    pub fn ready_counts(&self) -> Vec<(String, u64)> {
        self.existing_states()
            .into_iter()
            .filter_map(|(name, state)| {
                let count = lock(&state).index.ready_count() as u64;
                (count > 0).then_some((name, count))
            })
            .collect()
    }

    /// How many ready messages the topic has here (what the notice reports).
    pub fn ready_count_of(&self, topic: &str) -> u64 {
        self.existing_states()
            .into_iter()
            .find(|(name, _)| name == topic)
            .map_or(0, |(_, state)| lock(&state).index.ready_count() as u64)
    }

    /// Wakes up to `count` waiting `GET`s on this node (a new-message notice came from another).
    pub fn wake_listeners(&self, topic: &str, count: usize) {
        self.wake(topic, count);
    }

    /// Whoever waits for a new message of the topic on this node is notified through here.
    pub fn listener_notify(&self, topic: &str) -> Arc<Notify> {
        self.notifier(topic)
    }

    /// Is the node responsible for the ring position, in any role? Without a cluster, always.
    pub fn responsible_for(&self, position: u64) -> bool {
        self.cluster
            .get()
            .is_none_or(|hooks| hooks.placement.is_responsible(position))
    }

    pub fn is_clustered(&self) -> bool {
        self.cluster.get().is_some()
    }

    /// Does the node keep this message? Returns the metadata and whether it is the owner.
    pub fn message_info(&self, topic: &str, id: &str) -> Result<Option<MessageInfo>, TabaError> {
        let state = self.state_of(topic);
        let state = lock(&state);
        let owned = state.index.contains(id);
        if !owned && !state.replicas.contains(id) {
            return Ok(None);
        }
        let meta = match state.memory.get(id) {
            Some(stored) => stored.meta.clone(),
            None => match self
                .store
                .get(topic, id)
                .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
            {
                Some(meta) => meta,
                None => return Ok(None),
            },
        };
        Ok(Some(MessageInfo { meta, owned }))
    }

    /// The metadata and body of the message the node keeps.
    pub fn read_message(
        &self,
        topic: &str,
        id: &str,
    ) -> Result<Option<(MessageMeta, Vec<u8>)>, TabaError> {
        let state = self.state_of(topic);
        let state = lock(&state);
        if let Some(stored) = state.memory.get(id) {
            return Ok(Some((stored.meta.clone(), stored.body.clone())));
        }
        if !state.index.contains(id) && !state.replicas.contains(id) {
            return Ok(None);
        }
        let Some(meta) = self
            .store
            .get(topic, id)
            .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
        else {
            return Ok(None);
        };
        let body = match self.store.body(topic, id) {
            Ok(body) => body,
            Err(e) if e.kind() == io::ErrorKind::NotFound => match self.backup_body(topic, id) {
                Some(body) => body,
                None => return Err(self.disk_error("failed to read the body on disk", &e)),
            },
            Err(e) => return Err(self.disk_error("failed to read the body on disk", &e)),
        };
        Ok(Some((meta, body)))
    }

    /// Prepares a publication for the cluster: checks everything a queue's node would check and
    /// generates the `id`, but **stores** nothing. The coordinator sends the result to the responsible
    /// nodes. The FIFO queue needs the group's primary and does not exist in the cluster yet.
    pub fn draft_publish(
        &self,
        view: &TopicView,
        request: &PublishRequest,
    ) -> Result<(MessageMeta, Vec<u8>), TabaError> {
        let prepared = Prepared::new(request)?;
        let limit = view.max_message_bytes().unwrap_or(self.rules.max_bytes);
        if request.body.len() as u64 > limit {
            return Err(too_large(limit));
        }
        if view.definition.queue().is_some_and(|q| q.fifo) {
            return Err(TabaError::new(
                ErrorCode::InternalError,
                "The FIFO queue has its own draft (`draft_fifo_publish`).",
            ));
        }
        if request.deduplication_id.is_some() {
            return Err(TabaError::new(
                ErrorCode::ValidationNotApplicable,
                "`deduplicationId` only applies on a FIFO queue.",
            )
            .with_detail(
                "deduplicationId",
                ErrorCode::ValidationNotApplicable,
                "Only applies on a FIFO queue.",
            ));
        }
        let now = self.clock.now_millis();
        let id = self.ids.new_at(now);
        // The publication counts from the instant the `id` holds: that is where the `ttl` counts from on every node.
        let published_ms = crate::core::id::unix_millis_of(&id).unwrap_or(now);
        let meta = MessageMeta {
            id,
            content_type: prepared.content_type.as_str().to_owned(),
            group_id: request.group_id.clone(),
            deduplication_id: None,
            published_ms,
            delivery_count: 0,
            invisible_until_ms: 0,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
            position: 0,
        };
        Ok((meta, request.body.clone()))
    }

    /// What any node checks before forwarding the FIFO publication to the primary: body, type,
    /// size and `groupId` (required). Returns the checked content and the group.
    pub fn check_fifo_request(
        &self,
        view: &TopicView,
        request: &PublishRequest,
    ) -> Result<(Prepared, String), TabaError> {
        let prepared = Prepared::new(request)?;
        let limit = view.max_message_bytes().unwrap_or(self.rules.max_bytes);
        if request.body.len() as u64 > limit {
            return Err(too_large(limit));
        }
        let Some(group) = request.group_id.clone() else {
            return Err(TabaError::new(
                ErrorCode::ValidationGroupIdRequired,
                "A FIFO queue requires `groupId`.",
            )
            .with_detail(
                "groupId",
                ErrorCode::ValidationGroupIdRequired,
                "Required on a FIFO queue.",
            ));
        };
        Ok((prepared, group))
    }

    /// The `deleteAt` of a FIFO message's deduplication row: the `id`'s instant plus the window
    /// (doc 17, section 16.7).
    pub fn fifo_delete_at(&self, id: &str) -> u64 {
        crate::core::id::unix_millis_of(id)
            .unwrap_or(0)
            .saturating_add(self.rules.fifo_dedup_window_ms)
    }

    /// The draft of a publication on the FIFO queue, made by the **group's primary** (doc 17, section
    /// 16.4): checks the `groupId`, deduplication (a duplicate returns the original's `id`, without a
    /// register or commit) and generates the `id` as `max(now, group's last + 1)`, so the group's
    /// order is the `id`'s even if a previous primary's clock was ahead.
    pub fn draft_fifo_publish(
        &self,
        view: &TopicView,
        request: &PublishRequest,
    ) -> Result<Draft, TabaError> {
        let (prepared, group) = self.check_fifo_request(view, request)?;
        let now = self.clock.now_millis();
        let key = (
            group.clone(),
            request
                .deduplication_id
                .clone()
                .unwrap_or_else(|| body_hash(&request.body)),
        );
        let name = view.definition.name.clone();
        let state = self.state_of(&name);
        let mut state = lock(&state);
        if let Some(original) = self.duplicate_of(&mut state, &name, &key, now) {
            return Ok(Draft::Duplicate { id: original });
        }
        let at = now.max(state.group_ms.get(&group).map_or(0, |last| last + 1));
        let id = self.ids.new_at(at);
        let published_ms = crate::core::id::unix_millis_of(&id).unwrap_or(at);
        // Reserves the instant right away: the group's next publication starts from after it, even if this one
        // has not committed yet.
        state.group_ms.insert(group.clone(), published_ms);
        let meta = MessageMeta {
            position: self.position_of(view, &id, Some(&group)),
            id,
            content_type: prepared.content_type.as_str().to_owned(),
            group_id: Some(group),
            deduplication_id: Some(key.1),
            published_ms,
            delivery_count: 0,
            invisible_until_ms: 0,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
        };
        Ok(Draft::New {
            meta,
            body: request.body.clone(),
        })
    }

    /// Stores the message on the publication commit. The owner puts it in the index and wakes the waiting
    /// `GET`s; the replica only stores it. Repeating the commit does nothing.
    pub fn apply_publish(
        &self,
        topic: &str,
        mut meta: MessageMeta,
        body: Vec<u8>,
    ) -> Result<(), TabaError> {
        let now = self.clock.now_millis();
        meta.position = self.position_for(topic, &meta.id, meta.group_id.as_deref());
        let owned = self.owns(topic, &meta.id, meta.group_id.as_deref());
        {
            let state = self.state_of(topic);
            let mut state = lock(&state);
            if state.index.contains(&meta.id) || state.replicas.contains(&meta.id) {
                return Ok(());
            }
            // On FIFO every copy remembers the deduplication key and the instant of the group's last `id`:
            // whoever takes over the group inherits both without loss (doc 17, sections 16.5 and 16.7).
            if state.index.is_fifo()
                && let Some(group) = &meta.group_id
            {
                let last = state.group_ms.entry(group.clone()).or_default();
                *last = (*last).max(meta.published_ms);
                if let Some(key) = &meta.deduplication_id {
                    state
                        .dedup
                        .entry((group.clone(), key.clone()))
                        .or_insert((meta.id.clone(), meta.published_ms));
                }
            }
            let size = body.len() as u64;
            let id = meta.id.clone();
            let group = meta.group_id.clone();
            let age = meta.age_base_ms();
            let (until, deliveries) = (meta.invisible_until_ms, meta.delivery_count);
            if self.reserve_memory(&state, topic, size, now) {
                state.memory.insert(id.clone(), Stored { meta, body });
            } else {
                self.store
                    .put(topic, &meta, &body)
                    .map_err(|e| self.disk_error("failed to write the message to disk", &e))?;
                if !self.draining.load(Ordering::SeqCst) {
                    NodeCounters::add(&self.counters.overflow, 1);
                }
            }
            self.backup_stored(topic, &id, owned);
            if owned {
                state
                    .index
                    .insert(&id, group.as_deref(), age, until, deliveries, now);
                state
                    .rates
                    .published
                    .record(now, 1, self.rules.rate_window_secs);
                state.stats.published += 1;
                state.stats.published_bytes += size;
                state.stats.last_published_ms = Some(now);
            } else {
                state.replicas.insert(&id, age);
            }
            self.ready_changed(topic, &mut state);
        }
        if owned {
            self.wake(topic, 1);
        }
        Ok(())
    }

    /// Checks the register of an invisibility (or of an ack): the message exists, and delivery
    /// cannot catch it invisible, nor a change and ack with the wrong `receiptToken`.
    pub fn check_visibility(
        &self,
        topic: &str,
        id: &str,
        check: VisibilityCheck,
        receipt_token: &str,
    ) -> Result<(), TabaError> {
        let now = self.clock.now_millis();
        let info = self
            .message_info(topic, id)?
            .ok_or_else(message_not_found)?;
        match check {
            VisibilityCheck::Delivery if info.meta.invisible_until_ms > now => Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "The message is already invisible.",
            )),
            VisibilityCheck::Delivery => Ok(()),
            VisibilityCheck::Receipt
                if info.meta.receipt_token.as_deref() != Some(receipt_token) =>
            {
                Err(stale_receipt())
            }
            VisibilityCheck::Receipt => Ok(()),
        }
    }

    /// Writes the deadline (and, on a delivery, the counter and token) into the metadata on commit; on the owner, the
    /// index follows. The deadline is absolute and comes ready from the owner.
    pub fn apply_visibility(
        &self,
        topic: &str,
        id: &str,
        until_ms: u64,
        delivery: Option<(u32, &str)>,
    ) -> Result<(), TabaError> {
        let now = self.clock.now_millis();
        let state = self.state_of(topic);
        let mut state = lock(&state);
        let owned = state.index.contains(id);
        if !owned && !state.replicas.contains(id) {
            return Err(message_not_found());
        }
        let place = if state.memory.contains_key(id) {
            Place::Memory
        } else {
            Place::Disk
        };
        let mut meta = match place {
            Place::Memory => state.memory[id].meta.clone(),
            Place::Disk => self
                .store
                .get(topic, id)
                .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
                .ok_or_else(message_not_found)?,
        };
        meta.invisible_until_ms = until_ms;
        if let Some((count, token)) = delivery {
            meta.delivery_count = count;
            meta.receipt_token = Some(token.to_owned());
        }
        self.save_meta(&mut state, topic, &meta, place)?;
        if owned {
            match delivery {
                Some((count, _)) => state.index.deliver(id, until_ms, now, count),
                None => state.index.set_until(id, until_ms, now),
            }
        }
        let released = owned && delivery.is_none() && until_ms <= now;
        // Releasing a message that already used up its deliveries sends it to the dead-letter (doc 13, 12.10).
        if released
            && let Ok(view) = self.topics.get(topic)
            && self.exhausted(&view, meta.delivery_count)
        {
            self.dead_letter(&mut state, &view, vec![id.to_owned()], now);
        }
        self.ready_changed(topic, &mut state);
        drop(state);
        if released {
            self.wake(topic, 1);
        }
        Ok(())
    }

    /// Removes the message on the ack commit (leaving no trace, doc 13, section 12.4). Only the owner
    /// counts the ack in the statistics. Repeating the commit does nothing.
    pub fn apply_ack(&self, topic: &str, id: &str) -> Result<(), TabaError> {
        let now = self.clock.now_millis();
        let state = self.state_of(topic);
        let mut state = lock(&state);
        let owned = state.index.contains(id);
        if !owned && !state.replicas.contains(id) {
            return Ok(());
        }
        let place = if state.memory.contains_key(id) {
            Place::Memory
        } else {
            Place::Disk
        };
        if state.index.is_fifo() {
            // On FIFO the ack does not delete the row: it leaves the deduplication one until `deleteAt`.
            let meta = match state.memory.get(id) {
                Some(stored) => Some(stored.meta.clone()),
                None => self
                    .store
                    .get(topic, id)
                    .map_err(|e| self.disk_error("failed to read the message on disk", &e))?,
            };
            match meta {
                Some(meta) => self.fifo_ack(&mut state, topic, meta, place, now)?,
                None => self.remove_message(&mut state, topic, id, place)?,
            }
        } else {
            self.remove_message(&mut state, topic, id, place)?;
        }
        state.replicas.remove(id);
        if owned {
            state.stats.acked += 1;
            state.stats.last_consumed_ms = Some(now);
            state
                .rates
                .acked
                .record(now, 1, self.rules.rate_window_secs);
        }
        self.ready_changed(topic, &mut state);
        Ok(())
    }

    /// Empties the queue **on this node**: removes every message it keeps (as owner or replica, visible or
    /// invisible, in memory or on disk) and returns how many it was the **owner** of, so that the sum
    /// over the cluster counts each message once. The topic, its counters and the FIFO
    /// deduplication window stay. Idempotent by `prune_id`: a repeated id answers the first count without
    /// removing again (a message published after the first prune stays).
    pub fn prune(&self, topic: &str, prune_id: &str) -> Result<u64, TabaError> {
        self.queue_view(topic)?;
        {
            let done = lock(&self.prunes);
            if let Some((_, removed)) = done.iter().find(|(id, _)| id == prune_id) {
                return Ok(*removed);
            }
        }
        let state = self.state_of(topic);
        let mut state = lock(&state);
        let mut ids: BTreeSet<String> = state.memory.keys().cloned().collect();
        let rows = self
            .store
            .scan(topic)
            .map_err(|e| self.disk_error("failed to list the messages on disk", &e))?;
        // The FIFO queue's short rows (`deleteAt`) are the deduplication window, not messages.
        ids.extend(
            rows.into_iter()
                .filter(|row| row.delete_at_ms.is_none())
                .map(|row| row.id),
        );
        let mut removed = 0;
        for id in &ids {
            let place = if state.memory.contains_key(id) {
                Place::Memory
            } else {
                Place::Disk
            };
            removed += u64::from(state.index.contains(id));
            self.remove_message(&mut state, topic, id, place)?;
        }
        state.dead_letter_inflight.clear();
        self.ready_changed(topic, &mut state);
        drop(state);
        let mut done = lock(&self.prunes);
        done.push_back((prune_id.to_owned(), removed));
        while done.len() > MAX_REMEMBERED_PRUNES {
            done.pop_front();
        }
        Ok(removed)
    }

    /// Does on commit the move to the dead-letter of the copy the node has (doc 13, section 12.10).
    /// `at_ms` is the instant the owner fixed: it is where the `ttl` counts from in the dead-letter.
    pub fn apply_dead_letter(&self, source: &str, id: &str, at_ms: u64) -> Result<(), TabaError> {
        let view = self.topics.get(source)?;
        let state = self.state_of(source);
        let mut state = lock(&state);
        self.move_to_dead_letter(&mut state, &view, vec![id.to_owned()], at_ms);
        self.ready_changed(source, &mut state);
        Ok(())
    }

    /// The move to the dead-letter finished (or failed): if it failed, the message becomes
    /// ready again and the next pass of the sweep tries again.
    pub fn dead_letter_done(&self, topic: &str, id: &str, moved: bool) {
        let now = self.clock.now_millis();
        let state = self.state_of(topic);
        let mut state = lock(&state);
        state.dead_letter_inflight.remove(id);
        if !moved {
            state.index.set_until(id, 0, now);
            // The next settle checks again the ready ones that used up their deliveries and redoes the request.
            state.checked_max_deliveries = None;
            self.ready_changed(topic, &mut state);
        }
    }

    /// Chooses up to `max` ready messages of the owner, computes the new counter, deadline and token and
    /// **reserves** them in the index (in processing) without writing anything: the delivery only counts after the
    /// commit on the quorum (`apply_visibility` plus `finish_take`); if the quorum does not close,
    /// `abort_take` returns the message to ready.
    pub fn take_candidates(
        &self,
        view: &TopicView,
        max: usize,
        visibility_secs: u64,
    ) -> Result<Vec<Candidate>, TabaError> {
        let name = &view.definition.name;
        let now = self.clock.now_millis();
        self.settle(view, now);
        let state = self.state_of(name);
        let mut state = lock(&state);
        let until = now + visibility_secs * 1000;
        let mut out = Vec::new();
        while out.len() < max {
            let Some(id) = state.index.pick() else {
                break;
            };
            let (mut meta, body) = if let Some(stored) = state.memory.get(&id) {
                NodeCounters::add(&self.counters.reads_memory, 1);
                (stored.meta.clone(), stored.body.clone())
            } else {
                let Some(meta) = self
                    .store
                    .get(name, &id)
                    .map_err(|e| self.disk_error("failed to read the message on disk", &e))?
                else {
                    state.index.remove(&id);
                    continue;
                };
                match self.store.body(name, &id) {
                    Ok(body) => {
                        NodeCounters::add(&self.counters.reads_disk, 1);
                        (meta, body)
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        match self.backup_body(name, &id) {
                            Some(body) => {
                                NodeCounters::add(&self.counters.reads_disk, 1);
                                (meta, body)
                            }
                            None => {
                                tracing::error!(topic = %name, id = %id, "corpo sumiu do disco");
                                let _ = self.store.remove(name, &id);
                                state.index.remove(&id);
                                continue;
                            }
                        }
                    }
                    Err(e) => return Err(self.disk_error("failed to read the body on disk", &e)),
                }
            };
            let band = state.index.band_of_entry(&id).unwrap_or(0);
            let previous_deliveries = meta.delivery_count;
            meta.delivery_count += 1;
            meta.invisible_until_ms = until;
            // On FIFO the receipt carries the group's position: whoever receives the ack or the change from any
            // node finds the block without knowing the group (doc 17, section 16.3).
            let token = self.ids.new_at(now);
            meta.receipt_token = Some(if state.index.is_fifo() {
                format!("{:016x}-{token}", meta.position)
            } else {
                token
            });
            state
                .index
                .deliver(&id, until.max(now + 1), now, meta.delivery_count);
            out.push(Candidate {
                meta,
                body,
                previous_deliveries,
                band,
            });
        }
        self.ready_changed(name, &mut state);
        Ok(out)
    }

    /// The quorum did not close: the reserved message goes back to ready, as it was.
    pub fn abort_take(&self, topic: &str, candidate: &Candidate) {
        let now = self.clock.now_millis();
        let state = self.state_of(topic);
        let mut state = lock(&state);
        state
            .index
            .deliver(&candidate.meta.id, 0, now, candidate.previous_deliveries);
        self.ready_changed(topic, &mut state);
        drop(state);
        self.wake(topic, 1);
    }

    /// Counts the deliveries the quorum confirmed: consumption, redeliveries, the wait until the first
    /// delivery and the rates (only the owner counts, doc 11).
    pub fn finish_take(&self, topic: &str, delivered: &[&Candidate], started: Instant) {
        if delivered.is_empty() {
            return;
        }
        let now = self.clock.now_millis();
        let state = self.state_of(topic);
        let mut state = lock(&state);
        for candidate in delivered {
            if candidate.previous_deliveries == 0 {
                let waited_ms = now.saturating_sub(candidate.meta.age_base_ms());
                self.counters
                    .observe_wait(candidate.band, waited_ms as f64 / 1000.0);
            } else {
                state.stats.redelivered += 1;
                NodeCounters::add(&self.counters.redeliveries, 1);
            }
        }
        state
            .rates
            .consumed
            .record(now, delivered.len() as u64, self.rules.rate_window_secs);
        state.stats.consumed += delivered.len() as u64;
        state.stats.last_consumed_ms = Some(now);
        state
            .stats
            .consume_latency
            .record(started.elapsed().as_secs_f64());
    }

    /// Re-evaluates who the owner is after a member change (join, leave, rebalancing):
    /// the messages the node stopped owning leave the index and become replicas, and the replicas it
    /// became the owner of enter the index, ready or invisible according to the metadata
    /// (doc 14, section 13.7).
    pub fn reown(&self) {
        let now = self.clock.now_millis();
        for (name, state) in self.existing_states() {
            let mut woke = false;
            {
                let mut state = lock(&state);
                let fifo = state.index.is_fifo();
                for id in state.index.ids() {
                    // On FIFO the owner comes from the group, which only the metadata tells.
                    let group = if fifo {
                        self.held_meta(&state, &name, &id).and_then(|m| m.group_id)
                    } else {
                        None
                    };
                    if !self.owns(&name, &id, group.as_deref()) {
                        let age = state.index.age_of(&id).unwrap_or(now);
                        state.index.remove(&id);
                        state.replicas.insert(&id, age);
                        if let Some(sink) = self.backup.get() {
                            sink.released(&name, &id);
                        }
                    }
                }
                for id in state.replicas.ids() {
                    let meta = match state.memory.get(&id) {
                        Some(stored) => Some(stored.meta.clone()),
                        None => self.store.get(&name, &id).ok().flatten(),
                    };
                    let Some(meta) = meta else {
                        state.replicas.remove(&id);
                        continue;
                    };
                    if !self.owns(&name, &id, meta.group_id.as_deref()) {
                        continue;
                    }
                    state.replicas.remove(&id);
                    state.index.insert(
                        &id,
                        meta.group_id.as_deref(),
                        meta.age_base_ms(),
                        meta.invisible_until_ms,
                        meta.delivery_count,
                        now,
                    );
                    if let Some(sink) = self.backup.get() {
                        sink.adopted(&name, &id);
                    }
                    woke |= meta.invisible_until_ms <= now;
                }
                self.ready_changed(&name, &mut state);
            }
            if woke {
                self.wake(&name, 1);
            }
        }
    }

    /// Deletes everything the node keeps of messages, in memory and on disk (the node comes back from the drain as a
    /// new node and does not reuse what it had, doc 04, section 6.10).
    pub fn reset_all(&self) {
        let mut names: Vec<String> = self
            .existing_states()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        if let Ok(on_disk) = self.store.topics_on_disk() {
            names.extend(on_disk);
        }
        names.sort();
        names.dedup();
        for name in names {
            self.drop_topic(&name);
        }
    }

    /// A distributor's `publish`, after each copy has been published: the counts and the
    /// `no_target` and `partial` events.
    pub fn record_distributor(&self, name: &str, deliveries: &[DistributorDelivery]) {
        let failed = deliveries.iter().filter(|d| d.result.is_err()).count() as u64;
        {
            let mut distributors = lock(&self.distributors);
            let counters = distributors.entry(name.to_owned()).or_default();
            counters.received += 1;
            counters.copies += deliveries.len() as u64 - failed;
            counters.copy_errors += failed;
        }
        if deliveries.is_empty() {
            tracing::info!(
                event.name = "taba.distributor.no_target",
                messaging.destination.name = %name,
                "publication with no matching queue: the message was discarded"
            );
        } else if failed > 0 {
            tracing::warn!(
                event.name = "taba.distributor.partial",
                messaging.destination.name = %name,
                failed,
                copies = deliveries.len(),
                "at least one copy of the publication failed"
            );
        }
    }

    /// The maximum body size a publication to the topic accepts (for whoever reads the body first).
    pub fn message_limit(&self, view: &TopicView) -> u64 {
        view.max_message_bytes().unwrap_or(self.rules.max_bytes)
    }

    /// The distributor's limit (the general one).
    pub fn distributor_limit(&self) -> u64 {
        self.rules.max_bytes
    }

    /// The ceiling of a distributor's destinations.
    pub fn distributor_max_targets(&self) -> Option<u64> {
        self.rules.distributor_max_targets
    }

    /// Validates what the publication brings before knowing the topic's type (`Content-Type`, body,
    /// `groupId`, `deduplicationId`).
    pub fn validate_publish(&self, request: &PublishRequest) -> Result<(), TabaError> {
        Prepared::new(request).map(|_| ())
    }
}

/// What is validated in the publication before knowing the topic's type.
pub struct Prepared {
    content_type: ContentType,
}

impl Prepared {
    fn new(request: &PublishRequest) -> Result<Self, TabaError> {
        let content_type = ContentType::parse(
            request
                .content_type
                .as_deref()
                .unwrap_or(DEFAULT_CONTENT_TYPE),
        )?;
        content_type.validate_body(&request.body)?;
        if let Some(group) = &request.group_id {
            validation::message_key("groupId", group)?;
        }
        if let Some(key) = &request.deduplication_id {
            validation::message_key("deduplicationId", key)?;
        }
        Ok(Self { content_type })
    }
}

#[cfg(test)]
mod tests;
