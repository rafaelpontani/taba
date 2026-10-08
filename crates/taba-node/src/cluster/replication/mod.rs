//! A replicação entre nós (doc 05, seção 7.2; doc 13; doc 14): a escrita, a invisibilidade e o ack
//! por quórum, o `take` e os sinais entre nós.
//!
//! - [`Replica`]: o lado que **recebe** o registro, o commit e o rollback.
//! - [`coordinator`]: o lado que **coordena** as duas fases sobre os nós responsáveis.
//! - [`ClusterMessages`]: a API de mensagens do nó no cluster (publicar, consumir, ack...).
//! - [`Signals`]: quais nós têm prontas e ouvintes, e quem está em drain.
//!
//! [`Replication`] junta as peças e atende as chamadas de outros nós que não são de membros.

pub mod coordinator;
mod definitions;
mod forward;
mod messages_api;
mod replica;
mod signals;

use std::sync::Arc;

pub use definitions::Definitions;
pub use forward::{ForwardExecutor, Forwarder};
pub use messages_api::ClusterMessages;
pub use replica::Replica;
pub use signals::{ListenerGuard, Signals};

use super::protocol::{Request, Response};
use super::service::ClusterService;
use crate::core::clock::Clock;
use crate::core::error::TabaError;
use crate::core::ring::message_position;
use crate::engine::messages::{ClusterHooks, DeadLetterJob, Messages, Placement};
use crate::engine::tokens::Tokens;
use crate::engine::topics::Topics;
use crate::telemetry::Telemetry;

/// O nó é o dono da entrega de uma mensagem se é o primário do bloco dela no anel (doc 13, 12.7).
struct RingPlacement(Arc<ClusterService>);

impl Placement for RingPlacement {
    fn is_owner(&self, _topic: &str, id: &str, _group: Option<&str>) -> bool {
        self.0
            .membership()
            .ring()
            .primary(message_position(id))
            .is_some_and(|primary| primary == self.0.node_id())
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
pub fn attach(
    service: &Arc<ClusterService>,
    messages: &Arc<Messages>,
    topics: &Arc<Topics>,
    tokens: &Arc<Tokens>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<Telemetry>,
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
    topics.set_replicator(definitions.clone());
    tokens.set_replicator(definitions.clone());
    let forwarder = Forwarder::new(service.clone(), signals.clone());
    let replication = Replication::new(
        replica,
        cluster_messages,
        signals.clone(),
        definitions,
        forwarder,
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
    pub replica: Arc<Replica>,
    pub messages: Arc<ClusterMessages>,
    pub signals: Arc<Signals>,
    pub definitions: Arc<Definitions>,
    pub forwarder: Arc<Forwarder>,
}

impl Replication {
    pub fn new(
        replica: Arc<Replica>,
        messages: Arc<ClusterMessages>,
        signals: Arc<Signals>,
        definitions: Arc<Definitions>,
        forwarder: Arc<Forwarder>,
    ) -> Arc<Self> {
        Arc::new(Self {
            replica,
            messages,
            signals,
            definitions,
            forwarder,
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
            Request::Take { topic, request } => self
                .messages
                .take_for_peer(&topic, request)
                .await
                .map(Response::Taken),
            Request::Signal(frame) => {
                self.signals.receive(frame);
                Ok(Response::Empty)
            }
            _ => Err(TabaError::route_not_found()),
        }
    }
}
