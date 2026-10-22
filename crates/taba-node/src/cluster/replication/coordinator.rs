//! Whoever **coordinates** a two-phase operation over the nodes responsible for a message (doc 05,
//! section 7.2): register on all, commit decided by quorum, rollback before the decision and
//! insistence after it.
//!
//! 1. Phase 1 goes to **all** the responsible nodes at the same time, each with the attempts of
//!    `write.retry.*`. The node itself, if it is one of them, is called without the network.
//! 2. As soon as the **quorum** confirms, **primary included**, the coordinator decides the commit. Before the
//!    decision, if the primary fails or the quorum can no longer close, it decides the **rollback**
//!    (`503 cluster.quorum-unavailable`, or the error the primary returned).
//! 3. Once the commit is decided, the call only answers after the commit is confirmed on the quorum
//!    (the primary and `write.quorum − 1` more nodes). The other nodes carry on in the background: they register
//!    and commit for as long as it takes, without rollback, and an exhausted `write.commit.timeout`
//!    without confirmation only generates the warning and the metric (the block stays below the full factor).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, watch};
use tracing::Instrument;

use crate::cluster::members::Member;
use crate::cluster::protocol::Request;
use crate::cluster::service::ClusterService;
use crate::cluster::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::metrics::{
    WRITE_COMMIT_TIMEOUTS, WRITE_COORDINATED, WRITE_RETRIES, WRITE_ROLLBACKS,
};
use crate::telemetry::trace;

/// What the coordinator does on the responsible nodes.
pub struct Plan {
    /// The responsible nodes, the primary first.
    pub owners: Vec<Member>,
    /// `write.quorum`: the primary and `quorum − 1` more nodes.
    pub quorum: usize,
    /// How many of the first responsible nodes the quorum must include: 1 (the primary) on the plain queue;
    /// 2 (the primary and the secondary) on the FIFO queue (doc 17, section 16.5).
    pub required: usize,
    pub register: Request,
    pub commit: Request,
    pub rollback: Request,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Decision {
    Pending,
    Commit,
    Abort,
}

enum Event {
    Registered(usize),
    RegisterFailed(usize, CallError),
    Committed(usize),
    CommitFailed(usize, CallError),
    CommitSlow(usize),
}

fn quorum_unavailable() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterQuorumUnavailable,
        "There are not enough responsible nodes to close the quorum.",
    )
    .with_retry_after(5)
}

fn commit_timeout() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterCommitTimeout,
        "The quorum decided, but the commit was not confirmed in time.",
    )
    .with_retry_after(5)
}

/// Runs the operation. `Ok` when the commit is confirmed on the quorum.
pub async fn run(service: &Arc<ClusterService>, plan: Plan) -> Result<(), TabaError> {
    let result = run_inner(service, plan).await;
    service.telemetry().registry.add(
        &WRITE_COORDINATED,
        &[(
            "taba.outcome",
            if result.is_ok() {
                "committed"
            } else {
                "failed"
            },
        )],
        1.0,
    );
    result
}

