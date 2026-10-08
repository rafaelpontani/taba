//! O repasse das chamadas de cliente de um nó em drain (doc 04, seção 6.9): a ponta que **envia**
//! (`forward_call`, usada pelo `drain_guard`) e a que **executa** o repasse recebido
//! (`executor`, que roda a chamada contra a API de cliente deste nó).
//!
//! A chamada repassada leva o marcador [`Forwarded`] e o IP do cliente que quem repassou já
//! resolveu: o destino não conta o rate limit de novo (quem repassou contou, por IP) e usa esse IP.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, Request};
use axum::http::{HeaderName, HeaderValue, Request as HttpRequest, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt;

use super::AppState;
use super::common::ApiError;
use crate::cluster::protocol::{ForwardRequest, ForwardResponse};
use crate::cluster::replication::ForwardExecutor;
use crate::core::error::{ErrorCode, TabaError};

/// O marcador de uma chamada que chegou por repasse: a porta do cluster é quem o põe, nunca o
/// cliente.
#[derive(Debug, Clone, Copy)]
pub struct Forwarded {
    pub client_ip: IpAddr,
}

/// Cabeçalhos que não atravessam um repasse: dizem respeito a uma conexão, não à chamada.
fn hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "connection" | "transfer-encoding" | "keep-alive" | "upgrade"
    )
}

/// O maior corpo de cliente que um repasse carrega: bem acima de `message.hard-limit`.
const MAX_BODY: usize = 256 * 1024 * 1024;

fn bad_forward(what: &str) -> TabaError {
    TabaError::new(ErrorCode::ValidationFailed, what.to_owned())
}

/// Executa a chamada repassada contra a API de cliente deste nó.
pub fn executor(router: Router) -> ForwardExecutor {
    Arc::new(move |forward| {
        let router = router.clone();
        Box::pin(async move { run(router, forward).await })
    })
}

async fn run(router: Router, forward: ForwardRequest) -> Result<ForwardResponse, TabaError> {
    let mut builder = HttpRequest::builder()
        .method(forward.method.as_str())
        .uri(forward.path_and_query.as_str());
    for (name, value) in &forward.headers {
        if hop_by_hop(name) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(name, value);
        }
    }
    let mut request = builder
        .body(Body::from(forward.body))
        .map_err(|_| bad_forward("A chamada repassada é inválida."))?;
    let client_ip = forward
        .client_ip
        .and_then(|ip| ip.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    request.extensions_mut().insert(Forwarded { client_ip });
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(client_ip, 0)));
    let response = match router.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    let (parts, body) = response.into_parts();
    let body = to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| bad_forward("A resposta repassada é grande demais."))?;
    Ok(ForwardResponse {
        status: parts.status.as_u16(),
        headers: parts
            .headers
            .iter()
            .filter(|(name, _)| !hop_by_hop(name.as_str()))
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect(),
        body: body.to_vec(),
    })
}

/// Repassa a chamada de cliente ao nó mais livre e devolve a resposta dele como veio.
pub async fn forward_call(state: &AppState, request: Request) -> Response {
    let Some(forwarder) = state
        .cluster
        .as_ref()
        .and_then(|cluster| cluster.replication())
        .map(|replication| replication.forwarder.clone())
    else {
        return ApiError(no_cluster()).into_response();
    };
    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, MAX_BODY).await else {
        return ApiError(bad_forward("Não foi possível ler o corpo da requisição."))
            .into_response();
    };
    let client_ip = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(peer)| state.client_ip.resolve(*peer, &parts.headers).to_string());
    let forward = ForwardRequest {
        method: parts.method.as_str().to_owned(),
        path_and_query: parts
            .uri
            .path_and_query()
            .map_or_else(|| parts.uri.path().to_owned(), |pq| pq.as_str().to_owned()),
        headers: parts
            .headers
            .iter()
            .filter(|(name, _)| !hop_by_hop(name.as_str()))
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect(),
        body: body.to_vec(),
        client_ip,
    };
    match forwarder.forward(forward).await {
        Ok(reply) => {
            let mut response = Response::new(Body::from(reply.body));
            *response.status_mut() =
                StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_GATEWAY);
            for (name, value) in reply.headers {
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(&value),
                ) && name != header::CONTENT_LENGTH
                {
                    response.headers_mut().append(name, value);
                }
            }
            response
        }
        Err(error) => ApiError(error).into_response(),
    }
}

fn no_cluster() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterNodeDraining,
        "O nó está em drain e não há outro nó para repassar a chamada.",
    )
    .with_retry_after(5)
}
