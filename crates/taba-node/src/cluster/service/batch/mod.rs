//! The batch of message operations to another node (doc 06): the `register`, `commit` and `rollback` of the two-phase
//! writes that this node coordinates go to each responsible node in **one call** when several are waiting, instead of
//! one call each.
//!
//! The coordinator does not know: it keeps calling [`ClusterService::call_member_retrying`] once per operation, with
//! the same attempts and the same errors. Under that call, an operation for another node enters the queue of that
//! node and waits for its result.
//!
//! **When a batch closes.** Each node has a queue (first in, first out) and [`IN_FLIGHT`] slots of batches on the way.
//! A task per node repeats: wait for an item **and** a free slot, then close the batch at that instant with what is in
//! the queue (up to `cluster.batch.max-items` items and `cluster.batch.max-bytes`) and send it; the slot comes back
//! when the reply arrives or the call fails. There is no timer and no waiting for the batch to fill: with few operations
//! a slot is always free and each goes at once, alone; with many, the slots are busy and the queue grows for as long
//! as one reply takes, so the next batch leaves full. A batch that did not reach either limit leaves as it is.
//!
//! The result of each item comes back in its own position, and the operation id of every accepted result is checked
//! against the item's: a reply that does not match counts as a failure of the whole batch (a commit must never be
//! confirmed for another operation). A failure of the whole call (network, timeout, `5xx`) gives every item the same
//! error, and each one's retry goes into a later batch.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{Notify, Semaphore, oneshot};

use super::ClusterService;
use crate::cluster::members::Member;
use crate::cluster::protocol::wire::BatchItem;
use crate::cluster::protocol::{Request, Response, Step};
use crate::cluster::transport::CallError;
use crate::telemetry::metrics::{CLUSTER_BATCH_BYTES, CLUSTER_BATCH_ITEMS, CLUSTER_BATCH_WAIT};

/// Batches on the way to the same node at the same time.
const IN_FLIGHT: usize = 4;

/// What an item weighs besides the body of its message, for the byte limit (the fields around the message).
const ITEM_OVERHEAD: usize = 512;

/// The nodes' queues.
#[derive(Default)]
pub(super) struct Batcher {
    queues: Mutex<HashMap<String, Arc<NodeQueue>>>,
}

struct NodeQueue {
    state: Mutex<QueueState>,
    wake: Notify,
}

struct QueueState {
    /// Where the node is now (its addresses can change while the queue lives).
    member: Member,
    items: VecDeque<Pending>,
}

struct Pending {
    request: Request,
    /// The trace context of the call, to continue it on the node that serves the operation.
    traceparent: Option<String>,
    operation_id: String,
    /// Estimated size for the byte limit.
    size: usize,
    queued: Instant,
    reply: oneshot::Sender<Result<Response, CallError>>,
}

/// Whether the request is an operation the batch carries, and which one.
fn operation_of(request: &Request) -> Option<(String, usize)> {
    fn of<R>(step: &Step<R>, operation_id: impl Fn(&R) -> &str) -> String {
        match step {
            Step::Register(register) => operation_id(register).to_owned(),
            Step::Commit(op) | Step::Rollback(op) => op.operation_id.clone(),
        }
    }
    match request {
        Request::Write { step, .. } => {
            let size = match step {
                Step::Register(register) => register.message.body.len(),
                _ => 0,
            };
            Some((of(step, |r| &r.operation_id), size))
        }
        Request::Visibility { step, .. } => Some((of(step, |r| &r.operation_id), 0)),
        Request::Ack { step, .. } => Some((of(step, |r| &r.operation_id), 0)),
        _ => None,
    }
}

impl Batcher {
    /// The queue of the node, created (with the task that sends from it) on the first operation.
    fn queue_for(&self, service: &Arc<ClusterService>, member: &Member) -> Arc<NodeQueue> {
        let mut queues = self.queues.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(queue) = queues.get(&member.node_id) {
            return queue.clone();
        }
        let queue = Arc::new(NodeQueue {
            state: Mutex::new(QueueState {
                member: member.clone(),
                items: VecDeque::new(),
            }),
            wake: Notify::new(),
        });
        queues.insert(member.node_id.clone(), queue.clone());
        tokio::spawn(sender(service.clone(), queue.clone()));
        queue
    }
}

