//! Operations API (doc 06b): node health, a single node's `/health/cluster`, the drain, `/metrics`
//! (`http/metrics`) and `/metrics/topics/*` (`http/topic_stats`).
//!
//! These routes use no token and only accept the IPs of `operations.allowed-ips` (empty list:
//! no restriction). The IP is the resolved client's (`client-ip.*`, doc 12).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::ApiError;
use super::consolidate::{self, Missing};
use super::{AppState, ratelimit};
use crate::cluster::drain::{Cancelled, DrainStatus, Started};
use crate::cluster::health::{self, HealthSignals};
use crate::cluster::members::Member;
use crate::cluster::protocol::{Request as PeerRequest, Response as PeerResponse};
use crate::cluster::service::ClusterService;
use crate::cluster::state::{DrainState, NodeShared, NodeState};
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time::{format_ms, parse_ms};
use crate::engine::messages::Messages;
use crate::engine::messages::stats::StatsRules;

/// Body of `/health/live` and `/health/ready`. These routes never return an `ErrorResponse`
/// (doc 06b), so the orchestrator can read the reason without handling errors.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub health: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// `DrainStatus` (doc 06b).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrainStatusDto {
    drain_state: &'static str,
    trigger: Option<&'static str>,
    started_at: Option<String>,
    pending_messages: u64,
}

impl From<DrainStatus> for DrainStatusDto {
    fn from(status: DrainStatus) -> Self {
        Self {
            drain_state: status.state.as_str(),
            trigger: status.trigger.map(|t| t.as_str()),
            started_at: status.started_at.map(|at| format_ms(unix_ms(at))),
            pending_messages: status.pending_messages,
        }
    }
}

fn unix_ms(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The operations routes without the IP filter and the rate limit: what a node serves to itself and to
/// the other members (`/nodes/{nodeId}/...` of the client API).
fn bare_routes(state: &AppState) -> Router<AppState> {
    let mut routes = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/health/cluster", get(cluster_health))
        .route(
            "/node/drain",
            get(drain_status).post(drain_start).delete(drain_cancel),
        )
        .merge(super::topic_stats::routes())
        .merge(super::backup::routes())
        .merge(super::restore::routes());
    // With `telemetry.prometheus.enabled` off the route does not exist (`404`).
    if state.telemetry.prometheus_enabled {
        routes = routes.route("/metrics", get(super::metrics::prometheus));
    }
    routes
}

/// The operations routes as a router of their own, with no IP filter or rate limit: the calls that come
/// from another member (`Request::NodeCall`) and the node's own, already authorized by the client API.
pub fn local_router(state: AppState) -> Router {
    super::common::finish(bare_routes(&state), state)
}

/// The operations routes, already with the `operations.allowed-ips` filter.
pub fn routes(state: &AppState) -> Router<AppState> {
    bare_routes(state)
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            ratelimit::rate_limit,
        ))
        .route_layer(middleware::from_fn_with_state(state.clone(), ip_filter))
}

/// `403 ops.ip-not-allowed` for an IP outside `operations.allowed-ips`.
async fn ip_filter(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let allowed = &state.allowed_ips;
    let ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(peer)| state.client_ip.resolve(*peer, request.headers()));
    let permitted =
        allowed.is_empty() || ip.is_some_and(|ip| allowed.iter().any(|n| n.contains(&ip)));
    if permitted {
        next.run(request).await
    } else {
        ApiError(TabaError::new(
            ErrorCode::OpsIpNotAllowed,
            "The source IP is not in operations.allowed-ips.",
        ))
        .into_response()
    }
}

async fn live() -> Json<HealthResponse> {
    Json(HealthResponse {
        health: "live",
        reason: None,
    })
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    match state.node.not_ready_reason() {
        None => (
            StatusCode::OK,
            Json(HealthResponse {
                health: "ready",
                reason: None,
            }),
        ),
        Some(reason) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                health: "not-ready",
                reason: Some(reason),
            }),
        ),
    }
}

/// What the `NodeStatus` reads from the node. Kept apart from `AppState` because the cluster service keeps the
/// source of `GET /cluster/status`, and `AppState` keeps the service: a reference cycle.
#[derive(Clone)]
pub struct StatusParts {
    clock: Arc<dyn Clock>,
    node: Arc<NodeShared>,
    messages: Arc<Messages>,
    stats_rules: Arc<StatsRules>,
    telemetry: Arc<crate::telemetry::Telemetry>,
}

impl StatusParts {
    pub fn of(state: &AppState) -> Self {
        Self {
            clock: state.clock.clone(),
            node: state.node.clone(),
            messages: state.messages.clone(),
            stats_rules: state.stats_rules.clone(),
            telemetry: state.telemetry.clone(),
        }
    }

    /// Did the free space in `storage.dir` fall below `stats.disk.free-min`?
    pub fn storage_pressure(&self) -> bool {
        matches!(
            (self.messages.disk_free_bytes(), self.stats_rules.disk_free_min),
            (Some(free), Some(min)) if free < min
        )
    }
}

