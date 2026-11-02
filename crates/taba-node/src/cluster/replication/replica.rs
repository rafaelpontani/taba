//! The side that **receives** the register, the commit and the rollback of an operation on a message
//! (doc 05, section 7.2; doc 06c, "Messages"): publication, the move to the dead-letter,
//! invisibility and the ack.
//!
//! The register (phase 1) only checks and leaves the operation **pending**, without changing anything a
//! consumer sees; the commit (phase 2) applies it to the message engine; the rollback discards it. The
//! three phases are idempotent by `operationId`. A pending operation with neither commit nor rollback
//! disappears by itself after `write.pending.ttl`. Only one pending operation per message: the second is
//! `409 cluster.conflict`.

use bytes::Bytes;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
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

/// How many already-decided operations the node remembers, to repeat the `Ack` of a repeated commit or rollback
/// instead of saying the operation is unknown.
const FINISHED_MEMORY: usize = 4096;

/// What the commit does.
enum Action {
    Publish {
        meta: MessageMeta,
        body: Bytes,
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
    /// The topic where the message is (on the dead-letter, the source one).
    topic: String,
    id: String,
    registered_at: SystemTime,
    /// Order of registration: with `registered_at` it is the key of the operation in `State::by_age`.
    seq: u64,
    action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Finished {
    Committed,
    RolledBack,
}

/// The decided operations the node still remembers: lookup by `operationId` without scanning, and the oldest
/// leaves first when the memory is full.
#[derive(Default)]
struct Decided {
    outcomes: HashMap<String, (Finished, u64)>,
    /// `operationId` and the number it was remembered with, oldest first. An entry whose number is no longer the
    /// one in `outcomes` is a repeated decision, already replaced.
    order: VecDeque<(String, u64)>,
    next: u64,
}

impl Decided {
    fn get(&self, operation_id: &str) -> Option<Finished> {
        self.outcomes.get(operation_id).map(|(outcome, _)| *outcome)
    }

    fn remember(&mut self, operation_id: &str, outcome: Finished) {
        self.next += 1;
        self.outcomes
            .insert(operation_id.to_owned(), (outcome, self.next));
        self.order.push_back((operation_id.to_owned(), self.next));
        while self.outcomes.len() > FINISHED_MEMORY {
            let Some((oldest, number)) = self.order.pop_front() else {
                break;
            };
            if self
                .outcomes
                .get(&oldest)
                .is_some_and(|(_, n)| *n == number)
            {
                self.outcomes.remove(&oldest);
            }
        }
    }
}

#[derive(Default)]
struct State {
    ops: HashMap<String, Pending>,
    /// `(topic, id)` → pending operation.
    by_message: HashMap<(String, String), String>,
    /// The pending operations from the oldest to the newest: the sweep of the expired ones only touches the
    /// expired ones, not every pending operation of every call.
    by_age: BTreeMap<(SystemTime, u64), String>,
    next_seq: u64,
    finished: Decided,
}

impl State {
    /// Takes the operation out of the pendings and out of the indexes that point to it.
    fn take(&mut self, operation_id: &str) -> Option<Pending> {
        let op = self.ops.remove(operation_id)?;
        self.by_message.remove(&(op.topic.clone(), op.id.clone()));
        self.by_age.remove(&(op.registered_at, op.seq));
        Some(op)
    }
}

/// The reservation of a message while its register is checked: released if the check fails or the call is dropped.
struct Reservation<'a> {
    replica: &'a Replica,
    operation_id: &'a str,
    armed: bool,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.replica.lock().take(self.operation_id);
        }
    }
}

/// A message taken by [`Replica::hold`]: released when it goes.
pub(super) struct Held<'a> {
    replica: &'a Replica,
    key: (String, String),
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.replica.lock().by_message.remove(&self.key);
    }
}

/// A register of a batch between nodes, ready to reserve and check ([`Replica::register_many`]).
pub(super) struct RegisterItem {
    operation_id: String,
    topic: String,
    id: String,
    action: Action,
    check: Check,
}

/// What a register checks before it stays, as in its single call.
#[derive(Clone)]
enum Check {
    Nothing,
    /// The publication: the topic stores messages, and an `id` that already exists only passes with the same content.
    Publish(Bytes),
    /// The delivery or the change of deadline (the `receiptToken`), and the ack.
    Visibility(VisibilityCheck, String),
}

