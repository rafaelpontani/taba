//! Token authentication, permissions, prefixes and the `/tokens` routes (doc 06a, section 8.1.3).

mod common;

use std::time::Duration;

use common::{TestNode, TestNodeBuilder, error_code};
use reqwest::{Method, Response};
use serde_json::{Value, json};

const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";
const OTHER_ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0002";

/// Production (authentication never turns on in development) with the root token.
fn builder() -> TestNodeBuilder {
    TestNode::builder()
        .production()
        .env("TABA_AUTH_ROOT_TOKEN", ROOT)
}

async fn start() -> TestNode {
    builder().start().await
}

async fn call(
    node: &TestNode,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut request = node.client.request(method, node.client_url(path));
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    request.send().await.unwrap()
}

async fn get(node: &TestNode, path: &str, token: Option<&str>) -> Response {
    call(node, Method::GET, path, token, None).await
}

async fn create_topic(node: &TestNode, token: &str, body: Value) -> Response {
    call(node, Method::POST, "/topics", Some(token), Some(body)).await
}

async fn queue(node: &TestNode, name: &str) {
    // The clock is manual: one second refills the administrative counter.
    node.clock.advance(Duration::from_secs(1));
    let response = create_topic(node, ROOT, json!({ "name": name, "type": "queue" })).await;
    assert_eq!(response.status(), 201, "{name}");
}

/// Creates a token with the root one and returns `(id, full token)`.
async fn token(node: &TestNode, permissions: &[&str], prefixes: &[&str]) -> (String, String) {
    node.clock.advance(Duration::from_secs(1));
    let response = call(
        node,
        Method::POST,
        "/tokens",
        Some(ROOT),
        Some(json!({ "name": "test", "permissions": permissions, "topicPrefixes": prefixes })),
    )
    .await;
    assert_eq!(response.status(), 201);
    let body: Value = response.json().await.unwrap();
    (
        body["id"].as_str().unwrap().to_owned(),
        body["token"].as_str().unwrap().to_owned(),
    )
}

async fn publish(node: &TestNode, topic: &str, token: &str) -> Response {
    node.client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body("1")
        .send()
        .await
        .unwrap()
}

/// UC: UC-AUT-001, UC-AUT-007, UC-AUT-025
#[tokio::test]
async fn without_a_root_token_or_in_development_nothing_is_required_and_tokens_are_404() {
    let node = TestNode::builder().production().start().await;
    assert_eq!(get(&node, "/topics", None).await.status(), 200);
    assert_eq!(
        error_code(get(&node, "/tokens", None).await, 404).await,
        "route.not-found"
    );
    node.shutdown().await;

    // In development authentication is never required, even with the root token defined.
    let node = TestNode::builder()
        .env("TABA_AUTH_ROOT_TOKEN", ROOT)
        .start()
        .await;
    assert_eq!(get(&node, "/topics", None).await.status(), 200);
    assert_eq!(
        error_code(get(&node, "/tokens", Some(ROOT)).await, 404).await,
        "route.not-found"
    );
    node.shutdown().await;
}

/// UC: UC-AUT-003
#[tokio::test]
async fn auth_disabled_overrides_the_root_token() {
    let node = builder().env("TABA_AUTH_ENABLED", "false").start().await;
    assert_eq!(get(&node, "/topics", None).await.status(), 200);
    assert_eq!(
        error_code(get(&node, "/tokens", None).await, 404).await,
        "route.not-found"
    );
    node.shutdown().await;
}

