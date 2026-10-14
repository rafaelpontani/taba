//! API de operação (doc 06b): saúde do nó, `/health/cluster` de um nó só, o drain, `/metrics`
//! (`http/metrics`) e `/metrics/topics/*` (`http/topic_stats`).
//!
//! Estas rotas não usam token e só aceitam os IPs de `operations.allowed-ips` (lista vazia:
//! sem restrição). O IP é o do cliente resolvido (`client-ip.*`, doc 12).

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
use super::{AppState, ratelimit};
use crate::cluster::drain::{Cancelled, DrainStatus, Started};
use crate::cluster::health::{self, HealthSignals};
use crate::cluster::members::Member;
use crate::cluster::protocol::{Request as PeerRequest, Response as PeerResponse};
use crate::cluster::service::ClusterService;
use crate::cluster::state::{DrainState, NodeShared, NodeState};
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time::format_ms;
use crate::engine::messages::Messages;
use crate::engine::messages::stats::StatsRules;

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
        .merge(super::topic_stats::routes())
        .merge(super::backup::routes());
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

/// O que o `NodeStatus` lê do nó. Fica separado do `AppState` porque o serviço do cluster guarda a
/// fonte do `GET /cluster/status`, e o `AppState` guarda o serviço: um ciclo de referências.
#[derive(Clone)]
pub struct StatusParts {
    clock: Arc<dyn Clock>,
    node: Arc<NodeShared>,
    messages: Arc<Messages>,
    stats_rules: Arc<StatsRules>,
}

impl StatusParts {
    pub fn of(state: &AppState) -> Self {
        Self {
            clock: state.clock.clone(),
            node: state.node.clone(),
            messages: state.messages.clone(),
            stats_rules: state.stats_rules.clone(),
        }
    }

    /// O espaço livre em `storage.dir` caiu abaixo de `stats.disk.free-min`?
    pub fn storage_pressure(&self) -> bool {
        matches!(
            (self.messages.disk_free_bytes(), self.stats_rules.disk_free_min),
            (Some(free), Some(min)) if free < min
        )
    }
}

/// O `NodeStatus` deste nó (doc 06b), sem as métricas. Com a tabela de membros, os sinais vêm do
/// cluster; antes dela (ou no desenvolvimento) o nó é um cluster de um só.
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
        "version": env!("CARGO_PKG_VERSION"),
        "mode": node.mode.as_str(),
        "state": node.state().as_str(),
        "drainState": node.drain_state().as_str(),
        "startedAt": format_ms(unix_ms(node.started_at)),
        "collectedAt": now,
        "storage": storage_json,
        "signals": signals,
    })
}

/// Um nó na resposta de `GET /health/cluster`: o `NodeStatus`, ou `None` se não respondeu.
type Reported = (String, Option<Value>);

/// `GET /health/cluster`. Com `scope=cluster` (o padrão) pergunta a todos os membros pelo
/// `GET /cluster/status` e junta as condições; um que não responde aparece com `reachable: false`.
/// Com `scope=node`, ou sem a tabela de membros (nó em desenvolvimento, ou que ainda não entrou),
/// o cluster é este nó.
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
    let parts = StatusParts::of(&state);
    let local = node_status(&parts, state.cluster.as_deref());
    let table = state.cluster.as_ref().and_then(|c| c.membership().table());
    let reported: Vec<Reported> = match (&state.cluster, &table) {
        (Some(cluster), Some(table)) if scope == "cluster" => {
            ask_members(cluster, &table.members).await
        }
        _ => vec![(state.node.node_id(), Some(local.clone()))],
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

    // As condições: as de cada nó que respondeu, mais as que só aparecem comparando os nós.
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
    Ok(Json(json!({
        "scope": scope,
        "collectedAt": format_ms(state.clock.now_millis()),
        "partial": unreachable,
        "worstCondition": worst,
        "conditions": conditions.iter().map(|c| c.entry()).collect::<Vec<_>>(),
        "summary": {
            "members": members,
            "membersVersion": members_version,
            "quorumAvailable": quorum_available,
            "syncingBlocks": syncing_blocks,
        },
        "nodes": nodes,
    })))
}

/// Pergunta o `NodeStatus` a cada membro (a si mesmo sem rede), todos ao mesmo tempo, e devolve
/// na ordem da tabela.
async fn ask_members(cluster: &Arc<ClusterService>, members: &[Member]) -> Vec<Reported> {
    let mut set = tokio::task::JoinSet::new();
    for (index, member) in members.iter().enumerate() {
        let (cluster, member) = (cluster.clone(), member.clone());
        set.spawn(async move {
            let status = match cluster.call_member(&member, PeerRequest::Status).await {
                Ok(PeerResponse::Status(status)) => Some(status),
                _ => None,
            };
            (index, (member.node_id, status))
        });
    }
    let mut answers = Vec::with_capacity(members.len());
    while let Some(Ok(answer)) = set.join_next().await {
        answers.push(answer);
    }
    answers.sort_by_key(|(index, _)| *index);
    answers.into_iter().map(|(_, reported)| reported).collect()
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
