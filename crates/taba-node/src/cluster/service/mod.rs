//! The node inside the cluster: serves the calls of other nodes (`Handler`), coordinates the join of
//! a new node (doc 04, section 6.2) and joins an existing cluster (the bootstrap).
//!
//! Node register and deregister use the two-phase protocol and require the confirmation of
//! **all** nodes, with no reduced quorum. Before the commit the operation can be aborted (rollback);
//! once decided it does not go back, and the coordinator insists up to the `write.commit.timeout` ceiling.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::backup::{BackupPort, StartupDecision, not_configured};
use super::members::{Change, Member, MemberState, MemberTable, Membership, MembershipError};
use super::protocol::{
    Ack, DefinitionApply, JoinRequest, NodeRegisterRequest, NodeUnregisterRequest, OperationRef,
    Request, Response, UnregisterReason,
};
use super::replication::Replication;
use super::restore;
use super::state::{NodeShared, NodeState};
use super::sync::SyncError;
use super::transport::{
    BoxFuture, CallError, ChannelLink, ChannelOpen, Handler, PeerAddr, Transport,
};
use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id;
use crate::telemetry::metrics::{CLUSTER_BOOTSTRAP_FAILURES, CLUSTER_MEMBERSHIP_CHANGES};
use crate::telemetry::{Telemetry, trace};

/// The deadline of a call to another node. Not configurable for now (D93).
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The `join` deadline: the request waits its turn and coordinates two phases, so it takes longer.
const JOIN_TIMEOUT: Duration = Duration::from_secs(60);
/// How often the `last_seen` of the manifest of the running life moves (doc 37, section 4.5).
const MANIFEST_INTERVAL: Duration = Duration::from_secs(10);
/// Seconds suggested in `Retry-After` when the node does not have the table yet.
const SYNCING_RETRY_AFTER_SECS: u64 = 5;

/// The parameters of the join protocol (`write.*`, `cluster.*`).
#[derive(Debug, Clone)]
pub struct ClusterRules {
    /// `write.retry.attempts`
    pub retry_attempts: u32,
    /// `write.retry.backoff`
    pub retry_backoff: Duration,
    /// `write.commit.timeout`
    pub commit_timeout: Duration,
    /// `cluster.bootstrap.retry.interval`
    pub bootstrap_retry_interval: Duration,
    /// `cluster.bootstrap.retry.max-attempts`; `0` does not limit.
    pub bootstrap_max_attempts: u32,
    /// `cluster.first-node`
    pub first_node: bool,
    /// `cluster.entry-address`
    pub entry: Option<PeerAddr>,
    /// `write.pending.ttl`
    pub pending_ttl: Duration,
    /// `ring.virtual-nodes`
    pub virtual_nodes: u32,
    /// `replication.factor`
    pub replication_factor: u32,
    /// `write.quorum`
    pub quorum: u32,
    /// `topics.volatile.idle-check.retry-interval` (provisional, D114: 30 s): the manager's wait
    /// when some node did not answer the idleness query.
    pub volatile_retry_interval: Duration,
    /// `topics.volatile.takeover.jitter` (provisional, D114: 2 s): the window over which the manager that
    /// takes over a queue spreads the first query.
    pub volatile_jitter: Duration,
    /// `delivery.get.max-attempts` (provisional, D114: 3): how many invisibility registers that
    /// do not close a `GET` tolerates, trying the next ID on the list, before stopping.
    pub get_max_attempts: usize,
    /// `delivery.take.max-attempts` (to be defined in doc 10: 3): how many nodes a `GET` with an empty
    /// local list queries at most.
    pub take_max_attempts: usize,
    /// `cluster.notify.window`: how long the party responsible for notices accumulates ready messages.
    pub notify_window: Duration,
    /// `cluster.notify.wait` (to be defined: 50 ms): how long it waits for the notified node to come fetch (`take`).
    pub notify_wait: Duration,
    /// `cluster.channel.keepalive.interval` (to be defined: 2 s): how often the node repeats
    /// the state signals, in place of the channel's heartbeat.
    pub keepalive_interval: Duration,
    pub call_timeout: Duration,
    pub join_timeout: Duration,
    /// `cluster.channel.reconnect.retries`: consecutive calls to the channel that fail before the node
    /// opens the exclusion vote on the peer.
    pub reconnect_retries: u32,
    /// `cluster.channel.reconnect.backoff`.
    pub reconnect_backoff: Duration,
    /// `cluster.channel.queue.max-size`.
    pub queue_max_size: usize,
    /// `cluster.channel.queue.max-age`.
    pub queue_max_age: Duration,
    /// `cluster.channel.keepalive.max-topics`.
    pub keepalive_max_topics: usize,
    /// `cluster.vote.quorum-percent`.
    pub vote_quorum_percent: u32,
    /// `cluster.vote.min-voters`.
    pub vote_min_voters: u32,
    /// `cluster.freezing.duration`.
    pub freezing_duration: Duration,
    /// `cluster.message-loss.ttl` (to be defined: 14 days, the message's default `ttl`).
    pub message_loss_ttl: Duration,
    /// `cluster.recovery.timeout` (to be defined: 5 minutes).
    pub recovery_timeout: Duration,
    /// `cluster.reunion.interval` (to be defined: ten times the heartbeat, at least 1 s): how often
    /// the node checks whether the other side of a symmetric partition has come back (D101).
    pub reunion_interval: Duration,
    /// `fifo.forward.max-attempts` (to be defined: 3).
    pub fifo_forward_max_attempts: u32,
    /// The version of this node's program, which the others read in its `NodeStatus` (D165): the build's
    /// (`CARGO_PKG_VERSION`), unless whoever assembles the node hands in another (the tests).
    pub program_version: String,
    /// How often the node checks whether there are members of an earlier version to retire (D165): the same
    /// interval as the reunion of partitions.
    pub upgrade_interval: Duration,
    /// How long a drain asked for by the upgrade may take before the upgrade stops on it: `drain.timeout` plus
    /// the time the node takes to leave the table.
    pub upgrade_drain_deadline: Duration,
}

impl ClusterRules {
    pub fn from_config(config: &Config) -> Self {
        let keepalive_interval = config
            .cluster
            .keepalive_interval
            .unwrap_or(Duration::from_secs(2));
        Self {
            retry_attempts: config.write.retry_attempts,
            retry_backoff: config.write.retry_backoff,
            commit_timeout: config.write.commit_timeout,
            bootstrap_retry_interval: config.cluster.bootstrap_retry_interval,
            bootstrap_max_attempts: config.cluster.bootstrap_retry_max_attempts,
            first_node: config.cluster.first_node,
            entry: config
                .cluster
                .entry_address
                .as_deref()
                .and_then(|a| a.parse().ok()),
            pending_ttl: config.write.pending_ttl,
            virtual_nodes: config.cluster.virtual_nodes,
            replication_factor: config.write.replication_factor,
            quorum: config.write.quorum,
            volatile_retry_interval: config
                .topics
                .volatile_idle_check_retry_interval
                .unwrap_or(Duration::from_secs(30)),
            volatile_jitter: config
                .topics
                .volatile_takeover_jitter
                .unwrap_or(Duration::from_secs(2)),
            get_max_attempts: config
                .delivery
                .get_max_attempts
                .map_or(3, |n| n.max(1) as usize),
            take_max_attempts: config.delivery.take_max_attempts.map_or(3, |n| n as usize),
            notify_window: config.cluster.notify_window,
            notify_wait: config
                .cluster
                .notify_wait
                .unwrap_or(Duration::from_millis(50)),
            keepalive_interval,
            call_timeout: CALL_TIMEOUT,
            join_timeout: JOIN_TIMEOUT,
            reconnect_retries: config.cluster.reconnect_retries.map_or(3, |n| n.max(1)),
            reconnect_backoff: config
                .cluster
                .reconnect_backoff
                .unwrap_or(Duration::from_secs(1)),
            queue_max_size: config.cluster.queue_max_size.map_or(10_000, |n| n as usize),
            queue_max_age: config
                .cluster
                .queue_max_age
                .unwrap_or(Duration::from_secs(60)),
            keepalive_max_topics: config
                .cluster
                .keepalive_max_topics
                .map_or(1000, |n| n as usize),
            vote_quorum_percent: config.cluster.vote_quorum_percent,
            vote_min_voters: config.cluster.vote_min_voters,
            freezing_duration: config
                .cluster
                .freezing_duration
                .unwrap_or(Duration::from_secs(300)),
            message_loss_ttl: config
                .cluster
                .message_loss_ttl
                .unwrap_or(Duration::from_secs(1_209_600)),
            recovery_timeout: config
                .cluster
                .recovery_timeout
                .unwrap_or(Duration::from_secs(300)),
            reunion_interval: config
                .cluster
                .reunion_interval
                .unwrap_or_else(|| (keepalive_interval * 10).max(Duration::from_secs(1))),
            fifo_forward_max_attempts: config.fifo.forward_max_attempts.map_or(3, |n| n.max(1)),
            program_version: env!("CARGO_PKG_VERSION").to_owned(),
            upgrade_interval: config
                .cluster
                .reunion_interval
                .unwrap_or_else(|| (keepalive_interval * 10).max(Duration::from_secs(1))),
            upgrade_drain_deadline: config.drain.timeout + Duration::from_secs(30),
        }
    }
}

