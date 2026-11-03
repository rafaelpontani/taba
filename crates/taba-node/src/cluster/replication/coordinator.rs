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
//!
//! [`run_many`] does the same for a **group** of operations (D198): the operations with the same responsible nodes, in
//! the same order, go through the two phases together, with one task per node instead of one per node and operation,
//! and each phase reaches each node as one group of the batch between nodes. The decision is still per operation (an
//! operation whose register failed does not hold back the others); only what is rare (a node that lost the register,
//! a commit that did not go through at the first attempt) goes back to the insistence of one operation.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, watch};
use tracing::Instrument;

use crate::cluster::members::Member;
use crate::cluster::protocol::{Request, Step};
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

/// The waits, in milliseconds, of a copy that does not know the message yet (see [`copy_behind`]): about 1 s in all, more than a publication commit lost once takes to come back (`write.retry.backoff`).
const COPY_WAITS_MS: [u64; 8] = [5, 10, 20, 40, 80, 160, 320, 400];

/// Whether the "no" of a node that is not one of the `required` is only a copy that is behind: on a plain queue the owner
/// delivers a message as soon as its publication is committed there (D199), and a consumer can ack it before the commit
/// (or even the register) of the publication reaches the other nodes. Their register of the ack (or of a change) then
/// finds no message, or the publication still pending; it passes a few milliseconds later. The primary's answer is
/// never this: a message that really does not exist is the primary's `404`.
fn copy_behind(request: &Request, error: &CallError) -> bool {
    matches!(
        request,
        Request::Ack {
            step: Step::Register(_),
            ..
        } | Request::Visibility {
            step: Step::Register(_),
            ..
        }
    ) && matches!(error, CallError::Remote(e)
        if matches!(e.code, ErrorCode::MessageNotFound | ErrorCode::ClusterConflict))
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
    count(service, result.is_ok());
    result
}

/// Counts an operation this node coordinated, also one it applied alone (factor 1, D191).
pub fn count(service: &ClusterService, committed: bool) {
    count_many(service, u64::from(committed), u64::from(!committed));
}

/// Counts at once the operations of a batch this node applied alone.
pub fn count_many(service: &ClusterService, committed: u64, failed: u64) {
    for (outcome, n) in [("committed", committed), ("failed", failed)] {
        if n > 0 {
            service.telemetry().registry.add(
                &WRITE_COORDINATED,
                &[("taba.outcome", outcome)],
                n as f64,
            );
        }
    }
}

