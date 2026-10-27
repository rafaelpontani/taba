//! The block synchronization of a node that joins (doc 05, section 7.3).
//!
//! The new node is already in the member table and receives the normal cluster protocol, but cannot
//! process the events: it stores each one in the event database (`_sync/sync.db`) and confirms the
//! register after writing. Meanwhile it loads, in order:
//!
//! 1. **topics and tokens**, pulled from a member;
//! 2. **blocks**: for each block it is responsible for, the ID list of all the other sources
//!    (union, by ordered *merge*), the messages in ranges balanced among them, and what was missing
//!    from another source, on the single-ID route and, finally, on the relisting (what still shows up and has no
//!    content on any node is a **loss**);
//! 3. **events**, on top of the load: always the oldest, applied and deleted, until empty;
//! 4. **final check**: repeats the listings and discards what the load brought and no longer shows up;
//! 5. **switch to normal mode** (*cutover*): when the database empties, under the same lock the
//!    reception uses to decide each event's destination, the node starts processing in real time.
//!
//! Only then does the node become ready and start owning the messages of its blocks. This module is
//! also the **side that serves** the synchronization (`source`) and the cleanup of what the node no longer has to
//! keep (step 7).

mod source;

pub use source::BlockSource;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tracing::Instrument;

use super::members::{Member, MemberState};
use super::protocol::{Ack, LossRecord, MessageRecord, Request, Response, Step, StreamQuery};
use super::replication::{Definitions, Pulled, Replica};
use super::service::ClusterService;
use super::state::NodeState;
use super::transport::CallError;
use crate::config::Config;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring::{BlockId, Ring};
use crate::core::time;
use crate::engine::messages::{Loaded, Messages};
use crate::storage::losses::{Loss, LossStore};
use crate::storage::messages::ExcludedRow;
use crate::storage::synclog::{StoredEvent, SyncLog};
use crate::telemetry::metrics::{
    SYNC_BLOCKS_ACTIVE, SYNC_BYTES, SYNC_DURATION, SYNC_FAILURES, SYNC_IDS_LISTED, SYNC_LOG_BYTES,
    SYNC_LOG_EVENTS, SYNC_MESSAGES_PULLED, SYNC_PHASE,
};
use crate::telemetry::trace;

/// The `cluster.sync.*` parameters (doc 10) with the defaults chosen by the implementation.
#[derive(Debug, Clone)]
pub struct SyncRules {
    /// `cluster.sync.stream.max-items`.
    pub max_items: usize,
    /// `cluster.sync.stream.max-bytes`.
    pub max_bytes: u64,
    /// `cluster.sync.range.size`: IDs per range asked of a source.
    pub range_size: usize,
    /// `cluster.sync.log.max-bytes`.
    pub log_max_bytes: u64,
    /// `cluster.sync.timeout`: how long to keep trying a source that does not serve.
    pub timeout: Duration,
    pub storage_dir: PathBuf,
}

impl SyncRules {
    pub fn from_config(config: &Config) -> Self {
        let sync = &config.cluster.sync;
        Self {
            max_items: sync.stream_max_items.map_or(500, |n| (n as usize).max(2)),
            max_bytes: sync.stream_max_bytes.unwrap_or(4 * 1024 * 1024),
            range_size: sync.range_size.map_or(200, |n| (n as usize).max(1)),
            log_max_bytes: sync.log_max_bytes.unwrap_or(1024 * 1024 * 1024),
            timeout: sync.timeout.unwrap_or(Duration::from_secs(60)),
            storage_dir: config.storage.dir.clone(),
        }
    }
}

/// Why the synchronization did not finish.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("no source node answered: {0}")]
    NoSource(String),
    #[error("disk failure: {0}")]
    Disk(String),
    #[error("the event database exceeded cluster.sync.log.max-bytes")]
    LogOverflow,
    #[error("the node stopped during the synchronization")]
    Cancelled,
    #[error("the sources excluded this node while it synchronized")]
    Excluded,
}

impl SyncError {
    fn reason(&self) -> &'static str {
        match self {
            SyncError::Excluded => "excluded",
            SyncError::NoSource(_) => "no-source",
            SyncError::Disk(_) => "disk",
            SyncError::LogOverflow => "log-overflow",
            SyncError::Cancelled => "cancelled",
        }
    }
}

fn disk(e: &io::Error) -> SyncError {
    SyncError::Disk(e.to_string())
}

const PHASES: [&str; 5] = ["topics", "blocks", "replay", "verification", "ready"];

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// A call the syncing node stores in the event database instead of processing.
fn is_event(request: &Request) -> bool {
    matches!(
        request,
        Request::DefinitionApply(_)
            | Request::Write { .. }
            | Request::Visibility { .. }
            | Request::Ack { .. }
    )
}

fn step_operation<R>(step: &Step<R>, register: impl Fn(&R) -> &str) -> String {
    match step {
        Step::Register(body) => register(body).to_owned(),
        Step::Commit(op) | Step::Rollback(op) => op.operation_id.clone(),
    }
}

