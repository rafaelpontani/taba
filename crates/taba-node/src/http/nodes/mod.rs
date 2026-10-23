//! The cluster and the nodes in the client API, for the root token (doc 34, D-TUI-3, D-TUI-4 and D-TUI-9):
//! the operations API's routes, which have no token, reachable with one so that a client with a single URL
//! can use them.
//!
//! | Route | Who | What |
//! |---|---|---|
//! | `GET /nodes` | any valid token | the nodes: id, state, drain state, version (no addresses) |
//! | `GET /cluster/status` | root | the same as `GET /health/cluster` |
//! | `GET /cluster/backup` | root | the S3 backup state of every node, consolidated |
//! | `GET /nodes/{nodeId}/metrics` | root | the node's Prometheus text |
//! | `GET`, `POST`, `DELETE /nodes/{nodeId}/drain` | root | the drain of one node: status, start, return |
//! | `GET /nodes/{nodeId}/backup`, `POST /nodes/{nodeId}/backup/flush` | root | the S3 backup of one node |
//! | `POST /cluster/backup/flush`, `POST /cluster/stop` | root | the whole cluster |
//!
//! **The node rule** (the same as for a message): the node that receives `/nodes/{nodeId}/...` compares the
//! `nodeId` with its own. If it is that node, it answers from its own data; if not, it calls the node
//! that has the data **once**, on the cluster port (`Request::NodeCall`, members only), and returns the
//! answer as it came. It never forwards a call that was already forwarded; an unknown `nodeId` is
//! `404 node.not-found`, and a node that does not answer is a failure that names it, with no retry.

use axum::extract::{Path, Request, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::sync::Arc;

use serde_json::{Value, json};

use super::common::ApiError;
use super::consolidate;
use super::operations::{self, StatusParts};
use super::{AppState, backup, forward};
use crate::cluster::members::Member;
use crate::cluster::protocol::{ForwardRequest, Request as PeerRequest, Response as PeerResponse};
use crate::cluster::service::ClusterService;
use crate::cluster::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::time::format_ms;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/nodes", get(list))
        .route("/cluster/status", get(operations::cluster_health))
        .route("/cluster/backup", get(cluster_backup))
        .route("/nodes/{node_id}/metrics", get(metrics))
        .route(
            "/nodes/{node_id}/drain",
            get(drain).post(drain).delete(drain),
        )
        .route("/nodes/{node_id}/backup", get(backup_status))
        .route("/nodes/{node_id}/backup/flush", post(backup_flush))
}

/// The routes that apply to the whole cluster and have the same name as the operations API's: here behind a
/// token (`admin`, or the root token for the stop and the forced restoration), there on the operations port with
/// none.
pub fn cluster_wide() -> Router<AppState> {
    Router::new()
        .route("/cluster/backup/flush", post(backup::cluster_flush))
        .route("/cluster/stop", post(backup::cluster_stop))
        .route("/cluster/restore/force", post(super::restore::force))
}

fn node_not_found(node_id: &str) -> TabaError {
    TabaError::new(
        ErrorCode::NodeNotFound,
        format!("The cluster has no node {node_id}."),
    )
}

/// `GET /nodes`: who is in the cluster and in what state. A node that does not answer shows up with
/// `reachable: false` and the state the member table has; a node that left by drain shows up as `drained`.
async fn list(State(state): State<AppState>) -> Json<Value> {
    let me = state.node.node_id();
    let table = state.cluster.as_ref().and_then(|c| c.membership().table());
    let (reported, missing) = match (&state.cluster, &table) {
        // A node that left the cluster is not in its own table: it answers for itself.
        (Some(cluster), Some(table)) if table.members.iter().any(|m| m.node_id == me) => {
            operations::ask_members(cluster, &table.members).await
        }
        _ => {
            let parts = StatusParts::of(&state);
            (
                vec![(
                    me.clone(),
                    Some(operations::node_status(&parts, state.cluster.as_deref())),
                )],
                Vec::new(),
            )
        }
    };
    let mut items: Vec<Value> = reported
        .into_iter()
        .map(|(node_id, status)| {
            let member_state = table
                .as_ref()
                .and_then(|t| t.members.iter().find(|m| m.node_id == node_id))
                .map(|m| m.state.as_str());
            let field = |name: &str| {
                status
                    .as_ref()
                    .and_then(|s| s.get(name))
                    .cloned()
                    .unwrap_or(Value::Null)
            };
            json!({
                "nodeId": node_id,
                "self": node_id == me,
                "reachable": status.is_some(),
                "state": if status.is_some() { field("state") } else { json!(member_state) },
                "drainState": field("drainState"),
                "version": field("version"),
                "memberState": member_state,
            })
        })
        .collect();
    if let Some(cluster) = &state.cluster {
        let members = table.as_ref().map(|t| t.members.as_slice()).unwrap_or(&[]);
        for left in departed_everywhere(cluster, members).await {
            items.push(json!({
                "nodeId": left.node_id,
                "self": false,
                "reachable": false,
                "state": "drained",
                "drainState": "drained",
                "version": Value::Null,
                "memberState": Value::Null,
            }));
        }
    }
    let mut out = serde_json::Map::new();
    out.insert(
        "collectedAt".into(),
        json!(format_ms(state.clock.now_millis())),
    );
    out.insert(
        "membersVersion".into(),
        json!(table.map_or(0, |t| t.version)),
    );
    consolidate::put_envelope(&mut out, &missing);
    out.insert("items".into(), Value::Array(items));
    Json(Value::Object(out))
}

