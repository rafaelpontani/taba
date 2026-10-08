//! A API do cluster, na porta do cluster (doc 06c). Uma só rota, `/cluster/{*caminho}`, lê a
//! chamada com o codec do protocolo (`protocol::http`), entrega à `ClusterService` e devolve a
//! resposta. A rede em memória dos testes chama a mesma `ClusterService`, então as duas pontas se
//! comportam igual.
//!
//! Quem chama é reconhecido pelo IP de origem da conexão: todos os nós usam o mesmo certificado do
//! cluster (doc 06, seção 8.0.1), e a tabela de membros guarda os IPs de cada um.

use std::net::SocketAddr;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, Request as HttpRequest, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::Response as HttpResponse;
use axum::routing::{any, get};

use tracing::Instrument;

use super::AppState;
use super::common::ApiError;
use crate::cluster::protocol::Request;
use crate::cluster::transport::ChannelOpen;
use crate::cluster::transport::http::bridge;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::trace;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cluster/channel", get(channel))
        .route("/cluster/{*caminho}", any(dispatch))
}

/// `from` e `to` do canal: os `nodeId` de quem abre e de quem deve atender.
#[derive(serde::Deserialize)]
struct ChannelQuery {
    from: String,
    #[serde(default)]
    to: String,
}

/// `GET /cluster/channel`: o canal permanente entre dois nós, um WebSocket. Os erros (não é membro,
/// outro processo no endereço) saem antes do upgrade, como status HTTP.
async fn channel(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    Query(query): Query<ChannelQuery>,
    upgrade: WebSocketUpgrade,
) -> Result<HttpResponse, ApiError> {
    let service = state
        .cluster
        .clone()
        .ok_or_else(|| ApiError(TabaError::route_not_found()))?;
    let open = ChannelOpen {
        from: query.from,
        to: query.to,
    };
    service.admit_channel(&caller.ip().to_string(), &open)?;
    Ok(upgrade.on_upgrade(move |socket| async move {
        let link = bridge(
            socket,
            |text| Message::Text(text.into()),
            |message| match message {
                Message::Text(text) => Some(text.as_str().to_owned()),
                _ => None,
            },
        );
        service.accept_channel(open, link);
    }))
}

async fn dispatch(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    // Só há serviço de cluster em `mode=production`, e só nele a porta do cluster é aberta.
    let service = state
        .cluster
        .as_ref()
        .ok_or_else(|| ApiError(TabaError::route_not_found()))?;
    let (parts, body) = request.into_parts();
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map_or_else(|| parts.uri.path().to_owned(), |pq| pq.as_str().to_owned());
    let body = to_bytes(body, usize::MAX).await.map_err(|_| {
        ApiError(TabaError::new(
            ErrorCode::ValidationFailed,
            "Não foi possível ler o corpo da requisição.",
        ))
    })?;
    let request = Request::from_http(parts.method.as_str(), &path_and_query, &headers, &body)?;
    let caller_ip = caller.ip().to_string();
    let handled = service.handle(&caller_ip, request);
    // Uma chamada que veio de uma operação com trace continua o mesmo trace neste nó.
    let reply =
        match trace::inbound_span(&format!("{} {}", parts.method, parts.uri.path()), &headers) {
            Some(span) => handled.instrument(span).await?,
            None => handled.await?,
        }
        .into_http();

    let mut response = HttpResponse::new(Body::from(reply.body));
    *response.status_mut() = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::OK);
    for (name, value) in reply.headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            response.headers_mut().append(name, value);
        }
    }
    Ok(response)
}
