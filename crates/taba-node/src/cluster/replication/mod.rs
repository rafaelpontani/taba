//! Replication between nodes (doc 05, section 7.2; doc 13; doc 14): the write, invisibility and ack
//! by quorum, `take` and the signals between nodes.
//!
//! - [`Replica`]: the side that **receives** the register, the commit and the rollback.
//! - [`coordinator`]: the side that **coordinates** the two phases over the responsible nodes.
//! - [`ClusterMessages`]: the node's messages API in the cluster (publish, consume, ack...).
//! - [`Signals`]: which nodes have ready messages and listeners, and who is in drain.
//!
//! - [`Syncer`] and [`BlockSource`]: the block synchronization of a node that joins and the side that serves it.
//!
//! [`Replication`] puts the pieces together and serves the calls of other nodes that are not members'.

pub mod coordinator;
mod definitions;
mod forward;
mod messages_api;
mod replica;
mod signals;

use std::sync::Arc;

pub use definitions::{Definitions, Pulled, Stored};
pub use forward::{ForwardExecutor, Forwarder};
pub use messages_api::ClusterMessages;
pub use replica::Replica;
pub use signals::{ListenerGuard, Signals};

use super::channel::Channels;
use super::protocol::{Request, Response};
use super::recovery::Recovery;
use super::service::ClusterService;
use super::state::NodeState;
use super::sync::{BlockSource, SyncRules, Syncer};
use super::vote::Voting;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::{ClusterHooks, DeadLetterJob, Messages, Placement};
use crate::engine::tokens::Tokens;
use crate::engine::topics::Topics;
use crate::storage::losses::LossStore;
use crate::storage::versions::{VersionRow, VersionStore};
use crate::telemetry::Telemetry;

/// The node is a message's delivery owner if it is the primary of its block on the ring (doc 13, 12.7).
struct RingPlacement(Arc<ClusterService>);

impl Placement for RingPlacement {
    /// A node that is still syncing owns nothing: the messages of its blocks are only delivered
    /// when it becomes ready (doc 13, section 12.7).
    fn is_owner(&self, _topic: &str, _id: &str, _group: Option<&str>, position: u64) -> bool {
        self.0.node_state() == NodeState::Active
            && self
                .0
                .membership()
                .ring()
                .primary(position)
                .is_some_and(|primary| primary == self.0.node_id())
    }

    fn is_responsible(&self, position: u64) -> bool {
        let factor = self.0.rules().replication_factor.max(1) as usize;
        let me = self.0.node_id();
        self.0
            .membership()
            .ring()
            .owners(position, factor)
            .contains(&me.as_str())
    }
}

/// What is left to whoever starts the node after attaching the cluster to the engine.
pub struct Attached {
    pub replication: Arc<Replication>,
    /// The background work: moves to the dead-letter to coordinate.
    pub dead_letters: tokio::sync::mpsc::UnboundedReceiver<DeadLetterJob>,
}

