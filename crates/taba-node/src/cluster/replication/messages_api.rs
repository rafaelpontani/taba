//! A API de mensagens de um nó **no cluster** (doc 06a, "Mensagens"; doc 13): o que o cliente
//! chama (`POST`, `GET`, ack e `PUT visibility`), agora com a escrita, a invisibilidade e o ack
//! decididos por quórum nos nós responsáveis e a entrega completada com o `take` em outros nós.
//!
//! O nó que recebe a chamada é o coordenador, qualquer que seja ele. O `GET` entrega o que o
//! próprio nó tem como dono e, se faltar, pede ao dono de outras mensagens (`take`); sem nada,
//! espera como ouvinte (long polling).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::value::RawValue;
use tokio::task::JoinSet;

use super::coordinator::{self, Plan};
use super::signals::Signals;
use crate::cluster::members::Member;
use crate::cluster::protocol::{
    AckRegisterRequest, FIFO_PLACEHOLDER_ID, FifoPublished, MessageRecord, MessageRegisterRequest,
    OperationRef, Request, Response, Step, TakeRequest, TakeResponse, VisibilityKind,
    VisibilityRegisterRequest, WriteKind,
};
use crate::cluster::service::ClusterService;
use crate::cluster::state::{DrainState, NodeState};
use crate::cluster::transport::CallError;
use crate::core::content::{Body, ContentType, DEFAULT_CONTENT_TYPE};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring::message_position;
use crate::engine::messages::{
    Candidate, ConsumeQuery, Delivered, DistributorDelivery, Draft, Messages, PublishRequest,
    Published, VisibilityUpdated,
};
use crate::engine::topics::{TopicView, Topics};
use crate::telemetry::metrics::{DELIVERY_TAKE, FIFO_FORWARD};

/// Uma trava por (tópico, grupo) FIFO.
type GroupLocks = Mutex<HashMap<(String, String), Arc<tokio::sync::Mutex<()>>>>;

pub struct ClusterMessages {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    topics: Arc<Topics>,
    signals: Arc<Signals>,
    /// Uma publicação por vez em cada grupo FIFO deste nó (o primário): é o que ordena o grupo e
    /// serializa a deduplicação (doc 17, seções 16.4 e 16.7).
    group_locks: GroupLocks,
}

fn primary_unavailable() -> TabaError {
    TabaError::new(
        ErrorCode::FifoPrimaryUnavailable,
        "O primário do grupo não está disponível para coordenar a publicação.",
    )
    .with_retry_after(5)
}

fn delivery_unavailable() -> TabaError {
    TabaError::new(
        ErrorCode::DeliveryUnavailable,
        "Não foi possível entregar: o quórum não fechou para as mensagens escolhidas.",
    )
    .with_retry_after(5)
}

