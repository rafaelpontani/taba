//! A API do cluster, na porta do cluster (doc 06c, "Membros e canal"): `GET /cluster/nodes`,
//! `POST /cluster/nodes/join`, `register`, `unregister`, `commit` e `rollback`. Cada rota só
//! traduz o HTTP para uma `Request` do protocolo e a devolve à mesma `ClusterService` que a rede
//! em memória dos testes chama, então as duas pontas se comportam igual.
//!
//! Quem chama é reconhecido pelo IP de origem da conexão: todos os nós usam o mesmo certificado
//! do cluster (doc 06, seção 8.0.1), e a tabela de membros guarda os IPs de cada um.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::common::ApiError;
use super::{AppState, json as body};
use crate::cluster::protocol::{Request, Response};
use crate::cluster::service::ClusterService;
use crate::core::error::TabaError;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cluster/nodes", get(list_nodes))
        .route("/cluster/nodes/join", post(join))
        .route("/cluster/nodes/register", post(register))
        .route("/cluster/nodes/unregister", post(unregister))
        .route("/cluster/nodes/commit", post(commit))
        .route("/cluster/nodes/rollback", post(rollback))
}

fn service(state: &AppState) -> Result<&Arc<ClusterService>, ApiError> {
    // Só há serviço de cluster em `mode=production`, e só nele a porta do cluster é aberta.
    state
        .cluster
        .as_ref()
        .ok_or_else(|| ApiError(TabaError::route_not_found()))
}

async fn dispatch(
    state: &AppState,
    caller: SocketAddr,
    request: Request,
) -> Result<HttpResponse, ApiError> {
    let response = service(state)?
        .handle(&caller.ip().to_string(), request)
        .await?;
    Ok(match response {
        Response::Members(table) => Json(table).into_response(),
        Response::Ack(ack) => Json(ack).into_response(),
    })
}

async fn list_nodes(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
) -> Result<HttpResponse, ApiError> {
    dispatch(&state, caller, Request::ListNodes).await
}

async fn join(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<HttpResponse, ApiError> {
    let request = body::parse(&headers, &bytes)?;
    dispatch(&state, caller, Request::Join(request)).await
}

async fn register(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<HttpResponse, ApiError> {
    let request = body::parse(&headers, &bytes)?;
    dispatch(&state, caller, Request::Register(request)).await
}

async fn unregister(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<HttpResponse, ApiError> {
    let request = body::parse(&headers, &bytes)?;
    dispatch(&state, caller, Request::Unregister(request)).await
}

async fn commit(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<HttpResponse, ApiError> {
    let request = body::parse(&headers, &bytes)?;
    dispatch(&state, caller, Request::Commit(request)).await
}

async fn rollback(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<HttpResponse, ApiError> {
    let request = body::parse(&headers, &bytes)?;
    dispatch(&state, caller, Request::Rollback(request)).await
}
