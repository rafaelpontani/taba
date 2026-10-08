//! O lado que **recebe** o registro, o commit e o rollback de uma operação sobre uma mensagem
//! (doc 05, seção 7.2; doc 06c, "Mensagens"): a publicação, o movimento para a dead-letter, a
//! invisibilidade e o ack.
//!
//! O registro (fase 1) só confere e deixa a operação **pendente**, sem mudar nada que um
//! consumidor veja; o commit (fase 2) a aplica ao motor de mensagens; o rollback a descarta. As
//! três fases são idempotentes pelo `operationId`. Uma operação pendente sem commit nem rollback
//! some sozinha depois de `write.pending.ttl`. Só uma operação pendente por mensagem: a segunda é
//! `409 cluster.conflict`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::cluster::protocol::{
    Ack, AckRegisterRequest, MessageRegisterRequest, OperationRef, Request, Step, VisibilityKind,
    VisibilityRegisterRequest, WriteKind,
};
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::{Messages, VisibilityCheck};
use crate::engine::topics::Topics;
use crate::storage::messages::MessageMeta;
use crate::telemetry::Telemetry;
use crate::telemetry::metrics::{WRITE_PENDING_EXPIRED, WRITE_RECEIVED};

/// Quantas operações já decididas o nó lembra, para repetir o `Ack` de um commit ou rollback
/// repetido em vez de dizer que a operação é desconhecida.
const FINISHED_MEMORY: usize = 4096;

/// O que o commit faz.
enum Action {
    Publish {
        meta: MessageMeta,
        body: Vec<u8>,
    },
    DeadLetter {
        source: String,
        at_ms: u64,
    },
    Visibility {
        until_ms: u64,
        delivery: Option<(u32, String)>,
    },
    Ack,
}

struct Pending {
    /// O tópico onde a mensagem está (na dead-letter, o de origem).
    topic: String,
    id: String,
    registered_at: SystemTime,
    action: Action,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Finished {
    Committed,
    RolledBack,
}

#[derive(Default)]
struct State {
    ops: HashMap<String, Pending>,
    /// `(tópico, id)` → operação pendente.
    by_message: HashMap<(String, String), String>,
    finished: VecDeque<(String, Finished)>,
}

pub struct Replica {
    messages: Arc<Messages>,
    topics: Arc<Topics>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<Telemetry>,
    pending_ttl: Duration,
    state: Mutex<State>,
}

fn unknown_operation() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterOperationUnknown,
        "O nó não conhece essa operação (nunca chegou ou já expirou).",
    )
}

fn conflict(message: &str) -> TabaError {
    TabaError::new(ErrorCode::ClusterConflict, message)
}

fn ack(operation_id: &str) -> Ack {
    Ack {
        operation_id: operation_id.to_owned(),
        accepted: true,
    }
}