async fn run_inner(service: &Arc<ClusterService>, plan: Plan) -> Result<(), TabaError> {
    let total = plan.owners.len();
    // On FIFO the quorum includes the primary and the secondary, but never more nodes than the factor
    // has: `min(2, factor)` (doc 37, section 4.16).
    let factor = service.rules().replication_factor.max(1) as usize;
    let required = plan.required.min(factor).max(1);
    let quorum = plan.quorum.max(required);
    if total == 0 || total < quorum {
        return Err(quorum_unavailable());
    }
    let plan = Arc::new(plan);
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let (decision_tx, decision_rx) = watch::channel(Decision::Pending);
    for index in 0..total {
        tokio::spawn(
            owner_task(
                service.clone(),
                plan.clone(),
                index,
                decision_rx.clone(),
                events_tx.clone(),
            )
            .in_current_span(),
        );
    }
    drop(events_tx);

    let mut confirmed: Vec<usize> = Vec::new();
    let mut remaining = total;
    let mut primary_error: Option<CallError> = None;
    let mut required_failed = false;
    // All the failures so far were answers from the nodes themselves (`4xx`), not a network failure.
    let mut only_semantic = true;
    let mut committed: Vec<usize> = Vec::new();
    let mut quorum_set: Vec<usize> = Vec::new();
    let mut decided = false;
    while let Some(event) = events.recv().await {
        match event {
            Event::Registered(index) => {
                confirmed.push(index);
                remaining -= 1;
            }
            Event::RegisterFailed(index, error) => {
                remaining -= 1;
                if !matches!(&error, CallError::Remote(e) if e.status() < 500) {
                    only_semantic = false;
                }
                if index == 0 {
                    primary_error = Some(error);
                } else if index < required {
                    required_failed = true;
                }
            }
            Event::Committed(index) => committed.push(index),
            Event::CommitFailed(index, error) => {
                if decided && quorum_set.contains(&index) {
                    tracing::error!(%error, "commit refused by a quorum node after the decision");
                    return Err(commit_timeout());
                }
            }
            Event::CommitSlow(index) => {
                if decided && quorum_set.contains(&index) {
                    return Err(commit_timeout());
                }
            }
        }
        if !decided {
            if (0..required).all(|i| confirmed.contains(&i)) && confirmed.len() >= quorum {
                decided = true;
                quorum_set.extend(0..required);
                quorum_set.extend(
                    confirmed
                        .iter()
                        .copied()
                        .filter(|i| *i >= required)
                        .take(quorum - required),
                );
                let _ = decision_tx.send(Decision::Commit);
            } else if primary_error.is_some()
                || required_failed
                || confirmed.len() + remaining < quorum
            {
                let _ = decision_tx.send(Decision::Abort);
                trace::durability_event();
                service.telemetry().registry.add(&WRITE_ROLLBACKS, &[], 1.0);
                tracing::error!(
                    confirmed = confirmed.len(),
                    quorum,
                    "operation aborted (rollback): the quorum did not close"
                );
                // The primary has not answered yet and the other owners only said "no" (the message
                // does not exist, the token expired...): what the primary says counts for more than the quorum
                // arithmetic. The order in which answers arrive changes from machine to machine (on a
                // slow node the other owners' arrive first), so it waits for its own instead of returning
                // `503` for an error that is `4xx`. With a network failure it does not wait: `503` is right.
                if only_semantic && primary_error.is_none() && !confirmed.contains(&0) {
                    while let Some(event) = events.recv().await {
                        match event {
                            Event::RegisterFailed(0, error) => {
                                primary_error = Some(error);
                                break;
                            }
                            Event::Registered(0) => break,
                            _ => {}
                        }
                    }
                }
                return Err(match primary_error {
                    // What the primary says counts: a message that does not exist, an expired token...
                    Some(CallError::Remote(error)) if error.status() < 500 => error,
                    _ => quorum_unavailable(),
                });
            }
        }
        if decided && quorum_set.iter().all(|i| committed.contains(i)) {
            return Ok(());
        }
    }
    Err(commit_timeout())
}

/// The work of a responsible node: register, wait for the decision and commit (or rollback).
/// The future is erased to `dyn`: without that the compiler does not close the cycle `run` → `owner_task` →
/// `ClusterService::handle` → `ClusterMessages` → `run`.
fn owner_task(
    service: Arc<ClusterService>,
    plan: Arc<Plan>,
    index: usize,
    decision: watch::Receiver<Decision>,
    events: mpsc::UnboundedSender<Event>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(owner_work(service, plan, index, decision, events))
}

