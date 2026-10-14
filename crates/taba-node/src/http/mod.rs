//! As três APIs HTTP, cada uma num listener próprio (doc 06, seção 8.0): cliente
//! (`api.port`), operação (`operations.port`, ou a porta da API) e cluster
//! (`cluster.port`). O mTLS da porta do cluster entra na etapa 16.

pub mod auth;
pub mod backup;
pub mod client_ip;
pub mod cluster_api;
mod common;
pub mod forward;
mod json;
pub mod messages;
pub mod metrics;
pub mod observe;
pub mod operations;
pub mod ratelimit;
pub mod tokens;
mod topic_stats;
pub mod topics;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;

use crate::cluster::drain::DrainController;
use crate::cluster::replication::ClusterMessages;
use crate::cluster::service::ClusterService;
use crate::cluster::state::{DrainState, NodeShared, NodeState};
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::Messages;
use crate::engine::messages::stats::StatsRules;
use crate::engine::topics::Topics;
use crate::telemetry::Telemetry;
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
    /// O backup opcional em S3; `None` sem `backup.s3.bucket`.
    pub backup: Option<Arc<crate::cluster::backup::Backup>>,
    pub topics: Arc<Topics>,
    pub messages: Arc<Messages>,
    pub drain: Arc<DrainController>,
    /// O nó no cluster; só existe em `mode=production`.
    pub cluster: Option<Arc<ClusterService>>,
    /// As mensagens no cluster (escrita e entrega por quórum); `None` num nó só.
    pub replicated: Option<Arc<ClusterMessages>>,
    /// `operations.allowed-ips`; vazia, sem restrição.
    pub allowed_ips: Arc<Vec<IpNet>>,
    pub client_ip: Arc<ClientIpRules>,
    pub rate_limiter: Arc<RateLimiter>,
    pub auth: Arc<Auth>,
    pub telemetry: Arc<Telemetry>,
    /// Os limites das condições dos tópicos (`stats.topics.*`).
    pub stats_rules: Arc<StatsRules>,
    /// Os certificados do cluster; `None` sem pasta (a porta do cluster atende em HTTP puro).
    pub tls: Option<Arc<crate::cluster::tls::TlsStore>>,
}

/// Em drain o nó não atende chamada de cliente (doc 04, seção 6.10): num cluster repassa a chamada
/// ao nó mais livre, e a resposta volta como o destino a deu. Sem outro nó (ou numa chamada que já
/// chegou por repasse, que nunca é repassada de novo) responde `503` com `Retry-After`. O nó em
/// falha (`failed`) também não atende: repassa, ou responde `503` `node.failed` (D78.2).
async fn drain_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // Um nó que ainda sincroniza também não atende: repassa a chamada a outro (doc 04, seção 6.9).
    let syncing = state.cluster.is_some() && state.node.state() == NodeState::Syncing;
    let failed = state.node.state() == NodeState::Failed;
    if state.node.drain_state() == DrainState::Active && !syncing && !failed {
        return next.run(request).await;
    }
    if state.cluster.is_some() && request.extensions().get::<forward::Forwarded>().is_none() {
        return forward::forward_call(&state, request).await;
    }
    if failed {
        return common::ApiError(
            TabaError::new(
                ErrorCode::NodeFailed,
                "O nó está em falha e não atende chamadas de cliente.",
            )
            .with_retry_after(DRAIN_RETRY_AFTER_SECS),
        )
        .into_response();
    }
    if syncing {
        return common::ApiError(
            TabaError::new(
                ErrorCode::ClusterNodeSyncing,
                "O nó ainda está em sincronização.",
            )
            .with_retry_after(DRAIN_RETRY_AFTER_SECS),
        )
        .into_response();
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
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authorize,
        ))
        .route_layer(middleware::from_fn_with_state(state.clone(), drain_guard))
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

/// API do cluster: só `/cluster/*`; o resto responde `404`.
pub fn cluster_router(state: AppState) -> Router {
    common::finish(cluster_api::routes(), state)
}