/// This node's `NodeStatus` (doc 06b), without the metrics. With the member table, the signals come from the
/// cluster; before it (or in development) the node is a cluster of one.
pub fn node_status(parts: &StatusParts, cluster: Option<&ClusterService>) -> Value {
    let node = &parts.node;
    let storage = parts.messages.storage_stats();
    let disk_free = parts.messages.disk_free_bytes();
    let mut storage_json = json!({
        "memoryUsedBytes": storage.memory_bytes,
        "memoryLimitBytes": storage.memory_limit_bytes,
        "memoryMessages": storage.memory_messages,
        "diskMessages": storage.disk_messages,
    });
    if let (Some(free), Some(object)) = (disk_free, storage_json.as_object_mut()) {
        object.insert("diskFreeBytes".into(), json!(free));
    }
    let signals = cluster
        .and_then(health::collect)
        .unwrap_or_else(|| HealthSignals {
            quorum_available: true,
            ..HealthSignals::default()
        });
    let now = format_ms(parts.clock.now_millis());
    json!({
        "nodeId": node.node_id(),
        "version": cluster.map_or(env!("CARGO_PKG_VERSION"), |c| c.program_version()),
        "state": node.state().as_str(),
        "drainState": node.drain_state().as_str(),
        "startedAt": format_ms(unix_ms(node.started_at)),
        "collectedAt": now,
        "excludedSince": format_ms(parts.messages.excluded_since()),
        "storage": storage_json,
        "runtime": runtime_json(&parts.telemetry),
        "signals": signals,
    })
}

/// What the node's registry says about its load, for a client that cannot read the Prometheus text:
/// the requests in progress, the requests refused with `429` since the start, and the worst `p95` of the
/// round trip of its channels to the other nodes (`null` with no observation).
fn runtime_json(telemetry: &crate::telemetry::Telemetry) -> Value {
    use crate::telemetry::metrics::Value as Metric;
    let (mut active, mut rejected, mut rtt) = (0.0_f64, 0.0_f64, None::<f64>);
    for family in telemetry.registry.snapshot() {
        for (_, value) in &family.series {
            match (family.def.name, value) {
                ("http.server.active_requests", Metric::Number(n)) => active += n,
                ("taba.ratelimit.rejected", Metric::Number(n)) => rejected += n,
                ("taba.cluster.channel.rtt", Metric::Histogram(h)) => {
                    rtt = match (rtt, h.percentile(0.95)) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        (a, b) => a.or(b),
                    };
                }
                _ => {}
            }
        }
    }
    json!({
        "activeRequests": active.max(0.0),
        "rateLimitRejected": rejected,
        "channelRttP95Seconds": rtt,
    })
}

/// A node in the `GET /health/cluster` response: the `NodeStatus`, or `None` if it did not answer.
type Reported = (String, Option<Value>);

