//! As três APIs HTTP, cada uma num listener próprio (doc 06, seção 8.0): cliente
//! (`api.port`), operação (`operations.port`, ou a porta da API) e cluster
//! (`cluster.port`). Autenticação, rate limit e mTLS entram aqui depois.

pub mod auth;
pub mod client_ip;
mod common;
mod json;
pub mod messages;
pub mod operations;
pub mod ratelimit;
pub mod tokens;
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
use auth::Auth;
use client_ip::ClientIpRules;
use ratelimit::RateLimiter;

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
    pub client_ip: Arc<ClientIpRules>,
    pub rate_limiter: Arc<RateLimiter>,
    pub auth: Arc<Auth>,
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

/// API de cliente. Sem `operations.port`, serve também as rotas de operação. As rotas `/tokens`
/// só existem com a autenticação ligada (senão respondem `404`).
///
/// As camadas, de fora para dentro: quem chama (`authenticate`), o rate limit, a recusa do
/// drain e a autorização (`authorize`). O rate limit vem depois de saber quem chama, para o
/// token virar a chave do contador, e antes de recusar, para o `401` também contar por IP.
pub fn client_router(state: AppState, with_operations: bool) -> Router {
    let mut routes = Router::new()
        .merge(topics::routes())
        .merge(messages::routes());
    if state.auth.enabled {
        routes = routes.merge(tokens::routes());
    }
    let routes = routes
        .route_layer(middleware::from_fn(auth::authorize))
        .route_layer(middleware::from_fn_with_state(
            state.node.clone(),
            drain_guard,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            ratelimit::rate_limit,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authenticate,
        ));
    let routes = if with_operations {
        routes.merge(operations::routes(&state))
    } else {
        routes
    };
    common::finish(routes, state)
}

/// API de operação, na porta própria.
pub fn operations_router(state: AppState) -> Router {
    common::finish(operations::routes(&state), state)
}

/// API do cluster: só `/cluster/*`. Ainda sem rotas; tudo responde `404`.
pub fn cluster_router(state: AppState) -> Router {
    common::finish(Router::new(), state)
}
