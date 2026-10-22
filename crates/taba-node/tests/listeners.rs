//! The three listeners and the node's health (doc 06, section 8.0; doc 06b).

mod common;

use common::{TestNode, error_code};
use serde_json::json;

/// UC: UC-OPS-001, UC-OPS-002
#[tokio::test]
async fn a_single_node_is_live_and_ready() {
    let node = TestNode::single().await;

    let live = node.client_get("/health/live").await;
    assert_eq!(live.status(), 200);
    assert_eq!(
        live.json::<serde_json::Value>().await.unwrap(),
        json!({ "health": "live" })
    );

    let ready = node.client_get("/health/ready").await;
    assert_eq!(ready.status(), 200);
    assert_eq!(
        ready.json::<serde_json::Value>().await.unwrap(),
        json!({ "health": "ready" })
    );

    node.shutdown().await;
}

/// UC: UC-OPS-002, UC-OPS-003
#[tokio::test]
async fn production_node_without_cluster_protocol_is_live_but_not_ready() {
    let node = TestNode::production().await;

    assert_eq!(node.client_get("/health/live").await.status(), 200);

    let ready = node.client_get("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.headers()["content-type"], "application/json");
    // The body is the `HealthResponse`, never the `ErrorResponse` (UC-OPS-003).
    let body: serde_json::Value = ready.json().await.unwrap();
    assert_eq!(
        body,
        json!({ "health": "not-ready", "reason": "bootstrapping" })
    );

    node.shutdown().await;
}

/// UC: UC-DEV-002
#[tokio::test]
async fn a_node_with_factor_one_starts_alone_and_keeps_the_cluster_routes_off_the_client_port() {
    // Without `cluster.entry-address`, which the one-node profile does not need.
    let node = TestNode::single().await;
    assert_ne!(node.node.cluster_addr().port(), 0);

    // The client port answers no /cluster/* route.
    let code = error_code(node.client_get("/cluster/nodes").await, 404).await;
    assert_eq!(code, "route.not-found");

    node.shutdown().await;
}

/// UC: UC-SEG-001, UC-SEG-002, UC-CNV-011
#[tokio::test]
async fn client_port_hides_cluster_routes_and_cluster_port_serves_only_cluster_routes() {
    let node = TestNode::production().await;
    let client_addr = node.node.client_addr();
    let cluster_addr = node.node.cluster_addr();
    assert_ne!(client_addr.port(), cluster_addr.port());

    assert_eq!(
        error_code(node.client_get("/cluster/nodes").await, 404).await,
        "route.not-found"
    );

    for path in ["/", "/topics", "/health/live", "/metrics"] {
        let response = node
            .client
            .get(node.cluster_url(path))
            .send()
            .await
            .unwrap();
        assert_eq!(
            error_code(response, 404).await,
            "route.not-found",
            "{path} na porta do cluster"
        );
    }

    node.shutdown().await;
}

/// UC: UC-SEG-003
#[tokio::test]
async fn operations_routes_move_to_their_own_port_when_one_is_set() {
    let node = TestNode::builder()
        .env("TABA_OPERATIONS_PORT", "0")
        .start()
        .await;
    let operations = node.node.operations_addr().expect("operations port open");
    assert_ne!(operations, node.node.client_addr());

    let response = node
        .client
        .get(node.operations_url("/health/live"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // On the API port the operations routes cease to exist.
    assert_eq!(
        error_code(node.client_get("/health/live").await, 404).await,
        "route.not-found"
    );
    assert_eq!(
        error_code(node.client_get("/health/ready").await, 404).await,
        "route.not-found"
    );

    node.shutdown().await;
}

/// UC: UC-CNV-004
#[tokio::test]
async fn every_response_carries_the_request_id() {
    let node = TestNode::single().await;

    let response = node
        .client
        .get(node.client_url("/health/live"))
        .header("x-request-id", "abc-123")
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["x-request-id"], "abc-123");

    let response = node.client_get("/health/live").await;
    let generated = response.headers()["x-request-id"].to_str().unwrap();
    assert!(taba_node::core::id::is_guid_v7(generated), "{generated}");

    node.shutdown().await;
}

#[tokio::test]
async fn shutdown_closes_the_listeners() {
    let node = TestNode::single().await;
    let url = node.client_url("/health/live");
    let client = node.client.clone();
    assert_eq!(client.get(&url).send().await.unwrap().status(), 200);

    node.shutdown().await;
    assert!(
        client.get(&url).send().await.is_err(),
        "the port should be closed"
    );
}
