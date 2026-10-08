//! API de operação (doc 06b): saúde do nó, `/health/cluster` de um nó só, o drain, `/metrics`
//! (`http/metrics`) e `/metrics/topics/*` (`http/topic_stats`).
//!
//! Estas rotas não usam token e só aceitam os IPs de `operations.allowed-ips` (lista vazia:
//! sem restrição). O IP é o do cliente resolvido (`client-ip.*`, doc 12).

use std::collections::HashMap;
use std::net::SocketAddr;
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
use super::{AppState, ratelimit};
use crate::cluster::drain::{Cancelled, DrainStatus, Started};
use crate::cluster::state::{DrainState, NodeState};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time::format_ms;

/// Corpo de `/health/live` e `/health/ready`. Essas rotas nunca devolvem `ErrorResponse`
/// (doc 06b), para o orquestrador poder ler o motivo sem tratar erros.
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

/// As rotas de operação, já com o filtro de `operations.allowed-ips`.
pub fn routes(state: &AppState) -> Router<AppState> {
    let mut routes = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/health/cluster", get(cluster_health))
        .route(
            "/node/drain",
            get(drain_status).post(drain_start).delete(drain_cancel),
        )
        .merge(super::topic_stats::routes());
    // Com `telemetry.prometheus.enabled` desligado a rota não existe (`404`).
    if state.telemetry.prometheus_enabled {
        routes = routes.route("/metrics", get(super::metrics::prometheus));
    }
    routes
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            ratelimit::rate_limit,
        ))
        .route_layer(middleware::from_fn_with_state(state.clone(), ip_filter))
}

/// `403 ops.ip-not-allowed` para o IP fora de `operations.allowed-ips`.
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
            "O IP de origem não está em operations.allowed-ips.",
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

/// `GET /health/cluster`. Ainda sem a consulta aos outros nós (`GET /cluster/status`), o cluster é
/// este nó: `scope=cluster` e `scope=node` devolvem o mesmo, com os membros e a época da tabela
/// quando o nó já a tem.
async fn cluster_health(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let scope = match query.get("scope").map(String::as_str) {
        None | Some("cluster") => "cluster",
        Some("node") => "node",
        Some(_) => {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "A requisição tem campos inválidos.",
            )
            .with_detail(
                "scope",
                ErrorCode::ValidationFailed,
                "Esperado `cluster` ou `node`.",
            )
            .into());
        }
    };
    let now = format_ms(state.clock.now_millis());
    let node = &state.node;
    let storage = state.messages.storage_stats();
    // Com a tabela de membros (nó em produção que já entrou), os membros e a época vêm dela.
    let table = state.cluster.as_ref().and_then(|c| c.membership().table());
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
            match (node.drain_state(), node.state()) {
                (DrainState::Draining, _) => json!({ "draining": 1 }),
                (DrainState::Drained, _) => json!({}),
                (_, NodeState::Active) => json!({ "active": 1 }),
                (_, NodeState::Syncing) => json!({ "syncing": 1 }),
                _ => json!({}),
            },
            0,
        ),
    };
    let disk_free = state.messages.disk_free_bytes();
    let mut storage_json = json!({
        "memoryUsedBytes": storage.memory_bytes,
        "memoryLimitBytes": storage.memory_limit_bytes,
        "memoryMessages": storage.memory_messages,
        "diskMessages": storage.disk_messages,
    });
    if let (Some(free), Some(object)) = (disk_free, storage_json.as_object_mut()) {
        object.insert("diskFreeBytes".into(), json!(free));
    }
    // `storage-pressure`: o espaço livre em `storage.dir` caiu abaixo de `stats.disk.free-min`.
    let pressure = matches!(
        (disk_free, state.stats_rules.disk_free_min),
        (Some(free), Some(min)) if free < min
    );
    let (worst, conditions) = if pressure {
        (
            "storage-pressure",
            json!([{ "condition": "storage-pressure", "severity": "warning" }]),
        )
    } else {
        ("ok", json!([]))
    };
    let status = json!({
        "nodeId": node.node_id(),
        "version": env!("CARGO_PKG_VERSION"),
        "mode": node.mode.as_str(),
        "state": node.state().as_str(),
        "drainState": node.drain_state().as_str(),
        "startedAt": format_ms(unix_ms(node.started_at)),
        "collectedAt": now,
        "storage": storage_json,
        "signals": {
            "membersVersion": members_version,
            "quorumAvailable": true,
            "channels": {},
            "syncingBlocks": 0,
            "blocksBelowReplication": 0,
            "blocksBelowQuorum": 0,
            "frozenNodes": 0,
        },
    });
    Ok(Json(json!({
        "scope": scope,
        "collectedAt": now,
        "partial": false,
        "worstCondition": worst,
        "conditions": conditions,
        "summary": {
            "members": members,
            "membersVersion": members_version,
            "quorumAvailable": true,
            "syncingBlocks": 0,
        },
        "nodes": [{ "nodeId": node.node_id(), "reachable": true, "status": status }],
    })))
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
