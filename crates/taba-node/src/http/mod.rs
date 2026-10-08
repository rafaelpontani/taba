//! As três APIs HTTP, cada uma num listener próprio (doc 06, seção 8.0): cliente
//! (`api.port`), operação (`operations.port`, ou a porta da API) e cluster
//! (`cluster.port`). Autenticação, rate limit e mTLS entram aqui depois.

mod common;
mod json;
pub mod messages;
pub mod operations;
pub mod topics;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;

use crate::cluster::drain::DrainController;
use crate::cluster::state::{DrainState, NodeShared};
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::Messages;
use crate::engine::topics::Topics;

/// `Retry-After` das chamadas de cliente recusadas durante o drain, em segundos.
const DRAIN_RETRY_AFTER_SECS: u64 = 5;

/// O que os handlers compartilham.
#[derive(Clone)]
pub struct AppState {
    pub clock: Arc<dyn Clock>,
    pub node: Arc<NodeShared>,
    pub topics: Arc<Topics>,
    pub messages: Arc<Messages>,
    pub drain: Arc<DrainController>,
    /// `operations.allowed-ips`; vazia, sem restrição.
    pub allowed_ips: Arc<Vec<IpNet>>,
}

/// Em drain o nó não atende chamada de cliente (doc 04, seção 6.10). Sem outro nó para
/// repassar, responde `503` com `Retry-After`.
async fn drain_guard(
    State(node): State<Arc<NodeShared>>,
    request: Request,
    next: Next,
) -> Response {
    if node.drain_state() == DrainState::Active {
        return next.run(request).await;
    }
    common::ApiError(
        TabaError::new(
            ErrorCode::ClusterNodeDraining,
            "O nó está em drain e não há outro nó para repassar a chamada.",
        )
        .with_retry_after(DRAIN_RETRY_AFTER_SECS),
    )
    .into_response()
}

/// API de cliente. Sem `operations.port`, serve também as rotas de operação.
pub fn client_router(state: AppState, with_operations: bool) -> Router {
    let routes = Router::new()
        .merge(topics::routes())
        .merge(messages::routes())
        .route_layer(middleware::from_fn_with_state(
            state.node.clone(),
            drain_guard,
        ));
    let routes = if with_operations {
        routes.merge(operations::routes(state.allowed_ips.clone()))
    } else {
        routes
    };
    common::finish(routes, state)
}

/// API de operação, na porta própria.
pub fn operations_router(state: AppState) -> Router {
    common::finish(operations::routes(state.allowed_ips.clone()), state)
}

/// API do cluster: só `/cluster/*`. Ainda sem rotas; tudo responde `404`.
pub fn cluster_router(state: AppState) -> Router {
    common::finish(Router::new(), state)
}
