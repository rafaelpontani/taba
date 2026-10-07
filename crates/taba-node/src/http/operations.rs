//! API de operação (doc 06b): saúde do nó. `/metrics`, `/health/cluster` e `/node/drain`
//! entram com a observabilidade e o drain.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::AppState;

/// Corpo de `/health/live` e `/health/ready`. Essas rotas nunca devolvem `ErrorResponse`
/// (doc 06b), para o orquestrador poder ler o motivo sem tratar erros.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub health: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
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
