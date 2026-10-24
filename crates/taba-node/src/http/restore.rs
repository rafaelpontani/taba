//! The operator's force of the restoration of the previous lives (doc 07, section 10.7), on the operations API
//! (no token, only the IPs of `operations.allowed-ips`, `admin` category of the rate limit) and, for the root token,
//! on the client API (`http::nodes`).
//!
//! | Route | What it does |
//! |---|---|
//! | `POST /cluster/restore/force` | Declares the cluster restored from disk with the nodes that are present: the FIFO queues are released and a restoration held by the strict mode goes on. Spread to every node |

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::AppState;
use super::common::ApiError;
use super::openapi::{AuthErrors, RateLimited, RestoreForce};

pub fn routes() -> Router<AppState> {
    Router::new().route("/cluster/restore/force", post(force))
}

/// Declares the cluster restored from disk with the nodes that are present: the FIFO queues are released and a
/// restoration held by the strict mode goes on. It is spread to every node. **Messages already consumed may
/// reappear.** It is what releases a cluster stuck by the loss of two nodes.
///
/// Permission: **the root token** only.
#[utoipa::path(
    post,
    path = "/cluster/restore/force",
    tag = "Cluster",
    operation_id = "forceRestore",
    responses(
        (status = 200, description = "What the force did.", body = RestoreForce),
        AuthErrors, RateLimited
    )
)]
pub(super) async fn force(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(cluster) = state.cluster.clone() else {
        return Ok(Json(json!({ "wasRestoring": false, "status": "restored" })));
    };
    let Some(replication) = cluster.replication().cloned() else {
        return Ok(Json(json!({ "wasRestoring": false, "status": "restored" })));
    };
    let was_restoring = cluster.restoration().is_restoring();
    let silent = replication.recovery.force_cluster().await;
    Ok(Json(json!({
        "wasRestoring": was_restoring,
        "status": cluster.restoration().status().as_str(),
        "silentNodes": silent,
    })))
}
