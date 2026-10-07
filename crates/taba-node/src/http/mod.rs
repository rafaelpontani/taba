//! As três APIs HTTP, cada uma num listener próprio (doc 06, seção 8.0): cliente
//! (`api.port`), operação (`operations.port`, ou a porta da API) e cluster
//! (`cluster.port`). Autenticação, rate limit e mTLS entram aqui depois.

mod common;
mod json;
pub mod operations;
pub mod topics;

use std::sync::Arc;

use axum::Router;

use crate::cluster::state::NodeShared;
use crate::core::clock::Clock;
use crate::engine::topics::Topics;

/// O que os handlers compartilham.
#[derive(Clone)]
pub struct AppState {
    pub clock: Arc<dyn Clock>,
    pub node: Arc<NodeShared>,
    pub topics: Arc<Topics>,
}

/// API de cliente. Sem `operations.port`, serve também as rotas de operação.
pub fn client_router(state: AppState, with_operations: bool) -> Router {
    let routes = Router::new().merge(topics::routes());
    let routes = if with_operations {
        routes.merge(operations::routes())
    } else {
        routes
    };
    common::finish(routes, state)
}

/// API de operação, na porta própria.
pub fn operations_router(state: AppState) -> Router {
    common::finish(operations::routes(), state)
}

/// API do cluster: só `/cluster/*`. Ainda sem rotas; tudo responde `404`.
pub fn cluster_router(state: AppState) -> Router {
    common::finish(Router::new(), state)
}