impl ClusterService {
    /// Sends the operation to the member through its queue and waits for its result. The caller's deadline is the
    /// `call_timeout`, counted from the moment the operation enters the queue.
    pub(super) async fn call_batched(
        self: &Arc<Self>,
        member: &Member,
        request: Request,
    ) -> Result<Response, CallError> {
        let Some((operation_id, body)) = operation_of(&request) else {
            return self
                .call_peer(member, request, self.rules.call_timeout)
                .await;
        };
        let (reply, result) = oneshot::channel();
        let traceparent = crate::telemetry::trace::current_traceparent();
        let queue = self.batcher.queue_for(self, member);
        {
            let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.member.addresses != member.addresses || state.member.port != member.port {
                state.member = member.clone();
            }
            state.items.push_back(Pending {
                request,
                traceparent,
                operation_id,
                size: body + ITEM_OVERHEAD,
                queued: Instant::now(),
                reply,
            });
        }
        queue.wake.notify_one();
        match tokio::time::timeout(self.rules.call_timeout, result).await {
            Ok(Ok(result)) => result,
            // The task that sends ended: the node is stopping.
            Ok(Err(_)) => Err(CallError::Unreachable("the node is stopping".into())),
            Err(_) => Err(CallError::Timeout),
        }
    }
}

/// Takes from the queue what goes in the next batch: in order, up to the item limit and to the byte limit, which is
/// soft (the first item always goes, however large). Items whose caller already gave up are dropped on the way.
fn take(queue: &NodeQueue, max_items: usize, max_bytes: usize) -> (Member, Vec<Pending>) {
    let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
    let mut taken: Vec<Pending> = Vec::new();
    let mut bytes = 0usize;
    while taken.len() < max_items {
        let Some(next) = state.items.front() else {
            break;
        };
        if next.reply.is_closed() {
            state.items.pop_front();
            continue;
        }
        if !taken.is_empty() && bytes.saturating_add(next.size) > max_bytes {
            break;
        }
        bytes = bytes.saturating_add(next.size);
        if let Some(item) = state.items.pop_front() {
            taken.push(item);
        }
    }
    (state.member.clone(), taken)
}

fn has_items(queue: &NodeQueue) -> bool {
    !queue
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .items
        .is_empty()
}

/// The task of a node: closes a batch whenever there is an item and a free slot (see the module).
async fn sender(service: Arc<ClusterService>, queue: Arc<NodeQueue>) {
    let slots = Arc::new(Semaphore::new(IN_FLIGHT));
    let rules = service.rules();
    let (max_items, max_bytes) = (rules.batch_max_items, rules.batch_max_bytes);
    loop {
        while !has_items(&queue) {
            tokio::select! {
                () = service.cancel_token().cancelled() => return,
                () = queue.wake.notified() => {}
            }
        }
        let permit = tokio::select! {
            () = service.cancel_token().cancelled() => return,
            permit = slots.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => return,
            },
        };
        let (member, items) = take(&queue, max_items, max_bytes);
        if items.is_empty() {
            continue;
        }
        let service = service.clone();
        tokio::spawn(async move {
            send(&service, &member, items).await;
            drop(permit);
        });
    }
}

/// Sends one batch and hands each item its result.
async fn send(service: &Arc<ClusterService>, member: &Member, items: Vec<Pending>) {
    let registry = &service.telemetry().registry;
    let now = Instant::now();
    let mut requests = Vec::with_capacity(items.len());
    let mut waiting = Vec::with_capacity(items.len());
    let mut bytes = 0usize;
    for item in items {
        registry.observe(
            &CLUSTER_BATCH_WAIT,
            &[],
            now.saturating_duration_since(item.queued).as_secs_f64(),
        );
        bytes += item.size.saturating_sub(ITEM_OVERHEAD);
        requests.push(Ok(BatchItem {
            request: item.request,
            traceparent: item.traceparent,
        }));
        waiting.push((item.operation_id, item.reply));
    }
    registry.observe(&CLUSTER_BATCH_ITEMS, &[], requests.len() as f64);
    registry.observe(&CLUSTER_BATCH_BYTES, &[], bytes as f64);
    let call = service
        .call_peer(
            member,
            Request::Batch(requests),
            service.rules().call_timeout,
        )
        .await;
    match call {
        Ok(Response::Batch(results)) if results.len() == waiting.len() => {
            // A reply of another operation must never be taken for this one's.
            let matches = results
                .iter()
                .zip(&waiting)
                .all(|(result, (operation, _))| {
                    result
                        .as_ref()
                        .map_or(true, |ack| &ack.operation_id == operation)
                });
            if !matches {
                fail_all(
                    waiting,
                    &CallError::Unreachable(
                        "the reply to the batch does not match its operations".into(),
                    ),
                );
                return;
            }
            for (result, (_, reply)) in results.into_iter().zip(waiting) {
                let _ = reply.send(match result {
                    Ok(ack) => Ok(Response::Ack(ack)),
                    Err(error) => Err(CallError::Remote(error)),
                });
            }
        }
        Ok(_) => fail_all(
            waiting,
            &CallError::Unreachable("the reply to the batch has another shape".into()),
        ),
        Err(error) => fail_all(waiting, &error),
    }
}

fn fail_all(
    waiting: Vec<(String, oneshot::Sender<Result<Response, CallError>>)>,
    error: &CallError,
) {
    for (_, reply) in waiting {
        let _ = reply.send(Err(error.clone()));
    }
}

#[cfg(test)]
mod tests;