/// `GET /health/cluster`. With `scope=cluster` (the default) it asks all the members for
/// `GET /cluster/status` and joins the conditions; one that does not answer shows up with `reachable: false`.
/// With `scope=node`, or without the member table (a node in development, or one that has not joined yet),
/// the cluster is this node.
pub(super) async fn cluster_health(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let scope = match query.get("scope").map(String::as_str) {
        None | Some("cluster") => "cluster",
        Some("node") => "node",
        Some(_) => {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "The request has invalid fields.",
            )
            .with_detail(
                "scope",
                ErrorCode::ValidationFailed,
                "Expected `cluster` or `node`.",
            )
            .into());
        }
    };
    let parts = StatusParts::of(&state);
    let local = node_status(&parts, state.cluster.as_deref());
    let table = state.cluster.as_ref().and_then(|c| c.membership().table());
    let (reported, missing): (Vec<Reported>, Vec<Missing>) = match (&state.cluster, &table) {
        (Some(cluster), Some(table))
            if scope == "cluster"
                && table
                    .members
                    .iter()
                    .any(|m| m.node_id == state.node.node_id()) =>
        {
            ask_members(cluster, &table.members).await
        }
        _ => (
            vec![(state.node.node_id(), Some(local.clone()))],
            Vec::new(),
        ),
    };

    let (members, members_version) = match &table {
        Some(table) => {
            let mut counts = serde_json::Map::new();
            for member in &table.members {
                let entry = counts
                    .entry(member.state.as_str().to_owned())
                    .or_insert(json!(0));
                *entry = json!(entry.as_u64().unwrap_or(0) + 1);
            }
            (Value::Object(counts), table.version)
        }
        None => (
            match (state.node.drain_state(), state.node.state()) {
                (DrainState::Draining, _) => json!({ "draining": 1 }),
                (DrainState::Drained, _) => json!({}),
                (_, NodeState::Active) => json!({ "active": 1 }),
                (_, NodeState::Syncing) => json!({ "syncing": 1 }),
                _ => json!({}),
            },
            0,
        ),
    };

    // The conditions: those of each node that answered, plus those that only appear when comparing the nodes.
    let disk_free_min = state.stats_rules.disk_free_min;
    let unreachable = reported.iter().any(|(_, status)| status.is_none());
    let mut per_node = Vec::new();
    let (mut quorum_available, mut syncing_blocks) = (true, 0u64);
    for (_, status) in &reported {
        let Some(status) = status else { continue };
        let signals = status
            .get("signals")
            .and_then(|s| serde_json::from_value::<HealthSignals>(s.clone()).ok())
            .unwrap_or_else(|| HealthSignals {
                quorum_available: true,
                ..HealthSignals::default()
            });
        let free = status["storage"]["diskFreeBytes"].as_u64();
        let pressure = matches!((free, disk_free_min), (Some(free), Some(min)) if free < min);
        quorum_available &= signals.quorum_available;
        syncing_blocks = syncing_blocks.max(signals.syncing_blocks);
        per_node.push((signals.members_version, signals.conditions(pressure)));
    }
    let conditions = health::merge(per_node, unreachable);
    let worst = conditions.first().map_or("ok", |c| c.as_str());
    let nodes: Vec<Value> = reported
        .iter()
        .map(|(node_id, status)| {
            json!({ "nodeId": node_id, "reachable": status.is_some(), "status": status })
        })
        .collect();
    // Nobody serving: every node that answered is draining, drained, syncing or failed. With S3 the stop list
    // says whether it was a planned stop (`POST /cluster/stop`), so the operator knows why the API refuses.
    let serving = reported
        .iter()
        .filter(|(_, status)| {
            status
                .as_ref()
                .is_some_and(|s| s["state"] == "active" && s["drainState"] == "active")
        })
        .count();
    // Stopped is every node that answered being in drain; a cluster that is only starting up (syncing) is not.
    let answered: Vec<&Value> = reported.iter().filter_map(|(_, s)| s.as_ref()).collect();
    let stopped = !answered.is_empty()
        && answered
            .iter()
            .all(|s| s["drainState"] == "draining" || s["drainState"] == "drained");
    // The cut-off of the excluded log: the most recent start among the active members, because an exclusion
    // older than that may be missing from some of them (doc 07, section 10.3).
    let excluded_cutoff = answered
        .iter()
        .filter(|s| s["state"] == "active")
        .filter_map(|s| s["excludedSince"].as_str().and_then(parse_ms))
        .max();
    let stop_list = match (&state.backup, stopped) {
        (Some(backup), true) => backup.read_stop_list().await,
        _ => None,
    };
    Ok(Json(json!({
        "scope": scope,
        "collectedAt": format_ms(state.clock.now_millis()),
        "partial": unreachable,
        "missingNodes": missing.iter().map(Missing::json).collect::<Vec<_>>(),
        "worstCondition": worst,
        "conditions": conditions.iter().map(|c| c.entry()).collect::<Vec<_>>(),
        "summary": {
            "members": members,
            "membersVersion": members_version,
            "quorumAvailable": quorum_available,
            "syncingBlocks": syncing_blocks,
            "servingNodes": serving,
            "stopped": stopped,
            "plannedStop": stop_list.is_some(),
            "stoppedAt": stop_list.as_ref().map(|l| format_ms(l.at_ms)),
            "excludedCutoff": excluded_cutoff.map(format_ms),
        },
        "nodes": nodes,
    })))
}

/// Asks each member for its `NodeStatus` (itself without the network), all at once, and returns in table
/// order what each said and who did not answer (and why).
pub(super) async fn ask_members(
    cluster: &Arc<ClusterService>,
    members: &[Member],
) -> (Vec<Reported>, Vec<Missing>) {
    let answers = consolidate::ask_members(cluster, members, &PeerRequest::Status).await;
    let mut reported = Vec::with_capacity(answers.len());
    let mut missing = Vec::new();
    for (node_id, answer) in answers {
        match answer {
            Ok(PeerResponse::Status(status)) => reported.push((node_id, Some(status))),
            Ok(_) => {
                missing.push(Missing {
                    node_id: node_id.clone(),
                    reason: "error",
                });
                reported.push((node_id, None));
            }
            Err(reason) => {
                missing.push(Missing {
                    node_id: node_id.clone(),
                    reason,
                });
                reported.push((node_id, None));
            }
        }
    }
    (reported, missing)
}

async fn drain_status(State(state): State<AppState>) -> Json<DrainStatusDto> {
    Json(state.drain.status().into())
}

async fn drain_start(State(state): State<AppState>) -> (StatusCode, Json<DrainStatusDto>) {
    match state.drain.start_api() {
        Started::Started(status) => (StatusCode::ACCEPTED, Json(status.into())),
        Started::AlreadyDraining(status) => (StatusCode::OK, Json(status.into())),
    }
}

async fn drain_cancel(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<DrainStatusDto>), ApiError> {
    Ok(match state.drain.cancel()? {
        Cancelled::Nothing(status) | Cancelled::Cancelled(status) => {
            (StatusCode::OK, Json(status.into()))
        }
        Cancelled::Returned(status) => (StatusCode::ACCEPTED, Json(status.into())),
    })
}