async fn owner_work(
    service: Arc<ClusterService>,
    plan: Arc<Plan>,
    index: usize,
    mut decision: watch::Receiver<Decision>,
    events: mpsc::UnboundedSender<Event>,
) {
    let owner = &plan.owners[index];
    let registered = match trace::traced(
        trace::peer_span("taba.write.register", &owner.node_id, false),
        service.call_member_retrying(owner, plan.register.clone()),
    )
    .await
    {
        Ok(_) => {
            let _ = events.send(Event::Registered(index));
            true
        }
        Err(error) => {
            let semantic = matches!(&error, CallError::Remote(e) if e.status() < 500);
            if error.is_network() {
                count_retry(&service, "register", owner);
            }
            let _ = events.send(Event::RegisterFailed(index, error));
            if semantic {
                return;
            }
            // Network failure: if the quorum closes without this node, the register and the commit carry on here
            // in the background, until it comes back (doc 05, section 7.2, step 7).
            false
        }
    };

    let decided = loop {
        let current = *decision.borrow_and_update();
        if current != Decision::Pending {
            break current;
        }
        if decision.changed().await.is_err() {
            // The coordinator finished without deciding: the register expires by itself.
            return;
        }
    };
    if decided == Decision::Abort {
        if !registered {
            return;
        }
        if let Err(error) = trace::traced(
            trace::peer_span("taba.write.rollback", &owner.node_id, true),
            service.call_member_retrying(owner, plan.rollback.clone()),
        )
        .await
        {
            tracing::warn!(node_id = %owner.node_id, %error, "rollback without confirmation: the register expires by itself");
        }
        return;
    }
    commit_until_done(&service, &plan, index, &events, registered).await;
}

fn count_retry(service: &ClusterService, phase: &str, owner: &Member) {
    service.telemetry().registry.add(
        &WRITE_RETRIES,
        &[("taba.peer", &owner.node_id), ("taba.phase", phase)],
        1.0,
    );
}

/// Insists on the commit until the node confirms (after the decision there is no rollback). If the node lost the
/// register (expired or restarted), it registers again and repeats the commit.
async fn commit_until_done(
    service: &Arc<ClusterService>,
    plan: &Plan,
    index: usize,
    events: &mpsc::UnboundedSender<Event>,
    mut registered: bool,
) {
    let owner = &plan.owners[index];
    let started = Instant::now();
    let timeout = service.rules().commit_timeout;
    let mut slow = false;
    loop {
        // The node that did not get to register (or lost the register) registers before the commit.
        let step = if registered {
            trace::traced(
                trace::peer_span("taba.write.commit", &owner.node_id, false),
                service.call_member(owner, plan.commit.clone()),
            )
            .await
        } else {
            match trace::traced(
                trace::peer_span("taba.write.register", &owner.node_id, false),
                service.call_member(owner, plan.register.clone()),
            )
            .await
            {
                Ok(_) => {
                    registered = true;
                    continue;
                }
                Err(error) => Err(error),
            }
        };
        match step {
            Ok(_) => {
                let _ = events.send(Event::Committed(index));
                return;
            }
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterOperationUnknown => {
                // Registers again and tries the commit on the way back.
                registered = false;
                continue;
            }
            Err(CallError::Remote(error)) if error.status() < 500 => {
                let _ = events.send(Event::CommitFailed(index, CallError::Remote(error)));
                return;
            }
            Err(_) => count_retry(service, "commit", owner),
        }
        if !slow && started.elapsed() >= timeout {
            slow = true;
            service
                .telemetry()
                .registry
                .add(&WRITE_COMMIT_TIMEOUTS, &[], 1.0);
            tracing::warn!(
                node_id = %owner.node_id,
                "write.commit.timeout ran out without a confirmed commit: the block stays below the full factor and the attempt goes on in the background"
            );
            let _ = events.send(Event::CommitSlow(index));
        }
        if service.cancel_token().is_cancelled() || !service.membership().is_member(&owner.node_id)
        {
            let _ = events.send(Event::CommitFailed(
                index,
                CallError::Unreachable("the node left the cluster".into()),
            ));
            return;
        }
        tokio::select! {
            () = service.cancel_token().cancelled() => {}
            () = tokio::time::sleep(service.rules().retry_backoff) => {}
        }
    }
}
