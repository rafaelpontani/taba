//! O rate limit por nó e o IP do cliente (doc 12).

mod common;

use std::time::Duration;

use common::{TestNode, error_code};
use reqwest::Response;
use serde_json::json;

async fn get(node: &TestNode, path: &str, forwarded: Option<&str>) -> Response {
    let mut request = node.client.get(node.client_url(path));
    if let Some(ip) = forwarded {
        request = request.header("x-forwarded-for", ip);
    }
    request.send().await.unwrap()
}

async fn post_message(node: &TestNode, topic: &str) -> Response {
    node.client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .header("content-type", "application/json")
        .body("1")
        .send()
        .await
        .unwrap()
}

async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
}

/// UC: UC-LIM-001, UC-LIM-002, UC-LIM-003
#[tokio::test]
async fn exceeding_a_category_gives_429_with_retry_after_and_only_that_category() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_PUBLISH_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "2")
        .start()
        .await;
    create_queue(&node, "pedidos").await;

    assert_eq!(post_message(&node, "pedidos").await.status(), 201);
    assert_eq!(post_message(&node, "pedidos").await.status(), 201);
    let limited = post_message(&node, "pedidos").await;
    assert_eq!(limited.headers()["retry-after"], "1");
    assert_eq!(error_code(limited, 429).await, "rate-limit.exceeded");

    // As outras categorias não foram afetadas.
    assert_eq!(
        get(&node, "/topics/pedidos/messages", None).await.status(),
        200
    );
    assert_eq!(get(&node, "/topics", None).await.status(), 200);

    // O contador se reabastece com o tempo.
    node.clock.advance(Duration::from_secs(1));
    assert_eq!(post_message(&node, "pedidos").await.status(), 201);
    assert_eq!(post_message(&node, "pedidos").await.status(), 429);
    node.shutdown().await;
}

/// UC: UC-LIM-001, UC-LIM-002
#[tokio::test]
async fn admin_and_consume_have_their_own_counters() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_ADMIN_RPS", "1")
        .env("TABA_RATELIMIT_CONSUME_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .start()
        .await;
    assert_eq!(get(&node, "/topics", None).await.status(), 200);
    assert_eq!(
        error_code(get(&node, "/topics", None).await, 429).await,
        "rate-limit.exceeded"
    );
    assert_eq!(
        error_code(
            node.client
                .post(node.client_url("/topics"))
                .json(&json!({}))
                .send()
                .await
                .unwrap(),
            429
        )
        .await,
        "rate-limit.exceeded"
    );

    // O `GET` de mensagens é consumo: conta à parte (e dá 404 porque o tópico não existe).
    assert_eq!(get(&node, "/topics/x/messages", None).await.status(), 404);
    assert_eq!(get(&node, "/topics/x/messages", None).await.status(), 429);
    node.shutdown().await;
}

/// UC: UC-LIM-009, UC-LIM-010
#[tokio::test]
async fn probes_are_exempt_and_the_cluster_query_has_its_own_category() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_ADMIN_RPS", "1")
        .env("TABA_RATELIMIT_OPS_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .start()
        .await;
    for _ in 0..20 {
        assert_eq!(get(&node, "/health/live", None).await.status(), 200);
        assert_eq!(get(&node, "/health/ready", None).await.status(), 200);
        assert_eq!(
            get(&node, "/health/cluster?scope=node", None)
                .await
                .status(),
            200
        );
    }
    assert_eq!(get(&node, "/health/cluster", None).await.status(), 200);
    assert_eq!(
        error_code(get(&node, "/health/cluster", None).await, 429).await,
        "rate-limit.exceeded"
    );
    assert_eq!(
        error_code(get(&node, "/health/cluster?scope=cluster", None).await, 429).await,
        "rate-limit.exceeded"
    );
    assert_eq!(
        get(&node, "/health/cluster?scope=node", None)
            .await
            .status(),
        200
    );

    // `/node/drain` é administrativa.
    assert_eq!(get(&node, "/node/drain", None).await.status(), 200);
    assert_eq!(
        error_code(get(&node, "/node/drain", None).await, 429).await,
        "rate-limit.exceeded"
    );
    node.shutdown().await;
}

/// UC: UC-LIM-005, UC-LIM-011
#[tokio::test]
async fn without_a_token_the_counter_is_per_client_ip_from_the_headers() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_ADMIN_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .start()
        .await;
    assert_eq!(
        get(&node, "/topics", Some("9.9.9.1, 10.0.0.1"))
            .await
            .status(),
        200
    );
    assert_eq!(
        get(&node, "/topics", Some("9.9.9.2")).await.status(),
        200,
        "outro cliente"
    );
    assert_eq!(
        get(&node, "/topics", Some("9.9.9.1")).await.status(),
        429,
        "o primeiro endereço manda"
    );
    assert_eq!(
        get(&node, "/topics", Some("lixo")).await.status(),
        200,
        "inválido: vale a conexão"
    );
    assert_eq!(
        get(&node, "/topics", Some("outro-lixo")).await.status(),
        429
    );
    node.shutdown().await;
}

/// UC: UC-LIM-012
#[tokio::test]
async fn with_the_headers_disabled_a_forged_one_does_not_escape_the_limit() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_ADMIN_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .env("TABA_CLIENT_IP_HEADERS_ENABLED", "false")
        .start()
        .await;
    assert_eq!(get(&node, "/topics", Some("9.9.9.1")).await.status(), 200);
    assert_eq!(get(&node, "/topics", Some("9.9.9.2")).await.status(), 429);
    node.shutdown().await;
}

/// UC: UC-LIM-011
#[tokio::test]
async fn the_headers_only_count_from_the_trusted_proxies() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_ADMIN_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .env("TABA_CLIENT_IP_TRUSTED_PROXIES", "10.0.0.0/8")
        .start()
        .await;
    // A conexão é 127.0.0.1, fora da lista: o cabeçalho é ignorado.
    assert_eq!(get(&node, "/topics", Some("9.9.9.1")).await.status(), 200);
    assert_eq!(get(&node, "/topics", Some("9.9.9.2")).await.status(), 429);
    node.shutdown().await;

    let node = TestNode::builder()
        .env("TABA_RATELIMIT_ADMIN_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .env("TABA_CLIENT_IP_TRUSTED_PROXIES", "127.0.0.0/8")
        .start()
        .await;
    assert_eq!(get(&node, "/topics", Some("9.9.9.1")).await.status(), 200);
    assert_eq!(get(&node, "/topics", Some("9.9.9.2")).await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-LIM-013
#[tokio::test]
async fn the_resolved_ip_is_the_one_checked_against_allowed_ips() {
    let node = TestNode::builder()
        .env("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8")
        .start()
        .await;
    assert_eq!(
        error_code(get(&node, "/health/ready", None).await, 403).await,
        "ops.ip-not-allowed"
    );
    assert_eq!(
        get(&node, "/health/ready", Some("10.1.2.3")).await.status(),
        200
    );
    assert_eq!(
        error_code(get(&node, "/health/ready", Some("11.1.2.3")).await, 403).await,
        "ops.ip-not-allowed"
    );
    node.shutdown().await;
}

/// UC: UC-LIM-001
#[tokio::test]
async fn without_configured_rates_nothing_is_limited() {
    let node = TestNode::development().await;
    for _ in 0..50 {
        assert_eq!(get(&node, "/topics", None).await.status(), 200);
    }
    node.shutdown().await;
}