fn operation_of(request: &Request) -> String {
    match request {
        Request::DefinitionApply(change) => {
            format!("{}:{}:{}", change.kind.as_str(), change.key, change.version)
        }
        Request::Write { step, .. } => step_operation(step, |r| &r.operation_id),
        Request::Visibility { step, .. } => step_operation(step, |r| &r.operation_id),
        Request::Ack { step, .. } => step_operation(step, |r| &r.operation_id),
        _ => String::new(),
    }
}

/// A row of a block's ID listing.
#[derive(Debug, Clone)]
struct IdLine {
    id: String,
    topic: String,
}

/// An ID of the union of the lists, with the sources that have it (indexes into the source list).
#[derive(Debug, Clone)]
struct Merged {
    line: IdLine,
    holders: Vec<usize>,
}

struct Cursor {
    last: Option<String>,
    buf: VecDeque<IdLine>,
    more: bool,
    dead: bool,
}

/// The ordered *merge* of the ID lists from several sources, without materializing them: each source has
/// a stretch in memory and the next is only requested when the previous one ends.
struct MergedIds<'a> {
    syncer: &'a Syncer,
    block: String,
    sources: &'a [Member],
    cursors: Vec<Cursor>,
}

impl<'a> MergedIds<'a> {
    fn new(syncer: &'a Syncer, block: String, sources: &'a [Member]) -> Self {
        Self {
            syncer,
            block,
            sources,
            cursors: sources
                .iter()
                .map(|_| Cursor {
                    last: None,
                    buf: VecDeque::new(),
                    more: true,
                    dead: false,
                })
                .collect(),
        }
    }

    /// Some source stopped answering: the union is incomplete.
    fn degraded(&self) -> bool {
        self.cursors.iter().any(|c| c.dead)
    }

    async fn fill(&mut self, index: usize) -> Result<(), SyncError> {
        let request = Request::BlockIds {
            block: self.block.clone(),
            query: StreamQuery {
                from: self.cursors[index].last.clone(),
                to: None,
                limit: Some(u32::try_from(self.syncer.rules().max_items).unwrap_or(u32::MAX)),
            },
        };
        let reply = self.syncer.call_source(&self.sources[index], request).await;
        let cursor = &mut self.cursors[index];
        let page = match reply {
            Ok(Response::Stream(page)) => page,
            Ok(_) | Err(_) => {
                tracing::warn!(
                    block = %self.block,
                    peer = %self.sources[index].node_id,
                    "the source did not list the block's IDs"
                );
                cursor.dead = true;
                cursor.more = false;
                return Ok(());
            }
        };
        for item in &page.items {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            // The resumption returns the last line read again: it is ignored.
            if cursor.last.as_deref() == Some(id) {
                continue;
            }
            cursor.buf.push_back(IdLine {
                id: id.to_owned(),
                topic: item
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            });
        }
        cursor.more = page.end.has_more && page.end.last != cursor.last && !page.items.is_empty();
        cursor.last = page.end.last;
        Ok(())
    }

    /// Up to `count` IDs of the union, in order. Empty when it is over.
    async fn next_batch(&mut self, count: usize) -> Result<Vec<Merged>, SyncError> {
        let mut out = Vec::new();
        while out.len() < count {
            for index in 0..self.cursors.len() {
                if self.cursors[index].buf.is_empty()
                    && self.cursors[index].more
                    && !self.cursors[index].dead
                {
                    self.fill(index).await?;
                }
            }
            if self.cursors.iter().all(|c| c.dead) {
                return Err(SyncError::NoSource(format!(
                    "no source listed block {}",
                    self.block
                )));
            }
            let Some(min) = self
                .cursors
                .iter()
                .filter_map(|c| c.buf.front())
                .map(|l| (l.id.clone(), l.topic.clone()))
                .min()
            else {
                break;
            };
            let mut holders = Vec::new();
            let mut line = None;
            for (index, cursor) in self.cursors.iter_mut().enumerate() {
                if cursor
                    .buf
                    .front()
                    .is_some_and(|l| l.id == min.0 && l.topic == min.1)
                {
                    line = cursor.buf.pop_front();
                    holders.push(index);
                }
            }
            if let Some(line) = line {
                out.push(Merged { line, holders });
            }
        }
        Ok(out)
    }
}