async fn run_inner(service: &Arc<ClusterService>, plan: Plan) -> Result<(), TabaError> {
    let total = plan.owners.len();
    // On FIFO the quorum includes the primary and the secondary, but never more nodes than the factor
    // has: `min(2, factor)` (doc 07, section 8).
    let factor = service.rules().replication_factor.max(1) as usize;
    let required = plan.required.min(factor).max(1);
    let quorum = plan.quorum.max(required);
    if total == 0 || total < quorum {
        return Err(quorum_unavailable());
    }
    if total == 1 {
        return run_single(service, plan).await;
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

/// A single responsible node (`replication.factor` 1, or a cluster of one node): it is the primary and the
/// whole quorum, so its register **is** the decision. Same outcome as the general path, without a task per
/// node, channels or waiting for a decision: register, then commit, on the caller's task. Only a commit
/// that does not go through at the first attempt goes to the background insistence of the general path.
/// Erased to `dyn` for the same reason as [`owner_task`].
fn run_single(
    service: &Arc<ClusterService>,
    plan: Plan,
) -> Pin<Box<dyn Future<Output = Result<(), TabaError>> + Send + '_>> {
    Box::pin(run_single_work(service, plan))
}

async fn run_single_work(service: &Arc<ClusterService>, plan: Plan) -> Result<(), TabaError> {
    let owner = &plan.owners[0];
    if let Err(error) = trace::traced(
        trace::peer_span("taba.write.register", &owner.node_id, false),
        service.call_member_retrying(owner, plan.register.clone()),
    )
    .await
    {
        if error.is_network() {
            count_retry(service, "register", owner);
        }
        trace::durability_event();
        service.telemetry().registry.add(&WRITE_ROLLBACKS, &[], 1.0);
        tracing::error!(
            confirmed = 0,
            quorum = 1,
            "operation aborted (rollback): the quorum did not close"
        );
        return Err(match error {
            CallError::Remote(error) if error.status() < 500 => error,
            _ => quorum_unavailable(),
        });
    }
    let registered = match trace::traced(
        trace::peer_span("taba.write.commit", &owner.node_id, false),
        service.call_member(owner, plan.commit.clone()),
    )
    .await
    {
        Ok(_) => return Ok(()),
        Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterOperationUnknown => false,
        Err(CallError::Remote(error)) if error.status() < 500 => {
            tracing::error!(%error, "commit refused by a quorum node after the decision");
            return Err(commit_timeout());
        }
        Err(_) => {
            count_retry(service, "commit", owner);
            true
        }
    };
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let service = service.clone();
    tokio::spawn(
        async move {
            commit_until_done(
                &service,
                &plan,
                0,
                &|e| {
                    let _ = events_tx.send(e);
                },
                registered,
            )
            .await
        }
        .in_current_span(),
    );
    match events.recv().await {
        Some(Event::Committed(_)) => Ok(()),
        Some(Event::CommitFailed(_, error)) => {
            tracing::error!(%error, "commit refused by a quorum node after the decision");
            Err(commit_timeout())
        }
        _ => Err(commit_timeout()),
    }
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
    let mut result = trace::traced(
        trace::peer_span("taba.write.register", &owner.node_id, false),
        service.call_member_retrying(owner, plan.register.clone()),
    )
    .await;
    if index >= required_of(&service, &plan) {
        for wait in COPY_WAITS_MS {
            match &result {
                Err(error) if copy_behind(&plan.register, error) => {}
                _ => break,
            }
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            result = service.call_member(owner, plan.register.clone()).await;
        }
    }
    let registered = match result {
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
    commit_until_done(
        &service,
        &plan,
        index,
        &|e| {
            let _ = events.send(e);
        },
        registered,
    )
    .await;
}

/// What the task of a node tells the coordinator of a group.
enum GroupEvent {
    /// The results of the registers on the node (by its position among the responsible nodes), one per operation.
    Registered(usize, Vec<Result<(), CallError>>),
    /// The commit of one operation of the group, as the coordinator of one operation hears it.
    Commit(usize, Event),
}

/// Where an operation of a group is.
struct GroupItem {
    confirmed: Vec<usize>,
    primary_error: Option<CallError>,
    required_failed: bool,
    /// All the failures so far were answers from the nodes themselves (`4xx`), not a network failure.
    only_semantic: bool,
    decision: Decision,
    quorum_set: Vec<usize>,
    committed: Vec<usize>,
    result: Option<Result<(), TabaError>>,
}

/// Runs a group of operations (D198) and returns the result of each one, in the order of `plans`: the same outcome
/// as [`run`] for each, with the operations that share the responsible nodes (in the same order) and the quorum
/// rules going through the phases together, in groups of at most `cluster.batch.max-items`.
pub async fn run_many(
    service: &Arc<ClusterService>,
    plans: Vec<Plan>,
) -> Vec<Result<(), TabaError>> {
    let total = plans.len();
    let max = service.rules().batch_max_items.max(1);
    let mut groups: Vec<Vec<(usize, Plan)>> = Vec::new();
    let mut open: HashMap<(Vec<String>, usize, usize), usize> = HashMap::new();
    for (position, mut plan) in plans.into_iter().enumerate() {
        // Only the first `required` nodes have a fixed role: the order of the others changes nothing, so it does not
        // split the groups (with virtual nodes, three nodes give six orders of the responsible nodes, but three groups).
        let fixed = plan.required.max(1).min(plan.owners.len());
        plan.owners[fixed..].sort_by(|a, b| a.node_id.cmp(&b.node_id));
        let key = (
            plan.owners.iter().map(|m| m.node_id.clone()).collect(),
            plan.quorum,
            plan.required,
        );
        let group = match open.get(&key) {
            Some(&group) if groups[group].len() < max => group,
            _ => {
                groups.push(Vec::new());
                open.insert(key, groups.len() - 1);
                groups.len() - 1
            }
        };
        groups[group].push((position, plan));
    }
    let done = futures_util::future::join_all(groups.into_iter().map(|group| async move {
        let (positions, plans): (Vec<usize>, Vec<Plan>) = group.into_iter().unzip();
        let results = run_group(service, plans).await;
        positions.into_iter().zip(results).collect::<Vec<_>>()
    }))
    .await;
    let mut placed: Vec<Option<Result<(), TabaError>>> = (0..total).map(|_| None).collect();
    for (position, result) in done.into_iter().flatten() {
        placed[position] = Some(result);
    }
    placed
        .into_iter()
        .map(|result| result.unwrap_or_else(|| Err(TabaError::internal())))
        .collect()
}

/// One group of [`run_many`]: the same responsible nodes and quorum rules for every operation.
async fn run_group(service: &Arc<ClusterService>, plans: Vec<Plan>) -> Vec<Result<(), TabaError>> {
    let results = run_group_inner(service, plans).await;
    let committed = results.iter().filter(|r| r.is_ok()).count() as u64;
    count_many(service, committed, results.len() as u64 - committed);
    results
}

async fn run_group_inner(
    service: &Arc<ClusterService>,
    plans: Vec<Plan>,
) -> Vec<Result<(), TabaError>> {
    let Some(first) = plans.first() else {
        return Vec::new();
    };
    let total = first.owners.len();
    let factor = service.rules().replication_factor.max(1) as usize;
    let required = first.required.min(factor).max(1);
    let quorum = first.quorum.max(required);
    if total == 0 || total < quorum {
        return plans.iter().map(|_| Err(quorum_unavailable())).collect();
    }
    if total == 1 || plans.len() == 1 {
        return futures_util::future::join_all(
            plans.into_iter().map(|plan| run_inner(service, plan)),
        )
        .await;
    }
    let items = plans.len();
    let plans = Arc::new(plans);
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let (decision_tx, decision_rx) = watch::channel::<Option<Arc<Vec<Decision>>>>(None);
    for owner in 0..total {
        tokio::spawn(
            group_owner_task(
                service.clone(),
                plans.clone(),
                owner,
                decision_rx.clone(),
                events_tx.clone(),
            )
            .in_current_span(),
        );
    }
    drop(events_tx);

    let mut states: Vec<GroupItem> = (0..items)
        .map(|_| GroupItem {
            confirmed: Vec::new(),
            primary_error: None,
            required_failed: false,
            only_semantic: true,
            decision: Decision::Pending,
            quorum_set: Vec::new(),
            committed: Vec::new(),
            result: None,
        })
        .collect();
    let mut remaining = total;
    let mut primary_answered = false;
    let mut decided = false;
    let mut done = 0;
    while let Some(event) = events.recv().await {
        match event {
            GroupEvent::Registered(owner, results) => {
                remaining -= 1;
                primary_answered |= owner == 0;
                for (state, result) in states.iter_mut().zip(results) {
                    match result {
                        Ok(()) => state.confirmed.push(owner),
                        Err(error) => {
                            if !matches!(&error, CallError::Remote(e) if e.status() < 500) {
                                state.only_semantic = false;
                            }
                            if owner == 0 {
                                state.primary_error = Some(error);
                            } else if owner < required {
                                state.required_failed = true;
                            }
                        }
                    }
                }
            }
            GroupEvent::Commit(item, Event::Committed(owner)) => {
                states[item].committed.push(owner);
            }
            GroupEvent::Commit(item, Event::CommitFailed(owner, error)) => {
                let state = &mut states[item];
                if state.result.is_none() && state.quorum_set.contains(&owner) {
                    tracing::error!(%error, "commit refused by a quorum node after the decision");
                    state.result = Some(Err(commit_timeout()));
                    done += 1;
                }
            }
            GroupEvent::Commit(item, Event::CommitSlow(owner)) => {
                let state = &mut states[item];
                if state.result.is_none() && state.quorum_set.contains(&owner) {
                    state.result = Some(Err(commit_timeout()));
                    done += 1;
                }
            }
            GroupEvent::Commit(..) => {}
        }
        if !decided {
            for state in states
                .iter_mut()
                .filter(|s| s.decision == Decision::Pending)
            {
                if (0..required).all(|i| state.confirmed.contains(&i))
                    && state.confirmed.len() >= quorum
                {
                    state.decision = Decision::Commit;
                    state.quorum_set.extend(0..required);
                    let others = state.confirmed.iter().copied().filter(|i| *i >= required);
                    state
                        .quorum_set
                        .extend(others.take(quorum - required).collect::<Vec<_>>());
                } else if state.primary_error.is_some()
                    || state.required_failed
                    || state.confirmed.len() + remaining < quorum
                {
                    // What the primary says counts for more than the quorum arithmetic (see `run_inner`): when the
                    // others only said "no", the operation waits for the primary's own answer.
                    if state.only_semantic && !primary_answered {
                        continue;
                    }
                    state.decision = Decision::Abort;
                    state.result = Some(Err(match &state.primary_error {
                        Some(CallError::Remote(error)) if error.status() < 500 => error.clone(),
                        _ => quorum_unavailable(),
                    }));
                    done += 1;
                }
            }
            if states.iter().all(|s| s.decision != Decision::Pending) {
                decided = true;
                let aborted = states
                    .iter()
                    .filter(|s| s.decision == Decision::Abort)
                    .count();
                if aborted > 0 {
                    trace::durability_event();
                    service
                        .telemetry()
                        .registry
                        .add(&WRITE_ROLLBACKS, &[], aborted as f64);
                    tracing::error!(
                        aborted,
                        operations = items,
                        quorum,
                        "operations aborted (rollback): the quorum did not close"
                    );
                }
                let decisions = states.iter().map(|s| s.decision).collect();
                let _ = decision_tx.send(Some(Arc::new(decisions)));
            }
        }
        if decided {
            for state in states.iter_mut() {
                if state.result.is_none()
                    && state.decision == Decision::Commit
                    && state.quorum_set.iter().all(|i| state.committed.contains(i))
                {
                    state.result = Some(Ok(()));
                    done += 1;
                }
            }
        }
        if done == items {
            break;
        }
    }
    states
        .into_iter()
        .map(|state| state.result.unwrap_or_else(|| Err(commit_timeout())))
        .collect()
}

/// The work of a responsible node in a group, erased to `dyn` for the same reason as [`owner_task`].
fn group_owner_task(
    service: Arc<ClusterService>,
    plans: Arc<Vec<Plan>>,
    owner: usize,
    decision: watch::Receiver<Option<Arc<Vec<Decision>>>>,
    events: mpsc::UnboundedSender<GroupEvent>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(group_owner_work(service, plans, owner, decision, events))
}

/// What [`owner_work`] does for one operation, for the whole group: the registers in one group, then the commits of
/// the operations decided and registered here in one group, and the rollbacks of the others in another.
async fn group_owner_work(
    service: Arc<ClusterService>,
    plans: Arc<Vec<Plan>>,
    owner: usize,
    mut decision: watch::Receiver<Option<Arc<Vec<Decision>>>>,
    events: mpsc::UnboundedSender<GroupEvent>,
) {
    let member = &plans[0].owners[owner];
    let registers = plans.iter().map(|plan| plan.register.clone()).collect();
    let mut results = service
        .call_member_many_retrying(member, registers)
        .instrument(trace::peer_span(
            "taba.write.register",
            &member.node_id,
            false,
        ))
        .await;
    // A copy that is behind (see `copy_behind`) gets the registers that it refused again, together, a little later.
    if owner >= required_of(&service, &plans[0]) {
        for wait in COPY_WAITS_MS {
            let again: Vec<usize> = (0..results.len())
                .filter(|i| matches!(&results[*i], Err(e) if copy_behind(&plans[*i].register, e)))
                .collect();
            if again.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            let requests = again.iter().map(|i| plans[*i].register.clone()).collect();
            let retried = service.call_member_many(member, requests).await;
            for (index, result) in again.into_iter().zip(retried) {
                results[index] = result;
            }
        }
    }
    // Per operation: registered here, a network failure (it carries on in the background if the commit is decided), or
    // a "no" from the node (nothing more to do here).
    let mut registered: Vec<Option<bool>> = Vec::with_capacity(results.len());
    let mut network = false;
    let reported = results
        .into_iter()
        .map(|result| match result {
            Ok(_) => {
                registered.push(Some(true));
                Ok(())
            }
            Err(error) => {
                network |= error.is_network();
                let semantic = matches!(&error, CallError::Remote(e) if e.status() < 500);
                registered.push((!semantic).then_some(false));
                Err(error)
            }
        })
        .collect();
    if network {
        count_retry(&service, "register", member);
    }
    let _ = events.send(GroupEvent::Registered(owner, reported));

    let decisions = loop {
        let current = decision.borrow_and_update().clone();
        if let Some(decisions) = current {
            break decisions;
        }
        if decision.changed().await.is_err() {
            // The coordinator finished without deciding: the registers expire by themselves.
            return;
        }
    };
    let (mut commits, mut rollbacks, mut late) = (Vec::new(), Vec::new(), Vec::new());
    for (item, registered) in registered.iter().enumerate() {
        match (decisions[item], registered) {
            (Decision::Commit, Some(true)) => commits.push(item),
            (Decision::Commit, Some(false)) => late.push((item, false)),
            (Decision::Abort, Some(true)) => rollbacks.push(item),
            _ => {}
        }
    }
    let rollback = async {
        if rollbacks.is_empty() {
            return;
        }
        let requests = rollbacks
            .iter()
            .map(|i| plans[*i].rollback.clone())
            .collect();
        let failed = service
            .call_member_many_retrying(member, requests)
            .instrument(trace::peer_span(
                "taba.write.rollback",
                &member.node_id,
                true,
            ))
            .await
            .iter()
            .filter(|r| r.is_err())
            .count();
        if failed > 0 {
            tracing::warn!(node_id = %member.node_id, failed, "rollback without confirmation: the registers expire by themselves");
        }
    };
    let commit = async {
        let mut again = Vec::new();
        if commits.is_empty() {
            return again;
        }
        let requests = commits.iter().map(|i| plans[*i].commit.clone()).collect();
        let results = service
            .call_member_many(member, requests)
            .instrument(trace::peer_span(
                "taba.write.commit",
                &member.node_id,
                false,
            ))
            .await;
        let mut network = false;
        for (item, result) in commits.iter().copied().zip(results) {
            match result {
                Ok(_) => {
                    let _ = events.send(GroupEvent::Commit(item, Event::Committed(owner)));
                }
                Err(CallError::Remote(error))
                    if error.code == ErrorCode::ClusterOperationUnknown =>
                {
                    again.push((item, false));
                }
                Err(CallError::Remote(error)) if error.status() < 500 => {
                    let _ = events.send(GroupEvent::Commit(
                        item,
                        Event::CommitFailed(owner, CallError::Remote(error)),
                    ));
                }
                Err(_) => {
                    network = true;
                    again.push((item, true));
                }
            }
        }
        if network {
            count_retry(&service, "commit", member);
        }
        again
    };
    let ((), again) = futures_util::future::join(rollback, commit).await;
    late.extend(again);
    // What is rare (a node that lost or never got the register, a commit that did not go through) insists alone, as in
    // the operation of one message.
    futures_util::future::join_all(late.into_iter().map(|(item, registered)| {
        let (service, plans, events) = (&service, &plans, &events);
        async move {
            let report = |event| {
                let _ = events.send(GroupEvent::Commit(item, event));
            };
            commit_until_done(service, &plans[item], owner, &report, registered).await;
        }
    }))
    .await;
}

/// How many of the first responsible nodes the quorum must include, as [`run_inner`] computes it.
fn required_of(service: &ClusterService, plan: &Plan) -> usize {
    let factor = service.rules().replication_factor.max(1) as usize;
    plan.required.min(factor).max(1)
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
    report: &(impl Fn(Event) + Sync),
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
                service.call_member(owner, plan.register.decided()),
            )
            .await
            {
                Ok(_) => {
                    registered = true;
                    continue;
                }
                // Another operation has the message right now (it took it while this node had lost the register).
                // The decision is final and the others already applied it, so this node waits for the other
                // operation to end (or expire) and tries again, instead of giving up and keeping a message that no
                // longer exists anywhere else (PRB-076).
                Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterConflict => {
                    Err(CallError::Unreachable(format!(
                        "another operation holds the message: {}",
                        error.message
                    )))
                }
                Err(error) => Err(error),
            }
        };
        match step {
            Ok(_) => {
                report(Event::Committed(index));
                return;
            }
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterOperationUnknown => {
                // Registers again and tries the commit on the way back.
                registered = false;
                continue;
            }
            Err(CallError::Remote(error)) if error.status() < 500 => {
                report(Event::CommitFailed(index, CallError::Remote(error)));
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
            report(Event::CommitSlow(index));
        }
        if service.cancel_token().is_cancelled() || !service.membership().is_member(&owner.node_id)
        {
            report(Event::CommitFailed(
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