/// UC: UC-AUT-002, UC-AUT-008
#[tokio::test]
async fn with_a_root_token_every_client_route_asks_for_a_valid_one_and_the_401_never_says_why() {
    let node = start().await;
    let (id, secret) = token(&node, &["consume"], &[]).await;
    call(
        &node,
        Method::DELETE,
        &format!("/tokens/{id}"),
        Some(ROOT),
        None,
    )
    .await;

    let mut bodies = Vec::new();
    for presented in [
        None,
        Some("lixo"),
        Some(OTHER_ROOT),
        Some("tb_00000000-0000-7000-8000-000000000000.abc"),
        Some(secret.as_str()), // revogado
    ] {
        let response = get(&node, "/topics", presented).await;
        assert_eq!(response.status(), 401, "{presented:?}");
        assert_eq!(response.headers()["www-authenticate"], "Bearer");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["code"], "auth.unauthorized");
        bodies.push(body["message"].clone());
    }
    assert!(
        bodies.windows(2).all(|w| w[0] == w[1]),
        "the message is the same in the five cases"
    );

    // Wrong scheme and header without a value.
    let basic = node
        .client
        .get(node.client_url("/topics"))
        .header("authorization", format!("Basic {ROOT}"))
        .send()
        .await
        .unwrap();
    assert_eq!(basic.status(), 401);
    assert_eq!(get(&node, "/topics", Some(ROOT)).await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-AUT-035
#[tokio::test]
async fn the_operations_routes_do_not_use_the_access_token() {
    let node = start().await;
    assert_eq!(node.client_get("/health/live").await.status(), 200);
    assert_eq!(
        get(&node, "/health/cluster?scope=node", None)
            .await
            .status(),
        200
    );
    assert_eq!(get(&node, "/node/drain", None).await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-AUT-022, UC-AUT-026, UC-AUT-027, UC-AUT-021, UC-AUT-017
#[tokio::test]
async fn creating_a_token_returns_the_secret_once_and_reading_never_does() {
    let node = start().await;
    let response = call(
        &node,
        Method::POST,
        "/tokens",
        Some(ROOT),
        Some(json!({
            "name": "servico-orders",
            "permissions": ["publish", "create"],
            "topicPrefixes": ["orders"],
            "expiresAt": "2026-12-01T00:00:00.000Z"
        })),
    )
    .await;
    assert_eq!(response.status(), 201);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let created: Value = response.json().await.unwrap();
    let id = created["id"].as_str().unwrap();
    assert_eq!(location, format!("/tokens/{id}"));
    assert_eq!(
        created["token"]
            .as_str()
            .unwrap()
            .split('.')
            .next()
            .unwrap(),
        format!("tb_{id}")
    );
    assert_eq!(created["name"], "servico-orders");
    assert_eq!(created["permissions"], json!(["publish", "create"]));
    assert_eq!(created["topicPrefixes"], json!(["orders"]));
    assert_eq!(created["createdAt"], "2026-10-06T12:00:00.000Z");
    assert_eq!(created["expiresAt"], "2026-12-01T00:00:00.000Z");

    let secret = created["token"].as_str().unwrap();
    for path in [format!("/tokens/{id}"), "/tokens".to_owned()] {
        let text = get(&node, &path, Some(ROOT)).await.text().await.unwrap();
        assert!(!text.contains(secret.split('.').nth(1).unwrap()), "{path}");
        assert!(
            !text.contains("secret") && !text.contains("salt") && !text.contains("hash"),
            "{path}: {text}"
        );
    }
    let one: Value = get(&node, &format!("/tokens/{id}"), Some(ROOT))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(one["id"], id);
    assert!(one.get("token").is_none());

    // No file per token: they live in `_node.db`.
    assert!(!node.storage_dir().join("_tokens").exists());
    node.shutdown().await;
}

/// UC: UC-AUT-023
#[tokio::test]
async fn an_invalid_token_request_is_400() {
    let node = start().await;
    for (body, code) in [
        (json!({ "permissions": ["publish"] }), "validation.required"),
        (json!({ "name": "x" }), "validation.required"),
        (
            json!({ "name": "x", "permissions": [] }),
            "validation.failed",
        ),
        (
            json!({ "name": "x", "permissions": ["voar"] }),
            "validation.failed",
        ),
        (
            json!({ "name": "with space", "permissions": ["publish"] }),
            "validation.invalid-name",
        ),
        (
            json!({ "name": "x", "permissions": ["publish"], "topicPrefixes": ["with space"] }),
            "validation.failed",
        ),
        (
            json!({ "name": "x", "permissions": ["publish"], "expiresAt": "2020-01-01T00:00:00.000Z" }),
            "validation.out-of-range",
        ),
        (
            json!({ "name": "x", "permissions": ["publish"], "expiresAt": "tomorrow" }),
            "validation.failed",
        ),
        (
            json!({ "name": "x", "permissions": ["publish"], "extra": 1 }),
            "validation.unknown-field",
        ),
    ] {
        let response = call(
            &node,
            Method::POST,
            "/tokens",
            Some(ROOT),
            Some(body.clone()),
        )
        .await;
        assert_eq!(error_code(response, 400).await, code, "{body}");
    }
    node.shutdown().await;
}

/// UC: UC-AUT-024, UC-AUT-017
#[tokio::test]
async fn only_the_root_token_manages_tokens() {
    let node = start().await;
    let (_, admin) = token(&node, &["admin", "publish", "consume"], &[]).await;
    for (method, path) in [
        (Method::GET, "/tokens"),
        (Method::POST, "/tokens"),
        (Method::GET, "/tokens/x"),
        (Method::DELETE, "/tokens/x"),
    ] {
        let body =
            (method == Method::POST).then(|| json!({ "name": "x", "permissions": ["admin"] }));
        let response = call(&node, method.clone(), path, Some(&admin), body.clone()).await;
        assert_eq!(
            error_code(response, 403).await,
            "auth.forbidden",
            "{method} {path}"
        );
        let response = call(&node, method.clone(), path, None, body).await;
        assert_eq!(
            error_code(response, 401).await,
            "auth.unauthorized",
            "{method} {path}"
        );
    }
    node.shutdown().await;
}

/// UC: UC-AUT-009, UC-AUT-010, UC-AUT-011
#[tokio::test]
async fn permissions_gate_each_route() {
    let node = start().await;
    queue(&node, "orders").await;
    let (_, publisher) = token(&node, &["publish"], &[]).await;
    let (_, consumer) = token(&node, &["consume"], &[]).await;
    let (_, creator) = token(&node, &["create"], &[]).await;
    let (_, admin) = token(&node, &["admin"], &[]).await;
    let forbidden = |response: Response| error_code(response, 403);

    // publish: only publish.
    assert_eq!(publish(&node, "orders", &publisher).await.status(), 201);
    assert_eq!(
        forbidden(get(&node, "/topics/orders/messages", Some(&publisher)).await).await,
        "auth.forbidden"
    );
    assert_eq!(
        forbidden(create_topic(&node, &publisher, json!({ "name": "a", "type": "queue" })).await)
            .await,
        "auth.forbidden"
    );

    // consume: consume, ack and change the invisibility, but not publish.
    let messages: Value = get(&node, "/topics/orders/messages", Some(&consumer))
        .await
        .json()
        .await
        .unwrap();
    let message = &messages["messages"][0];
    let ack = format!(
        "/topics/orders/messages/{}?receiptToken={}",
        message["id"].as_str().unwrap(),
        message["receiptToken"].as_str().unwrap()
    );
    assert_eq!(
        call(&node, Method::DELETE, &ack, Some(&consumer), None)
            .await
            .status(),
        204
    );
    assert_eq!(
        forbidden(publish(&node, "orders", &consumer).await).await,
        "auth.forbidden"
    );

    // create: only creates; changing and removing is admin.
    let created = create_topic(&node, &creator, json!({ "name": "new", "type": "queue" })).await;
    assert_eq!(created.status(), 201);
    let patch = call(
        &node,
        Method::PATCH,
        "/topics/new",
        Some(&creator),
        Some(json!({ "ttl": 120 })),
    )
    .await;
    assert_eq!(forbidden(patch).await, "auth.forbidden");
    assert_eq!(
        forbidden(call(&node, Method::DELETE, "/topics/new", Some(&creator), None).await).await,
        "auth.forbidden"
    );

    // admin: inclui create.
    let created = create_topic(
        &node,
        &admin,
        json!({ "name": "do-admin", "type": "queue" }),
    )
    .await;
    assert_eq!(created.status(), 201);
    let patch = call(
        &node,
        Method::PATCH,
        "/topics/new",
        Some(&admin),
        Some(json!({ "ttl": 120 })),
    )
    .await;
    assert_eq!(patch.status(), 200);
    assert_eq!(
        call(&node, Method::DELETE, "/topics/new", Some(&admin), None)
            .await
            .status(),
        204
    );

    // Every valid token queries the topics.
    for t in [&publisher, &consumer, &creator, &admin] {
        assert_eq!(get(&node, "/topics", Some(t)).await.status(), 200);
        assert_eq!(get(&node, "/topics/orders", Some(t)).await.status(), 200);
    }
    node.shutdown().await;
}

/// UC: UC-AUT-012, UC-AUT-013, UC-AUT-015
#[tokio::test]
async fn prefixes_confine_a_token_to_its_topics_including_the_dead_letter() {
    let node = start().await;
    queue(&node, "orders").await;
    queue(&node, "orders-eu").await;
    queue(&node, "estoque").await;
    let (_, scoped) = token(&node, &["publish", "consume"], &["orders"]).await;

    assert_eq!(publish(&node, "orders", &scoped).await.status(), 201);
    assert_eq!(publish(&node, "orders-eu", &scoped).await.status(), 201);
    assert_eq!(
        error_code(publish(&node, "estoque", &scoped).await, 403).await,
        "auth.out-of-scope"
    );
    assert_eq!(
        error_code(get(&node, "/topics/estoque", Some(&scoped)).await, 403).await,
        "auth.out-of-scope"
    );
    // Outside the prefix it is `403` even if the topic does not exist: it does not reveal what exists.
    assert_eq!(
        error_code(
            get(&node, "/topics/does-not-exist", Some(&scoped)).await,
            403
        )
        .await,
        "auth.out-of-scope"
    );
    // Inside the prefix and nonexistent it is `404`.
    assert_eq!(
        error_code(
            get(&node, "/topics/orders-does-not-exist", Some(&scoped)).await,
            404
        )
        .await,
        "topic.not-found"
    );
    // The dead-letter starts with the queue's name.
    assert_eq!(
        get(&node, "/topics/orders-dead-letter/messages", Some(&scoped))
            .await
            .status(),
        200
    );

    // `GET /topics` only shows what is within reach (the queues and their dead-letters).
    let list: Value = get(&node, "/topics", Some(&scoped))
        .await
        .json()
        .await
        .unwrap();
    let mut names: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "orders",
            "orders-dead-letter",
            "orders-eu",
            "orders-eu-dead-letter"
        ]
    );
    let all: Value = get(&node, "/topics", Some(ROOT))
        .await
        .json()
        .await
        .unwrap();
    assert!(all["items"].as_array().unwrap().len() > names.len());
    node.shutdown().await;
}

/// UC: UC-AUT-014
#[tokio::test]
async fn a_token_only_creates_inside_its_prefixes_including_the_distribution_prefixes() {
    let node = start().await;
    queue(&node, "estoque").await;
    let (_, creator) = token(&node, &["create", "admin"], &["orders"]).await;

    let make = |body: Value| create_topic(&node, &creator, body);
    assert_eq!(
        make(json!({ "name": "orders-br", "type": "queue" }))
            .await
            .status(),
        201
    );
    assert_eq!(
        error_code(
            make(json!({ "name": "estoque-x", "type": "queue" })).await,
            403
        )
        .await,
        "auth.out-of-scope"
    );
    let ok =
        make(json!({ "name": "orders-dist", "type": "distributor", "prefixes": ["orders-br"] }))
            .await;
    assert_eq!(ok.status(), 201);
    for bad in [vec!["estoque"], vec!["orders-br", "estoque"], vec!["ped"]] {
        let response =
            make(json!({ "name": "orders-outside", "type": "distributor", "prefixes": bad })).await;
        assert_eq!(
            error_code(response, 403).await,
            "auth.out-of-scope",
            "{bad:?}"
        );
    }
    // The `PATCH` of `prefixes` follows the same rule.
    let patch = |prefixes: Value| {
        call(
            &node,
            Method::PATCH,
            "/topics/orders-dist",
            Some(&creator),
            Some(json!({ "prefixes": prefixes })),
        )
    };
    assert_eq!(patch(json!(["orders-us"])).await.status(), 200);
    assert_eq!(
        error_code(patch(json!(["estoque"])).await, 403).await,
        "auth.out-of-scope"
    );
    node.shutdown().await;
}

/// UC: UC-AUT-016
#[tokio::test]
async fn publishing_to_a_distributor_checks_only_the_distributor() {
    let node = start().await;
    queue(&node, "queue-a").await;
    let made = create_topic(
        &node,
        ROOT,
        json!({ "name": "dist", "type": "distributor", "prefixes": ["queue-"] }),
    )
    .await;
    assert_eq!(made.status(), 201);
    let (_, publisher) = token(&node, &["publish"], &["dist"]).await;
    let response = publish(&node, "dist", &publisher).await;
    assert_eq!(
        response.status(),
        200,
        "the copy on queue-a is not checked again"
    );
    node.shutdown().await;
}

/// UC: UC-AUT-028, UC-AUT-029, UC-AUT-027
#[tokio::test]
async fn patching_and_revoking_tokens() {
    let node = start().await;
    let (id, secret) = token(&node, &["consume"], &[]).await;
    let path = format!("/tokens/{id}");

    let patched = call(
        &node,
        Method::PATCH,
        &path,
        Some(ROOT),
        Some(json!({ "name": "new-name", "expiresAt": "2026-12-01T00:00:00.000Z" })),
    )
    .await;
    assert_eq!(patched.status(), 200);
    let body: Value = patched.json().await.unwrap();
    assert_eq!(
        (body["name"].as_str(), body["expiresAt"].as_str()),
        (Some("new-name"), Some("2026-12-01T00:00:00.000Z"))
    );
    let cleared: Value = call(
        &node,
        Method::PATCH,
        &path,
        Some(ROOT),
        Some(json!({ "expiresAt": null })),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(cleared["expiresAt"], Value::Null);
    assert_eq!(cleared["name"], "new-name");
    assert_eq!(
        get(&node, "/topics", Some(&secret)).await.status(),
        200,
        "the secret still works"
    );

    for field in ["permissions", "topicPrefixes"] {
        let response = call(
            &node,
            Method::PATCH,
            &path,
            Some(ROOT),
            Some(json!({ field: ["admin"] })),
        )
        .await;
        assert_eq!(
            error_code(response, 400).await,
            "validation.immutable-field",
            "{field}"
        );
    }
    let unknown = call(
        &node,
        Method::PATCH,
        &path,
        Some(ROOT),
        Some(json!({ "foo": 1 })),
    )
    .await;
    assert_eq!(error_code(unknown, 400).await, "validation.unknown-field");

    assert_eq!(
        call(&node, Method::DELETE, &path, Some(ROOT), None)
            .await
            .status(),
        204
    );
    assert_eq!(
        error_code(get(&node, "/topics", Some(&secret)).await, 401).await,
        "auth.unauthorized"
    );
    assert_eq!(
        error_code(get(&node, &path, Some(ROOT)).await, 404).await,
        "token.not-found"
    );
    assert_eq!(
        error_code(
            call(&node, Method::DELETE, &path, Some(ROOT), None).await,
            404
        )
        .await,
        "token.not-found"
    );
    assert_eq!(
        error_code(
            call(&node, Method::PATCH, &path, Some(ROOT), Some(json!({}))).await,
            404
        )
        .await,
        "token.not-found"
    );
    node.shutdown().await;
}

/// UC: UC-AUT-026
#[tokio::test]
async fn listing_tokens_is_paginated_filtered_and_refuses_sorting_by_name() {
    let node = start().await;
    let mut ids = Vec::new();
    for name in ["alfa", "beta", "alfa-2"] {
        let created: Value = call(
            &node,
            Method::POST,
            "/tokens",
            Some(ROOT),
            Some(json!({ "name": name, "permissions": ["consume"] })),
        )
        .await
        .json()
        .await
        .unwrap();
        ids.push(created["id"].as_str().unwrap().to_owned());
        node.clock.advance(Duration::from_millis(5));
    }
    let page: Value = get(&node, "/tokens?limit=2", Some(ROOT))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    let cursor = page["nextCursor"].as_str().unwrap();
    let rest: Value = get(
        &node,
        &format!("/tokens?limit=2&cursor={cursor}"),
        Some(ROOT),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(rest["items"].as_array().unwrap().len(), 1);
    assert!(rest.get("nextCursor").is_none());

    let filtered: Value = get(&node, "/tokens?name=alfa", Some(ROOT))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(filtered["items"].as_array().unwrap().len(), 2);
    let newest: Value = get(&node, "/tokens?sort=-createdAt", Some(ROOT))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(newest["items"][0]["id"], ids[2].as_str());
    assert_eq!(
        error_code(get(&node, "/tokens?sort=name", Some(ROOT)).await, 400).await,
        "validation.failed"
    );
    node.shutdown().await;
}

/// UC: UC-AUT-018
#[tokio::test]
async fn an_expired_token_is_a_401() {
    let node = start().await;
    let created: Value = call(
        &node,
        Method::POST,
        "/tokens",
        Some(ROOT),
        Some(json!({
            "name": "short", "permissions": ["consume"], "expiresAt": "2026-10-06T12:01:00.000Z"
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let secret = created["token"].as_str().unwrap();
    assert_eq!(get(&node, "/topics", Some(secret)).await.status(), 200);

    node.clock.advance(Duration::from_secs(60));
    assert_eq!(
        error_code(get(&node, "/topics", Some(secret)).await, 401).await,
        "auth.unauthorized"
    );
    node.shutdown().await;
}

/// UC: UC-AUT-006
#[tokio::test]
async fn the_root_token_file_is_reloaded_without_restarting() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("root-token");
    std::fs::write(&file, format!("{ROOT}\n")).unwrap();
    let node = TestNode::builder()
        .production()
        .env("TABA_AUTH_ROOT_TOKEN_FILE", &file.display().to_string())
        .env("TABA_AUTH_RELOAD_INTERVAL", "50ms")
        .start()
        .await;
    let (_, worker) = token(&node, &["consume"], &[]).await;
    assert_eq!(get(&node, "/topics", Some(ROOT)).await.status(), 200);

    // During the swap both work.
    std::fs::write(&file, format!("{OTHER_ROOT}\n{ROOT}\n")).unwrap();
    wait_until(|| async { get(&node, "/topics", Some(OTHER_ROOT)).await.status() == 200 }).await;
    assert_eq!(get(&node, "/topics", Some(ROOT)).await.status(), 200);

    // With the swap finished, only the new one; the token already created keeps working.
    std::fs::write(&file, format!("{OTHER_ROOT}\n")).unwrap();
    wait_until(|| async { get(&node, "/topics", Some(ROOT)).await.status() == 401 }).await;
    assert_eq!(get(&node, "/topics", Some(OTHER_ROOT)).await.status(), 200);
    assert_eq!(get(&node, "/topics", Some(&worker)).await.status(), 200);
    node.shutdown().await;
}

async fn wait_until<F, Fut>(mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the condition was not met in time");
}

/// UC: UC-LIM-004, UC-LIM-005, UC-LIM-006, UC-LIM-007
#[tokio::test]
async fn the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise() {
    let node = builder()
        .env("TABA_RATELIMIT_PUBLISH_RPS", "1")
        .env("TABA_RATELIMIT_ADMIN_RPS", "1000")
        .env("TABA_RATELIMIT_BURST", "1")
        .start()
        .await;
    queue(&node, "orders").await;
    let (_, a) = token(&node, &["publish"], &[]).await;
    let (_, b) = token(&node, &["publish"], &[]).await;
    let (_, consumer) = token(&node, &["consume"], &[]).await;

    assert_eq!(publish(&node, "orders", &a).await.status(), 201);
    assert_eq!(
        publish(&node, "orders", &b).await.status(),
        201,
        "another token, another counter"
    );
    // The same token, even from another IP, stays on the same counter.
    let spoofed = node
        .client
        .post(node.client_url("/topics/orders/messages"))
        .header("authorization", format!("Bearer {a}"))
        .header("x-forwarded-for", "9.9.9.9")
        .header("content-type", "application/json")
        .body("1")
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(spoofed, 429).await, "rate-limit.exceeded");

    // A `403` also counts on the token.
    assert_eq!(
        error_code(publish(&node, "orders", &consumer).await, 403).await,
        "auth.forbidden"
    );
    assert_eq!(
        error_code(publish(&node, "orders", &consumer).await, 429).await,
        "rate-limit.exceeded"
    );

    // Without a valid token the key is the IP: the `401` counts (whoever tries to guess does not go without a limit).
    assert_eq!(
        error_code(publish(&node, "orders", "tb_x.y").await, 401).await,
        "auth.unauthorized"
    );
    assert_eq!(
        error_code(publish(&node, "orders", "other-junk").await, 429).await,
        "rate-limit.exceeded"
    );

    // Citing another token's `id` without the secret does not spend its counter.
    node.clock.advance(Duration::from_secs(1));
    let (id_b, _) = token(&node, &["publish"], &[]).await;
    let forged = format!("tb_{id_b}.wrong-secret");
    assert_eq!(
        error_code(publish(&node, "orders", &forged).await, 401).await,
        "auth.unauthorized"
    );
    node.shutdown().await;
}