/// The bootstrap ran out of attempts (`cluster.bootstrap.retry.max-attempts`) or the node stopped.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("the node could not join the cluster in {attempts} attempts: {last}")]
    Exhausted { attempts: u32, last: String },
    #[error("the node stopped before joining the cluster")]
    Cancelled,
}

/// A bootstrap attempt that did not work out.
struct Failure {
    reason: &'static str,
    message: String,
}

pub struct ClusterService {
    local: RwLock<Member>,
    membership: Membership,
    node: Arc<NodeShared>,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<Telemetry>,
    rules: ClusterRules,
    /// One member change at a time on this node (doc 04, section 6.8): the `join` waits its turn.
    change_turn: Mutex<()>,
    cancel: CancellationToken,
    /// The replication of definitions and messages, attached after the node starts.
    replication: OnceLock<Arc<Replication>>,
    /// Called when the member table changes (the engine re-evaluates who the owner is).
    on_change: OnceLock<Arc<dyn Fn() + Send + Sync>>,
    /// The exclusion votes in progress (candidate → when it opened), on this node and on those it
    /// helped vote: while there is one, no node join takes effect (doc 04, 6.8).
    open_votes: StdMutex<HashMap<String, Instant>>,
    /// The node is redoing the join because it was excluded.
    excluded: AtomicBool,
    /// The process has already queried the other nodes' loss list (D111): only the first listing of the
    /// bootstrap does this, and a node returning from an exclusion or from a drain has already done it.
    boot_recovered: AtomicBool,
    /// The task that keeps the manifest of the running life up to date is already running.
    manifest_writer: AtomicBool,
    /// The members the restart's listing brought, to whom the node returns what the previous life's
    /// disk has of the losses, once active (PRB-048).
    old_disk_peers: StdMutex<Vec<Member>>,
    /// Who builds this node's `NodeStatus` (the operations layer; `GET /cluster/status`).
    status_source: OnceLock<StatusSource>,
    /// Who runs a call of the operations API against this node (`Request::NodeCall`).
    node_calls: OnceLock<crate::cluster::replication::ForwardExecutor>,
    /// Who reads what this node sees of its topics (`Request::TopicStats`, `Request::TopicGroups`).
    reads: OnceLock<ReadSource>,
    /// The optional S3 backup (doc 23): its service, by weak reference (the service keeps the
    /// cluster, and the cluster cannot hold it back).
    backup: OnceLock<std::sync::Weak<dyn BackupPort>>,
    /// The versions of the other members and the upgrade in progress (doc 37, section 4.12).
    upgrade: super::upgrade::Upgrade,
}

/// The nodes that left by drain, as `Request::Departed` answers (and as `GET /nodes` reads them).
pub fn departed_json(members: &[Member]) -> serde_json::Value {
    serde_json::Value::Array(
        members
            .iter()
            .map(|m| serde_json::json!({ "nodeId": m.node_id, "addresses": m.addresses, "port": m.port }))
            .collect(),
    )
}

/// Answers a read of what this node sees (`Request::TopicStats`, `Request::TopicGroups`), as JSON.
pub type ReadSource = Arc<dyn Fn(&Request) -> Result<serde_json::Value, TabaError> + Send + Sync>;

/// Returns this node's `NodeStatus` (doc 06b), as JSON.
pub type StatusSource = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;

/// The node's `Handler`: what the `/cluster/*` routes and the in-memory network call.
pub struct ClusterHandler(pub Arc<ClusterService>);

impl Handler for ClusterHandler {
    fn handle<'a>(
        &'a self,
        caller: &'a str,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, TabaError>> {
        Box::pin(async move { self.0.handle(caller, request).await })
    }

    fn admit_channel(&self, caller: &str, open: &ChannelOpen) -> Result<(), TabaError> {
        self.0.admit_channel(caller, open)
    }

    fn accept_channel(&self, open: ChannelOpen, link: ChannelLink) {
        self.0.accept_channel(open, link);
    }
}

/// The wait between two deregister attempts when another member change is in progress:
/// 100 to 200 ms. With a fixed deadline, two nodes leaving together repeated the register at the same instant,
/// one refused the other again and they only stopped at `drain.timeout`; the draw breaks the rhythm (the
/// deterministically fair order between different changes is D94).
pub(crate) fn conflict_wait() -> Duration {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the system has no source of random numbers");
    Duration::from_millis(100 + u64::from_le_bytes(bytes) % 100)
}

/// In the symmetric partition, the side with fewer members loses (redoes the join); on a tie, the one with the
/// lowest `nodeId`. Both sides reach the same conclusion (D101).
fn loses_reunion(mine: &MemberTable, theirs: &MemberTable) -> bool {
    let smallest = |t: &MemberTable| t.members.iter().map(|m| m.node_id.clone()).min();
    (mine.members.len(), smallest(mine)) < (theirs.members.len(), smallest(theirs))
}

/// The `operationId` of a phase 1, so whoever coordinates it can check whether the change is still its own.
fn request_operation(request: &Request) -> Option<&str> {
    match request {
        Request::Register(r) => Some(&r.operation_id),
        Request::Unregister(r) => Some(&r.operation_id),
        _ => None,
    }
}

fn node_syncing() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterNodeSyncing,
        "The node does not have the member table yet.",
    )
    .with_retry_after(SYNCING_RETRY_AFTER_SECS)
}

fn from_membership(error: MembershipError) -> TabaError {
    match error {
        MembershipError::NoTable => node_syncing(),
        MembershipError::Conflict(message) => TabaError::new(ErrorCode::ClusterConflict, message),
        MembershipError::UnknownOperation => TabaError::new(
            ErrorCode::ClusterOperationUnknown,
            "The node does not know that operation (it never arrived or already expired).",
        ),
    }
}

fn random_below(n: usize) -> usize {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the system has no source of random numbers");
    (u64::from_le_bytes(bytes) % n.max(1) as u64) as usize
}