pub struct Syncer {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    definitions: Arc<Definitions>,
    replica: Arc<Replica>,
    losses: LossStore,
    rules: SyncRules,
    /// `Some` while the node writes the events to the database (synchronization mode). The lock also
    /// decides, on each event, between the database and normal processing (*cutover*).
    recording: Mutex<Option<Arc<SyncLog>>>,
    overflow: AtomicBool,
    pulled: Mutex<Pulled>,
    round_robin: AtomicUsize,
    /// The table version for which the cleanup already ran.
    cleaned_version: Mutex<Option<u64>>,
    /// When the definitions were pulled again because of an unknown topic.
    refreshed_at: Mutex<Option<Instant>>,
    /// The start of the continuous recording of each source that sent its log of excluded messages in this
    /// synchronization (doc 07, section 10.3).
    excluded_sources: Mutex<HashMap<String, u64>>,
    /// A source did not send its log or its date: the node does not inherit the date.
    excluded_incomplete: AtomicBool,
    /// Test hook: waits on the semaphore on entering the phase.
    pause: Mutex<Option<(&'static str, Arc<tokio::sync::Semaphore>)>>,
}

impl Syncer {
    pub fn new(
        service: Arc<ClusterService>,
        messages: Arc<Messages>,
        definitions: Arc<Definitions>,
        replica: Arc<Replica>,
        losses: LossStore,
        rules: SyncRules,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            messages,
            definitions,
            replica,
            losses,
            rules,
            recording: Mutex::new(None),
            overflow: AtomicBool::new(false),
            pulled: Mutex::new(Pulled::default()),
            round_robin: AtomicUsize::new(0),
            cleaned_version: Mutex::new(None),
            refreshed_at: Mutex::new(None),
            excluded_sources: Mutex::new(HashMap::new()),
            excluded_incomplete: AtomicBool::new(false),
            pause: Mutex::new(None),
        })
    }

    fn rules(&self) -> &SyncRules {
        &self.rules
    }

    /// Is the node writing the events to the database?
    pub fn is_recording(&self) -> bool {
        lock(&self.recording).is_some()
    }

    /// Opens the event database and starts writing. It comes before the node installs the member table:
    /// from then on it receives the normal protocol.
    pub fn begin(&self) -> io::Result<()> {
        let log = SyncLog::create(&self.rules.storage_dir)?;
        self.overflow.store(false, Ordering::SeqCst);
        self.excluded_incomplete.store(false, Ordering::SeqCst);
        lock(&self.excluded_sources).clear();
        *lock(&self.recording) = Some(Arc::new(log));
        Ok(())
    }

    /// Drops the event database without processing it (the node goes back to being new).
    pub fn abandon(&self) {
        let recording = lock(&self.recording).take();
        if let Some(log) = recording {
            let _ = log.discard();
        }
    }

    // ---- Receiving events during the synchronization ----

    /// Stores the event in the database instead of processing it and returns the register's confirmation. `None`
    /// when the node processes normally (it is not an event, or the synchronization already ended).
    pub fn intercept(&self, request: &Request) -> Result<Option<Response>, TabaError> {
        if !is_event(request) {
            return Ok(None);
        }
        let guard = lock(&self.recording);
        let Some(log) = guard.as_ref() else {
            return Ok(None);
        };
        let call = request.to_http();
        log.append(&call.method, &call.path, &call.headers, &call.body)
            .map_err(|e| {
                tracing::error!(error = %e, "failed to write the event to the event database");
                TabaError::internal()
            })?;
        let registry = &self.service.telemetry().registry;
        registry.add(&SYNC_LOG_EVENTS, &[], 1.0);
        if let Ok(size) = log.size_bytes() {
            registry.set(&SYNC_LOG_BYTES, &[], size as f64);
            if size > self.rules.log_max_bytes {
                self.overflow.store(true, Ordering::SeqCst);
            }
        }
        if let Request::DefinitionApply(change) = request {
            return Ok(Some(Response::Definition(change.clone())));
        }
        Ok(Some(Response::Ack(Ack {
            operation_id: operation_of(request),
            accepted: true,
        })))
    }

    // ---- Sincronizar ----

    /// Only for the tests: the synchronization stops on entering `phase` and goes on when the semaphore
    /// gains a permit. It lets the test act (publish, acknowledge, change the table) with the node
    /// in synchronization.
    #[doc(hidden)]
    pub fn pause_at(&self, phase: &'static str, gate: Arc<tokio::sync::Semaphore>) {
        *lock(&self.pause) = Some((phase, gate));
    }

    async fn phase(&self, current: &str) {
        let registry = &self.service.telemetry().registry;
        for phase in PHASES {
            registry.set(
                &SYNC_PHASE,
                &[("taba.phase", phase)],
                if phase == current { 1.0 } else { 0.0 },
            );
        }
        let gate = lock(&self.pause)
            .as_ref()
            .filter(|(phase, _)| *phase == current)
            .map(|(_, gate)| gate.clone());
        if let Some(gate) = gate
            && let Ok(permit) = gate.acquire().await
        {
            permit.forget();
        }
    }

    fn log(&self) -> Result<Arc<SyncLog>, SyncError> {
        lock(&self.recording)
            .clone()
            .ok_or_else(|| SyncError::Disk("the event database was not opened".into()))
    }

    fn check(&self) -> Result<(), SyncError> {
        if self.service.cancel_token().is_cancelled() {
            return Err(SyncError::Cancelled);
        }
        if self.overflow.load(Ordering::SeqCst) {
            return Err(SyncError::LogOverflow);
        }
        Ok(())
    }

    /// Loads everything the node needs and switches to normal mode. `sources` are the members
    /// to pull the definitions from (the one that coordinated the join first).
    pub async fn run(&self, sources: &[Member]) -> Result<(), SyncError> {
        let result = self.run_phases(sources).await;
        if let Err(e) = &result {
            self.service.telemetry().registry.add(
                &SYNC_FAILURES,
                &[("taba.reason", e.reason())],
                1.0,
            );
        }
        result
    }

    async fn run_phases(&self, sources: &[Member]) -> Result<(), SyncError> {
        let log = self.log()?;
        self.phase("topics").await;
        let pulled = self.pull_definitions(sources).await?;
        *lock(&self.pulled) = pulled.clone();

        self.phase("blocks").await;
        let ring = self.service.membership().ring();
        let me = self.service.node_id();
        let factor = self.service.rules().replication_factor.max(1) as usize;
        let blocks = ring.blocks_of(&me, factor);
        for block in &blocks {
            self.check()?;
            self.sync_block_waiting(&ring, *block, &log).await?;
        }
        self.inherit_excluded_since();

        self.phase("replay").await;
        self.replay(&log, false).await?;

        self.phase("verification").await;
        self.verify(&ring, &blocks, &log, &pulled, sources).await?;

        self.phase("ready").await;
        self.replay(&log, true).await?;
        log.discard().map_err(|e| disk(&e))?;
        self.service
            .telemetry()
            .registry
            .set(&SYNC_LOG_BYTES, &[], 0.0);
        Ok(())
    }

    /// `sync_block` that, if no source answered, tries again until `cluster.sync.timeout`. The
    /// sources may not know this node yet, because their join commit is slower than
    /// `write.commit.timeout` and they answer `cluster.not-member` until it arrives (D105).
    async fn sync_block_waiting(
        &self,
        ring: &Ring,
        block: BlockId,
        log: &Arc<SyncLog>,
    ) -> Result<(), SyncError> {
        // One trace per synchronized block (doc 11, "Traces"); each call to a source is a span.
        let span = tracing::info_span!(
            "sync.block",
            otel.name = "taba.sync.block",
            otel.status_code = tracing::field::Empty,
            taba.block.id = %block,
        );
        let result = async {
            let deadline = Instant::now() + self.rules.timeout;
            loop {
                match self.sync_block(ring, block, log).await {
                    Err(SyncError::NoSource(why)) if Instant::now() < deadline => {
                        tracing::warn!(%why, "no source answered the block: trying again");
                        self.check()?;
                        self.check_not_excluded().await?;
                        self.nap().await;
                    }
                    other => return other,
                }
            }
        }
        .instrument(span.clone())
        .await;
        if result.is_err() {
            span.record("otel.status_code", "ERROR");
        }
        result
    }

    /// Pulls the definitions from a member; tries the others and repeats until `cluster.sync.timeout`.
    async fn pull_definitions(&self, sources: &[Member]) -> Result<Pulled, SyncError> {
        let deadline = Instant::now() + self.rules.timeout;
        loop {
            self.check()?;
            let mut last = String::from("no source");
            for source in sources {
                match self.definitions.pull(source).await {
                    Ok(pulled) => return Ok(pulled),
                    Err(e) => last = format!("{}: {}", source.node_id, e.message),
                }
            }
            if Instant::now() >= deadline {
                return Err(SyncError::NoSource(last));
            }
            self.check_not_excluded().await?;
            self.nap().await;
        }
    }

    /// The sources that do not answer may be the ones that already excluded this node (a join that
    /// crossed a partition's reunion): retrying would wait for `cluster.sync.timeout` and end in `failed`.
    async fn check_not_excluded(&self) -> Result<(), SyncError> {
        if self.service.excluded_while_syncing().await {
            return Err(SyncError::Excluded);
        }
        Ok(())
    }

    async fn nap(&self) {
        tokio::select! {
            () = self.service.cancel_token().cancelled() => {}
            () = tokio::time::sleep(self.service.rules().retry_backoff) => {}
        }
    }

    /// Calls a source. A network failure, `5xx` and `409` (the table there is still another) repeat until
    /// `cluster.sync.timeout`; the rest is the source's response.
    async fn call_source(&self, source: &Member, request: Request) -> Result<Response, TabaError> {
        let span = trace::peer_span("taba.sync.fetch", &source.node_id, false);
        trace::traced(span, self.call_source_inner(source, request)).await
    }

    async fn call_source_inner(
        &self,
        source: &Member,
        request: Request,
    ) -> Result<Response, TabaError> {
        let deadline = Instant::now() + self.rules.timeout;
        loop {
            if self.service.cancel_token().is_cancelled() {
                return Err(TabaError::new(
                    ErrorCode::ClusterNodeSyncing,
                    "the node stopped",
                ));
            }
            let failure = match self
                .service
                .call_peer(source, request.clone(), self.service.rules().call_timeout)
                .await
            {
                Ok(reply) => return Ok(reply),
                Err(CallError::Remote(e))
                    if e.status() < 500 && e.code != ErrorCode::ClusterConflict =>
                {
                    return Err(e);
                }
                Err(CallError::Remote(e)) => e,
                Err(other) => TabaError::new(ErrorCode::ClusterNodeSyncing, other.to_string()),
            };
            if Instant::now() >= deadline {
                return Err(failure);
            }
            self.nap().await;
        }
    }

    /// A block's sources: the other nodes responsible for it.
    fn sources_of(&self, ring: &Ring, block: BlockId) -> Vec<Member> {
        let me = self.service.node_id();
        let factor = self.service.rules().replication_factor.max(1) as usize;
        let Some(table) = self.service.membership().table() else {
            return Vec::new();
        };
        ring.block_owners(block, factor)
            .unwrap_or_default()
            .into_iter()
            .filter(|owner| *owner != me)
            .filter_map(|owner| table.member(owner).cloned())
            .collect()
    }

    async fn sync_block(
        &self,
        ring: &Ring,
        block: BlockId,
        log: &SyncLog,
    ) -> Result<(), SyncError> {
        let sources = self.sources_of(ring, block);
        if sources.is_empty() {
            return Ok(());
        }
        let registry = &self.service.telemetry().registry;
        let started = Instant::now();
        registry.add(&SYNC_BLOCKS_ACTIVE, &[], 1.0);
        let result = self.load_block(block, &sources, log).await;
        registry.add(&SYNC_BLOCKS_ACTIVE, &[], -1.0);
        registry.observe(&SYNC_DURATION, &[], started.elapsed().as_secs_f64());
        if result.is_ok() {
            self.pull_excluded(&block.to_string(), &sources).await;
            tracing::info!(block = %block, sources = sources.len(), "bloco sincronizado");
        }
        result
    }

    async fn load_block(
        &self,
        block: BlockId,
        sources: &[Member],
        log: &SyncLog,
    ) -> Result<(), SyncError> {
        let text = block.to_string();
        let mut ids = MergedIds::new(self, text.clone(), sources);
        loop {
            self.check()?;
            let batch = ids.next_batch(self.rules.range_size).await?;
            if batch.is_empty() {
                return Ok(());
            }
            self.service
                .telemetry()
                .registry
                .add(&SYNC_IDS_LISTED, &[], batch.len() as f64);
            let missing = self.pull_range(&text, sources, &batch, log).await?;
            self.settle_missing(&text, sources, missing).await?;
        }
    }

    /// Adds to the node's log the exclusions each source recorded for the block, and learns from when the
    /// source records. A source that fails leaves the log incomplete: the node then keeps its own date.
    async fn pull_excluded(&self, block: &str, sources: &[Member]) {
        for source in sources {
            if self.pull_excluded_from(block, source).await.is_err() {
                self.excluded_incomplete.store(true, Ordering::SeqCst);
                tracing::warn!(
                    block,
                    source = %source.node_id,
                    "could not copy the log of excluded messages"
                );
            }
        }
    }

    async fn pull_excluded_from(&self, block: &str, source: &Member) -> Result<(), TabaError> {
        let mut cursor: Option<String> = None;
        loop {
            self.check().map_err(|_| TabaError::internal())?;
            let request = Request::BlockExcluded {
                block: block.to_owned(),
                query: StreamQuery {
                    from: cursor.clone(),
                    to: None,
                    limit: Some(u32::try_from(self.rules.max_items).unwrap_or(u32::MAX)),
                },
            };
            let Response::Stream(page) = self.call_source(source, request).await? else {
                return Err(TabaError::internal());
            };
            let mut by_topic: HashMap<String, Vec<ExcludedRow>> = HashMap::new();
            for item in &page.items {
                let (Some(id), Some(topic), Some(position), Some(at_ms)) = (
                    item["id"].as_str(),
                    item["topic"].as_str(),
                    item["position"].as_u64(),
                    item["atMs"].as_u64(),
                ) else {
                    return Err(TabaError::internal());
                };
                by_topic
                    .entry(topic.to_owned())
                    .or_default()
                    .push(ExcludedRow {
                        id: id.to_owned(),
                        position,
                        at_ms,
                    });
            }
            let messages = self.messages.clone();
            tokio::task::spawn_blocking(move || {
                for (topic, rows) in by_topic {
                    messages.import_excluded(&topic, &rows)?;
                }
                Ok::<(), TabaError>(())
            })
            .await
            .map_err(|_| TabaError::internal())??;
            if !page.end.has_more || page.end.last == cursor || page.items.is_empty() {
                break;
            }
            cursor = page.end.last;
        }
        self.learn_excluded_since(source).await
    }

    /// Reads the start of the source's recording (its `NodeStatus.excludedSince`), once per source.
    async fn learn_excluded_since(&self, source: &Member) -> Result<(), TabaError> {
        if lock(&self.excluded_sources).contains_key(&source.node_id) {
            return Ok(());
        }
        let Response::Status(status) = self.call_source(source, Request::Status).await? else {
            return Err(TabaError::internal());
        };
        let since = status["excludedSince"]
            .as_str()
            .and_then(time::parse_ms)
            .ok_or_else(TabaError::internal)?;
        lock(&self.excluded_sources).insert(source.node_id.clone(), since);
        Ok(())
    }

    /// The first synchronization of a disk: the node covers from when its sources cover (the latest of
    /// their dates), because that is what it copied. Without every source's log, it keeps its own date.
    fn inherit_excluded_since(&self) {
        if self.excluded_incomplete.load(Ordering::SeqCst) {
            return;
        }
        let latest = lock(&self.excluded_sources).values().copied().max();
        if let Some(since) = latest
            && let Err(e) = self.messages.inherit_excluded_since(since)
        {
            tracing::warn!(error = %e, "could not record the start of the excluded log");
        }
    }

    /// Asks a source for the messages of an ID range, and the others for what was missing.
    async fn pull_range(
        &self,
        block: &str,
        sources: &[Member],
        batch: &[Merged],
        log: &SyncLog,
    ) -> Result<Vec<Merged>, SyncError> {
        let wanted: Vec<&Merged> = batch.iter().collect();
        let (Some(first), Some(last)) = (wanted.first(), wanted.last()) else {
            return Ok(Vec::new());
        };
        // The range's source is the one that has more of the IDs; on a tie, the sources take turns.
        let turn = self.round_robin.fetch_add(1, Ordering::Relaxed);
        let score = |index: usize| wanted.iter().filter(|m| m.holders.contains(&index)).count();
        let best = (0..sources.len()).map(score).max().unwrap_or(0);
        let tied: Vec<usize> = (0..sources.len()).filter(|i| score(*i) == best).collect();
        let chosen = tied[turn % tied.len()];

        let mut got: HashSet<String> = HashSet::new();
        self.stream_range(
            block,
            &sources[chosen],
            &first.line.id,
            &last.line.id,
            log,
            &mut got,
        )
        .await;

        let missing_now = |got: &HashSet<String>| -> Vec<&Merged> {
            wanted
                .iter()
                .copied()
                .filter(|m| !got.contains(&m.line.id))
                .collect()
        };
        // What was missing from the range is fetched from another source that has it.
        for index in (0..sources.len()).filter(|i| *i != chosen) {
            let missing = missing_now(&got);
            let from_here: Vec<&&Merged> = missing
                .iter()
                .filter(|m| m.holders.contains(&index))
                .collect();
            let (Some(low), Some(high)) = (from_here.first(), from_here.last()) else {
                continue;
            };
            let (low, high) = (low.line.id.clone(), high.line.id.clone());
            self.stream_range(block, &sources[index], &low, &high, log, &mut got)
                .await;
        }
        // The last resort: the single-ID route.
        let still: Vec<Merged> = missing_now(&got).into_iter().cloned().collect();
        let mut gone = Vec::new();
        for entry in still {
            let mut found = false;
            for holder in &entry.holders {
                let request = Request::BlockMessage {
                    block: block.to_owned(),
                    id: entry.line.id.clone(),
                };
                if let Ok(Response::Record(record)) =
                    self.call_source(&sources[*holder], request).await
                {
                    let mut fresh = HashSet::new();
                    self.apply_record(block, &sources[*holder], record, log, &mut fresh)
                        .await;
                    found = !fresh.is_empty();
                    if found {
                        got.insert(entry.line.id.clone());
                        break;
                    }
                }
            }
            if !found {
                gone.push(entry);
            }
        }
        Ok(gone)
    }

    /// Reads the message stream `from`–`to` from a source, stretch by stretch, and applies each one.
    /// A source failure only stops adding to `got`: what is missing is fetched elsewhere.
    async fn stream_range(
        &self,
        block: &str,
        source: &Member,
        from: &str,
        to: &str,
        log: &SyncLog,
        got: &mut HashSet<String>,
    ) {
        let mut cursor = Some(from.to_owned());
        loop {
            let request = Request::BlockMessages {
                block: block.to_owned(),
                query: StreamQuery {
                    from: cursor.clone(),
                    to: Some(to.to_owned()),
                    limit: Some(u32::try_from(self.rules.max_items).unwrap_or(u32::MAX)),
                },
            };
            let Ok(Response::Stream(page)) = self.call_source(source, request).await else {
                return;
            };
            for item in &page.items {
                match serde_json::from_value::<MessageRecord>(item.clone()) {
                    Ok(record) => self.apply_record(block, source, record, log, got).await,
                    Err(e) => {
                        tracing::warn!(error = %e, "unreadable message in the synchronization")
                    }
                }
            }
            if !page.end.has_more || page.end.last == cursor || page.items.is_empty() {
                return;
            }
            cursor = page.end.last;
        }
    }

    /// Stores a message of the load. One of a topic still unknown makes the node pull the definitions
    /// again (the topic was created after the first load).
    async fn apply_record(
        &self,
        block: &str,
        source: &Member,
        record: MessageRecord,
        log: &SyncLog,
        got: &mut HashSet<String>,
    ) {
        let (topic, id) = (record.topic.clone(), record.id.clone());
        let size = record.body.len() as f64;
        // The FIFO's short row is carried without a body; there is nothing to lose if it is missing.
        let mut outcome = self.load(&record).await;
        if matches!(outcome, Ok(Loaded::UnknownTopic)) {
            self.refresh_definitions(source).await;
            outcome = self.load(&record).await;
        }
        let registry = &self.service.telemetry().registry;
        match outcome {
            Ok(Loaded::Stored | Loaded::Existing) => {
                registry.add(
                    &SYNC_MESSAGES_PULLED,
                    &[("taba.peer", &source.node_id)],
                    1.0,
                );
                registry.add(&SYNC_BYTES, &[("taba.peer", &source.node_id)], size);
                if let Err(e) = log.mark_loaded(block, &[(topic, id.clone())]) {
                    tracing::error!(error = %e, "failed to record the loaded message");
                }
                got.insert(id);
            }
            // It expired by ttl, or the topic was removed: there is nothing to store or to charge for.
            Ok(Loaded::Expired | Loaded::UnknownTopic) => {
                got.insert(id);
            }
            Err(e) => {
                tracing::error!(topic = %topic, id = %id, error = %e.message, "failed to store the loaded message");
            }
        }
    }

    async fn load(&self, record: &MessageRecord) -> Result<Loaded, TabaError> {
        let (messages, topic) = (self.messages.clone(), record.topic.clone());
        let (meta, body) = (record.to_meta(), record.body.clone());
        if let Some(delete_at_ms) = record.delete_at_ms {
            return tokio::task::spawn_blocking(move || {
                messages.apply_tombstone(&topic, meta, delete_at_ms)
            })
            .await
            .map_err(|_| TabaError::internal())?;
        }
        tokio::task::spawn_blocking(move || messages.apply_load(&topic, meta, body))
            .await
            .map_err(|_| TabaError::internal())?
    }

    async fn refresh_definitions(&self, source: &Member) {
        {
            let mut at = lock(&self.refreshed_at);
            if at.is_some_and(|t| t.elapsed() < Duration::from_millis(200)) {
                return;
            }
            *at = Some(Instant::now());
        }
        match self.definitions.pull(source).await {
            Ok(pulled) => {
                let mut all = lock(&self.pulled);
                all.topics.extend(pulled.topics);
                all.tokens.extend(pulled.tokens);
            }
            Err(e) => {
                tracing::warn!(error = %e.message, "could not pull the definitions again")
            }
        }
    }

    /// The listed messages that no source delivered: the relisting says whether they were acknowledged
    /// (they vanish from the lists, and the case is silently discarded) or whether they are a loss.
    async fn settle_missing(
        &self,
        block: &str,
        sources: &[Member],
        missing: Vec<Merged>,
    ) -> Result<(), SyncError> {
        for entry in missing {
            let mut listed = false;
            let mut consulted = Vec::new();
            for source in sources {
                let request = Request::BlockIds {
                    block: block.to_owned(),
                    query: StreamQuery {
                        from: Some(entry.line.id.clone()),
                        to: Some(entry.line.id.clone()),
                        limit: Some(2),
                    },
                };
                consulted.push(source.node_id.clone());
                if let Ok(Response::Stream(page)) = self.call_source(source, request).await
                    && page.items.iter().any(|item| {
                        item.get("id").and_then(|v| v.as_str()) == Some(entry.line.id.as_str())
                    })
                {
                    listed = true;
                }
            }
            if !listed {
                continue;
            }
            // Before declaring the loss, the bucket (doc 23, section 1.7, example B): the message that
            // no live node has is fetched from S3. Only if S3 does not have it either does it become a loss.
            if let Some(port) = self.service.backup()
                && let Some((meta, body)) = port
                    .backup()
                    .find_message(&entry.line.topic, &entry.line.id)
                    .await
                && self
                    .messages
                    .apply_load(&entry.line.topic, meta, body)
                    .is_ok()
            {
                tracing::warn!(
                    event.name = "taba.message.restored_from_backup",
                    messaging.destination.name = %entry.line.topic,
                    message.id = %entry.line.id,
                    block,
                    "no live node had the message: restored from the S3 backup"
                );
                continue;
            }
            tracing::error!(
                event.name = "taba.message.lost",
                messaging.destination.name = %entry.line.topic,
                message.id = %entry.line.id,
                block,
                nodes = ?consulted,
                "message lost: it is on the lists and no node has the content"
            );
            let loss = Loss {
                topic: entry.line.topic.clone(),
                id: entry.line.id.clone(),
                block: block.to_owned(),
                detected_ms: self.service.clock().now_millis(),
            };
            if let Err(e) = self.losses.record(&loss) {
                tracing::error!(error = %e, "failed to record the loss");
            }
            self.share_loss(&loss);
        }
        Ok(())
    }

    /// Sends the loss to the other nodes responsible for the block, without waiting (best effort): the
    /// information survives the fall of whoever detected it (doc 04, section 6.6, "Loss table").
    fn share_loss(&self, loss: &Loss) {
        let Ok(block) = loss.block.parse::<BlockId>() else {
            return;
        };
        let factor = self.service.rules().replication_factor.max(1) as usize;
        let me = self.service.node_id();
        let (Some(table), Some(owners)) = (
            self.service.membership().table(),
            self.service
                .membership()
                .ring()
                .block_owners(block, factor)
                .map(|owners| owners.into_iter().map(str::to_owned).collect::<Vec<_>>()),
        ) else {
            return;
        };
        let record = LossRecord {
            topic: loss.topic.clone(),
            id: loss.id.clone(),
            block: loss.block.clone(),
            detected_at: time::format_ms(loss.detected_ms),
        };
        for owner in owners.into_iter().filter(|owner| *owner != me) {
            let Some(member) = table.member(&owner).cloned() else {
                continue;
            };
            let (service, request) = (self.service.clone(), Request::LossRegister(record.clone()));
            tokio::spawn(async move {
                let _ = service.call_member(&member, request).await;
            });
        }
    }

    // ---- Eventos ----

    /// Processes the database events, always the oldest, until empty. With `cutover`, when the
    /// database empties the node switches to normal mode under the same lock as the reception.
    async fn replay(&self, log: &SyncLog, cutover: bool) -> Result<(), SyncError> {
        loop {
            self.check()?;
            let next = {
                let mut guard = lock(&self.recording);
                let next = log.oldest().map_err(|e| disk(&e))?;
                if next.is_none() && cutover {
                    *guard = None;
                }
                next
            };
            let Some(event) = next else {
                return Ok(());
            };
            self.apply_event(&event).await;
            log.delete(event.seq).map_err(|e| disk(&e))?;
            self.service
                .telemetry()
                .registry
                .add(&SYNC_LOG_EVENTS, &[], -1.0);
        }
    }

    async fn apply_event(&self, event: &StoredEvent) {
        let request = match Request::from_http(
            &event.method,
            &event.path,
            &event.headers,
            &event.body,
        ) {
            Ok(request) => request,
            Err(e) => {
                tracing::warn!(path = %event.path, error = %e.message, "unreadable event discarded");
                return;
            }
        };
        // The event beats the load and is idempotent: an error (the message is no longer here, the
        // definition already exists) changes nothing.
        let result = match request {
            Request::DefinitionApply(change) => {
                self.definitions.handle_apply(change).await.map(|_| ())
            }
            other => self.replica.replay(other).await,
        };
        if let Err(e) = result {
            tracing::debug!(path = %event.path, code = %e.code, "synchronization event with no effect");
        }
    }

    // ---- Final check ----

    /// Repeats the listings only for what the node means to keep and discards what the load brought and
    /// no longer shows up on any source (doc 05, section 7.3, step 5.5).
    async fn verify(
        &self,
        ring: &Ring,
        blocks: &[BlockId],
        log: &SyncLog,
        pulled: &Pulled,
        sources: &[Member],
    ) -> Result<(), SyncError> {
        let all = lock(&self.pulled).clone();
        let pulled = Pulled {
            topics: pulled.topics.union(&all.topics).cloned().collect(),
            tokens: pulled.tokens.union(&all.tokens).cloned().collect(),
        };
        for source in sources {
            if let Ok(dropped) = self.definitions.verify(source, &pulled).await {
                if dropped > 0 {
                    tracing::info!(dropped, "definitions removed in the final check");
                }
                break;
            }
        }
        for block in blocks {
            self.check()?;
            let text = block.to_string();
            let loaded = log.loaded_in(&text).map_err(|e| disk(&e))?;
            if loaded.is_empty() {
                continue;
            }
            let block_sources = self.sources_of(ring, *block);
            if block_sources.is_empty() {
                continue;
            }
            let mut ids = MergedIds::new(self, text.clone(), &block_sources);
            let mut listed: HashSet<String> = HashSet::new();
            loop {
                let batch = ids.next_batch(self.rules.max_items).await?;
                if batch.is_empty() {
                    break;
                }
                listed.extend(batch.into_iter().map(|m| m.line.id));
            }
            if ids.degraded() {
                // A source did not answer: the union is incomplete and does not prove that something vanished.
                tracing::warn!(block = %text, "block check postponed: a source did not answer");
                continue;
            }
            for (topic, id) in loaded {
                if listed.contains(&id) {
                    continue;
                }
                let messages = self.messages.clone();
                let (t, i) = (topic.clone(), id.clone());
                let _ = tokio::task::spawn_blocking(move || messages.apply_ack(&t, &i)).await;
                tracing::info!(topic = %topic, id = %id, "message acknowledged during the load discarded in the final check");
            }
        }
        Ok(())
    }

    // ---- Cleanup of the blocks the node lost ----

    /// The cleanup of the nodes that lost the block (doc 05, section 7.3, step 7): only with the whole cluster
    /// healthy (no node syncing, no pending member change), and once per
    /// table version.
    pub async fn clean_once(&self) {
        if self.service.node_state() != NodeState::Active {
            return;
        }
        let Some(table) = self.service.membership().table() else {
            return;
        };
        // Whoever left the table (drain) keeps what it has: it is what the loss recovery looks for on
        // returning (doc 04, section 6.6), and no block is its own any more to clean.
        if table.member(&self.service.node_id()).is_none() {
            return;
        }
        if table.members.iter().any(|m| m.state != MemberState::Active)
            || self.service.membership().has_pending(self.service.now())
        {
            return;
        }
        if *lock(&self.cleaned_version) == Some(table.version) {
            return;
        }
        let ring = self.service.membership().ring();
        let me = self.service.node_id();
        let factor = self.service.rules().replication_factor.max(1) as usize;
        let messages = self.messages.clone();
        let removed = tokio::task::spawn_blocking(move || {
            messages
                .forget_foreign(&|position| ring.owners(position, factor).contains(&me.as_str()))
        })
        .await
        .unwrap_or(0);
        *lock(&self.cleaned_version) = Some(table.version);
        if removed > 0 {
            tracing::info!(
                removed,
                "copies of blocks that stopped being the node's deleted"
            );
        }
    }

    /// The cleanup task, until the node stops.
    pub async fn run_cleanup(self: Arc<Self>) {
        let cancel = self.service.cancel_token().clone();
        let mut ticker = tokio::time::interval(self.service.rules().keepalive_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => self.clean_once().await,
            }
        }
    }
}
