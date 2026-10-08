//! A API de mensagens de um nó **no cluster** (doc 06a, "Mensagens"; doc 13): o que o cliente
//! chama (`POST`, `GET`, ack e `PUT visibility`), agora com a escrita, a invisibilidade e o ack
//! decididos por quórum nos nós responsáveis e a entrega completada com o `take` em outros nós.
//!
//! O nó que recebe a chamada é o coordenador, qualquer que seja ele. O `GET` entrega o que o
//! próprio nó tem como dono e, se faltar, pede ao dono de outras mensagens (`take`); sem nada,
//! espera como ouvinte (long polling).

use std::sync::Arc;
use std::time::Duration;

use serde_json::value::RawValue;
use tokio::task::JoinSet;

use super::coordinator::{self, Plan};
use super::signals::Signals;
use crate::cluster::members::Member;
use crate::cluster::protocol::{
    AckRegisterRequest, MessageRecord, MessageRegisterRequest, OperationRef, Request, Step,
    TakeRequest, TakeResponse, VisibilityKind, VisibilityRegisterRequest, WriteKind,
};
use crate::cluster::service::ClusterService;
use crate::core::content::{Body, ContentType};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring::message_position;
use crate::engine::messages::{
    Candidate, ConsumeQuery, Delivered, DistributorDelivery, Messages, PublishRequest, Published,
    VisibilityUpdated,
};
use crate::engine::topics::{TopicView, Topics};
use crate::telemetry::metrics::DELIVERY_TAKE;

pub struct ClusterMessages {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    topics: Arc<Topics>,
    signals: Arc<Signals>,
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
        })
    }

    pub fn messages(&self) -> &Arc<Messages> {
        &self.messages
    }

    pub fn signals(&self) -> &Arc<Signals> {
        &self.signals
    }

    fn operation_plan(
        &self,
        topic: &str,
        id: &str,
        make: impl Fn(&str) -> Request,
        commit: impl Fn(&str, OperationRef) -> Request,
        rollback: impl Fn(&str, OperationRef) -> Request,
        operation_id: &str,
    ) -> Result<Plan, TabaError> {
        let owners = self.service.owners_of(message_position(id))?;
        let op = || OperationRef {
            operation_id: operation_id.to_owned(),
        };
        Ok(Plan {
            owners,
            quorum: self.service.rules().quorum as usize,
            register: make(operation_id),
            commit: commit(topic, op()),
            rollback: rollback(topic, op()),
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
            .map(|id| Published::Queue {
                id,
                duplicate: None,
            })
    }

    /// Uma mensagem numa fila: registro em todos os nós responsáveis e commit por quórum.
    async fn publish_queue(
        self: &Arc<Self>,
        view: &TopicView,
        request: &PublishRequest,
    ) -> Result<String, TabaError> {
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
        };
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            &topic,
            &id,
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
        Ok(id)
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
                let result = me.publish_queue(&view, &request).await;
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
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            &topic,
            id,
            |op| Request::Ack {
                topic: topic.clone(),
                id: id.to_owned(),
                step: Step::Register(AckRegisterRequest {
                    operation_id: op.to_owned(),
                    coordinator_id: coordinator_id.clone(),
                    receipt_token: receipt.clone(),
                    delete_at_ms: None,
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
        self.replicate_visibility(
            view.definition.name.as_str(),
            id,
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
    async fn replicate_visibility(
        self: &Arc<Self>,
        topic: &str,
        id: &str,
        kind: VisibilityKind,
        until_ms: u64,
        receipt_token: &str,
        delivery_count: Option<u32>,
    ) -> Result<(), TabaError> {
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            topic,
            id,
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
        };
        let operation_id = self.service.new_operation_id();
        let coordinator_id = self.service.node_id().to_owned();
        let plan = self.operation_plan(
            &target,
            id,
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
        let candidates = {
            let (messages, view) = (self.messages.clone(), view.clone());
            tokio::task::spawn_blocking(move || {
                messages.take_candidates(&view, max, visibility_secs)
            })
            .await
            .map_err(|_| TabaError::internal())??
        };
        if candidates.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut set = JoinSet::new();
        for (position, candidate) in candidates.iter().enumerate() {
            let me = self.clone();
            let topic = topic.clone();
            let id = candidate.meta.id.clone();
            let until = candidate.meta.invisible_until_ms;
            let token = candidate.meta.receipt_token.clone().unwrap_or_default();
            let count = candidate.meta.delivery_count;
            set.spawn(async move {
                let result = me
                    .replicate_visibility(
                        &topic,
                        &id,
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
        let (mut confirmed, mut failed) = (Vec::new(), 0usize);
        let mut aborted = Vec::new();
        for (candidate, ok) in candidates.into_iter().zip(ok) {
            if ok {
                confirmed.push(candidate);
            } else {
                failed += 1;
                aborted.push(candidate);
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