impl ClusterService {
    pub fn new(
        local: Member,
        node: Arc<NodeShared>,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn Clock>,
        telemetry: Arc<Telemetry>,
        rules: ClusterRules,
        cancel: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            local: RwLock::new(local),
            membership: Membership::new(rules.pending_ttl, rules.virtual_nodes),
            node,
            transport,
            clock,
            telemetry,
            rules,
            change_turn: Mutex::new(()),
            cancel,
            replication: OnceLock::new(),
            on_change: OnceLock::new(),
            open_votes: StdMutex::new(HashMap::new()),
            excluded: AtomicBool::new(false),
            boot_recovered: AtomicBool::new(false),
            manifest_writer: AtomicBool::new(false),
            old_disk_peers: StdMutex::new(Vec::new()),
            status_source: OnceLock::new(),
            node_calls: OnceLock::new(),
            reads: OnceLock::new(),
            backup: OnceLock::new(),
            upgrade: super::upgrade::Upgrade::default(),
        })
    }

    pub(crate) fn upgrade(&self) -> &super::upgrade::Upgrade {
        &self.upgrade
    }

    pub(crate) fn node_shared(&self) -> &Arc<NodeShared> {
        &self.node
    }

    /// Attaches the S3 backup. Only the first call counts.
    pub fn set_backup(&self, port: &Arc<dyn BackupPort>) {
        let _ = self.backup.set(Arc::downgrade(port));
    }

    /// The S3 backup, if the node has it.
    pub fn backup(&self) -> Option<Arc<dyn BackupPort>> {
        self.backup.get().and_then(std::sync::Weak::upgrade)
    }

    /// Attaches whoever runs the operations API calls other nodes send (`/nodes/{nodeId}/...`).
    pub fn set_node_calls(&self, executor: crate::cluster::replication::ForwardExecutor) {
        let _ = self.node_calls.set(executor);
    }

    /// Attaches whoever reads the node's topic statistics for the other nodes.
    pub fn set_read_source(&self, source: ReadSource) {
        let _ = self.reads.set(source);
    }

    /// Attaches whoever builds the `NodeStatus`, so `GET /cluster/status` can answer.
    pub fn set_status_source(&self, source: StatusSource) {
        let _ = self.status_source.set(source);
    }

    /// Attaches the replication (definitions, messages, signals and forward) to the service.
    pub fn set_replication(&self, replication: Arc<Replication>) {
        let _ = self.replication.set(replication);
    }

    pub fn replication(&self) -> Option<&Arc<Replication>> {
        self.replication.get()
    }

    /// Registers who is warned when the member table changes.
    pub fn set_on_change(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.on_change.set(hook);
    }

    fn membership_changed(&self) {
        // The vote on a node that left the table is over: if it comes back with the same ID, the old
        // mark does not apply again (and does not block joins until `write.pending.ttl`).
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|candidate, _| self.membership.is_member(candidate));
        if let Some(hook) = self.on_change.get() {
            hook();
        }
        self.write_manifest();
    }

    /// Writes the manifest of the running life in the root of the disk (doc 37, section 4.5): the ID, the active
    /// members of the table with their IPs, the version of the table and the instant it was last seen active. A
    /// restart moves it, with the topics, to the folder of the life. Only while the node is a member: a table
    /// that does not have it is not the one this life lived in.
    pub fn write_manifest(&self) {
        let Some(replication) = self.replication.get() else {
            return;
        };
        let me = self.node_id();
        let Some(table) = self.membership.table() else {
            return;
        };
        if !table
            .members
            .iter()
            .any(|m| m.node_id == me && m.state == MemberState::Active)
        {
            return;
        }
        let manifest = restore::Manifest {
            node_id: me,
            members: table
                .members
                .iter()
                .filter(|m| m.state == MemberState::Active)
                .map(|m| restore::ManifestMember {
                    node_id: m.node_id.clone(),
                    addresses: m.addresses.clone(),
                    port: m.port,
                })
                .collect(),
            table_version: table.version,
            last_seen_ms: self.clock.now_millis(),
            watermark: None,
        };
        if let Err(e) = manifest.write(replication.messages.messages().storage_root()) {
            tracing::warn!(error = %e, "could not write the manifest of the life");
        }
    }

    /// Keeps `last_seen` of the manifest moving while the node lives, and writes it once more on a clean
    /// stop (the cancellation). Started once, when the node becomes active.
    fn spawn_manifest_writer(self: &Arc<Self>) {
        if self.manifest_writer.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = me.cancel.cancelled() => break,
                    () = tokio::time::sleep(MANIFEST_INTERVAL) => me.write_manifest(),
                }
            }
            me.write_manifest();
        });
    }

    /// After the commit of an exclusion by vote, queues the `frozen` frame for all peers:
    /// the member that was down at the commit receives it when the channel comes back (doc 04, section 6.7).
    fn announce_frozen(&self, change: Option<&Change>) {
        let Some(Change::Exclude { node_id }) = change else {
            return;
        };
        let Some(frozen_at) = self.membership.frozen_at(node_id) else {
            return;
        };
        if let Some(replication) = self.replication.get() {
            replication.signals.announce_frozen(node_id, frozen_at);
        }
    }

    /// After the commit of a member change, queues the `membership` frame for all
    /// peers: the member that lost the register or the commit applies it on receipt (D126).
    fn announce_membership(&self, operation_id: &str, version: u64, change: &Change) {
        if let Some(replication) = self.replication.get() {
            replication
                .signals
                .announce_membership(operation_id, version, change);
        }
    }

    /// A member change already effected that a peer said through the channel (D126): this node lost its
    /// register or commit and applies it now, in version order.
    pub(crate) fn learn_membership(&self, operation_id: &str, version: u64, change: Change) {
        let me = self.node_id();
        if let Change::Remove { node_id } | Change::Exclude { node_id } = &change
            && *node_id == me
        {
            // This node's own exit does not apply to its table: the exclusion rule handles that.
            return;
        }
        let applied = self
            .membership
            .learn(operation_id, version, change, self.now());
        self.after_learning(applied);
    }

    /// A definition a peer said through the channel (D127): applies in the background, by the rule of
    /// `definition-apply`.
    pub(crate) fn learn_definition(&self, change: DefinitionApply) {
        let Some(replication) = self.replication.get().cloned() else {
            return;
        };
        tokio::spawn(async move { replication.definitions.learn(change).await });
    }

    /// Applies the stored changes that became the next ones (the normal commit may have
    /// advanced the version while they waited).
    fn settle_membership(&self) {
        let applied = self.membership.learn_held(self.now());
        self.after_learning(applied);
    }

    fn after_learning(&self, applied: Vec<(String, u64, Change)>) {
        if applied.is_empty() {
            return;
        }
        for (operation_id, version, change) in &applied {
            self.count_change(Some(change), "channel", "ok");
            tracing::info!(%operation_id, version, ?change, "member change learned through the channel");
            self.announce_membership(operation_id, *version, change);
        }
        self.membership_changed();
    }

    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub fn telemetry(&self) -> &Arc<Telemetry> {
        &self.telemetry
    }

    /// The nodes responsible for the position, in order (primary first): `replication.factor` distinct
    /// nodes, or fewer if the cluster has fewer. `503` if the node does not have the table yet.
    pub fn owners_of(&self, position: u64) -> Result<Vec<Member>, TabaError> {
        let table = self.membership.table().ok_or_else(node_syncing)?;
        let ring = self.membership.ring();
        let wanted = self.rules.replication_factor.max(1) as usize;
        Ok(ring
            .owners(position, wanted)
            .into_iter()
            .filter_map(|id| table.member(id).cloned())
            .collect())
    }

    /// The IP this node presents itself with (a local call goes through the same
    /// member check as one that came from the network).
    pub fn self_caller(&self) -> String {
        self.local().addresses.first().cloned().unwrap_or_default()
    }

    pub fn is_self(&self, member: &Member) -> bool {
        member.node_id == self.node_id()
    }

    /// Makes the call to a member: to itself without the network, to the others through the `Transport`.
    pub(crate) async fn call_member(
        self: &Arc<Self>,
        member: &Member,
        request: Request,
    ) -> Result<Response, CallError> {
        if self.is_self(member) {
            return self
                .handle(&self.self_caller(), request)
                .await
                .map_err(CallError::Remote);
        }
        self.call_peer(member, request, self.rules.call_timeout)
            .await
    }

    /// `call_member` with the attempts and interval of `write.retry.*` for a network failure and
    /// `5xx` error from the peer.
    pub(crate) async fn call_member_retrying(
        self: &Arc<Self>,
        member: &Member,
        request: Request,
    ) -> Result<Response, CallError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            crate::telemetry::trace::attempt(attempt);
            let result = self.call_member(member, request.clone()).await;
            let retry = match &result {
                Err(e) if e.is_network() => true,
                Err(CallError::Remote(e)) => e.status() >= 500,
                _ => false,
            };
            if !retry || attempt >= self.rules.retry_attempts {
                return result;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return result,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    pub fn local(&self) -> Member {
        self.local.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn node_id(&self) -> String {
        self.local
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .node_id
            .clone()
    }

    /// Notes that there is an exclusion vote on the node in progress.
    pub(crate) fn mark_vote(&self, candidate: &str) {
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(candidate.to_owned(), Instant::now());
    }

    pub(crate) fn clear_vote(&self, candidate: &str) {
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(candidate);
    }

    /// The candidates of votes in progress. One past `write.pending.ttl` is forgotten, and so is
    /// that of a node that already left the table.
    pub fn open_votes(&self) -> Vec<String> {
        let mut votes = self.open_votes.lock().unwrap_or_else(|e| e.into_inner());
        votes.retain(|candidate, at| {
            at.elapsed() < self.rules.pending_ttl && self.membership.is_member(candidate)
        });
        votes.keys().cloned().collect()
    }

    /// Called by the channel when a peer falls or says it failed: opens the exclusion vote.
    pub(crate) fn on_node_failed_hook(&self, hook: Box<dyn Fn() + Send + Sync>) {
        self.node.on_failed(hook);
    }

    /// The node's drain state.
    pub fn drain_state(&self) -> super::state::DrainState {
        self.node.drain_state()
    }

    /// The node's state in the cluster (`joining`, `syncing`, `active`...).
    pub fn node_state(&self) -> NodeState {
        self.node.state()
    }

    pub fn membership(&self) -> &Membership {
        &self.membership
    }

    pub fn rules(&self) -> &ClusterRules {
        &self.rules
    }

    pub(crate) fn now(&self) -> SystemTime {
        self.clock.now()
    }

    pub(crate) fn new_operation_id(&self) -> String {
        id::new_at(self.clock.now_millis())
    }

    /// Counts a phase of a member change: `register` for a node join, `unregister`
    /// (with `reason`: `vote` or `drain`) for a leave.
    fn count_change(&self, change: Option<&Change>, phase: &str, outcome: &str) {
        let (operation, reason) = match change {
            Some(Change::Remove { .. }) => ("unregister", Some("drain")),
            Some(Change::Exclude { .. }) => ("unregister", Some("vote")),
            _ => ("register", None),
        };
        let mut labels = vec![
            ("taba.operation", operation),
            ("taba.phase", phase),
            ("taba.outcome", outcome),
        ];
        if let Some(reason) = reason {
            labels.push(("taba.reason", reason));
        }
        self.telemetry
            .registry
            .add(&CLUSTER_MEMBERSHIP_CHANGES, &labels, 1.0);
    }

    fn count_bootstrap_failure(&self, reason: &str) {
        self.telemetry
            .registry
            .add(&CLUSTER_BOOTSTRAP_FAILURES, &[("taba.reason", reason)], 1.0);
    }

    // ---- Who serves ----

    /// Serves a call from another node. `caller` is the source IP.
    pub async fn handle(
        self: &Arc<Self>,
        caller: &str,
        request: Request,
    ) -> Result<Response, TabaError> {
        match request {
            Request::ListNodes => self
                .membership
                .table_with_frozen(self.now(), self.rules.freezing_duration)
                .map(Response::Members)
                .ok_or_else(node_syncing),
            Request::Join(request) => self.join(request).await,
            Request::Register(request) => {
                request.validate()?;
                self.require_member(caller)?;
                self.refuse_while_busy(None)?;
                let id = request.operation_id.clone();
                let change = Change::Add(request.node);
                let result = self.membership.register(&id, change.clone(), self.now());
                self.count_change(
                    Some(&change),
                    "register",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                result.map_err(from_membership)?;
                Ok(ack(id))
            }
            Request::Unregister(request) => {
                request.validate()?;
                self.require_member(caller)?;
                // The exit of the syncing node itself (it failed) is the exception (doc 04, 6.8).
                self.refuse_while_busy_unregistering(Some(&request.node_id))?;
                let id = request.operation_id.clone();
                let change = match request.reason {
                    UnregisterReason::Vote => Change::Exclude {
                        node_id: request.node_id,
                    },
                    UnregisterReason::Drain => Change::Remove {
                        node_id: request.node_id,
                    },
                };
                let result = self.membership.register(&id, change.clone(), self.now());
                self.count_change(
                    Some(&change),
                    "register",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                result.map_err(from_membership)?;
                Ok(ack(id))
            }
            Request::Commit(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let change = self.membership.pending_change(&request.operation_id);
                let result = self.membership.commit(&request.operation_id, self.now());
                self.count_change(
                    change.as_ref(),
                    "commit",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                let table = result.map_err(from_membership)?;
                self.membership_changed();
                self.announce_frozen(change.as_ref());
                if let Some(change) = change.as_ref() {
                    self.announce_membership(&request.operation_id, table.version, change);
                }
                self.settle_membership();
                Ok(ack(request.operation_id))
            }
            Request::Rollback(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let change = self.membership.pending_change(&request.operation_id);
                let result = self.membership.rollback(&request.operation_id, self.now());
                self.count_change(
                    change.as_ref(),
                    "rollback",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                result.map_err(from_membership)?;
                Ok(ack(request.operation_id))
            }
            Request::Status => {
                self.require_member(caller)?;
                let source = self
                    .status_source
                    .get()
                    .ok_or_else(TabaError::route_not_found)?;
                Ok(Response::Status(source()))
            }
            read @ (Request::TopicStats | Request::TopicGroups { .. }) => {
                self.require_member(caller)?;
                let source = self.reads.get().ok_or_else(TabaError::route_not_found)?;
                source(&read).map(Response::Json)
            }
            Request::Departed => {
                self.require_member(caller)?;
                Ok(Response::Json(departed_json(
                    &self.membership.departed_members(),
                )))
            }
            Request::BackupStatus => {
                self.require_member(caller)?;
                let port = self.backup().ok_or_else(not_configured)?;
                Ok(Response::Backup(port.backup().status().to_json()))
            }
            Request::NodeCall(call) => {
                self.require_member(caller)?;
                let executor = self
                    .node_calls
                    .get()
                    .ok_or_else(TabaError::route_not_found)?;
                executor(call).await.map(Response::Forwarded)
            }
            Request::BackupFlush => {
                self.require_member(caller)?;
                let port = self.backup().ok_or_else(not_configured)?;
                Ok(Response::Backup(port.backup().flush().await.to_json()))
            }
            Request::BackupStop => {
                self.require_member(caller)?;
                let port = self.backup().ok_or_else(not_configured)?;
                port.stop_node().await.map(Response::Backup)
            }
            Request::BackupSaved(saved) => {
                self.require_member(caller)?;
                saved.validate()?;
                let port = self.backup().ok_or_else(not_configured)?;
                port.backup().accept_saved(&saved);
                Ok(ack(String::new()))
            }
            // The forward of a client call also applies to whoever left by drain (doc 04,
            // section 6.9); whoever receives it never forwards again.
            Request::Forward(forward) => {
                let allowed = self.membership.is_member_address(caller)
                    || self.membership.is_departed_address(caller);
                if !allowed {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNotMember,
                        "The caller is not in the member table.",
                    ));
                }
                if self.node.drain_state() != super::state::DrainState::Active {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNodeDraining,
                        "The node is in drain and does not serve the forward.",
                    )
                    .with_retry_after(SYNCING_RETRY_AFTER_SECS));
                }
                if self.node.state() != NodeState::Active {
                    return Err(node_syncing());
                }
                let Some(replication) = self.replication.get().cloned() else {
                    return Err(TabaError::route_not_found());
                };
                replication.forwarder.execute(forward).await
            }
            // Definitions, messages, signals: the replication serves.
            other => {
                let Some(replication) = self.replication.get().cloned() else {
                    return Err(TabaError::route_not_found());
                };
                // The loss list and the recovered messages accept whoever was just excluded
                // (doc 04, section 6.6): the returning node is not in the table yet.
                if !matches!(other, Request::MessageLoss(_) | Request::Recover { .. }) {
                    self.require_member(caller)?;
                }
                replication.handle(caller, other).await
            }
        }
    }

    /// One member change at a time, until the node that joined becomes ready (doc 04, section 6.8): with
    /// a node syncing, another change's register is a conflict, and whoever coordinates waits.
    /// `except` is the node whose exit is allowed anyway.
    fn refuse_while_busy_unregistering(&self, except: Option<&str>) -> Result<(), TabaError> {
        self.refuse_while_syncing(except)
    }

    /// A join's register also waits for the exclusion votes in progress.
    fn refuse_while_busy(&self, except: Option<&str>) -> Result<(), TabaError> {
        self.refuse_while_syncing(except)?;
        if self.open_votes().is_empty() {
            Ok(())
        } else {
            Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "There is an exclusion vote in progress: no node join takes effect until it ends.",
            ))
        }
    }

    fn refuse_while_syncing(&self, except: Option<&str>) -> Result<(), TabaError> {
        let busy = self
            .membership
            .syncing()
            .into_iter()
            .any(|node| except != Some(node.as_str()));
        if busy {
            Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "There is a node syncing: no other member change takes effect until it becomes ready.",
            ))
        } else {
            Ok(())
        }
    }

    /// A join's coordinator waits for the syncing node to become ready (or leave).
    async fn wait_for_sync_turn(&self, joining: &str) -> Result<(), TabaError> {
        let deadline = tokio::time::Instant::now() + self.rules.join_timeout;
        while self.membership.syncing().iter().any(|n| n != joining)
            || !self.open_votes().is_empty()
        {
            if tokio::time::Instant::now() >= deadline || self.cancel.is_cancelled() {
                return Err(TabaError::new(
                    ErrorCode::ClusterRegistrationFailed,
                    "There is a node syncing and the join request waited out the deadline.",
                )
                .with_retry_after(SYNCING_RETRY_AFTER_SECS));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok(())
    }

    /// Only whoever is in the member table calls the phase routes (doc 06c): a node that does not
    /// have the table yet does not know who is a member and answers `503`.
    pub(crate) fn require_member(&self, caller: &str) -> Result<(), TabaError> {
        if self.membership.table().is_none() {
            return Err(node_syncing());
        }
        if self.membership.is_member_address(caller) {
            Ok(())
        } else {
            Err(TabaError::new(
                ErrorCode::ClusterNotMember,
                "The caller is not in the member table.",
            ))
        }
    }

    // ---- Coordinating a join ----

    /// `POST /cluster/nodes/join`: this node coordinates the new node's register (doc 04, section 6.2,
    /// steps 2 to 5) and only answers with the table after the commit.
    async fn join(self: &Arc<Self>, request: JoinRequest) -> Result<Response, TabaError> {
        // Two joins coordinated by different members cross: the older wins and the
        // other, which undid its register, waits a bit and starts over (D94).
        let give_up_at = tokio::time::Instant::now() + self.rules.join_timeout;
        loop {
            match self.join_once(request.clone()).await {
                Err(error) if error.code == ErrorCode::ClusterConflict => {
                    if tokio::time::Instant::now() >= give_up_at || self.cancel.is_cancelled() {
                        return Err(registration_failed());
                    }
                    tokio::time::sleep(conflict_wait()).await;
                }
                other => return other,
            }
        }
    }

    /// One attempt at the join. `cluster.conflict` is the change of another coordinator that won.
    async fn join_once(self: &Arc<Self>, request: JoinRequest) -> Result<Response, TabaError> {
        let span = trace::operation_span("taba.cluster.join", &request.node_id);
        self.join_once_traced(request).instrument(span).await
    }

    async fn join_once_traced(
        self: &Arc<Self>,
        request: JoinRequest,
    ) -> Result<Response, TabaError> {
        request.validate()?;
        if self.membership.table().is_none() {
            return Err(node_syncing());
        }
        // One member change at a time: another join in progress on this node makes this one wait.
        let _turn = self.change_turn.lock().await;
        let table = self.membership.table().ok_or_else(node_syncing)?;
        // A node excluded by vote does not come back before the end of the freezing (doc 04, section 6.7).
        if let Some(left) = self.membership.frozen_remaining(
            &request.node_id,
            self.now(),
            self.rules.freezing_duration,
        ) {
            tracing::warn!(node_id = %request.node_id, left_secs = left.as_secs(), "ingresso rejeitado (freezing)");
            return Err(TabaError::new(
                ErrorCode::ClusterNodeFrozen,
                "The node was excluded from the cluster and is in a freezing period.",
            )
            .with_retry_after(left.as_secs().max(1)));
        }
        if table.member(&request.node_id).is_some() {
            // The same node repeating the request (the previous one was accepted and the response got lost).
            return Ok(Response::Members(self.table_for_newcomer(table)));
        }
        self.wait_for_sync_turn(&request.node_id).await?;
        let table = self.membership.table().ok_or_else(node_syncing)?;

        self.probe(&request).await?;

        let node = Member {
            node_id: request.node_id.clone(),
            addresses: request.addresses.clone(),
            port: request.port,
            // The new node only becomes active when the synchronization ends (doc 05, section 7.3).
            state: MemberState::Syncing,
        };
        let operation_id = self.new_operation_id();
        let peers: Vec<Member> = table
            .members
            .iter()
            .filter(|m| m.node_id != self.node_id())
            .cloned()
            .collect();

        // Phase 1: the register on all nodes, this one included.
        let change = Change::Add(node.clone());
        if let Err(e) =
            self.membership
                .register_coordinating(&operation_id, change.clone(), self.now())
        {
            self.count_change(Some(&change), "register", "failed");
            tracing::warn!(node_id = %node.node_id, error = ?e, "join refused on this node");
            // Another change already pending here (that of a coordinator that arrived first): the join
            // waits its turn instead of failing (D94).
            let error = from_membership(e);
            return Err(if error.code == ErrorCode::ClusterConflict {
                error
            } else {
                registration_failed()
            });
        }
        let (confirmed, failures) = self
            .register_on_peers(
                &peers,
                Request::Register(NodeRegisterRequest {
                    operation_id: operation_id.clone(),
                    coordinator_id: self.node_id(),
                    node: node.clone(),
                }),
            )
            .await;
        if !failures.is_empty() {
            self.count_change(Some(&change), "register", "failed");
            for (peer, error) in &failures {
                tracing::warn!(node_id = %node.node_id, peer = %peer, %error, "register without confirmation");
            }
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            tracing::error!(
                node_id = %node.node_id,
                "join aborted (rollback): the register phase was not confirmed by all"
            );
            let contended = failures.iter().any(
                |(_, e)| matches!(e, CallError::Remote(r) if r.code == ErrorCode::ClusterConflict),
            );
            return Err(if contended {
                TabaError::new(
                    ErrorCode::ClusterConflict,
                    "There is another member change in progress.",
                )
            } else {
                registration_failed()
            });
        }
        if !self.membership.is_pending(&operation_id) {
            // An older coordinator displaced this register in the middle of phase 1.
            self.count_change(Some(&change), "register", "failed");
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            return Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "Another, older member change took its turn.",
            ));
        }
        self.count_change(Some(&change), "register", "ok");

        // Phase 2: the quorum of all was reached, the decision is made and does not go back.
        let table = self
            .membership
            .commit(&operation_id, self.now())
            .map_err(from_membership)?;
        self.count_change(Some(&change), "commit", "ok");
        self.membership_changed();
        self.announce_membership(&operation_id, table.version, &change);
        self.settle_membership();
        if self.commit_on_peers(&peers, &operation_id).await {
            tracing::info!(
                node_id = %node.node_id,
                version = table.version,
                "new node registered in the cluster"
            );
            Ok(Response::Members(self.table_for_newcomer(table)))
        } else {
            self.count_change(Some(&change), "commit", "timeout");
            tracing::warn!(
                node_id = %node.node_id,
                "the commit goes on in the background: the node was accepted"
            );
            Err(TabaError::new(
                ErrorCode::ClusterCommitTimeout,
                "The register was confirmed and the commit goes on in the background: the node was accepted. \
                 Check GET /cluster/nodes before repeating.",
            ))
        }
    }

    /// The table the new node receives carries the nodes in freezing (D106): whoever joins after the commit of
    /// an exclusion inherits the freezing from whoever received it.
    fn table_for_newcomer(&self, mut table: MemberTable) -> MemberTable {
        if let Some(with_frozen) = self
            .membership
            .table_with_frozen(self.now(), self.rules.freezing_duration)
        {
            table.frozen = with_frozen.frozen;
        }
        table
    }

    /// The connection test (step 2): connects back to the new node. An error response from the node
    /// also proves the connection works (the new node does not have the table yet and answers `503`).
    async fn probe(&self, request: &JoinRequest) -> Result<(), TabaError> {
        for address in &request.addresses {
            let to = PeerAddr::new(address.clone(), request.port);
            match self
                .call_addr(&to, Request::ListNodes, self.rules.call_timeout)
                .await
            {
                Ok(_) | Err(CallError::Remote(_)) => return Ok(()),
                Err(_) => {}
            }
        }
        tracing::warn!(node_id = %request.node_id, "the new node is not reachable at the given IPs");
        Err(TabaError::new(
            ErrorCode::ClusterJoinUnreachable,
            "Could not connect back to the given IPs.",
        ))
    }

    /// Phase 1 on all the other nodes. Returns who confirmed and who failed.
    async fn register_on_peers(
        self: &Arc<Self>,
        peers: &[Member],
        request: Request,
    ) -> (Vec<Member>, Vec<(String, CallError)>) {
        let mut set = JoinSet::new();
        for peer in peers {
            let (me, peer, request) = (self.clone(), peer.clone(), request.clone());
            let span = trace::peer_span("taba.cluster.register", &peer.node_id, false);
            set.spawn(
                async move {
                    // Another pending change on the peer (409) may be transient: tries again.
                    let result = trace::traced(
                        tracing::Span::current(),
                        me.call_retrying(&peer, request, true),
                    )
                    .await;
                    (peer, result)
                }
                .instrument(span),
            );
        }
        let (mut confirmed, mut failures) = (Vec::new(), Vec::new());
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((peer, Ok(_))) => confirmed.push(peer),
                Ok((peer, Err(error))) => failures.push((peer.node_id, error)),
                Err(e) => tracing::error!(error = %e, "register task failed"),
            }
        }
        (confirmed, failures)
    }

    /// Discards the pending register here and on those that had already confirmed it.
    async fn rollback_everywhere(
        self: &Arc<Self>,
        confirmed: &[Member],
        operation_id: &str,
        change: &Change,
    ) {
        let _ = self.membership.rollback(operation_id, self.now());
        self.count_change(Some(change), "rollback", "ok");
        let request = Request::Rollback(OperationRef {
            operation_id: operation_id.to_owned(),
        });
        let mut set = JoinSet::new();
        for peer in confirmed {
            let (me, peer, request) = (self.clone(), peer.clone(), request.clone());
            let span = trace::peer_span("taba.cluster.rollback", &peer.node_id, true);
            set.spawn(
                async move {
                    if let Err(error) = me.call_retrying(&peer, request, false).await {
                        // The pending register expires by itself in `write.pending.ttl`.
                        tracing::warn!(peer = %peer.node_id, %error, "rollback without confirmation");
                    }
                }
                .instrument(span),
            );
        }
        while set.join_next().await.is_some() {}
    }

    /// Phase 2 on all the other nodes. `true` if all confirmed within
    /// `write.commit.timeout`; otherwise the missing ones keep being tried in the background.
    async fn commit_on_peers(self: &Arc<Self>, peers: &[Member], operation_id: &str) -> bool {
        let mut set = JoinSet::new();
        for peer in peers {
            let (me, peer, operation_id) = (self.clone(), peer.clone(), operation_id.to_owned());
            let span = trace::peer_span("taba.cluster.commit", &peer.node_id, false);
            set.spawn(
                async move { me.commit_until_confirmed(peer, operation_id).await }.instrument(span),
            );
        }
        let all = tokio::time::timeout(self.rules.commit_timeout, async {
            while set.join_next().await.is_some() {}
        })
        .await
        .is_ok();
        if !all {
            set.detach_all();
        }
        all
    }

    /// Insists on a peer's commit until it confirms, leaves the table or the node stops: after the
    /// decision there is no rollback (doc 04, section 6.2, step 7).
    async fn commit_until_confirmed(self: Arc<Self>, peer: Member, operation_id: String) {
        let request = Request::Commit(OperationRef { operation_id });
        loop {
            match self
                .call_peer(&peer, request.clone(), self.rules.call_timeout)
                .await
            {
                Ok(_) => return,
                Err(CallError::Remote(error)) if error.status() < 500 => {
                    // The peer no longer has the register (restarted or expired): there is nothing to insist on.
                    tracing::warn!(peer = %peer.node_id, code = %error.code, "commit refused by the peer");
                    return;
                }
                Err(_) => {}
            }
            if !self.membership.is_member(&peer.node_id) {
                return;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    // ---- Calls to other nodes ----

    /// Is the channel `caller` wants to open accepted? The channel says whom it talks to: another process at the same
    /// address does not answer for the old one (D100); and only a member opens a channel.
    pub fn admit_channel(&self, caller: &str, open: &ChannelOpen) -> Result<(), TabaError> {
        if !open.to.is_empty() && open.to != self.node_id() {
            return Err(TabaError::new(
                ErrorCode::ClusterWrongNode,
                "The node at this address is not the one the channel is looking for.",
            ));
        }
        self.require_member(caller)
    }

    /// The channel was opened by another node: it becomes the peer's channel.
    pub fn accept_channel(&self, open: ChannelOpen, link: ChannelLink) {
        if let Some(replication) = self.replication.get() {
            replication.accept_channel(open, link);
        }
    }

    /// Opens the channel with a member, through its IPs in order, until one answers (even with an error).
    pub(crate) async fn open_channel_to(
        &self,
        peer: &Member,
        open: ChannelOpen,
        timeout: Duration,
    ) -> Result<ChannelLink, CallError> {
        let mut last = CallError::Unreachable("the node has no address".into());
        for address in &peer.addresses {
            let to = PeerAddr::new(address.clone(), peer.port);
            match tokio::time::timeout(timeout, self.transport.open_channel(&to, open.clone()))
                .await
            {
                Ok(Err(e)) if e.is_network() => last = e,
                Ok(other) => return other,
                Err(_) => last = CallError::Timeout,
            }
        }
        Err(last)
    }

    /// Calls an address, with a deadline.
    async fn call_addr(
        &self,
        to: &PeerAddr,
        request: Request,
        timeout: Duration,
    ) -> Result<Response, CallError> {
        match tokio::time::timeout(timeout, self.transport.call(to, request)).await {
            Ok(result) => result,
            Err(_) => Err(CallError::Timeout),
        }
    }

    /// Calls a member through its IPs, in order, until one answers (even with an error).
    pub(crate) async fn call_peer(
        &self,
        peer: &Member,
        request: Request,
        timeout: Duration,
    ) -> Result<Response, CallError> {
        let mut last = CallError::Unreachable("the node has no address".into());
        for address in &peer.addresses {
            let to = PeerAddr::new(address.clone(), peer.port);
            match self.call_addr(&to, request.clone(), timeout).await {
                Err(e) if e.is_network() => last = e,
                other => return other,
            }
        }
        Err(last)
    }

    /// `call_peer` with `write.retry.attempts` attempts and `write.retry.backoff` between them, for a
    /// network failure and a `5xx` error from the peer. The `409` conflict (if asked) is repeated for up to
    /// `write.commit.timeout`, while the change remains pending here: the oldest of those that
    /// cross wins, and whoever was displaced gives up right away (D94).
    async fn call_retrying(
        &self,
        peer: &Member,
        request: Request,
        retry_conflict: bool,
    ) -> Result<Response, CallError> {
        let started = tokio::time::Instant::now();
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self
                .call_peer(peer, request.clone(), self.rules.call_timeout)
                .await;
            let (retry, conflict) = match &result {
                Err(e) if e.is_network() => (true, false),
                Err(CallError::Remote(e)) => {
                    let conflict = retry_conflict && e.code == ErrorCode::ClusterConflict;
                    (e.status() >= 500 || conflict, conflict)
                }
                _ => (false, false),
            };
            let exhausted = if conflict {
                started.elapsed() >= self.rules.commit_timeout
                    || request_operation(&request)
                        .is_some_and(|operation| !self.membership.is_pending(operation))
            } else {
                attempt >= self.rules.retry_attempts
            };
            if !retry || exhausted {
                return result;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return result,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    // ---- Joining a cluster ----

    /// The voluntary deregister of the drain (doc 04, section 6.10, step 5): this node coordinates its own
    /// exit through the two-phase protocol, **without a vote**, and waits its turn behind other
    /// member changes. Before the commit, rollback; afterwards, the commit is insisted on until `write.commit.timeout`.
    pub async fn leave(self: &Arc<Self>) -> Result<(), TabaError> {
        let me = self.node_id();
        self.unregister_node(&me, UnregisterReason::Drain).await
    }

    /// The deregister of `target` through the two-phase protocol (doc 04, section 6.3): the node that
    /// leaves (`drain`) or the node excluded by vote (`vote`, which gets freezing). It requires the confirmation
    /// of all the remaining nodes, except those this node sees as down: they are excluded by
    /// a vote of their own, and waiting for them would block the exclusion of one that already fell.
    pub async fn unregister_node(
        self: &Arc<Self>,
        target: &str,
        reason: UnregisterReason,
    ) -> Result<(), TabaError> {
        let span = trace::operation_span("taba.cluster.unregister", target);
        self.unregister_node_traced(target, reason)
            .instrument(span)
            .await
    }

    async fn unregister_node_traced(
        self: &Arc<Self>,
        target: &str,
        reason: UnregisterReason,
    ) -> Result<(), TabaError> {
        let _turn = self.change_turn.lock().await;
        let table = self.membership.table().ok_or_else(node_syncing)?;
        let me = self.node_id();
        if table.member(target).is_none() {
            return Ok(());
        }
        let down: HashSet<String> = match (reason, self.replication.get()) {
            (UnregisterReason::Vote, Some(replication)) => replication
                .channels
                .peers()
                .into_iter()
                .filter(|p| !p.up)
                .map(|p| p.node_id)
                .collect(),
            _ => HashSet::new(),
        };
        let peers: Vec<Member> = table
            .members
            .iter()
            .filter(|m| m.node_id != me && m.node_id != target && !down.contains(&m.node_id))
            .cloned()
            .collect();
        let operation_id = self.new_operation_id();
        let change = match reason {
            UnregisterReason::Drain => Change::Remove {
                node_id: target.to_owned(),
            },
            UnregisterReason::Vote => Change::Exclude {
                node_id: target.to_owned(),
            },
        };
        if let Err(e) =
            self.membership
                .register_coordinating(&operation_id, change.clone(), self.now())
        {
            self.count_change(Some(&change), "register", "failed");
            return Err(from_membership(e));
        }
        let (confirmed, failures) = self
            .register_on_peers(
                &peers,
                Request::Unregister(NodeUnregisterRequest {
                    operation_id: operation_id.clone(),
                    coordinator_id: me.clone(),
                    node_id: target.to_owned(),
                    reason,
                }),
            )
            .await;
        if !failures.is_empty() {
            self.count_change(Some(&change), "register", "failed");
            for (peer, error) in &failures {
                tracing::warn!(peer = %peer, %error, "deregister without confirmation");
            }
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            tracing::error!("deregister aborted (rollback): phase 1 did not close on all");
            // Another pending change on a peer: whoever asks to leave waits its turn and tries again.
            let contended = failures.iter().any(
                |(_, e)| matches!(e, CallError::Remote(r) if r.code == ErrorCode::ClusterConflict),
            );
            return Err(if contended {
                TabaError::new(
                    ErrorCode::ClusterConflict,
                    "There is another member change in progress.",
                )
            } else {
                registration_failed()
            });
        }
        if !self.membership.is_pending(&operation_id) {
            self.count_change(Some(&change), "register", "failed");
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            return Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "Another, older member change took its turn.",
            ));
        }
        self.count_change(Some(&change), "register", "ok");
        let table = self
            .membership
            .commit(&operation_id, self.now())
            .map_err(from_membership)?;
        self.count_change(Some(&change), "commit", "ok");
        self.membership_changed();
        self.announce_frozen(Some(&change));
        self.announce_membership(&operation_id, table.version, &change);
        self.settle_membership();
        if self.commit_on_peers(&peers, &operation_id).await {
            tracing::info!(node_id = %target, ?reason, "node out of the cluster");
            Ok(())
        } else {
            self.count_change(Some(&change), "commit", "timeout");
            tracing::warn!("the deregister commit goes on in the background on some node");
            Err(TabaError::new(
                ErrorCode::ClusterCommitTimeout,
                "The deregister was decided and the commit goes on in the background.",
            ))
        }
    }

    /// The reunion after a partition in which both sides excluded each other (D101): from time to
    /// time the node queries the table of whoever excluded it. If it is from another cluster (no member in
    /// common, and without this node) and this side is the loser (fewer members; tie: the side of the lowest
    /// `nodeId`), the node redoes the join from scratch, like the excluded one that returns.
    pub async fn run_reunion(self: Arc<Self>) {
        let period = self.rules.reunion_interval;
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => return,
                () = tokio::time::sleep(period) => {}
            }
            self.check_reunion().await;
        }
    }

    /// One round of the reunion.
    pub async fn check_reunion(self: &Arc<Self>) {
        if self.node.state() != NodeState::Active
            || self.node.drain_state() != super::state::DrainState::Active
        {
            return;
        }
        for other in self.membership.excluded_members() {
            let Some(mine) = self.membership.table() else {
                return;
            };
            let Ok(Response::Members(theirs)) = self
                .call_peer(&other, Request::ListNodes, self.rules.call_timeout)
                .await
            else {
                continue;
            };
            let separate = !theirs.members.is_empty()
                && theirs.member(&self.node_id()).is_none()
                && theirs
                    .members
                    .iter()
                    .all(|m| mine.member(&m.node_id).is_none());
            if separate && loses_reunion(&mine, &theirs) {
                tracing::warn!(
                    node_id = %self.node_id(),
                    other = %other.node_id,
                    "the two halves of the cluster excluded each other and met again: this side redoes the join"
                );
                self.handle_exclusion().await;
                return;
            }
        }
    }

    /// A peer said several times that this node is not a member. It may be that the join's commit has not
    /// reached it yet (its table is older) or that this node was excluded (its
    /// table is newer and does not have it): only in this case does the node redo the join.
    pub async fn check_exclusion(self: &Arc<Self>, peer_id: &str) {
        let Some(member) = self
            .membership
            .table()
            .and_then(|t| t.member(peer_id).cloned())
        else {
            return;
        };
        let (Some(mine), Ok(Response::Members(theirs))) = (
            self.membership.version(),
            self.call_peer(&member, Request::ListNodes, self.rules.call_timeout)
                .await,
        ) else {
            return;
        };
        if theirs.member(&self.node_id()).is_none() && theirs.version > mine {
            self.handle_exclusion().await;
        }
    }

    /// For the node that is synchronizing: some member of the table it received has a newer table
    /// without it, which means the join it is finishing was undone.
    pub async fn excluded_while_syncing(self: &Arc<Self>) -> bool {
        let me = self.node_id();
        let Some(mine) = self.membership.table() else {
            return false;
        };
        for member in mine.members.iter().filter(|m| m.node_id != me) {
            if let Ok(Response::Members(theirs)) = self
                .call_peer(member, Request::ListNodes, self.rules.call_timeout)
                .await
                && theirs.member(&me).is_none()
                && theirs.version > mine.version
            {
                return true;
            }
        }
        false
    }

    /// The node found out it was excluded (the peers no longer have it in the table): deletes the local state
    /// and redoes the join from scratch (doc 04, section 6.6, step 6), with the same ID. The freezing delays the
    /// new register: the request is rejected until the period ends and the bootstrap loop repeats.
    pub async fn handle_exclusion(self: &Arc<Self>) {
        if self.node.state() != NodeState::Active
            || self.node.drain_state() != super::state::DrainState::Active
            || self.excluded.swap(true, Ordering::SeqCst)
        {
            return;
        }
        tracing::warn!(node_id = %self.node_id(), "this node was excluded from the cluster: recovering the losses and redoing the join from scratch");
        self.recover_losses().await;
        self.boot_recovered.store(true, Ordering::SeqCst);
        if let Some(replication) = self.replication.get().cloned() {
            replication.reset_local().await;
        }
        self.membership.clear();
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.node.set_state(NodeState::Joining);
        if let Some(replication) = self.replication.get().cloned() {
            replication.channels.sync_members();
        }
        let me = self.clone();
        tokio::spawn(async move {
            let result = me.bootstrap().await;
            me.excluded.store(false, Ordering::SeqCst);
            if let Err(e) = result {
                tracing::error!(error = %e, "the node could not get back into the cluster");
            }
        });
    }

    /// Before deleting the local state, the returning node (excluded or left by drain) recovers what
    /// only it has of the messages declared lost (doc 04, section 6.6, steps 1 to 5). It stays in
    /// `returning` meanwhile. The peers are those of the last table the node knew.
    async fn recover_losses(self: &Arc<Self>) {
        let Some(replication) = self.replication.get().cloned() else {
            return;
        };
        let me = self.node_id();
        let peers: Vec<Member> = self
            .membership
            .table()
            .map(|table| {
                table
                    .members
                    .into_iter()
                    .filter(|m| m.node_id != me)
                    .collect()
            })
            .unwrap_or_default();
        self.node.set_state(NodeState::Returning);
        replication.recovery.run(peers).await;
    }

    /// The process restart: sets aside the previous life's disk and deletes the rest, so the node joins
    /// empty (D111; PRB-048).
    async fn recover_on_boot(self: &Arc<Self>, table: &MemberTable) {
        let Some(replication) = self.replication.get().cloned() else {
            return;
        };
        // The previous life's disk is set aside and the node joins empty: the recovery does not delay the
        // join (PRB-048) and runs after it is active, in `recover_previous_life`.
        let messages = replication.messages.messages().clone();
        let aside = tokio::task::spawn_blocking(move || messages.set_aside_old_disk()).await;
        match aside {
            Ok(Ok(true)) => {
                *self
                    .old_disk_peers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = table.members.clone();
            }
            Ok(Ok(false)) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "could not set aside the previous life's disk");
            }
            Err(_) => {}
        }
        self.release_old_disk().await;
    }

    /// After becoming active, the restart returns to the responsible party what the previous life's disk has of the
    /// members' losses and deletes that disk (doc 04, section 6.6; PRB-048). Runs in the background:
    /// a mute member only delays this step, up to `cluster.recovery.timeout`, never the join.
    fn recover_previous_life(self: &Arc<Self>) {
        let peers = std::mem::take(
            &mut *self
                .old_disk_peers
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        );
        let Some(replication) = self.replication.get().cloned() else {
            return;
        };
        let messages = replication.messages.messages().clone();
        if peers.is_empty() {
            tokio::task::spawn_blocking(move || messages.finish_previous_life());
            return;
        }
        tokio::spawn(async move {
            replication.recovery.run_streaming(peers).await;
            let _ = tokio::task::spawn_blocking(move || messages.finish_previous_life()).await;
        });
    }

    /// Deletes what was left on the previous life's disk and releases the sweep (D111).
    async fn release_old_disk(&self) {
        if let Some(replication) = self.replication.get().cloned() {
            replication.reset_local().await;
            replication.messages.messages().release_old_disk();
        }
    }

    /// Comes back from the drain **as a new node** (doc 04, section 6.10): new ID, clean local state and the
    /// full bootstrap, with a new position on the ring. There is no freezing: the ID is different and the exit was
    /// voluntary.
    pub async fn rejoin_as_new(self: &Arc<Self>) {
        self.recover_losses().await;
        self.boot_recovered.store(true, Ordering::SeqCst);
        let new_id = id::new_at(self.clock.now_millis());
        if let Some(replication) = self.replication.get().cloned() {
            replication.reset_local().await;
        }
        self.local
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .node_id = new_id.clone();
        self.node.set_node_id(new_id.clone());
        self.membership.clear();
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.node.set_state(NodeState::Joining);
        tracing::info!(node_id = %new_id, "node came back from the drain as a new node; redoing the bootstrap");
        let me = self.clone();
        tokio::spawn(async move {
            if let Err(e) = me.bootstrap().await {
                tracing::error!(error = %e, "the node could not get back into the cluster");
            }
        });
    }

    /// The first node of a new cluster: the table is just itself.
    pub fn start_alone(self: &Arc<Self>) {
        // The first node has no one to return the old disk to, but it does not wipe it either: a disk with a
        // manifest becomes the folder of its life (doc 37, section 4.5); one without it follows the usual path.
        if let Some(replication) = self.replication.get() {
            let messages = replication.messages.messages();
            if let Err(e) = messages.set_aside_old_disk() {
                tracing::warn!(error = %e, "could not set aside the previous life's disk");
            }
            messages.finish_previous_life();
            messages.release_old_disk();
        }
        self.membership.install(MemberTable {
            version: 1,
            members: vec![self.local()],
            ..MemberTable::default()
        });
        self.node.set_state(NodeState::Active);
        self.membership_changed();
        self.spawn_manifest_writer();
        tracing::info!(node_id = %self.node_id(), "first node: the cluster starts with just it");
    }

    /// The table arrived: the node installs it, starts writing events to the event database and synchronizes
    /// (definitions, blocks, events, final check; doc 05, section 7.3). Only at the end does it become active.
    /// If the synchronization fails in a way that cannot be repaired (disk, event database full, no
    /// source), the node goes to `failed`.
    async fn finish_join(
        self: &Arc<Self>,
        table: MemberTable,
        via: &Member,
    ) -> Result<(), Failure> {
        // The member that coordinated the join already has the new node in the table; the others may not yet
        // have it (the commit reaches them next), so it comes first.
        let mut sources = vec![via.clone()];
        sources.extend(
            table
                .members
                .iter()
                .filter(|m| {
                    m.node_id != self.node_id()
                        && m.node_id != via.node_id
                        && m.state == MemberState::Active
                })
                .cloned(),
        );
        let Some(replication) = self.replication.get().cloned() else {
            self.membership.install(table.clone());
            self.activate(&table).await;
            return Ok(());
        };
        let syncer = replication.syncer.clone();
        syncer.begin().map_err(|e| Failure {
            reason: "sync",
            message: format!("could not open the event database: {e}"),
        })?;
        self.membership.install(table.clone());
        self.node.set_state(NodeState::Syncing);
        replication.channels.sync_members();
        match syncer.run(&sources).await {
            Ok(()) => {
                self.activate(&table).await;
                self.recover_previous_life();
                Ok(())
            }
            Err(SyncError::Cancelled) => {
                syncer.abandon();
                Err(Failure {
                    reason: "sync",
                    message: "the node stopped during the synchronization".into(),
                })
            }
            Err(SyncError::Excluded) => {
                // The table the join returned is already stale: forget it and start the join over,
                // as the active node that finds out it was excluded does.
                tracing::warn!(node_id = %self.node_id(), "the sources excluded this node while it synchronized: redoing the join");
                syncer.abandon();
                self.membership.clear();
                self.open_votes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                replication.reset_local().await;
                self.node.set_state(NodeState::Joining);
                replication.channels.sync_members();
                Err(Failure {
                    reason: "excluded",
                    message: "the sources excluded the node while it synchronized".into(),
                })
            }
            Err(e) => {
                tracing::error!(error = %e, "the synchronization failed: the node goes to `failed`");
                syncer.abandon();
                self.node.set_failed();
                Ok(())
            }
        }
    }

    /// The node is healthy: its state in the table becomes `active`, the peers get to know (with
    /// attempts) and only then does it start serving and owning the messages of its blocks.
    async fn activate(self: &Arc<Self>, table: &MemberTable) {
        let me = self.node_id();
        self.membership.set_state(&me, MemberState::Active);
        self.local.write().unwrap_or_else(|e| e.into_inner()).state = MemberState::Active;
        if let Some(replication) = self.replication.get().cloned() {
            replication.signals.announce_state_reliably("active").await;
        }
        let version = table.version;
        let members = table.members.len();
        self.node.set_state(NodeState::Active);
        self.membership_changed();
        self.spawn_manifest_writer();
        tracing::info!(node_id = %me, version, members, "node in the cluster");
    }

    /// The bootstrap (doc 04, section 6.2): lists the members through the load balancer, asks one of them for the
    /// join and installs the table that comes back. Repeats every `cluster.bootstrap.retry.interval`
    /// up to `cluster.bootstrap.retry.max-attempts` (`0` does not limit).
    pub async fn bootstrap(self: &Arc<Self>) -> Result<(), BootstrapError> {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let failure = match self.attempt_join().await {
                Ok(()) => return Ok(()),
                Err(failure) => failure,
            };
            self.count_bootstrap_failure(failure.reason);
            tracing::error!(
                attempt = attempts,
                reason = failure.reason,
                "bootstrap failure: {}",
                failure.message
            );
            let max = self.rules.bootstrap_max_attempts;
            if max != 0 && attempts >= max {
                return Err(BootstrapError::Exhausted {
                    attempts,
                    last: failure.message,
                });
            }
            tokio::select! {
                () = self.cancel.cancelled() => return Err(BootstrapError::Cancelled),
                () = tokio::time::sleep(self.rules.bootstrap_retry_interval) => {}
            }
        }
    }

    async fn attempt_join(self: &Arc<Self>) -> Result<(), Failure> {
        let entry = self.rules.entry.clone().ok_or_else(|| Failure {
            reason: "config",
            message: "cluster.entry-address is not a host:port".into(),
        })?;

        // 1. The member listing, through the load balancer.
        let listed = self
            .call_addr(&entry, Request::ListNodes, self.rules.call_timeout)
            .await;
        let table = match listed {
            Ok(Response::Members(table)) => table,
            Ok(_) => {
                return Err(Failure {
                    reason: "list",
                    message: "unexpected response to the node listing".into(),
                });
            }
            Err(e) if e.found_no_cluster() && self.backup().is_some() => {
                // With the S3 backup the bucket decides (doc 23, section 1.7): the node only starts
                // alone (and restores) if no node beats there and the restore lock is its own.
                let port = self.backup().expect("checked above");
                let backup = port.backup();
                match backup.startup_decision(self.rules.first_node).await {
                    StartupDecision::StartAlone => {
                        self.start_alone();
                        backup.spawn_restore();
                        return Ok(());
                    }
                    StartupDecision::KeepTrying => {
                        return Err(Failure {
                            reason: "list",
                            message: format!(
                                "the node listing at {entry} failed and the backup does not authorize starting alone: {e}"
                            ),
                        });
                    }
                }
            }
            Err(e) if e.is_network() && self.rules.first_node => {
                // Only the first node of a new cluster starts alone, and only when the listing
                // finds no one.
                self.start_alone();
                return Ok(());
            }
            Err(e) => {
                return Err(Failure {
                    reason: "list",
                    message: format!("the node listing at {entry} failed: {e}"),
                });
            }
        };

        // The process that restarts is born as a new node, but what is left on the previous life's disk
        // may still be a message the other nodes declared lost (doc 04, section 6.6): before
        // joining, it looks for it and returns it to the responsible party (D111).
        if !self.boot_recovered.swap(true, Ordering::SeqCst) {
            self.recover_on_boot(&table).await;
        }

        // 2. A random member; if it does not answer, another.
        let mut candidates = table.members;
        let mut shuffled = Vec::with_capacity(candidates.len());
        while !candidates.is_empty() {
            shuffled.push(candidates.swap_remove(random_below(candidates.len())));
        }
        let request = Request::Join(JoinRequest {
            node_id: self.local().node_id,
            addresses: self.local().addresses,
            port: self.local().port,
            version: self.rules.program_version.clone(),
        });
        let mut last = Failure {
            reason: "join",
            message: "no member answered the join request".into(),
        };
        for member in shuffled {
            match self
                .call_peer(&member, request.clone(), self.rules.join_timeout)
                .await
            {
                Ok(Response::Members(table)) => {
                    self.finish_join(table, &member).await?;
                    return Ok(());
                }
                Ok(_) => {
                    last = Failure {
                        reason: "join",
                        message: "unexpected response to the join request".into(),
                    };
                }
                Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterCommitTimeout => {
                    // The node was accepted: before repeating, the table says whether it is already there.
                    if let Ok(Response::Members(table)) = self
                        .call_peer(&member, Request::ListNodes, self.rules.call_timeout)
                        .await
                        && table.member(&self.node_id()).is_some()
                    {
                        self.finish_join(table, &member).await?;
                        return Ok(());
                    }
                    return Err(Failure {
                        reason: "phase2",
                        message: error.message,
                    });
                }
                Err(CallError::Remote(error)) => {
                    let reason = match error.code {
                        ErrorCode::ClusterJoinUnreachable => "connectivity",
                        ErrorCode::ClusterRegistrationFailed => "phase1",
                        ErrorCode::ClusterNodeFrozen => "frozen",
                        _ => "join",
                    };
                    return Err(Failure {
                        reason,
                        message: format!("{}: {}", error.code, error.message),
                    });
                }
                Err(e) => {
                    last = Failure {
                        reason: "join",
                        message: format!("member {} did not answer: {e}", member.node_id),
                    };
                }
            }
        }
        Err(last)
    }
}

fn ack(operation_id: String) -> Response {
    Response::Ack(Ack {
        operation_id,
        accepted: true,
    })
}

fn registration_failed() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterRegistrationFailed,
        "The register phase was not confirmed by all the nodes; the register was cancelled.",
    )
    .with_retry_after(SYNCING_RETRY_AFTER_SECS)
}

#[cfg(test)]
mod tests;