/// Attaches the cluster to the message engine: who the owner is, the dead-letter queue, the signals of
/// ready, and the owner's re-evaluation on each member change.
#[allow(clippy::too_many_arguments)]
pub fn attach(
    service: &Arc<ClusterService>,
    messages: &Arc<Messages>,
    topics: &Arc<Topics>,
    tokens: &Arc<Tokens>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<Telemetry>,
    losses: LossStore,
    sync_rules: SyncRules,
    versions: (VersionStore, Vec<VersionRow>),
) -> Attached {
    let replica = Replica::new(
        messages.clone(),
        topics.clone(),
        clock,
        telemetry,
        service.rules().pending_ttl,
    );
    let signals = Signals::new(service.clone(), messages.clone());
    let cluster_messages = ClusterMessages::new(
        service.clone(),
        messages.clone(),
        topics.clone(),
        signals.clone(),
    );
    let definitions = Definitions::new(
        service.clone(),
        topics.clone(),
        tokens.clone(),
        versions.0,
        versions.1,
    );
    let recovery = Recovery::new(
        service.clone(),
        messages.clone(),
        losses.clone(),
        sync_rules.max_items,
    );
    topics.set_replicator(definitions.clone());
    tokens.set_replicator(definitions.clone());
    let forwarder = Forwarder::new(service.clone(), signals.clone());
    let syncer = Syncer::new(
        service.clone(),
        messages.clone(),
        definitions.clone(),
        replica.clone(),
        losses,
        sync_rules.clone(),
    );
    let source = BlockSource::new(service.clone(), messages.clone(), sync_rules);
    let channels = Channels::new(service.clone());
    channels.set_signals(&signals);
    signals.set_channels(channels.clone());
    let voting = Voting::new(service.clone(), channels.clone());
    {
        let voting = voting.clone();
        channels.set_on_down(Arc::new(move |node, reason| {
            voting.open_in_background(node, reason);
        }));
        let for_exclusion = service.clone();
        channels.set_on_excluded(Arc::new(move |peer| {
            let service = for_exclusion.clone();
            tokio::spawn(async move { service.check_exclusion(&peer).await });
        }));
        let signals = signals.clone();
        service.on_node_failed_hook(Box::new(move || signals.announce_failed()));
    }
    let replication = Replication::new(
        service.clone(),
        replica,
        cluster_messages,
        signals.clone(),
        definitions,
        forwarder,
        syncer,
        source,
        channels,
        voting,
        recovery,
    );
    service.set_replication(replication.clone());

    let (dead_letters_tx, dead_letters) = tokio::sync::mpsc::unbounded_channel();
    let ready = signals.ready_sender();
    messages.enable_cluster(ClusterHooks {
        placement: Arc::new(RingPlacement(service.clone())),
        dead_letters: dead_letters_tx,
        ready: Arc::new(move |topic, has_ready| {
            let _ = ready.send((topic.to_owned(), has_ready));
        }),
    });
    {
        let (messages, signals) = (messages.clone(), signals);
        service.set_on_change(Arc::new(move || {
            let messages = messages.clone();
            tokio::task::spawn_blocking(move || messages.reown());
            signals.membership_changed();
        }));
    }
    Attached {
        replication,
        dead_letters,
    }
}

pub struct Replication {
    service: Arc<ClusterService>,
    pub replica: Arc<Replica>,
    pub messages: Arc<ClusterMessages>,
    pub signals: Arc<Signals>,
    pub definitions: Arc<Definitions>,
    pub forwarder: Arc<Forwarder>,
    pub syncer: Arc<Syncer>,
    pub source: BlockSource,
    pub channels: Arc<Channels>,
    pub voting: Arc<Voting>,
    pub recovery: Arc<Recovery>,
}