/// The nodes that left by drain, **as the whole cluster knows them**: each node only remembers the departures it
/// applied, and a node that returns from a drain comes back as a new node with an empty memory, so the answer would
/// depend on which node received the call. This asks every member (and uses this node's own list), unites them by id
/// and drops the ones that are members now or whose address a member took over (the same machine came back with a
/// new id).
async fn departed_everywhere(cluster: &Arc<ClusterService>, members: &[Member]) -> Vec<Departed> {
    let mut all: Vec<Departed> = cluster
        .membership()
        .departed_members()
        .into_iter()
        .map(|m| Departed {
            node_id: m.node_id,
            addresses: m.addresses,
            port: m.port,
        })
        .collect();
    let others: Vec<Member> = members
        .iter()
        .filter(|m| m.node_id != cluster.node_id())
        .cloned()
        .collect();
    for (_, answer) in consolidate::ask_members(cluster, &others, &PeerRequest::Departed).await {
        if let Ok(PeerResponse::Json(Value::Array(list))) = answer {
            all.extend(
                list.into_iter()
                    .filter_map(|v| serde_json::from_value::<Departed>(v).ok()),
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    all.retain(|left| {
        let replaced = members.iter().any(|m| {
            m.node_id == left.node_id || (m.addresses == left.addresses && m.port == left.port)
        });
        !replaced && seen.insert(left.node_id.clone())
    });
    all
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Departed {
    node_id: String,
    addresses: Vec<String>,
    port: u16,
}

/// The member the call is for: in the table, or among those that left by drain (to return them).
fn target_of(state: &AppState, node_id: &str) -> Result<Member, TabaError> {
    let cluster = state
        .cluster
        .as_ref()
        .ok_or_else(|| node_not_found(node_id))?;
    let membership = cluster.membership();
    membership
        .table()
        .and_then(|t| t.members.into_iter().find(|m| m.node_id == node_id))
        .or_else(|| {
            membership
                .departed_members()
                .into_iter()
                .find(|m| m.node_id == node_id)
        })
        .ok_or_else(|| node_not_found(node_id))
}

/// Runs the operations route `path_and_query` on node `node_id`: here if it is this node, otherwise once on
/// the node that has it.
async fn on_node(
    state: &AppState,
    node_id: &str,
    method: &str,
    path_and_query: String,
) -> Result<Response, ApiError> {
    let call = ForwardRequest {
        method: method.to_owned(),
        path_and_query,
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
    };
    if node_id == state.node.node_id() {
        let router = operations::local_router(state.clone());
        return Ok(forward::response_of(forward::run(router, call).await?));
    }
    let target = target_of(state, node_id)?;
    let cluster = state
        .cluster
        .as_ref()
        .ok_or_else(|| node_not_found(node_id))?;
    match cluster
        .call_member(&target, PeerRequest::NodeCall(call))
        .await
    {
        Ok(PeerResponse::Forwarded(reply)) => Ok(forward::response_of(reply)),
        Ok(_) => Err(unanswered(node_id, "unexpected response").into()),
        Err(CallError::Remote(error)) => Err(error.into()),
        Err(CallError::Timeout) => Err(TabaError::new(
            ErrorCode::ForwardTimeout,
            format!("Node {node_id} did not answer in time."),
        )
        .into()),
        Err(other) => Err(unanswered(node_id, &other.to_string()).into()),
    }
}

fn unanswered(node_id: &str, why: &str) -> TabaError {
    TabaError::new(
        ErrorCode::ForwardBadGateway,
        format!("Node {node_id} did not answer: {why}."),
    )
}

/// `GET /nodes/{nodeId}/metrics`: the node's Prometheus text.
async fn metrics(
    State(state): State<AppState>,
    Path(node_id): Path<String>,
) -> Result<Response, ApiError> {
    on_node(&state, &node_id, "GET", "/metrics".to_owned()).await
}

/// `GET /cluster/backup`: the S3 backup state of **every** node, each as `GET /nodes/{nodeId}/backup` gives
/// it, with the envelope of the consolidated reads (`partial`, `missingNodes`).
async fn cluster_backup(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = state
        .backup
        .as_ref()
        .ok_or_else(crate::cluster::backup::not_configured)?;
    let answers = match consolidate::ask_all(&state, &PeerRequest::BackupStatus).await {
        Some(answers) => answers,
        None => vec![(
            state.node.node_id(),
            Ok(PeerResponse::Backup(backup.status().to_json())),
        )],
    };
    let missing = consolidate::missing_of(&answers);
    let nodes: Vec<Value> = answers
        .into_iter()
        .filter_map(|(node_id, answer)| match answer {
            Ok(PeerResponse::Backup(Value::Object(mut report))) => {
                report.insert("nodeId".into(), json!(node_id));
                Some(Value::Object(report))
            }
            _ => None,
        })
        .collect();
    let mut out = serde_json::Map::new();
    out.insert(
        "collectedAt".into(),
        json!(format_ms(state.clock.now_millis())),
    );
    consolidate::put_envelope(&mut out, &missing);
    out.insert("nodes".into(), Value::Array(nodes));
    Ok(Json(Value::Object(out)))
}

async fn drain(
    State(state): State<AppState>,
    Path(node_id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let method = request.method().as_str().to_owned();
    on_node(&state, &node_id, &method, "/node/drain".to_owned()).await
}

async fn backup_status(
    State(state): State<AppState>,
    Path(node_id): Path<String>,
) -> Result<Response, ApiError> {
    on_node(&state, &node_id, "GET", "/node/backup".to_owned()).await
}

async fn backup_flush(
    State(state): State<AppState>,
    Path(node_id): Path<String>,
) -> Result<Response, ApiError> {
    on_node(&state, &node_id, "POST", "/node/backup/flush".to_owned()).await
}
