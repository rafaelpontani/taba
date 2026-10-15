use axum::body::to_bytes;
use axum::http::Request as HttpRequest;
use axum::routing::get;
use tower::ServiceExt;

use super::*;
use crate::cluster::drain::DrainController;
use crate::cluster::state::NodeShared;
use crate::config::DbSynchronous;
use crate::config::{Config, MapEnv, Mode};
use crate::core::clock::SystemClock;
use crate::core::error::ErrorCode;
use crate::engine::messages::{MessageRules, Messages};
use crate::engine::tokens::{TokenRules, Tokens};
use crate::engine::topics::{TopicRules, Topics};
use crate::http::auth::Auth;
use crate::http::client_ip::ClientIpRules;
use crate::http::ratelimit::RateLimiter;
use crate::storage::db::NodeDb;
use crate::storage::messages::MessageStore;
use crate::storage::state::NodeStateStore;
use crate::storage::tokens::MemoryTokenStore;
use crate::storage::topics::MemoryTopicStore;

async fn boom() -> &'static str {
    panic!("test failure")
}

fn app() -> Router {
    let clock = Arc::new(SystemClock);
    let config = Config::parse(None, &MapEnv::new([("TABA_MODE", "development")])).unwrap();
    let topics = Topics::new(
        TopicRules::from_config(&config),
        Arc::new(MemoryTopicStore::default()),
        clock.clone(),
        Vec::new(),
    );
    let topics = Arc::new(topics);
    let dir = tempfile::tempdir().unwrap();
    let messages = Messages::new(
        topics.clone(),
        clock.clone(),
        MessageRules::from_config(&config),
        Arc::new(MessageStore::new(dir.path(), DbSynchronous::Normal)),
        false,
    );
    let node = Arc::new(NodeShared::new(
        "n".into(),
        Mode::Development,
        std::time::SystemTime::now(),
    ));
    let db = Arc::new(NodeDb::open(&dir.path().join("db"), DbSynchronous::Normal).unwrap());
    let drain = DrainController::new(
        node.clone(),
        messages.clone(),
        NodeStateStore::new(db),
        clock.clone(),
        std::time::Duration::from_secs(25),
    );
    let state = AppState {
        clock: clock.clone(),
        backup: None,
        topics,
        messages,
        node,
        drain,
        cluster: None,
        replicated: None,
        allowed_ips: Arc::new(Vec::new()),
        client_ip: Arc::new(ClientIpRules::from_config(&config.client_ip)),
        auth: Arc::new(Auth::new(
            &config.auth,
            Arc::new(Tokens::new(
                TokenRules::from_config(&config),
                Arc::new(MemoryTokenStore::default()),
                clock.clone(),
                Vec::new(),
            )),
        )),
        rate_limiter: Arc::new(RateLimiter::from_config(&config.rate_limit, clock.clone())),
        telemetry: Arc::new(crate::telemetry::Telemetry::from_config(&config, "n")),
        stats_rules: Arc::new(crate::engine::messages::stats::StatsRules::from_config(
            &config,
        )),
        tls: None,
    };
    let routes = Router::new()
        .route("/boom", get(boom))
        .route(
            "/quota",
            get(|| async {
                ApiError(
                    TabaError::new(ErrorCode::RateLimitExceeded, "devagar").with_retry_after(7),
                )
            }),
        )
        .route(
            "/auth",
            get(|| async { ApiError(TabaError::new(ErrorCode::AuthUnauthorized, "no token")) }),
        )
        .route("/ok", get(|| async { "ok" }));
    finish(routes, state)
}

async fn call(app: &Router, request_id: Option<&str>, path: &str) -> (Response, serde_json::Value) {
    let mut builder = HttpRequest::get(path);
    if let Some(id) = request_id {
        builder = builder.header("x-request-id", id);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (Response::from_parts(parts, Body::empty()), json)
}

fn request_id_header(response: &Response) -> String {
    response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned()
}

/// UC: UC-CNV-004
#[tokio::test]
async fn client_request_id_is_echoed_in_header_and_error_body() {
    let (response, body) = call(&app(), Some("order-42:a.b_c"), "/does-not-exist").await;
    assert_eq!(request_id_header(&response), "order-42:a.b_c");
    assert_eq!(body["requestId"], "order-42:a.b_c");
}

/// UC: UC-CNV-004
#[tokio::test]
async fn missing_or_invalid_request_id_is_replaced_by_a_guid_v7() {
    let too_long = "a".repeat(129);
    for sent in [
        None,
        Some(""),
        Some("with space"),
        Some(too_long.as_str()),
        Some("a/b"),
    ] {
        let (response, _) = call(&app(), sent, "/ok").await;
        let id = request_id_header(&response);
        assert!(
            id::is_guid_v7(&id),
            "{sent:?} should become a GUID v7, got {id}"
        );
    }
    let (response, _) = call(&app(), Some(&"a".repeat(128)), "/ok").await;
    assert_eq!(
        request_id_header(&response).len(),
        128,
        "128 characters is still valid"
    );
}

/// UC: UC-CNV-004, UC-CNV-006
#[tokio::test]
async fn unknown_route_is_a_404_error_response_with_matching_request_id() {
    let (response, body) = call(&app(), None, "/does-not-exist").await;
    assert_eq!(response.status(), 404);
    assert_eq!(body["code"], "route.not-found");
    assert!(body["message"].is_string());
    assert_eq!(body["requestId"], request_id_header(&response));
    assert_eq!(response.headers()["content-type"], "application/json");
    assert!(
        body.get("status").is_none(),
        "the status goes only in the status line"
    );
}

/// UC: UC-CNV-012
#[tokio::test]
async fn a_panic_in_a_handler_becomes_500_internal_error() {
    let (response, body) = call(&app(), None, "/boom").await;
    assert_eq!(response.status(), 500);
    assert_eq!(body["code"], "internal.error");
    assert_eq!(body["requestId"], request_id_header(&response));
}

/// UC: UC-CNV-007
#[tokio::test]
async fn retry_after_goes_only_in_the_header_and_401_asks_for_a_bearer_token() {
    let (response, body) = call(&app(), None, "/quota").await;
    assert_eq!(response.status(), 429);
    assert_eq!(response.headers()["retry-after"], "7");
    assert!(body.get("retryAfter").is_none() && body.get("retry_after").is_none());

    let (response, _) = call(&app(), None, "/auth").await;
    assert_eq!(response.status(), 401);
    assert_eq!(response.headers()["www-authenticate"], "Bearer");
}

#[tokio::test]
async fn successful_responses_also_carry_the_request_id() {
    let (response, _) = call(&app(), Some("abc"), "/ok").await;
    assert_eq!(response.status(), 200);
    assert_eq!(request_id_header(&response), "abc");
}