impl ClusterMessages {
    pub fn new(
        service: Arc<ClusterService>,
        messages: Arc<Messages>,
        topics: Arc<Topics>,
        signals: Arc<Signals>,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            messages,
            topics,
            signals,
            group_locks: Mutex::new(HashMap::new()),
        })
    }

    pub fn messages(&self) -> &Arc<Messages> {
        &self.messages
    }

    pub fn signals(&self) -> &Arc<Signals> {
        &self.signals
    }

    /// Onde a operação vai: a posição no anel e quantos dos primeiros responsáveis o quórum
    /// precisa incluir (1 na fila comum; na FIFO, o primário e o secundário).
    fn operation_plan(
        &self,
        topic: &str,
        route: (u64, usize),
        make: impl Fn(&str) -> Request,
        commit: impl Fn(&str, OperationRef) -> Request,
        rollback: impl Fn(&str, OperationRef) -> Request,
        operation_id: &str,
    ) -> Result<Plan, TabaError> {
        let owners = self.service.owners_of(route.0)?;
        let op = || OperationRef {
            operation_id: operation_id.to_owned(),
        };
        Ok(Plan {
            owners,
            quorum: self.service.rules().quorum as usize,
            required: route.1,
            register: make(operation_id),
            commit: commit(topic, op()),
            rollback: rollback(topic, op()),
        })
    }

    /// A posição e os papéis obrigatórios de um ack ou de uma alteração de invisibilidade. Na FIFO
    /// a posição é a do grupo, que o recibo carrega (`{posição}-{guid}`); recibo malformado é
    /// `409 message.stale-receipt`.
    fn route_of(
        &self,
        view: &TopicView,
        id: &str,
        receipt_token: &str,
    ) -> Result<(u64, usize), TabaError> {
        if !view.definition.queue().is_some_and(|q| q.fifo) {
            return Ok((message_position(id), 1));
        }
        receipt_token
            .split_once('-')
            .filter(|(position, _)| position.len() == 16)
            .and_then(|(position, _)| u64::from_str_radix(position, 16).ok())
            .map(|position| (position, 2))
            .ok_or_else(|| {
                TabaError::new(
                    ErrorCode::MessageStaleReceipt,
                    "O receiptToken não é o da última entrega desta mensagem.",
                )
            })
    }

    // ---- Publicar ----

    /// `POST /topics/{topic}/messages`.
    pub async fn publish(
        self: &Arc<Self>,
        topic: &str,
        request: PublishRequest,
    ) -> Result<Published, TabaError> {
        let view = self.topics.get(topic)?;
        if let Some(prefixes) = view.definition.prefixes() {
            self.messages.validate_publish(&request)?;
            if request.body.len() as u64 > self.messages.distributor_limit() {
                return Err(TabaError::new(
                    ErrorCode::MessageTooLarge,
                    format!(
                        "O corpo passa do limite de {} bytes do tópico.",
                        self.messages.distributor_limit()
                    ),
                ));
            }
            return self
                .publish_distributor(&view.definition.name, prefixes, request)
                .await;
        }
        self.publish_queue(&view, &request)
            .await
            .map(|(id, duplicate)| Published::Queue { id, duplicate })
    }

    /// Uma mensagem numa fila: registro em todos os nós responsáveis e commit por quórum.
    async fn publish_queue(
        self: &Arc<Self>,
        view: &TopicView,
        request: &PublishRequest,
    ) -> Result<(String, Option<bool>), TabaError> {
        if view.definition.queue().is_some_and(|q| q.fifo) {
            return self
                .publish_fifo(view, request)
                .await
                .map(|(id, duplicate)| (id, Some(duplicate)));
        }
        let (meta, body) = self.messages.draft_publish(view, request)?;
        let topic = view.definition.name.clone();
        let id = meta.id.clone();
        let record = MessageRecord {
            id: id.clone(),
            topic: topic.clone(),
            body,
            content_type: meta.content_type.clone(),
            group_id: meta.group_id.clone(),
            deduplication_id: None,
            delivery_count: 0,
            invisible_until_ms: None,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
            delete_at_ms: None,
        };
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            &topic,
            (message_position(&id), 1),
            |op| Request::Write {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Register(MessageRegisterRequest {
                    operation_id: op.to_owned(),
                    coordinator_id: coordinator_id.clone(),
                    kind: WriteKind::Publish,
                    message: record.clone(),
                    source_topic: None,
                }),
            },
            |t, op| Request::Write {
                topic: t.to_owned(),
                id: id.clone(),
                step: Step::Commit(op),
            },
            |t, op| Request::Write {
                topic: t.to_owned(),
                id: id.clone(),
                step: Step::Rollback(op),
            },
            &operation_id,
        )?;
        coordinator::run(&self.service, plan).await?;
        Ok((id, None))
    }

    // ---- Publicar na fila FIFO ----

    /// Publica numa fila FIFO a partir de qualquer nó: acha o primário do grupo e repassa a ele,
    /// ou coordena aqui se for o próprio (doc 17, seção 16.4). O primário que já não é o dono
    /// responde `421` e a tabela de membros é relida, até `fifo.forward.max-attempts`.
    async fn publish_fifo(
        self: &Arc<Self>,
        view: &TopicView,
        request: &PublishRequest,
    ) -> Result<(String, bool), TabaError> {
        let (_, group) = self.messages.check_fifo_request(view, request)?;
        let topic = view.definition.name.clone();
        let position = self
            .messages
            .position_for(&topic, FIFO_PLACEHOLDER_ID, Some(&group));
        let record = MessageRecord {
            id: FIFO_PLACEHOLDER_ID.to_owned(),
            topic: topic.clone(),
            body: request.body.clone(),
            content_type: request
                .content_type
                .clone()
                .unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_owned()),
            group_id: Some(group),
            deduplication_id: request.deduplication_id.clone(),
            delivery_count: 0,
            invisible_until_ms: None,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
            delete_at_ms: None,
        };
        let count = |outcome: &str| {
            self.service
                .telemetry()
                .registry
                .add(&FIFO_FORWARD, &[("taba.outcome", outcome)], 1.0);
        };
        for attempt in 1..=self.service.rules().fifo_forward_max_attempts {
            let owners = self.service.owners_of(position)?;
            let Some(primary) = owners.into_iter().next() else {
                return Err(primary_unavailable());
            };
            let outcome = if self.service.is_self(&primary) {
                self.coordinate_fifo(view, request).await
            } else {
                match self
                    .service
                    .call_member(
                        &primary,
                        Request::FifoPublish {
                            topic: topic.clone(),
                            record: record.clone(),
                        },
                    )
                    .await
                {
                    Ok(Response::FifoPublished(FifoPublished { id, duplicate })) => {
                        count("ok");
                        return Ok((id, duplicate));
                    }
                    Ok(_) => return Err(primary_unavailable()),
                    Err(CallError::Remote(error)) => Err(error),
                    Err(_) => {
                        count("error");
                        return Err(primary_unavailable());
                    }
                }
            };
            match outcome {
                Ok(done) => return Ok(done),
                Err(error) if error.code == ErrorCode::FifoMisdirected => {
                    count("misdirected");
                    if attempt < self.service.rules().fifo_forward_max_attempts {
                        tokio::time::sleep(self.service.rules().retry_backoff).await;
                    }
                }
                Err(error)
                    if matches!(
                        error.code,
                        ErrorCode::ClusterNodeSyncing | ErrorCode::ClusterNodeDraining
                    ) =>
                {
                    count("error");
                    return Err(primary_unavailable());
                }
                Err(error) => {
                    if error.status() >= 500 {
                        count("error");
                    }
                    return Err(error);
                }
            }
        }
        Err(primary_unavailable())
    }

    /// `POST /cluster/topics/{topic}/fifo/publish`: este nó, o primário do grupo, coordena.
    pub async fn publish_fifo_for_peer(
        self: &Arc<Self>,
        topic: &str,
        record: MessageRecord,
    ) -> Result<FifoPublished, TabaError> {
        let view = self.messages.queue_view(topic)?;
        let request = PublishRequest {
            body: record.body,
            content_type: Some(record.content_type),
            group_id: record.group_id,
            deduplication_id: record.deduplication_id,
        };
        self.coordinate_fifo(&view, &request)
            .await
            .map(|(id, duplicate)| FifoPublished { id, duplicate })
    }

    fn group_lock(&self, topic: &str, group: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.group_locks.lock().unwrap_or_else(|e| e.into_inner());
        // Os grupos ociosos saem do mapa: só sobra quem alguém ainda segura.
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks
            .entry((topic.to_owned(), group.to_owned()))
            .or_default()
            .clone()
    }

    /// O que só o primário do grupo faz: conferir que é o dono (`421` se não for), que está pronto,
    /// e então deduplicar, gerar o `id` e fazer o registro + commit com o primário e o secundário no
    /// quórum. Uma publicação por vez no grupo.
    async fn coordinate_fifo(
        self: &Arc<Self>,
        view: &TopicView,
        request: &PublishRequest,
    ) -> Result<(String, bool), TabaError> {
        let (_, group) = self.messages.check_fifo_request(view, request)?;
        let topic = view.definition.name.clone();
        let position = self
            .messages
            .position_for(&topic, FIFO_PLACEHOLDER_ID, Some(&group));
        let owners = self.service.owners_of(position)?;
        if !owners.first().is_some_and(|m| self.service.is_self(m)) {
            return Err(TabaError::new(
                ErrorCode::FifoMisdirected,
                "Este nó não é o primário do grupo.",
            ));
        }
        if self.service.node_state() != NodeState::Active {
            return Err(TabaError::new(
                ErrorCode::ClusterNodeSyncing,
                "O primário do grupo ainda está em sincronização.",
            )
            .with_retry_after(5));
        }
        if self.service.drain_state() != DrainState::Active {
            return Err(TabaError::new(
                ErrorCode::ClusterNodeDraining,
                "O primário do grupo está em drain.",
            )
            .with_retry_after(5));
        }
        let lock = self.group_lock(&topic, &group);
        let _turn = lock.lock().await;
        let (meta, body) = match self.messages.draft_fifo_publish(view, request)? {
            Draft::Duplicate { id } => return Ok((id, true)),
            Draft::New { meta, body } => (meta, body),
        };
        let id = meta.id.clone();
        let record = MessageRecord {
            id: id.clone(),
            topic: topic.clone(),
            body,
            content_type: meta.content_type.clone(),
            group_id: meta.group_id.clone(),
            deduplication_id: meta.deduplication_id.clone(),
            delivery_count: 0,
            invisible_until_ms: None,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
            delete_at_ms: None,
        };
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            &topic,
            (meta.position, 2),
            |op| Request::Write {
                topic: topic.clone(),
                id: id.clone(),
                step: Step::Register(MessageRegisterRequest {
                    operation_id: op.to_owned(),
                    coordinator_id: coordinator_id.clone(),
                    kind: WriteKind::Publish,
                    message: record.clone(),
                    source_topic: None,
                }),
            },
            |t, op| Request::Write {
                topic: t.to_owned(),
                id: id.clone(),
                step: Step::Commit(op),
            },
            |t, op| Request::Write {
                topic: t.to_owned(),
                id: id.clone(),
                step: Step::Rollback(op),
            },
            &operation_id,
        )?;
        coordinator::run(&self.service, plan).await?;
        Ok((id, false))
    }

    /// Uma cópia independente em cada fila que casa com um prefixo (doc 15, seção 14.2).
    async fn publish_distributor(
        self: &Arc<Self>,
        name: &str,
        prefixes: &[String],
        request: PublishRequest,
    ) -> Result<Published, TabaError> {
        let targets = self.topics.queues_with_prefix(prefixes);
        if let Some(max) = self.messages.distributor_max_targets()
            && targets.len() as u64 > max
        {
            return Err(TabaError::new(
                ErrorCode::DistributorTooManyTargets,
                format!("A publicação casa com mais de {max} filas."),
            ));
        }
        let mut set = JoinSet::new();
        for (position, view) in targets.into_iter().enumerate() {
            let (me, request) = (self.clone(), request.clone());
            set.spawn(async move {
                let result = me.publish_queue(&view, &request).await.map(|(id, _)| id);
                (position, view.definition.name.clone(), result)
            });
        }
        let mut done = Vec::new();
        while let Some(joined) = set.join_next().await {
            if let Ok(item) = joined {
                done.push(item);
            }
        }
        done.sort_by_key(|(position, _, _)| *position);
        let deliveries: Vec<DistributorDelivery> = done
            .into_iter()
            .map(|(_, topic, result)| DistributorDelivery { topic, result })
            .collect();
        self.messages.record_distributor(name, &deliveries);
        Ok(Published::Distributor { deliveries })
    }

    // ---- Ack e invisibilidade ----

    /// `DELETE /topics/{topic}/messages/{id}?receiptToken=...`.
    pub async fn ack(
        self: &Arc<Self>,
        topic: &str,
        id: &str,
        receipt_token: Option<&str>,
    ) -> Result<(), TabaError> {
        let view = self.messages.queue_view(topic)?;
        let receipt = self.messages.ack_params(receipt_token)?;
        let topic = view.definition.name.clone();
        let route = self.route_of(&view, id, &receipt)?;
        let delete_at_ms = (route.1 > 1).then(|| self.messages.fifo_delete_at(id));
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            &topic,
            route,
            |op| Request::Ack {
                topic: topic.clone(),
                id: id.to_owned(),
                step: Step::Register(AckRegisterRequest {
                    operation_id: op.to_owned(),
                    coordinator_id: coordinator_id.clone(),
                    receipt_token: receipt.clone(),
                    delete_at_ms,
                }),
            },
            |t, op| Request::Ack {
                topic: t.to_owned(),
                id: id.to_owned(),
                step: Step::Commit(op),
            },
            |t, op| Request::Ack {
                topic: t.to_owned(),
                id: id.to_owned(),
                step: Step::Rollback(op),
            },
            &operation_id,
        )?;
        coordinator::run(&self.service, plan).await
    }

    /// `PUT /topics/{topic}/messages/{id}/visibility`.
    pub async fn set_visibility(
        self: &Arc<Self>,
        topic: &str,
        id: &str,
        receipt_token: Option<&str>,
        visibility_timeout: Option<i64>,
    ) -> Result<VisibilityUpdated, TabaError> {
        let view = self.messages.queue_view(topic)?;
        let (receipt, secs) = self
            .messages
            .visibility_params(receipt_token, visibility_timeout)?;
        let until_ms = self.messages.now_millis() + secs * 1000;
        let route = self.route_of(&view, id, &receipt)?;
        self.replicate_visibility(
            view.definition.name.as_str(),
            id,
            route,
            VisibilityKind::Change,
            until_ms,
            &receipt,
            None,
        )
        .await?;
        Ok(VisibilityUpdated {
            id: id.to_owned(),
            invisible_until_ms: until_ms,
        })
    }

    /// Registro + commit da invisibilidade de uma mensagem (entrega ou alteração).
    #[allow(clippy::too_many_arguments)]
    async fn replicate_visibility(
        self: &Arc<Self>,
        topic: &str,
        id: &str,
        route: (u64, usize),
        kind: VisibilityKind,
        until_ms: u64,
        receipt_token: &str,
        delivery_count: Option<u32>,
    ) -> Result<(), TabaError> {
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            topic,
            route,
            |op| Request::Visibility {
                topic: topic.to_owned(),
                id: id.to_owned(),
                step: Step::Register(VisibilityRegisterRequest {
                    operation_id: op.to_owned(),
                    coordinator_id: coordinator_id.clone(),
                    kind,
                    invisible_until_ms: until_ms,
                    receipt_token: receipt_token.to_owned(),
                    delivery_count,
                }),
            },
            |t, op| Request::Visibility {
                topic: t.to_owned(),
                id: id.to_owned(),
                step: Step::Commit(op),
            },
            |t, op| Request::Visibility {
                topic: t.to_owned(),
                id: id.to_owned(),
                step: Step::Rollback(op),
            },
            &operation_id,
        )?;
        coordinator::run(&self.service, plan).await
    }

    // ---- Dead-letter ----

    /// O movimento de uma mensagem para a dead-letter, coordenado pelo dono: uma operação só que
    /// cria a mensagem na dead-letter e a remove da fila de origem (doc 13, seção 12.10).
    pub async fn move_to_dead_letter(
        self: &Arc<Self>,
        source: &str,
        id: &str,
    ) -> Result<(), TabaError> {
        let messages = self.messages.clone();
        let (source_owned, id_owned) = (source.to_owned(), id.to_owned());
        let held =
            tokio::task::spawn_blocking(move || messages.read_message(&source_owned, &id_owned))
                .await
                .map_err(|_| TabaError::internal())??;
        let Some((meta, body)) = held else {
            // Já não está aqui: ack ou ttl ganharam a corrida.
            return Ok(());
        };
        let target = format!("{source}{}", self.topics.rules().dead_letter_suffix);
        let at_ms = self.messages.now_millis();
        // A dead-letter de uma FIFO fica no bloco do grupo da fila de origem (doc 17, seção 16.3).
        let fifo = self
            .topics
            .get(source)
            .is_ok_and(|v| v.definition.queue().is_some_and(|q| q.fifo));
        let route = if fifo {
            (meta.position, 2)
        } else {
            (message_position(id), 1)
        };
        let record = MessageRecord {
            id: id.to_owned(),
            topic: target.clone(),
            body,
            content_type: meta.content_type,
            group_id: meta.group_id,
            deduplication_id: None,
            delivery_count: 0,
            invisible_until_ms: None,
            receipt_token: None,
            dead_lettered_ms: Some(at_ms),
            dead_letter_reason: Some("max-deliveries".to_owned()),
            delete_at_ms: None,
        };
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            &target,
            route,
            |op| Request::Write {
                topic: target.clone(),
                id: id.to_owned(),
                step: Step::Register(MessageRegisterRequest {
                    operation_id: op.to_owned(),
                    coordinator_id: coordinator_id.clone(),
                    kind: WriteKind::DeadLetter,
                    message: record.clone(),
                    source_topic: Some(source.to_owned()),
                }),
            },
            |t, op| Request::Write {
                topic: t.to_owned(),
                id: id.to_owned(),
                step: Step::Commit(op),
            },
            |t, op| Request::Write {
                topic: t.to_owned(),
                id: id.to_owned(),
                step: Step::Rollback(op),
            },
            &operation_id,
        )?;
        coordinator::run(&self.service, plan).await
    }

    /// A tarefa que esvazia a fila de movimentos para a dead-letter que o motor preenche.
    pub async fn run_dead_letters(
        self: Arc<Self>,
        mut jobs: tokio::sync::mpsc::UnboundedReceiver<crate::engine::messages::DeadLetterJob>,
    ) {
        let cancel = self.service.cancel_token().clone();
        loop {
            let job = tokio::select! {
                () = cancel.cancelled() => return,
                job = jobs.recv() => job,
            };
            let Some(job) = job else { return };
            let me = self.clone();
            tokio::spawn(async move {
                let moved = me.move_to_dead_letter(&job.topic, &job.id).await;
                if let Err(error) = &moved {
                    tracing::warn!(topic = %job.topic, id = %job.id, %error, "movimento para a dead-letter não concluído");
                }
                let messages = me.messages.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    messages.dead_letter_done(&job.topic, &job.id, moved.is_ok());
                })
                .await;
            });
        }
    }

    // ---- Consumir ----

    /// `GET /topics/{topic}/messages`.
    pub async fn consume(
        self: &Arc<Self>,
        topic: &str,
        query: ConsumeQuery,
    ) -> Result<Vec<Delivered>, TabaError> {
        let (view, params) = self.messages.prepare_consume(topic, &query).await?;
        let name = view.definition.name.clone();
        let want = usize::try_from(params.max_messages).unwrap_or(usize::MAX);
        let notify = self.messages.listener_notify(&name);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(params.wait_secs);
        let mut polling = None;
        let mut listener = None;
        loop {
            // O interesse é registrado antes de olhar, para não perder uma mensagem que chegue
            // entre o olhar e a espera.
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let view = self.topics.get(&name)?;
            let (mut delivered, failures) =
                self.take_local(&view, want, params.visibility_secs).await?;
            if delivered.len() < want {
                let more = self
                    .take_remote(&name, want - delivered.len(), params.visibility_secs)
                    .await;
                delivered.extend(more);
            }
            let (next_visible, more_ready) = self.messages.wait_hints(&name);
            if !delivered.is_empty() && more_ready {
                notify.notify_one();
            }
            if !delivered.is_empty()
                || tokio::time::Instant::now() >= deadline
                || self.messages.is_draining()
            {
                if delivered.is_empty() && failures > 0 {
                    return Err(delivery_unavailable());
                }
                if let Some(poll) = polling.as_mut() {
                    let poll: &mut crate::engine::messages::LongPoll<'_> = poll;
                    poll.delivered = Some(!delivered.is_empty());
                }
                return Ok(delivered);
            }
            if failures > 0 {
                return Err(delivery_unavailable());
            }
            polling.get_or_insert_with(|| self.messages.begin_long_poll());
            listener.get_or_insert_with(|| self.signals.listener(&name));
            let mut until = deadline;
            if let Some(at) = next_visible {
                let in_ms = at.saturating_sub(self.messages.now_millis()).max(10);
                until = until.min(tokio::time::Instant::now() + Duration::from_millis(in_ms));
            }
            let _ = tokio::time::timeout_at(until, notified).await;
        }
    }

    /// As mensagens de que este nó é o dono: escolhe, registra a invisibilidade de cada uma nos
    /// nós responsáveis (todas ao mesmo tempo) e entrega as que o quórum confirmou. As que não
    /// fecharam voltam a ser prontas. Devolve também quantas falharam.
    async fn take_local(
        self: &Arc<Self>,
        view: &TopicView,
        max: usize,
        visibility_secs: u64,
    ) -> Result<(Vec<Delivered>, usize), TabaError> {
        let (confirmed, failures) = self.take_confirmed(view, max, visibility_secs).await?;
        Ok((
            confirmed
                .into_iter()
                .map(Candidate::into_delivered)
                .collect(),
            failures,
        ))
    }

    /// Como `take_local`, mas devolve os candidatos confirmados (o `take` de outro nó precisa dos
    /// bytes do corpo).
    async fn take_confirmed(
        self: &Arc<Self>,
        view: &TopicView,
        max: usize,
        visibility_secs: u64,
    ) -> Result<(Vec<Candidate>, usize), TabaError> {
        let started = std::time::Instant::now();
        let topic = view.definition.name.clone();
        let fifo = view.definition.queue().is_some_and(|q| q.fifo);
        let attempts = self.service.rules().get_max_attempts;
        let (mut confirmed, mut failed) = (Vec::new(), 0usize);
        let mut aborted = Vec::new();
        // Uma mensagem que falha fica reservada até o fim do `GET` e o laço segue para o próximo
        // ID da lista, sem esperar (doc 13, seção 12.2): são tolerados até
        // `delivery.get.max-attempts` registros que não fecham.
        while confirmed.len() < max && failed < attempts {
            let missing = max - confirmed.len();
            let candidates = {
                let (messages, view) = (self.messages.clone(), view.clone());
                let span = tracing::Span::current();
                tokio::task::spawn_blocking(move || {
                    span.in_scope(|| messages.take_candidates(&view, missing, visibility_secs))
                })
                .await
                .map_err(|_| TabaError::internal())??
            };
            if candidates.is_empty() {
                break;
            }
            let mut set = JoinSet::new();
            for (position, candidate) in candidates.iter().enumerate() {
                let me = self.clone();
                let topic = topic.clone();
                let id = candidate.meta.id.clone();
                let until = candidate.meta.invisible_until_ms;
                let token = candidate.meta.receipt_token.clone().unwrap_or_default();
                let count = candidate.meta.delivery_count;
                let route = (candidate.meta.position, if fifo { 2 } else { 1 });
                set.spawn(async move {
                    let result = me
                        .replicate_visibility(
                            &topic,
                            &id,
                            route,
                            VisibilityKind::Delivery,
                            until,
                            &token,
                            Some(count),
                        )
                        .await;
                    (position, result)
                });
            }
            let mut ok = vec![false; candidates.len()];
            while let Some(joined) = set.join_next().await {
                if let Ok((position, Ok(()))) = joined {
                    ok[position] = true;
                }
            }
            for (candidate, ok) in candidates.into_iter().zip(ok) {
                if ok {
                    confirmed.push(candidate);
                } else {
                    failed += 1;
                    aborted.push(candidate);
                }
            }
        }
        let messages = self.messages.clone();
        let topic_for_blocking = topic.clone();
        let confirmed = tokio::task::spawn_blocking(move || {
            for candidate in &aborted {
                messages.abort_take(&topic_for_blocking, candidate);
            }
            let refs: Vec<&Candidate> = confirmed.iter().collect();
            messages.finish_take(&topic_for_blocking, &refs, started);
            confirmed
        })
        .await
        .map_err(|_| TabaError::internal())?;
        Ok((confirmed, failed))
    }

    /// Pede a outros nós, que o sinal diz terem mensagens prontas, as `missing` que faltam ao
    /// `GET` (doc 13, seção 12.8). Tenta no máximo `delivery.take.max-attempts` nós.
    async fn take_remote(
        self: &Arc<Self>,
        topic: &str,
        mut missing: usize,
        visibility_secs: u64,
    ) -> Vec<Delivered> {
        let mut out = Vec::new();
        let peers = self.signals.ready_peers(topic);
        let table = self.service.membership().table();
        for node in peers
            .into_iter()
            .take(self.service.rules().take_max_attempts)
        {
            if missing == 0 {
                break;
            }
            let Some(member) = table.as_ref().and_then(|t| t.member(&node)).cloned() else {
                continue;
            };
            let request = Request::Take {
                topic: topic.to_owned(),
                request: TakeRequest {
                    max_messages: u32::try_from(missing).unwrap_or(u32::MAX),
                    visibility_timeout: visibility_secs,
                },
            };
            let outcome = match self.service.call_member(&member, request).await {
                Ok(crate::cluster::protocol::Response::Taken(TakeResponse { messages })) => {
                    if messages.is_empty() {
                        self.signals.mark_empty(topic, &node);
                        "empty"
                    } else {
                        for record in messages {
                            out.push(delivered_from(record));
                            missing = missing.saturating_sub(1);
                        }
                        "delivered"
                    }
                }
                _ => "error",
            };
            self.service.telemetry().registry.add(
                &DELIVERY_TAKE,
                &[("taba.direction", "out"), ("taba.outcome", outcome)],
                1.0,
            );
        }
        out
    }

    /// `POST /cluster/topics/{topic}/messages/take`: este nó, como dono, entrega até `max`
    /// mensagens prontas a quem pediu. Em drain responde vazio.
    pub async fn take_for_peer(
        self: &Arc<Self>,
        topic: &str,
        request: TakeRequest,
    ) -> Result<TakeResponse, TabaError> {
        let view = self.messages.queue_view(topic)?;
        let counted = |outcome: &str| {
            self.service.telemetry().registry.add(
                &DELIVERY_TAKE,
                &[("taba.direction", "in"), ("taba.outcome", outcome)],
                1.0,
            );
        };
        if self.messages.is_draining() {
            counted("empty");
            return Ok(TakeResponse {
                messages: Vec::new(),
            });
        }
        let max = usize::try_from(request.max_messages).unwrap_or(usize::MAX);
        let (confirmed, _) = self
            .take_confirmed(&view, max, request.visibility_timeout)
            .await?;
        counted(if confirmed.is_empty() {
            "empty"
        } else {
            "delivered"
        });
        Ok(TakeResponse {
            messages: confirmed
                .into_iter()
                .map(|c| MessageRecord {
                    id: c.meta.id.clone(),
                    topic: view.definition.name.clone(),
                    body: c.body,
                    content_type: c.meta.content_type,
                    group_id: c.meta.group_id,
                    deduplication_id: c.meta.deduplication_id,
                    delivery_count: c.meta.delivery_count,
                    invisible_until_ms: Some(c.meta.invisible_until_ms),
                    receipt_token: c.meta.receipt_token,
                    dead_lettered_ms: c.meta.dead_lettered_ms,
                    dead_letter_reason: c.meta.dead_letter_reason,
                    delete_at_ms: None,
                })
                .collect(),
        })
    }

    /// Os membros que são donos de alguma coisa não precisam ser conhecidos aqui; este método só
    /// existe para os testes e para o repasse escolherem entre os membros.
    pub fn peers(&self) -> Vec<Member> {
        self.service
            .membership()
            .table()
            .map(|t| {
                t.members
                    .into_iter()
                    .filter(|m| !self.service.is_self(m))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A mensagem que um `take` trouxe, como o `GET` do cliente a devolve.
fn delivered_from(record: MessageRecord) -> Delivered {
    let content_type = ContentType::parse(&record.content_type)
        .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("válido"));
    let published_ms = record.published_ms();
    let _: Option<&RawValue> = None;
    Delivered {
        id: record.id,
        body: Body::render(&content_type, &record.body),
        content_type: record.content_type,
        group_id: record.group_id,
        deduplication_id: record.deduplication_id,
        receipt_token: record.receipt_token.unwrap_or_default(),
        delivery_count: record.delivery_count,
        published_ms,
        invisible_until_ms: record.invisible_until_ms.unwrap_or(0),
        dead_lettered_ms: record.dead_lettered_ms,
        dead_letter_reason: record.dead_letter_reason,
    }
}
