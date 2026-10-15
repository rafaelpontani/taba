//! The three HTTP APIs, each on a listener of its own (doc 06, section 8.0): client
//! (`api.port`), operations (`operations.port`, or the API port) and cluster
//! (`cluster.port`). The cluster port's mTLS comes in at step 16.

pub mod auth;
pub mod backup;
pub mod client_ip;
pub mod cluster_api;
mod common;
pub mod forward;
mod json;
pub mod messages;
pub mod metrics;
pub mod observe;
pub mod operations;
pub mod ratelimit;
pub mod tokens;
mod topic_stats;
pub mod topics;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;

use crate::cluster::drain::DrainController;
use crate::cluster::replication::ClusterMessages;
use crate::cluster::service::ClusterService;
use crate::cluster::state::{DrainState, NodeShared, NodeState};
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::Messages;
use crate::engine::messages::stats::StatsRules;
use crate::engine::topics::Topics;
use crate::telemetry::Telemetry;
use auth::Auth;
use client_ip::ClientIpRules;
use ratelimit::RateLimiter;

/// `Retry-After` of the client calls refused during the drain, in seconds.
const DRAIN_RETRY_AFTER_SECS: u64 = 5;

/// What the handlers share.
#[derive(Clone)]
pub struct AppState {
    pub clock: Arc<dyn Clock>,
    pub node: Arc<NodeShared>,
    /// The optional S3 backup; `None` without `backup.s3.bucket`.
    pub backup: Option<Arc<crate::cluster::backup::Backup>>,
    pub topics: Arc<Topics>,
    pub messages: Arc<Messages>,
    pub drain: Arc<DrainController>,
    /// The node in the cluster; only exists in `mode=production`.
    pub cluster: Option<Arc<ClusterService>>,
    /// The messages in the cluster (quorum write and delivery); `None` on a single node.
    pub replicated: Option<Arc<ClusterMessages>>,
    /// `operations.allowed-ips`; empty, no restriction.
    pub allowed_ips: Arc<Vec<IpNet>>,
    pub client_ip: Arc<ClientIpRules>,
    pub rate_limiter: Arc<RateLimiter>,
    pub auth: Arc<Auth>,
    pub telemetry: Arc<Telemetry>,
    /// The limits of the topic conditions (`stats.topics.*`).
    pub stats_rules: Arc<StatsRules>,
    /// The cluster certificates; `None` without a folder (the cluster port serves plain HTTP).
    pub tls: Option<Arc<crate::cluster::tls::TlsStore>>,
}

/// In drain the node does not serve client calls (doc 04, section 6.10): in a cluster it forwards the call
/// to the freest node, and the response comes back as the destination gave it. With no other node (or on a call that already
/// arrived by forward, which is never forwarded again) it answers `503` with `Retry-After`. A node in
/// failure (`failed`) does not serve either: it forwards, or answers `503` `node.failed` (D78.2).
async fn drain_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // A node that is still syncing does not serve either: it forwards the call to another (doc 04, section 6.9).
    let syncing = state.cluster.is_some() && state.node.state() == NodeState::Syncing;
    let failed = state.node.state() == NodeState::Failed;
    if state.node.drain_state() == DrainState::Active && !syncing && !failed {
        return next.run(request).await;
    }
    if state.cluster.is_some() && request.extensions().get::<forward::Forwarded>().is_none() {
        return forward::forward_call(&state, request).await;
    }
    if failed {
        return common::ApiError(
            TabaError::new(
                ErrorCode::NodeFailed,
                "The node is in failure and does not serve client calls.",
            )
            .with_retry_after(DRAIN_RETRY_AFTER_SECS),
        )
        .into_response();
    }
    if syncing {
        return common::ApiError(
            TabaError::new(ErrorCode::ClusterNodeSyncing, "The node is still syncing.")
                .with_retry_after(DRAIN_RETRY_AFTER_SECS),
        )
        .into_response();
    }
    common::ApiError(
        TabaError::new(
            ErrorCode::ClusterNodeDraining,
            "The node is in drain and there is no other node to forward the call to.",
        )
        .with_retry_after(DRAIN_RETRY_AFTER_SECS),
    )
    .into_response()
}

/// Client API. Without `operations.port`, it also serves the operations routes. The `/tokens` routes
/// only exist with authentication on (otherwise they answer `404`).
///
/// The layers, from the outside in: who is calling (`authenticate`), the rate limit, the drain
/// refusal and authorization (`authorize`). The rate limit comes after knowing who is calling, so the
/// token becomes the counter's key, and before refusing, so the `401` also counts by IP.
pub fn client_router(state: AppState, with_operations: bool) -> Router {
    let mut routes = Router::new()
        .merge(topics::routes())
        .merge(messages::routes());
    if state.auth.enabled {
        routes = routes.merge(tokens::routes());
    }
    let routes = routes
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authorize,
        ))
        .route_layer(middleware::from_fn_with_state(state.clone(), drain_guard))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            ratelimit::rate_limit,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authenticate,
        ));
    let routes = if with_operations {
        routes.merge(operations::routes(&state))
    } else {
        routes
    };
    common::finish(routes, state)
}

/// Operations API, on its own port.
pub fn operations_router(state: AppState) -> Router {
    common::finish(operations::routes(&state), state)
}

/// Cluster API: only `/cluster/*`; the rest answers `404`.
pub fn cluster_router(state: AppState) -> Router {
    common::finish(cluster_api::routes(), state)
}
