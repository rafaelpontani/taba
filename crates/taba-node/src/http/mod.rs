//! The three HTTP APIs, each on a listener of its own (doc 06, section 8.0): client
//! (`api.port`), operations (`operations.port`) and cluster (`cluster.port`). The cluster port's
//! mTLS comes in at step 16.

pub mod auth;
pub mod backup;
pub mod client_ip;
pub mod cluster_api;
mod common;
pub mod consolidate;
pub mod forward;
mod json;
pub mod messages;
pub mod metrics;
pub mod nodes;
pub mod observe;
pub mod openapi;
pub mod operations;
pub mod ratelimit;
pub mod restore;
pub mod tokens;
pub mod topic_stats;
pub mod topics;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;
use utoipa_axum::router::OpenApiRouter;

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
    // The calls that say what the cluster is doing (`answers_in_any_state`) are forwarded like any other while a
    // node can take them; when none can (every node draining, syncing or in failure, or a stale member table), this
    // node answers them: they are how the operator finds out why the rest of the API refuses, and how a node returns.
    let arrived_by_forward = request.extensions().get::<forward::Forwarded>().is_some();
    let state_read = answers_in_any_state(request.method(), request.uri().path());
    if state_read && arrived_by_forward {
        return next.run(request).await;
    }
    if state.cluster.is_some() && !arrived_by_forward {
        if !state_read {
            return forward::forward_call(&state, request).await;
        }
        // The same call, kept to answer it here if the forward finds no node.
        let mut local = Request::builder()
            .method(request.method().clone())
            .uri(request.uri().clone())
            .body(axum::body::Body::empty())
            .expect("the method and the uri of a received call are valid");
        *local.headers_mut() = request.headers().clone();
        if let Some(authn) = request.extensions().get::<auth::Authn>() {
            local.extensions_mut().insert(authn.clone());
        }
        if let Some(info) = request
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        {
            local.extensions_mut().insert(*info);
        }
        let forwarded = forward::forward_call(&state, request).await;
        if forwarded.status() != axum::http::StatusCode::SERVICE_UNAVAILABLE {
            return forwarded;
        }
        return next.run(local).await;
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

/// The routes a node answers whatever its state: who the token is, the nodes and the cluster (reads), and the
/// calls on a node (its drain, backup and metrics), which is how a drained node is returned.
fn answers_in_any_state(method: &axum::http::Method, path: &str) -> bool {
    match path {
        "/auth/me" | "/nodes" | "/cluster/status" | "/cluster/backup" => {
            method == axum::http::Method::GET
        }
        _ => path.starts_with("/nodes/"),
    }
}

/// The routes of the client API that go through the whole stack (authentication, rate limit, drain refusal and
/// authorization). The `/tokens` routes only exist with authentication on (otherwise they answer `404`).
fn client_core(auth_enabled: bool) -> OpenApiRouter<AppState> {
    let mut routes = OpenApiRouter::new()
        .merge(topics::routes())
        .merge(messages::routes());
    if auth_enabled {
        routes = routes.merge(tokens::routes());
    }
    // Who the token is, the nodes and the cluster. The cluster-wide routes share their names with the
    // operations API's, and both ports serve them.
    routes
        .merge(auth::routes())
        .merge(nodes::routes())
        .merge(topic_stats::client_routes())
        .merge(nodes::cluster_wide())
}

/// The documented client API, without layers and without state: every route, with the same conditions as a node
/// running with `auth_enabled` and `prometheus_enabled`. `taba openapi client` prints it.
pub fn client_api(auth_enabled: bool, prometheus_enabled: bool) -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(openapi::client_base())
        .merge(client_core(auth_enabled))
        .merge(operations::metrics_route(prometheus_enabled))
        .merge(operations::probe_routes())
}

/// The documented monitoring API, without layers and without state. `taba openapi monitoring` prints it.
pub fn monitoring_api(prometheus_enabled: bool) -> OpenApiRouter<AppState> {
    operations::monitoring_api(prometheus_enabled)
}

/// Client API, with its documentation (`/openapi/client.json` and the interface at `/docs`, both open).
/// Of the monitoring routes it serves `/health/live` and `/health/ready`, which need no token (the
/// orchestrator's probes do not send one), and `/metrics`, which takes any valid token; the same
/// routes, without a token, live on the operations port ([`operations_router`]).
///
/// The layers, from the outside in: who is calling (`authenticate`), the rate limit, the drain
/// refusal and authorization (`authorize`). The rate limit comes after knowing who is calling, so the
/// token becomes the counter's key, and before refusing, so the `401` also counts by IP.
pub fn client_router(state: AppState) -> Router {
    let core = client_core(state.auth.enabled)
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
    let mut api = OpenApiRouter::with_openapi(openapi::client_base())
        .merge(core)
        .merge(operations::probe_routes());
    // `/metrics` describes this node, so it stays out of the drain refusal (which would forward it to another
    // node); it only asks for a token. With `telemetry.prometheus.enabled` off the route does not exist, and a
    // layer on an empty router is an error in axum.
    if state.telemetry.prometheus_enabled {
        api = api.merge(
            operations::metrics_route(true)
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth::authorize,
                ))
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth::authenticate,
                )),
        );
    }
    let (routes, spec) = api.split_for_parts();
    common::finish(
        routes.merge(openapi::docs("/openapi/client.json", spec)),
        state,
    )
}

/// Operations API, on its own port.
pub fn operations_router(state: AppState) -> Router {
    common::finish(operations::routes(&state), state)
}

/// Cluster API: only `/cluster/*`; the rest answers `404`.
pub fn cluster_router(state: AppState) -> Router {
    common::finish(cluster_api::routes(), state)
}