impl Check {
    fn run(
        &self,
        messages: &Messages,
        topics: &Topics,
        topic: &str,
        id: &str,
    ) -> Result<(), TabaError> {
        match self {
            Check::Nothing => Ok(()),
            Check::Publish(body) => check_publish(messages, topics, topic, id, body),
            Check::Visibility(check, token) => messages.check_visibility(topic, id, *check, token),
        }
    }
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
        "The node does not know that operation (it never arrived or already expired).",
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

/// Applies a decided operation to the engine.
fn apply(messages: &Messages, topic: &str, id: &str, action: Action) -> Result<(), TabaError> {
    match action {
        Action::Publish { meta, body } => messages.apply_publish(topic, meta, body),
        Action::DeadLetter { source, at_ms } => messages.apply_dead_letter(&source, id, at_ms),
        Action::Visibility { until_ms, delivery } => messages.apply_visibility(
            topic,
            id,
            until_ms,
            delivery
                .as_ref()
                .map(|(count, token)| (*count, token.as_str())),
        ),
        Action::Ack => messages.apply_ack(topic, id),
    }
}

/// The message a register of a publication brings, as the engine stores it.
fn publish_of(record: crate::cluster::protocol::MessageRecord) -> (MessageMeta, Bytes) {
    let meta = MessageMeta {
        id: record.id.clone(),
        content_type: record.content_type.clone(),
        group_id: record.group_id.clone(),
        deduplication_id: record.deduplication_id.clone(),
        published_ms: record.published_ms(),
        delivery_count: record.delivery_count,
        invisible_until_ms: record.invisible_until_ms.unwrap_or(0),
        receipt_token: record.receipt_token.clone(),
        dead_lettered_ms: None,
        dead_letter_reason: None,
        position: 0,
    };
    (meta, record.body)
}

/// The register of a publication: the topic stores messages, and an `id` that already exists only
/// passes with the same content (a repeated register is idempotent; another content, a conflict).
pub(super) fn check_publish(
    messages: &Messages,
    topics: &Topics,
    topic: &str,
    id: &str,
    body: &[u8],
) -> Result<(), TabaError> {
    let view = topics.get(topic)?;
    if view.definition.queue().is_none() {
        return Err(TabaError::new(
            ErrorCode::TopicWrongType,
            "A distributor topic does not store messages.",
        ));
    }
    match messages.message_info(topic, id)? {
        None => Ok(()),
        Some(_) => {
            let same = messages
                .read_message(topic, id)?
                .is_some_and(|(_, held)| held == body);
            if same {
                Ok(())
            } else {
                Err(conflict(
                    "The message already exists with different content.",
                ))
            }
        }
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

    /// How many operations are pending (without the expired ones).
    pub fn pending(&self) -> usize {
        let mut state = self.lock();
        self.purge(&mut state);
        state.ops.len()
    }

    /// Discards the pendings that passed `write.pending.ttl`.
    fn purge(&self, state: &mut State) {
        let now = self.clock.now();
        while let Some(entry) = state.by_age.first_entry() {
            let (registered_at, _) = *entry.key();
            let expired = now
                .duration_since(registered_at)
                .is_ok_and(|age| age >= self.pending_ttl);
            if !expired {
                break;
            }
            let id = entry.remove();
            state.take(&id);
            self.telemetry
                .registry
                .add(&WRITE_PENDING_EXPIRED, &[], 1.0);
            tracing::warn!(operation_id = %id, "pending register expired without commit or rollback");
        }
    }

    fn remember(state: &mut State, operation_id: &str, outcome: Finished) {
        state.finished.remember(operation_id, outcome);
    }

    /// Leaves the operation pending, or confirms again the one that already was (idempotent).
    ///
    /// The message is **reserved first** and `check` runs after, with the reservation held: the check reads the
    /// state of the engine (the receipt, whether the message is still there), and an operation that registered and
    /// committed between the check and the reservation (a move to the dead-letter, a new delivery) would leave
    /// the register answering for a message that is no longer the one the check saw (PRB-077). If the check fails
    /// or the call is dropped, the reservation is released.
    async fn register<C>(
        &self,
        operation_id: &str,
        topic: &str,
        id: &str,
        action: Action,
        check: C,
    ) -> Result<Ack, TabaError>
    where
        C: Future<Output = Result<(), TabaError>>,
    {
        if !self.reserve(operation_id, topic, id, action)? {
            return Ok(ack(operation_id));
        }
        let mut reservation = Reservation {
            replica: self,
            operation_id,
            armed: true,
        };
        check.await?;
        reservation.armed = false;
        Ok(ack(operation_id))
    }

    /// Takes the message for an operation this node applies alone (factor 1, D191): no register of another
    /// operation on the same message runs while the guard lives, as with a pending operation.
    pub(super) fn hold(&self, topic: &str, id: &str) -> Result<Held<'_>, TabaError> {
        let key = (topic.to_owned(), id.to_owned());
        let mut state = self.lock();
        self.purge(&mut state);
        if state.by_message.contains_key(&key) {
            return Err(conflict(
                "There is another pending operation for this message.",
            ));
        }
        state.by_message.insert(key.clone(), String::new());
        Ok(Held { replica: self, key })
    }

    /// Takes the message for the operation. `false` when the operation was already pending or decided.
    fn reserve(
        &self,
        operation_id: &str,
        topic: &str,
        id: &str,
        action: Action,
    ) -> Result<bool, TabaError> {
        let mut state = self.lock();
        self.purge(&mut state);
        if state.ops.contains_key(operation_id) || state.finished.get(operation_id).is_some() {
            return Ok(false);
        }
        let key = (topic.to_owned(), id.to_owned());
        if state.by_message.contains_key(&key) {
            return Err(conflict(
                "There is another pending operation for this message.",
            ));
        }
        state.by_message.insert(key, operation_id.to_owned());
        let registered_at = self.clock.now();
        state.next_seq += 1;
        let seq = state.next_seq;
        state
            .by_age
            .insert((registered_at, seq), operation_id.to_owned());
        state.ops.insert(
            operation_id.to_owned(),
            Pending {
                topic: topic.to_owned(),
                id: id.to_owned(),
                registered_at,
                seq,
                action,
            },
        );
        Ok(true)
    }

    /// The commit: applies the pending operation to the engine.
    async fn commit(&self, operation_id: &str) -> Result<Ack, TabaError> {
        let pending = {
            let mut state = self.lock();
            self.purge(&mut state);
            match state.take(operation_id) {
                Some(op) => op,
                None => {
                    return match state.finished.get(operation_id) {
                        Some(Finished::Committed) => Ok(ack(operation_id)),
                        Some(Finished::RolledBack) => {
                            Err(conflict("The operation was cancelled (rollback)."))
                        }
                        None => Err(unknown_operation()),
                    };
                }
            }
        };
        let Pending {
            topic, id, action, ..
        } = pending;
        let result = if self.is_light(&topic, &id, &action) {
            apply(&self.messages, &topic, &id, action)
        } else {
            let messages = self.messages.clone();
            // O commit em disco é esperado sem o tópico preso: as réplicas do mesmo tópico entram no mesmo commit.
            tokio::task::spawn_blocking(move || {
                messages
                    .apply_replicated(|| vec![apply(&messages, &topic, &id, action)])
                    .pop()
                    .unwrap_or_else(|| Err(TabaError::internal()))
            })
            .await
            .map_err(|_| TabaError::internal())?
        };
        // Applied or not, the operation left the pendings; the error (a message that vanished, for
        // example) goes to the coordinator.
        Self::remember(&mut self.lock(), operation_id, Finished::Committed);
        result.map(|()| ack(operation_id))
    }

    /// What a register of a batch reserves and checks, built from the call (the same checks of its single call).
    pub(super) fn register_item(&self, request: Request) -> Result<RegisterItem, TabaError> {
        match request {
            Request::Write {
                topic,
                id,
                step: Step::Register(request),
            } if matches!(request.kind, WriteKind::Publish) => {
                let record = request.message;
                if record.id != id || record.topic != topic {
                    return Err(TabaError::new(
                        ErrorCode::ValidationFailed,
                        "The path and the body speak of different messages.",
                    ));
                }
                let (meta, body) = publish_of(record);
                Ok(RegisterItem {
                    operation_id: request.operation_id,
                    check: Check::Publish(body.clone()),
                    action: Action::Publish { meta, body },
                    topic,
                    id,
                })
            }
            Request::Visibility {
                topic,
                id,
                step: Step::Register(request),
            } => {
                let (check, delivery) = match request.kind {
                    VisibilityKind::Delivery => (
                        VisibilityCheck::Delivery,
                        Some((
                            request.delivery_count.ok_or_else(|| {
                                TabaError::new(
                                    ErrorCode::ValidationRequired,
                                    "The delivery requires `deliveryCount`.",
                                )
                                .with_detail(
                                    "deliveryCount",
                                    ErrorCode::ValidationRequired,
                                    "Required field.",
                                )
                            })?,
                            request.receipt_token.clone(),
                        )),
                    ),
                    VisibilityKind::Change => (VisibilityCheck::Receipt, None),
                };
                Ok(RegisterItem {
                    operation_id: request.operation_id,
                    check: Check::Visibility(check, request.receipt_token),
                    action: Action::Visibility {
                        until_ms: request.invisible_until_ms,
                        delivery,
                    },
                    topic,
                    id,
                })
            }
            Request::Ack {
                topic,
                id,
                step: Step::Register(request),
            } => Ok(RegisterItem {
                operation_id: request.operation_id,
                // A discard has no receipt: it is the primary of the group dropping a duplicate (doc 07, section 10.9).
                check: if request.discard {
                    Check::Nothing
                } else {
                    Check::Visibility(VisibilityCheck::Receipt, request.receipt_token)
                },
                action: Action::Ack,
                topic,
                id,
            }),
            _ => Err(TabaError::internal()),
        }
    }

    /// The registers of a batch between nodes, together (D196): each one is what [`write`](Self::write),
    /// [`visibility`](Self::visibility) or [`ack`](Self::ack) does with a `register` (the reservation first, the check
    /// after), but the checks that read the disk go in **one** blocking call. The results keep the order of `items`.
    pub(super) async fn register_many(
        &self,
        items: Vec<Result<RegisterItem, TabaError>>,
    ) -> Vec<Result<Ack, TabaError>> {
        let mut results: Vec<Option<Result<Ack, TabaError>>> = Vec::with_capacity(items.len());
        // The reserved operations still to check: (position, operationId, topic, id, check).
        let mut checks: Vec<(usize, String, String, String, Check)> = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            self.count("register");
            let RegisterItem {
                operation_id,
                topic,
                id,
                action,
                check,
            } = match item {
                Ok(item) => item,
                Err(error) => {
                    results.push(Some(Err(error)));
                    continue;
                }
            };
            match self.reserve(&operation_id, &topic, &id, action) {
                Ok(false) => results.push(Some(Ok(ack(&operation_id)))),
                Ok(true) => {
                    results.push(None);
                    checks.push((index, operation_id, topic, id, check));
                }
                Err(error) => results.push(Some(Err(error))),
            }
        }
        let mut heavy = Vec::new();
        for (index, operation_id, topic, id, check) in checks {
            let light = match &check {
                Check::Nothing => true,
                Check::Publish(body) => self.messages.publish_is_light(&topic, body.len() as u64),
                Check::Visibility(..) => self.messages.is_in_memory(&topic, &id),
            };
            if light {
                // The check only reads memory: right here, without a blocking thread.
                let checked = check.run(&self.messages, &self.topics, &topic, &id);
                results[index] = Some(self.settle_register(&operation_id, checked));
            } else {
                heavy.push((index, operation_id, topic, id, check));
            }
        }
        if !heavy.is_empty() {
            let wanted: Vec<(String, String, Check)> = heavy
                .iter()
                .map(|(_, _, topic, id, check)| (topic.clone(), id.clone(), check.clone()))
                .collect();
            let (messages, topics) = (self.messages.clone(), self.topics.clone());
            let checked = tokio::task::spawn_blocking(move || {
                wanted
                    .iter()
                    .map(|(topic, id, check)| check.run(&messages, &topics, topic, id))
                    .collect::<Vec<_>>()
            })
            .await;
            match checked {
                Ok(checked) => {
                    for ((index, operation_id, ..), result) in heavy.into_iter().zip(checked) {
                        results[index] = Some(self.settle_register(&operation_id, result));
                    }
                }
                Err(_) => {
                    for (index, operation_id, ..) in heavy {
                        results[index] =
                            Some(self.settle_register(&operation_id, Err(TabaError::internal())));
                    }
                }
            }
        }
        results
            .into_iter()
            .map(|result| result.unwrap_or_else(|| Err(TabaError::internal())))
            .collect()
    }

