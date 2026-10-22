//! The messages API of a node **in the cluster** (doc 06a, "Messages"; doc 13): what the client
//! calls (`POST`, `GET`, ack and `PUT visibility`), now with the write, invisibility and ack
//! decided by quorum on the responsible nodes and delivery completed with `take` on other nodes.
//!
//! The node that receives the call is the coordinator, whichever it is. `GET` delivers what the
//! node itself owns and, if short, asks the owner of other messages (`take`); with nothing,
//! it waits as a listener (long polling).

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

/// One lock per FIFO (topic, group).
type GroupLocks = Mutex<HashMap<(String, String), Arc<tokio::sync::Mutex<()>>>>;

pub struct ClusterMessages {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    topics: Arc<Topics>,
    signals: Arc<Signals>,
    /// One publication at a time in each FIFO group of this node (the primary): it is what orders the group and
    /// serializes deduplication (doc 17, sections 16.4 and 16.7).
    group_locks: GroupLocks,
}

fn primary_unavailable() -> TabaError {
    TabaError::new(
        ErrorCode::FifoPrimaryUnavailable,
        "The group's primary is not available to coordinate the publication.",
    )
    .with_retry_after(5)
}

fn delivery_unavailable() -> TabaError {
    TabaError::new(
        ErrorCode::DeliveryUnavailable,
        "Could not deliver: the quorum did not close for the chosen messages.",
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

    /// Where the operation goes: the ring position and how many of the first responsible nodes the quorum
    /// must include (1 on the plain queue; on FIFO, the primary and the secondary).
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

    /// The position and mandatory roles of an ack or an invisibility change. On FIFO
    /// the position is the group's, which the receipt carries (`{position}-{guid}`); a malformed receipt is
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
                    "The receiptToken is not that of this message's last delivery.",
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
                        "The body exceeds the topic's limit of {} bytes.",
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

    /// A message on a queue: register on all the responsible nodes and commit by quorum.
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

    // ---- Publishing to the FIFO queue ----

    /// Publishes to a FIFO queue from any node: finds the group's primary and forwards to it,
    /// or coordinates here if it is itself (doc 17, section 16.4). A primary that is no longer the owner
    /// answers `421` and the member table is re-read, up to `fifo.forward.max-attempts`.
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

    /// `POST /cluster/topics/{topic}/fifo/publish`: this node, the group's primary, coordinates.
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
        // Idle groups leave the map: only whoever someone still holds remains.
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks
            .entry((topic.to_owned(), group.to_owned()))
            .or_default()
            .clone()
    }

    /// What only the group's primary does: check that it is the owner (`421` if not), that it is ready,
    /// and then deduplicate, generate the `id` and do the register + commit with the primary and the secondary in the
    /// quorum. One publication at a time in the group.
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
                "This node is not the group's primary.",
            ));
        }
        if self.service.node_state() != NodeState::Active {
            return Err(TabaError::new(
                ErrorCode::ClusterNodeSyncing,
                "The group's primary is still syncing.",
            )
            .with_retry_after(5));
        }
        if self.service.drain_state() != DrainState::Active {
            return Err(TabaError::new(
                ErrorCode::ClusterNodeDraining,
                "The group's primary is in drain.",
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

    /// An independent copy in each queue that matches a prefix (doc 15, section 14.2).
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
                format!("The publication matches more than {max} queues."),
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
                    discard: false,
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

    /// The release of the restoration discards a FIFO duplicate this node, the group's primary, accepted while the
    /// queue was held (doc 37, section 4.10): an ack on the owners with no receipt.
    pub async fn discard(self: &Arc<Self>, topic: &str, id: &str) -> Result<(), TabaError> {
        let view = self.messages.queue_view(topic)?;
        let Some(info) = self.messages.message_info(topic, id)? else {
            return Ok(());
        };
        let topic = view.definition.name.clone();
        let route = (info.meta.position, 2);
        let delete_at_ms = Some(self.messages.fifo_delete_at(id));
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
                    receipt_token: String::new(),
                    delete_at_ms,
                    discard: true,
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

    /// Register + commit of a message's invisibility (delivery or change).
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

    /// The move of a message to the dead-letter, coordinated by the owner: a single operation that
    /// creates the message in the dead-letter and removes it from the source queue (doc 13, section 12.10).
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
            // It is no longer here: ack or ttl won the race.
            return Ok(());
        };
        let target = format!("{source}{}", self.topics.rules().dead_letter_suffix);
        let at_ms = self.messages.now_millis();
        // A FIFO's dead-letter stays in the block of the source queue's group (doc 17, section 16.3).
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

    /// The task that empties the queue of moves to the dead-letter that the engine fills.
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
                    tracing::warn!(topic = %job.topic, id = %job.id, %error, "move to the dead-letter not completed");
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
            // Interest is registered before looking, so as not to miss a message that arrives
            // between the look and the wait.
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

    /// The messages this node owns: chooses, registers each one's invisibility on the
    /// responsible nodes (all at once) and delivers those the quorum confirmed. Those that did not
    /// close go back to ready. Also returns how many failed.
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

    /// Like `take_local`, but returns the confirmed candidates (another node's `take` needs the
    /// body bytes).
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
        // A message that fails stays reserved until the end of the `GET` and the loop goes on to the next
        // ID on the list, without waiting (doc 13, section 12.2): up to
        // `delivery.get.max-attempts` registers that do not close are tolerated.
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

    /// Asks other nodes, which the signal says have ready messages, for the `missing` the
    /// `GET` lacks (doc 13, section 12.8). Tries at most `delivery.take.max-attempts` nodes.
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
                        self.signals.notified_outcome(topic, &node, false);
                        "empty"
                    } else {
                        self.signals.notified_outcome(topic, &node, true);
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

    /// `POST /cluster/topics/{topic}/messages/take`: this node, as the owner, delivers up to `max`
    /// ready messages to whoever asked. In drain it answers empty.
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

    /// The members that own something need not be known here; this method only
    /// exists for the tests and for the forward to choose among the members.
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

/// The message a `take` brought, as the client's `GET` returns it.
fn delivered_from(record: MessageRecord) -> Delivered {
    let content_type = ContentType::parse(&record.content_type)
        .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("valid"));
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
