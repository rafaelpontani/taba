use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::{Value, json};

use super::*;

async fn who(ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Json<Value> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned()
    };
    Json(json!({
        "peer": peer.to_string(),
        "authorization": header("authorization"),
        "forwardedFor": header("x-forwarded-for"),
        "contentType": header("content-type"),
    }))
}

async fn echo(body: String) -> String {
    body
}

async fn refuse() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [("retry-after", "5")],
        Json(json!({"code": "cluster.node-draining", "message": "draining", "requestId": "r"})),
    )
}

async fn gone() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"code": "topic.not-found", "message": "no such topic", "requestId": "r"})),
    )
}

fn backend() -> Backend {
    Backend::new(
        Router::new()
            .route("/who", get(who))
            .route("/echo", post(echo))
            .route("/refuse", get(refuse))
            .route("/gone", get(gone)),
    )
}

fn caller(token: Option<&str>) -> Caller {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", "203.0.113.9".parse().unwrap());
    headers.insert(
        "authorization",
        "AWS4-HMAC-SHA256 secret-stuff".parse().unwrap(),
    );
    headers.insert("host", "gateway".parse().unwrap());
    Caller::of(
        token.map(str::to_owned),
        "10.1.2.3:5555".parse().unwrap(),
        &headers,
    )
}

/// UC: UC-GTW-006
#[tokio::test]
async fn the_call_carries_the_token_the_peer_and_the_proxy_headers() {
    let reply = backend()
        .json(&caller(Some("tb_x.y")), Method::GET, "/who", None)
        .await
        .expect("call");
    assert!(reply.is_success());
    let seen = reply.json();
    assert_eq!(seen["authorization"], "Bearer tb_x.y");
    assert_eq!(seen["peer"], "10.1.2.3:5555");
    assert_eq!(seen["forwardedFor"], "203.0.113.9");
}

/// UC: UC-GTW-006
#[tokio::test]
async fn the_aws_signature_never_reaches_the_client_api() {
    let reply = backend()
        .json(&caller(None), Method::GET, "/who", None)
        .await
        .expect("call");
    assert_eq!(
        reply.json()["authorization"],
        "",
        "the SigV4 header is for the gateway, not for the API"
    );
}

/// UC: UC-GTW-006
#[tokio::test]
async fn a_body_goes_with_its_content_type() {
    let reply = backend()
        .call(
            &caller(None),
            Method::POST,
            "/echo",
            Some("text/plain; charset=utf-8"),
            "café".as_bytes().to_vec(),
        )
        .await
        .expect("call");
    assert_eq!(reply.body, "café".as_bytes());
}

/// UC: UC-GTW-006
#[tokio::test]
async fn an_error_of_the_api_becomes_an_aws_error_with_the_retry_after() {
    let refused = backend()
        .ok_json(&caller(None), Method::GET, "/refuse", None)
        .await
        .unwrap_err();
    assert_eq!(refused.json_code, "ServiceUnavailable");
    assert_eq!(refused.retry_after, Some(5));
    let missing = backend()
        .ok_json(&caller(None), Method::GET, "/gone", None)
        .await
        .unwrap_err();
    assert_eq!(missing.json_code, "QueueDoesNotExist");
}
