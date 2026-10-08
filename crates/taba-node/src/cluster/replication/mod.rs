//! A replicação entre nós (doc 05, seção 7.2; doc 13; doc 14): a escrita, a invisibilidade e o ack
//! por quórum, o `take` e os sinais entre nós.
//!
//! - [`Replica`]: o lado que **recebe** o registro, o commit e o rollback.
//! - [`coordinator`]: o lado que **coordena** as duas fases sobre os nós responsáveis.
//! - [`ClusterMessages`]: a API de mensagens do nó no cluster (publicar, consumir, ack...).
//! - [`Signals`]: quais nós têm prontas e ouvintes, e quem está em drain.
//!
//! - [`Syncer`] e [`BlockSource`]: a sincronização de blocos de um nó que entra e o lado que a serve.
//!
//! [`Replication`] junta as peças e atende as chamadas de outros nós que não são de membros.

pub mod coordinator;
mod definitions;
mod forward;
mod messages_api;
mod replica;
mod signals;

use std::sync::Arc;

pub use definitions::{Definitions, Pulled};
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
use crate::telemetry::Telemetry;

/// O nó é o dono da entrega de uma mensagem se é o primário do bloco dela no anel (doc 13, 12.7).
struct RingPlacement(Arc<ClusterService>);

impl Placement for RingPlacement {
    /// Um nó que ainda sincroniza não é dono de nada: as mensagens dos blocos dele só são entregues
    /// quando ele fica pronto (doc 13, seção 12.7).
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

/// O que sobra a quem sobe o nó depois de ligar o cluster ao motor.
pub struct Attached {
    pub replication: Arc<Replication>,
    /// O trabalho de fundo: movimentos para a dead-letter a coordenar.
    pub dead_letters: tokio::sync::mpsc::UnboundedReceiver<DeadLetterJob>,
}

/// Liga o cluster ao motor de mensagens: quem é o dono, a fila de dead-letters, os sinais de
/// prontas, e a reavaliação do dono a cada mudança de membros.
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
    let definitions = Definitions::new(service.clone(), topics.clone(), tokens.clone());
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

    /// Limpa o estado local quando o nó volta do drain como nó novo: definições, tokens, mensagens
    /// e o que se sabia dos outros nós (doc 04, seção 6.10).
    pub async fn reset_local(&self) {
        self.definitions.wipe().await;
        let messages = self.messages.messages().clone();
        let _ = tokio::task::spawn_blocking(move || messages.reset_all()).await;
        self.signals.reset();
    }

    /// Atende uma chamada de outro membro (a conferência de membro já foi feita).
    pub async fn handle(&self, _caller: &str, request: Request) -> Result<Response, TabaError> {
        // Um nó que sincroniza guarda os eventos no banco em vez de processá-los (doc 05, 7.3).
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
            Request::Topic(step) => {
                step.validate_reference()?;
                self.definitions.handle_topic(step).await
            }
            Request::Token(step) => {
                step.validate_reference()?;
                self.definitions.handle_token(step).await
            }
            Request::ListTopics(query) => Ok(self.definitions.list_topics(&query)),
            Request::ListTokens(query) => Ok(self.definitions.list_tokens(&query)),
            Request::Take { topic, request } => {
                if self.service.node_state() != NodeState::Active {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNodeSyncing,
                        "O nó ainda está em sincronização e não entrega mensagens.",
                    )
                    .with_retry_after(5));
                }
                self.messages
                    .take_for_peer(&topic, request)
                    .await
                    .map(Response::Taken)
            }
            request @ (Request::BlockIds { .. }
            | Request::BlockMessages { .. }
            | Request::BlockMessage { .. }) => self.source.handle(request).await,
            Request::Channel(batch) => Ok(Response::Confirm(self.channels.receive(batch))),
            Request::Vote(vote) => self.voting.cast(vote).await,
            Request::FifoPublish { topic, record } => self
                .messages
                .publish_fifo_for_peer(&topic, record)
                .await
                .map(Response::FifoPublished),
            Request::MessageLoss(query) => self.recovery.serve_losses(&query),
            Request::LossRegister(record) => self.recovery.register_loss(record),
            Request::LossRemove { topic, id } => self.recovery.remove_loss(&topic, &id),
            Request::Recover { records, replicate } => {
                self.recovery.accept(records, replicate).await
            }
            _ => Err(TabaError::route_not_found()),
        }
    }
}