    /// The end of a register whose check ran: the reservation stays when the check passed and is released when it
    /// failed, as in [`register`](Self::register).
    fn settle_register(
        &self,
        operation_id: &str,
        checked: Result<(), TabaError>,
    ) -> Result<Ack, TabaError> {
        match checked {
            Ok(()) => Ok(ack(operation_id)),
            Err(error) => {
                self.lock().take(operation_id);
                Err(error)
            }
        }
    }

    /// An operation that only touches memory is applied right on the calling task: a blocking thread would cost
    /// more than the work (two thread switches per message). It is a publication that fits in memory, or an ack or a
    /// change of deadline of a message that is in memory.
    fn is_light(&self, topic: &str, id: &str, action: &Action) -> bool {
        match action {
            Action::Publish { body, .. } => {
                self.messages.publish_is_light(topic, body.len() as u64)
            }
            Action::Ack | Action::Visibility { .. } => self.messages.is_in_memory(topic, id),
            Action::DeadLetter { .. } => false,
        }
    }

    /// The commits of a batch between nodes (`POST /cluster/batch`), together (D196): each one is what
    /// [`commit`](Self::commit) does, but the ones that touch the disk go in **one** blocking call and one deferred
    /// scope, so their writes enter the topic queues together and the batch waits for one commit on the disk instead
    /// of one per message. The results keep the order of `operation_ids`.
    pub(super) async fn commit_many(
        &self,
        operation_ids: &[String],
    ) -> Vec<Result<Ack, TabaError>> {
        let mut results: Vec<Option<Result<(), TabaError>>> =
            Vec::with_capacity(operation_ids.len());
        let mut taken: Vec<(usize, String, String, Action)> = Vec::new();
        {
            let mut state = self.lock();
            self.purge(&mut state);
            for (index, operation_id) in operation_ids.iter().enumerate() {
                self.count("commit");
                match state.take(operation_id) {
                    Some(Pending {
                        topic, id, action, ..
                    }) => {
                        results.push(None);
                        taken.push((index, topic, id, action));
                    }
                    None => results.push(Some(match state.finished.get(operation_id) {
                        Some(Finished::Committed) => Ok(()),
                        Some(Finished::RolledBack) => {
                            Err(conflict("The operation was cancelled (rollback)."))
                        }
                        None => Err(unknown_operation()),
                    })),
                }
            }
        }
        let mut heavy: Vec<(usize, String, String, Action)> = Vec::new();
        let taken_indexes: Vec<usize> = taken.iter().map(|(index, ..)| *index).collect();
        for (index, topic, id, action) in taken {
            if self.is_light(&topic, &id, &action) {
                results[index] = Some(apply(&self.messages, &topic, &id, action));
            } else {
                heavy.push((index, topic, id, action));
            }
        }
        if !heavy.is_empty() {
            let indexes: Vec<usize> = heavy.iter().map(|(index, ..)| *index).collect();
            let messages = self.messages.clone();
            let applied = tokio::task::spawn_blocking(move || {
                messages.apply_replicated(|| {
                    heavy
                        .into_iter()
                        .map(|(_, topic, id, action)| apply(&messages, &topic, &id, action))
                        .collect()
                })
            })
            .await;
            match applied {
                Ok(applied) => {
                    for (index, result) in indexes.into_iter().zip(applied) {
                        results[index] = Some(result);
                    }
                }
                Err(_) => {
                    for index in indexes {
                        results[index] = Some(Err(TabaError::internal()));
                    }
                }
            }
        }
        // Applied or not, the operations left the pendings (as in `commit`).
        {
            let mut state = self.lock();
            for index in taken_indexes {
                Self::remember(&mut state, &operation_ids[index], Finished::Committed);
            }
        }
        results
            .into_iter()
            .zip(operation_ids)
            .map(|(result, operation_id)| {
                result
                    .unwrap_or_else(|| Err(TabaError::internal()))
                    .map(|()| ack(operation_id))
            })
            .collect()
    }

