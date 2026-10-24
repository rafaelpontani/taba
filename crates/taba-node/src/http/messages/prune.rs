//! `DELETE /topics/{topic}/messages`: empties a queue in the whole cluster (doc 34, D-TUI-2).
//!
//! The node that receives it asks **every member** (itself without the network) to empty its own copy, at
//! once, with a `pruneId` that makes the call idempotent, and waits for all. The answer is success only if all
//! of them succeeded; otherwise it is `503 cluster.prune-incomplete` naming the nodes that failed. It is a rare
//! act, so there is no retry or rollback: whoever sees the failure repeats the call, which is safe.

use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Value, json};

use super::blocking;
use crate::cluster::protocol::{Request as PeerRequest, Response as PeerResponse};
use crate::core::error::{ErrorCode, ErrorResponse, TabaError};
use crate::core::id;
use crate::http::AppState;
use crate::http::common::ApiError;
use crate::http::openapi::{AuthErrors, ClusterUnavailable, PruneResult, RateLimited};

/// Removes **every message** of the queue, visible and invisible and the copies the replicas keep, in the whole
/// cluster. The topic and its definition stay. It answers success only if every node succeeded; otherwise it is
/// `503 cluster.prune-incomplete` and repeating the call is safe.
///
/// Permission: `manage` on the topic.
#[utoipa::path(
    delete,
    path = "/topics/{topic}/messages",
    tag = "Messages",
    operation_id = "pruneQueue",
    params(("topic" = String, Path, description = "The queue name.")),
    responses(
        (status = 200, description = "What was removed.", body = PruneResult),
        (status = 404, description = "`topic.not-found`.", body = ErrorResponse),
        (status = 409, description = "`topic.wrong-type`: a distributor has no messages to remove.", body = ErrorResponse),
        AuthErrors, RateLimited, ClusterUnavailable
    )
)]
pub(super) async fn prune(
    State(state): State<AppState>,
    Path(topic): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // `404` for a topic that does not exist and `409` for one that is not a queue, before asking anyone.
    state.messages.queue_view(&topic)?;
    let prune_id = id::new_at(state.clock.now_millis());
    let members = state
        .cluster
        .as_ref()
        .and_then(|cluster| cluster.membership().table())
        .map(|table| table.members)
        .unwrap_or_default();
    let (Some(cluster), false) = (state.cluster.clone(), members.is_empty()) else {
        let local = topic.clone();
        let id = prune_id.clone();
        let removed = blocking(&state, move |messages| messages.prune(&local, &id)).await?;
        return Ok(Json(json!({
            "pruneId": prune_id,
            "removed": removed,
            "nodes": [{ "nodeId": state.node.node_id(), "removed": removed }],
        })));
    };
    let mut set = tokio::task::JoinSet::new();
    for member in members {
        let request = PeerRequest::Prune {
            topic: topic.clone(),
            prune_id: prune_id.clone(),
        };
        let cluster = cluster.clone();
        set.spawn(async move {
            let answer = match cluster.call_member(&member, request).await {
                Ok(PeerResponse::Json(body)) => Ok(body["removed"].as_u64().unwrap_or(0)),
                Ok(_) => Err("unexpected response".to_owned()),
                Err(error) => Err(error.to_string()),
            };
            (member.node_id, answer)
        });
    }
    let mut answers = Vec::new();
    while let Some(Ok(answer)) = set.join_next().await {
        answers.push(answer);
    }
    answers.sort_by(|a, b| a.0.cmp(&b.0));
    let failed: Vec<&(String, Result<u64, String>)> = answers
        .iter()
        .filter(|(_, answer)| answer.is_err())
        .collect();
    if !failed.is_empty() {
        let names: Vec<&str> = failed.iter().map(|(node, _)| node.as_str()).collect();
        let mut error = TabaError::new(
            ErrorCode::ClusterPruneIncomplete,
            format!(
                "The queue was not emptied on all nodes; it failed on: {}. Repeat the call.",
                names.join(", ")
            ),
        );
        for (node, answer) in &failed {
            if let Err(why) = answer {
                error =
                    error.with_detail(node.clone(), ErrorCode::ClusterPruneIncomplete, why.clone());
            }
        }
        return Err(error.into());
    }
    let removed: u64 = answers.iter().filter_map(|(_, a)| a.as_ref().ok()).sum();
    Ok(Json(json!({
        "pruneId": prune_id,
        "removed": removed,
        "nodes": answers
            .iter()
            .map(|(node, answer)| json!({ "nodeId": node, "removed": answer.as_ref().ok() }))
            .collect::<Vec<_>>(),
    })))
}
