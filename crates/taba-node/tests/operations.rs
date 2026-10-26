//! The operations API: the drain, `/health/cluster`, the IP filter and the failure state
//! (doc 06b; doc 04, section 6.10).

mod common;

use std::time::Duration;

use common::{TestNode, error_code};
use serde_json::{Value, json};

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

async fn publish(node: &TestNode, topic: &str, body: &str) -> reqwest::Response {
    node.client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

async fn consume(node: &TestNode, topic: &str) -> Vec<Value> {
    let response = node
        .client_get(&format!("/topics/{topic}/messages?maxMessages=10"))
        .await;
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

async fn drain_call(node: &TestNode, method: reqwest::Method) -> reqwest::Response {
    node.client
        .request(method, node.operations_url("/node/drain"))
        .send()
        .await
        .unwrap()
}

async fn wait_for_drain_state(node: &TestNode, state: &str) -> Value {
    for _ in 0..200 {
        let status: Value = drain_call(node, reqwest::Method::GET)
            .await
            .json()
            .await
            .unwrap();
        if status["drainState"] == state {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the node did not reach `{state}`");
}

/// UC: UC-DRN-001, UC-DRN-004, UC-DRN-005, UC-DRN-007, UC-DRN-020, UC-OPS-004
#[tokio::test]
async fn the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory() {
    let node = TestNode::single().await;
    create_queue(&node, "orders").await;
    assert_eq!(publish(&node, "orders", "1").await.status(), 201);
    let before = node.metrics().await;
    assert_eq!(
        common::series(&before, "taba_node_drain_pending"),
        Some(1.0),
        "the message is still in memory"
    );
    assert_eq!(
        common::series(&before, "taba_node_drain_state{taba_state=\"active\"}"),
        Some(1.0)
    );

    let idle: Value = drain_call(&node, reqwest::Method::GET)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        idle,
        json!({ "drainState": "active", "trigger": null, "startedAt": null, "pendingMessages": 1 })
    );

    let started = drain_call(&node, reqwest::Method::POST).await;
    assert_eq!(started.status(), 202);
    let started: Value = started.json().await.unwrap();
    assert_eq!(started["trigger"], "api");
    assert_eq!(started["startedAt"], "2026-10-06T12:00:00.000Z");
    let done = wait_for_drain_state(&node, "drained").await;
    assert_eq!(done["pendingMessages"], 0);
    assert!(
        node.storage_dir()
            .join("orders")
            .join("_topic.db")
            .is_file()
    );
    let after = node.metrics().await;
    assert_eq!(common::series(&after, "taba_node_drain_pending"), Some(0.0));
    assert_eq!(
        common::series(&after, "taba_node_drain_state{taba_state=\"drained\"}"),
        Some(1.0)
    );
    assert_eq!(
        common::series(&after, "taba_node_drain_state{taba_state=\"active\"}"),
        Some(0.0)
    );

    // Out of rotation: ready drops to 503 and live stays 200.
    let ready = node.client_get("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(
        ready.json::<Value>().await.unwrap(),
        json!({ "health": "not-ready", "reason": "drained" })
    );
    assert_eq!(node.client_get("/health/live").await.status(), 200);

    // The client calls are not served: 503 with Retry-After.
    let refused = node.client_get("/topics/orders/messages").await;
    assert_eq!(refused.headers()["retry-after"], "5");
    assert_eq!(error_code(refused, 503).await, "cluster.node-draining");
    assert_eq!(
        error_code(publish(&node, "orders", "2").await, 503).await,
        "cluster.node-draining"
    );

    // Start again: 200, no effect.
    let again = drain_call(&node, reqwest::Method::POST).await;
    assert_eq!(again.status(), 200);
    assert_eq!(
        again.json::<Value>().await.unwrap()["drainState"],
        "drained"
    );
    node.shutdown().await;
}

/// UC: UC-DRN-016, UC-DRN-019
#[tokio::test]
async fn deleting_the_drain_brings_the_node_back_with_its_messages() {
    let node = TestNode::single().await;
    create_queue(&node, "orders").await;
    publish(&node, "orders", r#"{ "n": 1 }"#).await;

    let nothing = drain_call(&node, reqwest::Method::DELETE).await;
    assert_eq!(nothing.status(), 200, "without a drain, nothing to cancel");
    assert_eq!(
        nothing.json::<Value>().await.unwrap()["drainState"],
        "active"
    );

    // The life is inside the window of the excluded log, and the manifest says when it was last seen.
    node.clock.advance(Duration::from_secs(60));
    node.node
        .cluster()
        .expect("in the cluster")
        .write_manifest();
    drain_call(&node, reqwest::Method::POST).await;
    wait_for_drain_state(&node, "drained").await;
    // Out of the cluster, the node comes back as a new one (a new life): the answer is `202`.
    let back = drain_call(&node, reqwest::Method::DELETE).await;
    assert_eq!(back.status(), 202);
    assert_eq!(back.json::<Value>().await.unwrap()["drainState"], "active");

    for _ in 0..400 {
        if node.client_get("/health/ready").await.status() == 200 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(node.client_get("/health/ready").await.status(), 200);
    // The restoration of the life that ended brings the messages back.
    let mut messages = Vec::new();
    for _ in 0..400 {
        messages = consume(&node, "orders").await;
        if !messages.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(messages.len(), 1, "what went to disk is still deliverable");
    assert_eq!(messages[0]["body"], json!({ "n": 1 }));
    // It went back to writing in memory: the new message does not go to disk.
    publish(&node, "orders", "2").await;
    let status: Value = drain_call(&node, reqwest::Method::GET)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(status["pendingMessages"], 1);
    node.shutdown().await;
}

/// UC: UC-DRN-006, UC-OPS-035
#[tokio::test]
async fn the_operations_routes_only_accept_the_allowed_ips() {
    let node = TestNode::builder()
        .env("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8, 192.168.1.10")
        .start()
        .await;
    // The list filters the operations port; the client API (and its probes) stay open.
    for path in [
        "/health/live",
        "/health/ready",
        "/health/cluster",
        "/node/drain",
        "/metrics",
    ] {
        assert_eq!(
            error_code(node.client_get_ops(path).await, 403).await,
            "ops.ip-not-allowed",
            "{path}"
        );
    }
    assert_eq!(
        error_code(
            node.client
                .post(node.operations_url("/node/drain"))
                .send()
                .await
                .unwrap(),
            403
        )
        .await,
        "ops.ip-not-allowed"
    );
    assert_eq!(node.client_get("/topics").await.status(), 200);
    assert_eq!(node.client_get("/health/ready").await.status(), 200);
    node.shutdown().await;

    let node = TestNode::builder()
        .env("TABA_OPERATIONS_ALLOWED_IPS", "127.0.0.0/8")
        .start()
        .await;
    assert_eq!(node.client_get_ops("/health/ready").await.status(), 200);
    assert_eq!(node.client_get_ops("/node/drain").await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-OPS-017, UC-OPS-018
#[tokio::test]
async fn cluster_health_describes_the_single_node() {
    let node = TestNode::builder()
        .env("TABA_HEALTH_SINGLE_NODE_CONDITION", "false")
        .env("TABA_MEMORY_LIMIT", "1000")
        .start()
        .await;
    create_queue(&node, "orders").await;
    publish(&node, "orders", "1").await;

    for scope in ["", "?scope=node", "?scope=cluster"] {
        let response = node
            .client_get_ops(&format!("/health/cluster{scope}"))
            .await;
        assert_eq!(response.status(), 200, "{scope}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["partial"], false);
        assert_eq!(body["worstCondition"], "ok");
        assert_eq!(body["conditions"], json!([]));
        assert_eq!(body["summary"]["members"], json!({ "active": 1 }));
        let nodes = body["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["reachable"], true);
        assert_eq!(nodes[0]["nodeId"], node.node.node_id());
        let status = &nodes[0]["status"];
        assert_eq!(status["state"], "active");
        assert_eq!(status["drainState"], "active");
        assert_eq!(status["storage"]["memoryMessages"], 1);
        assert_eq!(status["storage"]["memoryUsedBytes"], 1);
        assert_eq!(status["storage"]["memoryLimitBytes"], 1000);
        assert_eq!(status["storage"]["diskMessages"], 0);
        assert_eq!(status["signals"]["quorumAvailable"], true);
    }
    assert_eq!(
        node.client_get_ops("/health/cluster?scope=node")
            .await
            .json::<Value>()
            .await
            .unwrap()["scope"],
        "node"
    );

    let invalid = node.client_get_ops("/health/cluster?scope=other").await;
    assert_eq!(error_code(invalid, 400).await, "validation.failed");
    node.shutdown().await;
}

/// UC: UC-OPS-002, UC-FAL-023
#[tokio::test]
async fn a_corrupt_topic_database_fails_the_node() {
    let node = TestNode::builder()
        .env("TABA_MEMORY_LIMIT", "1")
        .start()
        .await;
    create_queue(&node, "orders").await;
    let folder = node.storage_dir().join("orders");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("_topic.db"), vec![0xAB; 4096]).unwrap();

    // The sweep (every up to 1 s) opens the database and finds it corrupted; the publish, if it gets there first,
    // finds it too. Whatever the publish answers, the node ends up in failure.
    let _ = publish(&node, "orders", r#"{ "big": true }"#).await;
    for _ in 0..400 {
        if node.client_get("/health/ready").await.status() == 503 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let ready = node.client_get("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(
        ready.json::<Value>().await.unwrap(),
        json!({ "health": "not-ready", "reason": "failed" })
    );
    assert_eq!(node.client_get("/health/live").await.status(), 200);
    let status: Value = node
        .client_get_ops("/health/cluster")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(status["nodes"][0]["status"]["state"], "failed");
    node.shutdown().await;
}

/// UC: UC-CNV-010
#[tokio::test]
async fn a_post_that_expects_no_body_ignores_the_one_it_gets() {
    let node = TestNode::single().await;
    let started = node
        .client
        .post(node.operations_url("/node/drain"))
        .header("content-type", "application/json")
        .body("{ not even json")
        .send()
        .await
        .unwrap();
    assert_eq!(started.status(), 202);
    node.shutdown().await;
}