impl Replica {
    pub fn new(
        messages: Arc<Messages>,
        topics: Arc<Topics>,
        clock: Arc<dyn Clock>,
        telemetry: Arc<Telemetry>,
        pending_ttl: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            messages,
            topics,
            clock,
            telemetry,
            pending_ttl,
            state: Mutex::new(State::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn count(&self, phase: &str) {
        self.telemetry
            .registry
            .add(&WRITE_RECEIVED, &[("taba.phase", phase)], 1.0);
    }

    /// Quantas operações estão pendentes (sem as vencidas).
    pub fn pending(&self) -> usize {
        let mut state = self.lock();
        self.purge(&mut state);
        state.ops.len()
    }

    /// Descarta as pendências que passaram de `write.pending.ttl`.
    fn purge(&self, state: &mut State) {
        let now = self.clock.now();
        let expired: Vec<String> = state
            .ops
            .iter()
            .filter(|(_, p)| {
                now.duration_since(p.registered_at)
                    .is_ok_and(|age| age >= self.pending_ttl)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(op) = state.ops.remove(&id) {
                state.by_message.remove(&(op.topic, op.id));
                self.telemetry
                    .registry
                    .add(&WRITE_PENDING_EXPIRED, &[], 1.0);
                tracing::warn!(operation_id = %id, "registro pendente expirado sem commit nem rollback");
            }
        }
    }

    fn remember(state: &mut State, operation_id: &str, outcome: Finished) {
        state.finished.retain(|(id, _)| id != operation_id);
        state.finished.push_back((operation_id.to_owned(), outcome));
        while state.finished.len() > FINISHED_MEMORY {
            state.finished.pop_front();
        }
    }

    /// Deixa a operação pendente, ou confirma de novo a que já estava (idempotente).
    fn register(
        &self,
        operation_id: &str,
        topic: &str,
        id: &str,
        action: Action,
    ) -> Result<Ack, TabaError> {
        let mut state = self.lock();
        self.purge(&mut state);
        if state.ops.contains_key(operation_id)
            || state.finished.iter().any(|(o, _)| o == operation_id)
        {
            return Ok(ack(operation_id));
        }
        let key = (topic.to_owned(), id.to_owned());
        if state.by_message.contains_key(&key) {
            return Err(conflict("Há outra operação pendente para esta mensagem."));
        }
        state.by_message.insert(key, operation_id.to_owned());
        state.ops.insert(
            operation_id.to_owned(),
            Pending {
                topic: topic.to_owned(),
                id: id.to_owned(),
                registered_at: self.clock.now(),
                action,
            },
        );
        Ok(ack(operation_id))
    }

    /// O commit: aplica a operação pendente ao motor.
    async fn commit(&self, operation_id: &str) -> Result<Ack, TabaError> {
        let pending = {
            let mut state = self.lock();
            self.purge(&mut state);
            match state.ops.remove(operation_id) {
                Some(op) => {
                    state.by_message.remove(&(op.topic.clone(), op.id.clone()));
                    op
                }
                None => {
                    return match state.finished.iter().find(|(o, _)| o == operation_id) {
                        Some((_, Finished::Committed)) => Ok(ack(operation_id)),
                        Some((_, Finished::RolledBack)) => {
                            Err(conflict("A operação foi cancelada (rollback)."))
                        }
                        None => Err(unknown_operation()),
                    };
                }
            }
        };
        let messages = self.messages.clone();
        let Pending {
            topic, id, action, ..
        } = pending;
        let result = tokio::task::spawn_blocking(move || match action {
            Action::Publish { meta, body } => messages.apply_publish(&topic, meta, body),
            Action::DeadLetter { source, at_ms } => messages.apply_dead_letter(&source, &id, at_ms),
            Action::Visibility { until_ms, delivery } => messages.apply_visibility(
                &topic,
                &id,
                until_ms,
                delivery
                    .as_ref()
                    .map(|(count, token)| (*count, token.as_str())),
            ),
            Action::Ack => messages.apply_ack(&topic, &id),
        })
        .await
        .map_err(|_| TabaError::internal())?;
        // Aplicada ou não, a operação saiu das pendentes; o erro (mensagem que sumiu, por
        // exemplo) vai ao coordenador.
        Self::remember(&mut self.lock(), operation_id, Finished::Committed);
        result.map(|()| ack(operation_id))
    }

    fn rollback(&self, operation_id: &str) -> Result<Ack, TabaError> {
        let mut state = self.lock();
        self.purge(&mut state);
        if let Some(op) = state.ops.remove(operation_id) {
            state.by_message.remove(&(op.topic, op.id));
            Self::remember(&mut state, operation_id, Finished::RolledBack);
            return Ok(ack(operation_id));
        }
        match state.finished.iter().find(|(o, _)| o == operation_id) {
            Some((_, Finished::RolledBack)) => Ok(ack(operation_id)),
            Some((_, Finished::Committed)) => Err(conflict(
                "A operação já foi efetivada: não há rollback depois do commit.",
            )),
            None => Err(unknown_operation()),
        }
    }

    async fn blocking<T, F>(&self, work: F) -> Result<T, TabaError>
    where
        T: Send + 'static,
        F: FnOnce(&Messages, &Topics) -> Result<T, TabaError> + Send + 'static,
    {
        let (messages, topics) = (self.messages.clone(), self.topics.clone());
        tokio::task::spawn_blocking(move || work(&messages, &topics))
            .await
            .map_err(|_| TabaError::internal())?
    }

    // ---- As rotas ----

    /// `POST /cluster/topics/{topic}/messages/{id}/register|commit|rollback`.
    pub async fn write(
        &self,
        topic: &str,
        id: &str,
        step: Step<MessageRegisterRequest>,
    ) -> Result<Ack, TabaError> {
        self.write_step(topic, id, step, true).await
    }

    async fn write_step(
        &self,
        topic: &str,
        id: &str,
        step: Step<MessageRegisterRequest>,
        checked: bool,
    ) -> Result<Ack, TabaError> {
        self.count(step.phase());
        match step {
            Step::Register(request) => self.register_write(topic, id, request, checked).await,
            Step::Commit(OperationRef { operation_id }) => self.commit(&operation_id).await,
            Step::Rollback(OperationRef { operation_id }) => self.rollback(&operation_id),
        }
    }

    async fn register_write(
        &self,
        topic: &str,
        id: &str,
        request: MessageRegisterRequest,
        checked: bool,
    ) -> Result<Ack, TabaError> {
        let record = request.message;
        if record.id != id || record.topic != topic {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "O caminho e o corpo falam de mensagens diferentes.",
            ));
        }
        let operation_id = request.operation_id;
        let (topic, id) = (topic.to_owned(), id.to_owned());
        match request.kind {
            WriteKind::Publish => {
                let published_ms = record.published_ms();
                let meta = MessageMeta {
                    id: record.id.clone(),
                    content_type: record.content_type.clone(),
                    group_id: record.group_id.clone(),
                    deduplication_id: record.deduplication_id.clone(),
                    published_ms,
                    delivery_count: record.delivery_count,
                    invisible_until_ms: record.invisible_until_ms.unwrap_or(0),
                    receipt_token: record.receipt_token.clone(),
                    dead_lettered_ms: None,
                    dead_letter_reason: None,
                    position: 0,
                };
                let body = record.body;
                let check = if !checked {
                    Ok(())
                } else {
                    let (topic, id) = (topic.clone(), id.clone());
                    let body = body.clone();
                    self.blocking(move |messages, topics| {
                        let view = topics.get(&topic)?;
                        if view.definition.queue().is_none() {
                            return Err(TabaError::new(
                                ErrorCode::TopicWrongType,
                                "Um tópico distribuidor não guarda mensagens.",
                            ));
                        }
                        match messages.message_info(&topic, &id)? {
                            None => Ok(()),
                            Some(_) => {
                                // O mesmo conteúdo repetido é idempotente; outro, conflito.
                                let same = messages
                                    .read_message(&topic, &id)?
                                    .is_some_and(|(_, held)| held == body);
                                if same {
                                    Ok(())
                                } else {
                                    Err(conflict("A mensagem já existe com conteúdo diferente."))
                                }
                            }
                        }
                    })
                    .await
                };
                check?;
                self.register(&operation_id, &topic, &id, Action::Publish { meta, body })
            }
            WriteKind::DeadLetter => {
                let source = request.source_topic.ok_or_else(|| {
                    TabaError::new(
                        ErrorCode::ValidationRequired,
                        "O movimento para a dead-letter exige `sourceTopic`.",
                    )
                    .with_detail(
                        "sourceTopic",
                        ErrorCode::ValidationRequired,
                        "Campo obrigatório.",
                    )
                })?;
                let at_ms = record
                    .dead_lettered_ms
                    .unwrap_or_else(|| self.clock.now_millis());
                if checked {
                    let (source, target, id) = (source.clone(), topic.clone(), id.clone());
                    self.blocking(move |messages, topics| {
                        topics.get(&source)?;
                        let at_source = messages.message_info(&source, &id)?.is_some();
                        let at_target = messages.message_info(&target, &id)?.is_some();
                        if at_source || at_target {
                            Ok(())
                        } else {
                            Err(TabaError::new(
                                ErrorCode::MessageNotFound,
                                "A mensagem não existe.",
                            ))
                        }
                    })
                    .await?;
                }
                self.register(
                    &operation_id,
                    &source,
                    &id,
                    Action::DeadLetter {
                        source: source.clone(),
                        at_ms,
                    },
                )
            }
        }
    }

    /// `POST /cluster/topics/{topic}/messages/{id}/visibility/register|commit|rollback`.
    pub async fn visibility(
        &self,
        topic: &str,
        id: &str,
        step: Step<VisibilityRegisterRequest>,
    ) -> Result<Ack, TabaError> {
        self.visibility_step(topic, id, step, true).await
    }

    async fn visibility_step(
        &self,
        topic: &str,
        id: &str,
        step: Step<VisibilityRegisterRequest>,
        checked: bool,
    ) -> Result<Ack, TabaError> {
        self.count(step.phase());
        match step {
            Step::Register(request) => {
                let (check, delivery) = match request.kind {
                    VisibilityKind::Delivery => (
                        VisibilityCheck::Delivery,
                        Some((
                            request.delivery_count.ok_or_else(|| {
                                TabaError::new(
                                    ErrorCode::ValidationRequired,
                                    "A entrega exige `deliveryCount`.",
                                )
                                .with_detail(
                                    "deliveryCount",
                                    ErrorCode::ValidationRequired,
                                    "Campo obrigatório.",
                                )
                            })?,
                            request.receipt_token.clone(),
                        )),
                    ),
                    VisibilityKind::Change => (VisibilityCheck::Receipt, None),
                };
                let (t, i, token) = (
                    topic.to_owned(),
                    id.to_owned(),
                    request.receipt_token.clone(),
                );
                if checked {
                    self.blocking(move |messages, _| {
                        messages.check_visibility(&t, &i, check, &token)
                    })
                    .await?;
                }
                self.register(
                    &request.operation_id,
                    topic,
                    id,
                    Action::Visibility {
                        until_ms: request.invisible_until_ms,
                        delivery,
                    },
                )
            }
            Step::Commit(OperationRef { operation_id }) => self.commit(&operation_id).await,
            Step::Rollback(OperationRef { operation_id }) => self.rollback(&operation_id),
        }
    }

    /// `POST /cluster/topics/{topic}/messages/{id}/ack/register|commit|rollback`.
    pub async fn ack(
        &self,
        topic: &str,
        id: &str,
        step: Step<AckRegisterRequest>,
    ) -> Result<Ack, TabaError> {
        self.ack_step(topic, id, step, true).await
    }

    async fn ack_step(
        &self,
        topic: &str,
        id: &str,
        step: Step<AckRegisterRequest>,
        checked: bool,
    ) -> Result<Ack, TabaError> {
        self.count(step.phase());
        match step {
            Step::Register(request) => {
                let (t, i, token) = (
                    topic.to_owned(),
                    id.to_owned(),
                    request.receipt_token.clone(),
                );
                if checked {
                    self.blocking(move |messages, _| {
                        messages.check_visibility(&t, &i, VisibilityCheck::Receipt, &token)
                    })
                    .await?;
                }
                self.register(&request.operation_id, topic, id, Action::Ack)
            }
            Step::Commit(OperationRef { operation_id }) => self.commit(&operation_id).await,
            Step::Rollback(OperationRef { operation_id }) => self.rollback(&operation_id),
        }
    }

    /// Processa um evento que ficou no banco de eventos durante a sincronização (doc 05, seção
    /// 7.3, passo 5.3). O registro não confere nada: o evento vence a carga, e aplicar de novo o
    /// que a carga já trazia não muda nada (a aplicação é idempotente). Só as chamadas de
    /// mensagem passam por aqui; as de definições seguem o caminho normal.
    pub async fn replay(&self, request: Request) -> Result<(), TabaError> {
        match request {
            Request::Write { topic, id, step } => {
                self.write_step(&topic, &id, step, false).await.map(|_| ())
            }
            Request::Visibility { topic, id, step } => self
                .visibility_step(&topic, &id, step, false)
                .await
                .map(|_| ()),
            Request::Ack { topic, id, step } => {
                self.ack_step(&topic, &id, step, false).await.map(|_| ())
            }
            _ => Ok(()),
        }
    }
}
