//! The cluster API, on the cluster port (doc 06c). A single route, `/cluster/{*path}`, reads the
//! call with the protocol codec (`protocol::http`), hands it to the `ClusterService` and returns the
//! response. The tests' in-memory network calls the same `ClusterService`, so both ends
//! behave the same.
//!
//! The caller is recognized by the connection's source IP: every node uses the same cluster
//! certificate (doc 06, section 8.0.1), and the member table keeps each one's IPs.

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
use crate::cluster::protocol::http::BATCH_PATH;
use crate::cluster::transport::ChannelOpen;
use crate::cluster::transport::http::bridge;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::trace;

/// What a batch can weigh beyond its items' bodies and the soft limit: the fields around each message.
const BATCH_ENVELOPE_SLACK: usize = 64 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cluster/channel", get(channel))
        .route("/cluster/{*path}", any(dispatch))
}

/// `from` and `to` of the channel: the `nodeId` of who opens and of who must serve.
#[derive(serde::Deserialize)]
struct ChannelQuery {
    from: String,
    #[serde(default)]
    to: String,
}

/// `GET /cluster/channel`: the permanent channel between two nodes, a WebSocket. The errors (not a member,
/// another process at the address) come out before the upgrade, as HTTP status.
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
    // There is only a cluster service in `mode=production`, and only then is the cluster port opened.
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
    // A batch is cut by the sender at `cluster.batch.max-bytes`, soft: it stops adding items when it is reached, so it
    // can pass it by one item, and an item is at most `message.max-bytes`. Beyond that (plus the envelope) it is refused
    // before it is read.
    let limit = if parts.uri.path() == BATCH_PATH {
        let rules = service.rules();
        rules
            .batch_max_bytes
            .saturating_add(rules.message_max_bytes)
            .saturating_add(BATCH_ENVELOPE_SLACK)
    } else {
        usize::MAX
    };
    let body = to_bytes(body, limit).await.map_err(|_| {
        if limit == usize::MAX {
            ApiError(TabaError::new(
                ErrorCode::ValidationFailed,
                "Could not read the request body.",
            ))
        } else {
            ApiError(TabaError::new(
                ErrorCode::MessageTooLarge,
                "The batch is larger than `cluster.batch.max-bytes` plus one message.",
            ))
        }
    })?;
    let request = Request::from_wire(parts.method.as_str(), &path_and_query, &headers, &body)?;
    let caller_ip = caller.ip().to_string();
    let handled = service.handle(&caller_ip, request);
    // A call that came from an operation with a trace continues the same trace on this node.
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