    fn rollback(&self, operation_id: &str) -> Result<Ack, TabaError> {
        let mut state = self.lock();
        self.purge(&mut state);
        if state.take(operation_id).is_some() {
            Self::remember(&mut state, operation_id, Finished::RolledBack);
            return Ok(ack(operation_id));
        }
        match state.finished.get(operation_id) {
            Some(Finished::RolledBack) => Ok(ack(operation_id)),
            Some(Finished::Committed) => Err(conflict(
                "The operation has already taken effect: there is no rollback after the commit.",
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
                "The path and the body speak of different messages.",
            ));
        }
        let operation_id = request.operation_id;
        let (topic, id) = (topic.to_owned(), id.to_owned());
        match request.kind {
            WriteKind::Publish => {
                let (meta, body) = publish_of(record);
                let held = body.clone();
                let check = async {
                    if !checked {
                        // Nothing to check.
                        Ok(())
                    } else if self.messages.publish_is_light(&topic, held.len() as u64) {
                        // The check only reads memory: right here, without a blocking thread.
                        check_publish(&self.messages, &self.topics, &topic, &id, &held)
                    } else {
                        let (topic, id) = (topic.clone(), id.clone());
                        self.blocking(move |messages, topics| {
                            check_publish(messages, topics, &topic, &id, &held)
                        })
                        .await
                    }
                };
                self.register(
                    &operation_id,
                    &topic,
                    &id,
                    Action::Publish { meta, body },
                    check,
                )
                .await
            }
            WriteKind::DeadLetter => {
                let source = request.source_topic.ok_or_else(|| {
                    TabaError::new(
                        ErrorCode::ValidationRequired,
                        "The move to the dead-letter requires `sourceTopic`.",
                    )
                    .with_detail(
                        "sourceTopic",
                        ErrorCode::ValidationRequired,
                        "Required field.",
                    )
                })?;
                let at_ms = record
                    .dead_lettered_ms
                    .unwrap_or_else(|| self.clock.now_millis());
                let check = async {
                    if !checked {
                        return Ok(());
                    }
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
                                "The message does not exist.",
                            ))
                        }
                    })
                    .await
                };
                self.register(
                    &operation_id,
                    &source,
                    &id,
                    Action::DeadLetter {
                        source: source.clone(),
                        at_ms,
                    },
                    check,
                )
                .await
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
                                    "The delivery requires `deliveryCount`.",
                                )
                                .with_detail(
                                    "deliveryCount",
                                    ErrorCode::ValidationRequired,
                                    "Required field.",
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
                let checking = async {
                    if !checked {
                        return Ok(());
                    }
                    // A message in memory is checked right here, without a blocking thread.
                    if self.messages.is_in_memory(&t, &i) {
                        return self.messages.check_visibility(&t, &i, check, &token);
                    }
                    self.blocking(move |messages, _| {
                        messages.check_visibility(&t, &i, check, &token)
                    })
                    .await
                };
                self.register(
                    &request.operation_id,
                    topic,
                    id,
                    Action::Visibility {
                        until_ms: request.invisible_until_ms,
                        delivery,
                    },
                    checking,
                )
                .await
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
                // A discard has no receipt: it is the group's primary dropping a duplicate (doc 07, section 10.9).
                let checking = async {
                    if !checked || request.discard {
                        return Ok(());
                    }
                    if self.messages.is_in_memory(&t, &i) {
                        return self.messages.check_visibility(
                            &t,
                            &i,
                            VisibilityCheck::Receipt,
                            &token,
                        );
                    }
                    self.blocking(move |messages, _| {
                        messages.check_visibility(&t, &i, VisibilityCheck::Receipt, &token)
                    })
                    .await
                };
                self.register(&request.operation_id, topic, id, Action::Ack, checking)
                    .await
            }
            Step::Commit(OperationRef { operation_id }) => self.commit(&operation_id).await,
            Step::Rollback(OperationRef { operation_id }) => self.rollback(&operation_id),
        }
    }

    /// Processes an event that stayed in the event database during synchronization (doc 05, section
    /// 7.3, step 5.3). The register checks nothing: the event beats the load, and applying again
    /// what the load already brought changes nothing (the application is idempotent). Only message
    /// calls come through here; those of definitions follow the normal path.
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

#[cfg(test)]
mod tests;