impl Replication {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        service: Arc<ClusterService>,
        replica: Arc<Replica>,
        messages: Arc<ClusterMessages>,
        signals: Arc<Signals>,
        definitions: Arc<Definitions>,
        forwarder: Arc<Forwarder>,
        syncer: Arc<Syncer>,
        source: BlockSource,
        channels: Arc<Channels>,
        voting: Arc<Voting>,
        recovery: Arc<Recovery>,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            replica,
            messages,
            signals,
            definitions,
            forwarder,
            syncer,
            source,
            channels,
            voting,
            recovery,
        })
    }

    /// Clears the local state when the node comes back from the drain as a new node: definitions, tokens, messages
    /// and what it knew of the other nodes (doc 04, section 6.10).
    pub async fn reset_local(&self) {
        self.definitions.wipe().await;
        let messages = self.messages.messages().clone();
        let _ = tokio::task::spawn_blocking(move || messages.reset_all()).await;
        self.signals.reset();
    }

    /// A member opened the channel with this node.
    pub fn accept_channel(
        &self,
        open: crate::cluster::transport::ChannelOpen,
        link: crate::cluster::transport::ChannelLink,
    ) {
        self.channels.accept(&open.from, link);
    }

    /// Serves a call from another member (the member check was already done).
    pub async fn handle(&self, caller: &str, request: Request) -> Result<Response, TabaError> {
        // A node that is syncing stores the events in the database instead of processing them (doc 05, 7.3).
        if let Some(reply) = self.syncer.intercept(&request)? {
            return Ok(reply);
        }
        match request {
            Request::Write { topic, id, step } => {
                crate::cluster::protocol::validate_message_path(&topic, &id)?;
                step.validate_reference()?;
                self.replica
                    .write(&topic, &id, step)
                    .await
                    .map(Response::Ack)
            }
            Request::Visibility { topic, id, step } => {
                crate::cluster::protocol::validate_message_path(&topic, &id)?;
                step.validate_reference()?;
                self.replica
                    .visibility(&topic, &id, step)
                    .await
                    .map(Response::Ack)
            }
            Request::Ack { topic, id, step } => {
                crate::cluster::protocol::validate_message_path(&topic, &id)?;
                step.validate_reference()?;
                self.replica.ack(&topic, &id, step).await.map(Response::Ack)
            }
            Request::DefinitionPropose(proposal) => self.definitions.handle_propose(proposal).await,
            Request::DefinitionApply(change) => self.definitions.handle_apply(change).await,
            Request::ListTopics(query) => Ok(self.definitions.list_topics(&query)),
            Request::ListTokens(query) => Ok(self.definitions.list_tokens(&query)),
            Request::ListTombstones(query) => Ok(self.definitions.list_tombstones(&query)),
            Request::Idle { topic_id } => {
                let (idle_seconds, since_start) = self.messages.messages().idle_of(&topic_id)?;
                Ok(Response::Idle(crate::cluster::protocol::IdleResponse {
                    topic_id,
                    idle_seconds,
                    since_start,
                }))
            }
            Request::Take { topic, request } => {
                // The notified node came to fetch: whoever is responsible for the topic's notices can stop waiting.
                self.signals.took(&topic, caller);
                if self.service.node_state() != NodeState::Active {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNodeSyncing,
                        "The node is still syncing and does not deliver messages.",
                    )
                    .with_retry_after(5));
                }
                self.messages
                    .take_for_peer(&topic, request)
                    .await
                    .map(Response::Taken)
            }
            request @ (Request::BlockIds { .. }
            | Request::BlockExcluded { .. }
            | Request::BlockMessages { .. }
            | Request::BlockMessage { .. }) => self.source.handle(request).await,
            Request::Vote(vote) => self.voting.cast(vote).await,
            Request::FifoPublish { topic, record } => self
                .messages
                .publish_fifo_for_peer(&topic, record)
                .await
                .map(Response::FifoPublished),
            Request::Prune { topic, prune_id } => {
                crate::cluster::protocol::require_guid("pruneId", &prune_id)?;
                let removed = self.messages.messages().prune(&topic, &prune_id)?;
                Ok(Response::Json(serde_json::json!({ "removed": removed })))
            }
            Request::MessageLoss(query) => self.recovery.serve_losses(&query),
            Request::LossRegister(record) => self.recovery.register_loss(record),
            Request::LossRemove { topic, id } => self.recovery.remove_loss(&topic, &id),
            Request::Recover {
                records,
                replicate,
                durable,
            } => self.recovery.accept(records, replicate, durable).await,
            Request::Restore {
                topic,
                topic_id,
                records,
            } => self.recovery.accept_restore(topic, topic_id, records).await,
            Request::Dead { topic, ids } => self.recovery.serve_dead(topic, ids).await,
            Request::RestoreState => self.recovery.serve_state(),
            Request::RestoreDecided => self.recovery.serve_decided().await,
            Request::RestoreForce => self.recovery.serve_force().await,
            _ => Err(TabaError::route_not_found()),
        }
    }
}
