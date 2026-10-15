//! Forwarding client calls (doc 04, section 6.9; doc 06c, `* /cluster/forward/{path}`).
//! A node in drain keeps receiving client calls (the load balancer takes a while to notice) and
//! forwards **all** of them to the freest node, from when the drain starts until the process ends, even
//! after leaving the cluster.
//!
//! - **Destination:** on each call, the ready member (out of drain) with the fewest forwarded calls in
//!   progress from this node, breaking ties at random. The heartbeat's load indicator comes in with the
//!   channel (step 15).
//! - **Transparent forward:** method, path, query, body and headers go as they came, and the
//!   response comes back as the destination gave it (`409`, `413`, `429`...). It also applies to `GET` with
//!   long polling.
//! - **Destination failure:** before the body arrives (no connection, or the destination in drain or
//!   syncing) it tries another, up to a small ceiling; once sent it **does not retry** (retrying a
//!   `POST` would duplicate the message): `504 forward.timeout`.
//! - **No loop:** whoever receives the forward executes it and never forwards again.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::signals::Signals;
use crate::cluster::members::Member;
use crate::cluster::protocol::{ForwardRequest, ForwardResponse, Request, Response};
use crate::cluster::service::ClusterService;
use crate::cluster::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::metrics::CLUSTER_FORWARDED;

/// A forward's deadline: covers a long polling (up to 20 s) with room to spare.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(60);
/// How many destinations, at most, a call tries before giving up.
const MAX_ATTEMPTS: usize = 3;

/// Executes a forwarded client call against this node's client API (whoever assembles the node
/// provides it: `cluster` does not know HTTP).
pub type ForwardExecutor = Arc<
    dyn Fn(
            ForwardRequest,
        ) -> Pin<Box<dyn Future<Output = Result<ForwardResponse, TabaError>> + Send>>
        + Send
        + Sync,
>;

pub struct Forwarder {
    service: Arc<ClusterService>,
    signals: Arc<Signals>,
    executor: OnceLock<ForwardExecutor>,
    /// Calls forwarded by this node and still in progress, by destination.
    in_flight: Mutex<HashMap<String, usize>>,
}

/// Subtracts the in-progress call on exit, even if the future is dropped.
struct InFlight<'a> {
    forwarder: &'a Forwarder,
    node_id: String,
}

impl<'a> InFlight<'a> {
    fn new(forwarder: &'a Forwarder, node_id: &str) -> Self {
        *forwarder.lock().entry(node_id.to_owned()).or_default() += 1;
        Self {
            forwarder,
            node_id: node_id.to_owned(),
        }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut map = self.forwarder.lock();
        if let Some(count) = map.get_mut(&self.node_id) {
            *count = count.saturating_sub(1);
        }
    }
}

fn no_destination(state: crate::cluster::state::NodeState) -> TabaError {
    if state == crate::cluster::state::NodeState::Failed {
        return TabaError::new(
            ErrorCode::NodeFailed,
            "The node is in failure and no other node could serve the call.",
        )
        .with_retry_after(5);
    }
    if state == crate::cluster::state::NodeState::Syncing {
        return TabaError::new(
            ErrorCode::ClusterNodeSyncing,
            "The node is syncing and no other node could serve the call.",
        )
        .with_retry_after(5);
    }
    TabaError::new(
        ErrorCode::ClusterNodeDraining,
        "The node is in drain and no other node could serve the call.",
    )
    .with_retry_after(5)
}

fn random_index(n: usize) -> usize {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the system has no source of random numbers");
    (u64::from_le_bytes(bytes) % n.max(1) as u64) as usize
}

impl Forwarder {
    pub fn new(service: Arc<ClusterService>, signals: Arc<Signals>) -> Arc<Self> {
        Arc::new(Self {
            service,
            signals,
            executor: OnceLock::new(),
            in_flight: Mutex::new(HashMap::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, usize>> {
        self.in_flight.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_executor(&self, executor: ForwardExecutor) {
        let _ = self.executor.set(executor);
    }

    /// This call's destination: among the ready members out of drain not yet
    /// tried, the one with the fewest calls in progress; tie, a draw.
    fn pick(&self, tried: &HashSet<String>) -> Option<Member> {
        let table = self.service.membership().table()?;
        let me = self.service.node_id();
        let in_flight = self.lock();
        let candidates: Vec<&Member> = table
            .members
            .iter()
            .filter(|m| {
                m.node_id != me
                    && !tried.contains(&m.node_id)
                    && m.state == crate::cluster::members::MemberState::Active
                    && !self.signals.is_draining(&m.node_id)
            })
            .collect();
        let least = candidates
            .iter()
            .map(|m| in_flight.get(&m.node_id).copied().unwrap_or(0))
            .min()?;
        let tied: Vec<&&Member> = candidates
            .iter()
            .filter(|m| in_flight.get(&m.node_id).copied().unwrap_or(0) == least)
            .collect();
        Some((**tied[random_index(tied.len())]).clone())
    }

    fn count(&self, outcome: &str) {
        self.service.telemetry().registry.add(
            &CLUSTER_FORWARDED,
            &[("taba.outcome", outcome)],
            1.0,
        );
    }

    /// Forwards the call to the freest node.
    pub async fn forward(&self, request: ForwardRequest) -> Result<ForwardResponse, TabaError> {
        let mut tried = HashSet::new();
        for _ in 0..MAX_ATTEMPTS {
            let Some(member) = self.pick(&tried) else {
                break;
            };
            tried.insert(member.node_id.clone());
            let _busy = InFlight::new(self, &member.node_id);
            match self
                .service
                .call_peer(&member, Request::Forward(request.clone()), FORWARD_TIMEOUT)
                .await
            {
                Ok(Response::Forwarded(response)) => {
                    self.count("ok");
                    return Ok(response);
                }
                Ok(_) => {
                    self.count("error");
                    return Err(TabaError::new(
                        ErrorCode::ForwardBadGateway,
                        "Unexpected response from the node that received the forward.",
                    ));
                }
                // No response in time: the body was already sent, so it does not retry.
                Err(CallError::Timeout) => {
                    self.count("timeout");
                    return Err(TabaError::new(
                        ErrorCode::ForwardTimeout,
                        "The node that received the forward did not answer in time.",
                    ));
                }
                // The destination could not serve (no connection, in drain, syncing): another one.
                Err(CallError::Unreachable(_)) => {}
                Err(CallError::Remote(error)) if error.status() == 503 => {}
                Err(CallError::Remote(error)) => {
                    self.count("error");
                    tracing::warn!(peer = %member.node_id, %error, "the forward was refused");
                    return Err(TabaError::new(
                        ErrorCode::ForwardBadGateway,
                        format!(
                            "Node {} refused the forward: {}",
                            member.node_id, error.message
                        ),
                    ));
                }
            }
        }
        self.count("no-destination");
        Err(no_destination(self.service.node_state()))
    }

    /// The side that receives the forward: executes the call against this node's client API.
    pub async fn execute(&self, request: ForwardRequest) -> Result<Response, TabaError> {
        let executor = self.executor.get().ok_or_else(TabaError::route_not_found)?;
        executor(request).await.map(Response::Forwarded)
    }
}
