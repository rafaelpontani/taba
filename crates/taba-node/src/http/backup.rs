//! The S3 backup operations routes (doc 23, "S3 backup", section 1.11), on the operations API
//! (like `/node/drain`: no token, only the IPs of `operations.allowed-ips`, `admin` category of the rate
//! limit).
//!
//! | Route | What it does |
//! |---|---|
//! | `GET /node/backup` | The backup state on this node (send queue, S3 up, segments) |
//! | `POST /node/backup/flush` | Forces the backup on this node: memory → disk, disk → S3, `outbox` → S3. Neither deregisters nor stops the node |
//! | `POST /cluster/backup/flush` | The node that receives it forwards to all the others and joins the responses |
//! | `POST /cluster/stop` | Drain on **all** nodes and the list of nodes alive at the stop |
//!
//! "Starting the cluster" is not a route: it is starting the processes (`docker compose start`); the restore
//! happens by itself on startup.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use super::AppState;
use super::common::ApiError;
use super::openapi::{AuthErrors, RateLimited, StopReport};
use crate::cluster::backup::{Backup, BackupStatus, not_configured};
use crate::cluster::protocol::{Request as PeerRequest, Response as PeerResponse};
use crate::core::error::ErrorResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/node/backup", get(status))
        .route("/node/backup/flush", post(node_flush))
        .route("/cluster/backup/flush", post(cluster_flush))
        .route("/cluster/stop", post(cluster_stop))
}

fn backup_of(state: &AppState) -> Result<&Arc<Backup>, ApiError> {
    state.backup.as_ref().ok_or_else(|| not_configured().into())
}

/// The `BackupStatus` as the API shows it.
pub fn status_json(status: &BackupStatus) -> Value {
    status.to_json()
}

async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = backup_of(&state)?;
    Ok(Json(status_json(&backup.status())))
}

async fn node_flush(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = backup_of(&state)?;
    let report = backup.flush().await;
    let mut body = report.to_json();
    body["nodeId"] = json!(backup.node_id());
    Ok(Json(body))
}

/// Calls `request` on each member (itself without the network), all at once, and returns the
/// responses in the order they arrived: `(nodeId, Ok(json) | Err(reason))`.
async fn ask_all(state: &AppState, request: PeerRequest) -> Vec<(String, Result<Value, String>)> {
    let Some(cluster) = &state.cluster else {
        return Vec::new();
    };
    let Some(table) = cluster.membership().table() else {
        return Vec::new();
    };
    let mut set = tokio::task::JoinSet::new();
    for member in table.members {
        let (cluster, request) = (cluster.clone(), request.clone());
        set.spawn(async move {
            let answer = match cluster.call_member(&member, request).await {
                Ok(PeerResponse::Backup(report)) => Ok(report),
                Ok(_) => Err("unexpected response".to_owned()),
                Err(e) => Err(e.to_string()),
            };
            (member.node_id, answer)
        });
    }
    let mut answers = Vec::new();
    while let Some(Ok(answer)) = set.join_next().await {
        answers.push(answer);
    }
    answers
}

/// Sends the memory and the disk of **every** node to the S3 bucket, without stopping anything. The node that receives
/// it forwards to the others and joins the answers. In the client API it asks for a token; on the monitoring port the
/// same route has none.
///
/// Permission: `admin`.
#[utoipa::path(
    post,
    path = "/cluster/backup/flush",
    tag = "Cluster",
    operation_id = "flushClusterBackup",
    responses(
        (status = 200, description = "`complete` says if nothing was left pending, and `nodes` has the report of each node.", body = Object),
        (status = 422, description = "`backup.not-configured`.", body = ErrorResponse),
        AuthErrors, RateLimited
    )
)]
pub(super) async fn cluster_flush(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = backup_of(&state)?;
    if state.cluster.is_none() {
        let mut body = backup.flush().await.to_json();
        body["nodeId"] = json!(backup.node_id());
        return Ok(Json(
            json!({ "complete": body["complete"], "nodes": [body] }),
        ));
    }
    let mut answers = ask_all(&state, PeerRequest::BackupFlush).await;
    answers.sort_by(|a, b| a.0.cmp(&b.0));
    let mut complete = true;
    let nodes: Vec<Value> = answers
        .into_iter()
        .map(|(node_id, answer)| match answer {
            Ok(mut report) => {
                complete &= report["complete"].as_bool().unwrap_or(false);
                report["nodeId"] = json!(node_id);
                report
            }
            // The slow or down node shows up as pending: the operator repeats the call.
            Err(why) => {
                complete = false;
                json!({ "nodeId": node_id, "complete": false, "pending": true, "error": why })
            }
        })
        .collect();
    Ok(Json(json!({ "complete": complete, "nodes": nodes })))
}

/// Stops the whole cluster: it drains every node and, with S3, writes the list of the nodes that were alive. The
/// processes keep running as `drained`: stopping them is up to the orchestrator.
///
/// Permission: **the root token** only.
#[utoipa::path(
    post,
    path = "/cluster/stop",
    tag = "Cluster",
    operation_id = "stopCluster",
    responses(
        (status = 200, description = "What the stop did.", body = StopReport),
        AuthErrors, RateLimited
    )
)]
pub(super) async fn cluster_stop(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = state.backup.clone();
    let mut warnings: Vec<String> = Vec::new();
    // The other nodes stop at the same time; this one, which coordinates, stops last and writes the list.
    let me = state.node.node_id();
    let mut stopped: Vec<String> = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();
    if let Some(cluster) = state.cluster.clone() {
        let others: Vec<_> = cluster
            .membership()
            .table()
            .map(|t| t.members)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.node_id != me)
            .collect();
        let mut set = tokio::task::JoinSet::new();
        for member in others {
            let cluster = cluster.clone();
            set.spawn(async move {
                let answer = cluster.call_member(&member, PeerRequest::BackupStop).await;
                (member.node_id, answer)
            });
        }
        while let Some(Ok((node_id, answer))) = set.join_next().await {
            match answer {
                Ok(PeerResponse::Backup(report)) => {
                    if report["drained"].as_bool().unwrap_or(false) {
                        stopped.push(node_id.clone());
                    } else {
                        warnings.push(format!("node {node_id} did not finish the drain in time"));
                    }
                    nodes.push(report);
                }
                Ok(_) | Err(_) => {
                    // The silent node stays out of the list: on starting it will be treated as dead.
                    warnings.push(format!("node {node_id} did not answer the stop"));
                    nodes.push(
                        json!({ "nodeId": node_id, "drained": false, "error": "no response" }),
                    );
                }
            }
        }
    }
    let drained = state.drain.drain_and_wait().await;
    if drained {
        stopped.push(me.clone());
    } else {
        warnings.push(format!("node {me} did not finish the drain in time"));
    }
    nodes.push(json!({ "nodeId": me, "drained": drained }));
    let mut stop_list = false;
    match &backup {
        Some(backup) => {
            stop_list = backup.write_stop_list(stopped.clone()).await;
            if !stop_list {
                warnings.push(
                    "the backup was not sent to S3: the stop list was not written (the disk drain was done)"
                        .to_owned(),
                );
            }
            // The coordinator is the last to leave: its heartbeat disappears last.
            backup.leave().await;
        }
        None => warnings.push("S3 backup not configured: there is no stop list".to_owned()),
    }
    Ok(Json(json!({
        "stopped": stopped,
        "stopList": stop_list,
        "warnings": warnings,
        "nodes": nodes,
    })))
}
