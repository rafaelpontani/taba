//! The one-node profile (doc 07, section 8; level **C**): `replication.factor=1`, no cluster TLS and no entry
//! address. The node is a cluster of one, starts alone and serves like any other.

mod common;

use serde_json::{Value, json};

use common::cluster::{eventually, wait_joined};
use common::{TestNode, error_code};
use taba_node::cluster::health::Condition;

const DEV_SECRET: &str = "dev-secret-with-at-least-32-characters";

fn profile() -> common::TestNodeBuilder {
    TestNode::builder().single_node()
}

/// UC: UC-RTN-043
#[tokio::test]
async fn a_node_with_factor_one_starts_alone_and_serves_the_plain_and_the_fifo_queue() {
    let node = profile().start().await;
    wait_joined(&node).await;
    let state = node.node.shared().clone();
    eventually("the node to be active", move || {
        state.state() == taba_node::cluster::state::NodeState::Active
    })
    .await;

    for body in [
        json!({ "name": "plain", "type": "queue" }),
        json!({ "name": "ordered", "type": "queue", "fifo": true }),
    ] {
        let created = node
            .client
            .post(node.client_url("/topics"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), 201, "{body}");
    }
    let published = node
        .client
        .post(node.client_url("/topics/plain/messages"))
        .header("content-type", "application/json")
        .body("{\"n\":1}")
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 201);
    let fifo = node
        .client
        .post(node.client_url("/topics/ordered/messages?groupId=g"))
        .header("content-type", "application/json")
        .body("{\"n\":1}")
        .send()
        .await
        .unwrap();
    assert_eq!(fifo.status(), 201, "{:?}", fifo.text().await);

    let health: Value = node.stats("/health/cluster").await;
    let conditions: Vec<&str> = health["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["condition"].as_str().unwrap())
        .collect();
    assert!(
        conditions.contains(&Condition::ReducedProfile.as_str()),
        "{conditions:?}"
    );
    assert!(!conditions.contains(&"quorum-lost"), "{conditions:?}");
    node.shutdown().await;
}

/// UC: UC-RTN-043
#[tokio::test]
async fn the_reduced_profile_condition_can_be_hidden() {
    let node = profile()
        .env("TABA_HEALTH_SINGLE_NODE_CONDITION", "false")
        .start()
        .await;
    wait_joined(&node).await;
    let health: Value = node.stats("/health/cluster").await;
    let text = health.to_string();
    assert!(!text.contains("reduced-profile"), "{text}");
    node.shutdown().await;
}

/// UC: UC-RTN-044
#[tokio::test]
async fn the_dev_token_is_an_admin_work_token_created_at_the_start_and_turns_authentication_on() {
    let node = profile()
        .env("TABA_AUTH_DEV_TOKEN", DEV_SECRET)
        .start()
        .await;
    wait_joined(&node).await;

    let without = node.client_get("/topics").await;
    assert_eq!(error_code(without, 401).await, "auth.unauthorized");

    let bearer = format!("Bearer tb_dev.{DEV_SECRET}");
    let list = || async {
        node.client
            .get(node.client_url("/topics"))
            .header("authorization", bearer.clone())
            .send()
            .await
            .unwrap()
            .status()
    };
    eventually_status(&list, 200).await;
    let wrong = node
        .client
        .get(node.client_url("/topics"))
        .header("authorization", format!("Bearer tb_dev.{DEV_SECRET}x"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);

    // `admin` reaches the tokens routes, and the secret never shows in them.
    let tokens = node
        .client
        .get(node.client_url("/tokens"))
        .header("authorization", bearer.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(tokens.status(), 200);
    let text = tokens.text().await.unwrap();
    assert!(text.contains("dev-token"), "{text}");
    assert!(!text.contains(DEV_SECRET), "{text}");
    node.shutdown().await;
}

/// Starts the profile and creates the queue, waiting for the node to be active.
async fn active_with_queue(name: &str) -> TestNode {
    let node = profile().start().await;
    wait_joined(&node).await;
    let state = node.node.shared().clone();
    eventually("the node to be active", move || {
        state.state() == taba_node::cluster::state::NodeState::Active
    })
    .await;
    let created = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    node
}

async fn post_json(node: &TestNode, path: &str, body: Value) -> Value {
    let response = node
        .client
        .post(node.client_url(path))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    response.json().await.unwrap()
}

/// The register, commit and rollback calls the node served to itself (the two phases).
fn phases(metrics: &str) -> f64 {
    common::cluster::total(metrics, "taba_write_received_total", "")
}

/// UC: UC-MSG-024
#[tokio::test]
async fn alone_the_batches_publish_deliver_and_ack_without_the_two_phases() {
    let node = active_with_queue("orders").await;
    let messages: Vec<Value> = (0..300).map(|n| json!({ "body": { "n": n } })).collect();
    let published = post_json(
        &node,
        "/topics/orders/messages/batch",
        json!({ "messages": messages }),
    )
    .await;
    let results = published["results"].as_array().unwrap();
    assert_eq!(results.len(), 300);
    assert!(
        results
            .iter()
            .all(|r| r["status"] == 201 && r["id"].is_string()),
        "{published}"
    );

    let delivered: Value = node
        .client
        .get(node.client_url("/topics/orders/messages?maxMessages=500"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let delivered = delivered["messages"].as_array().unwrap().clone();
    assert_eq!(delivered.len(), 300, "every message, each once");
    assert!(delivered.iter().all(|m| m["deliveryCount"] == 1));
    // Each body comes as it was published (a plain queue does not promise the order).
    let mut bodies: Vec<i64> = delivered
        .iter()
        .map(|m| m["body"]["n"].as_i64().unwrap())
        .collect();
    bodies.sort_unstable();
    assert_eq!(bodies, (0..300).collect::<Vec<_>>());

    // One stale receipt and the others right: the stale one alone fails.
    let mut acks: Vec<Value> = delivered
        .iter()
        .map(|m| json!({ "id": m["id"], "receiptToken": m["receiptToken"] }))
        .collect();
    acks[7]["receiptToken"] = json!("0000000000000000000000000");
    let acked = post_json(
        &node,
        "/topics/orders/messages/ack",
        json!({ "messages": acks }),
    )
    .await;
    let acked = acked["results"].as_array().unwrap();
    assert_eq!(acked[7]["status"], 409, "{:?}", acked[7]);
    assert_eq!(acked[7]["error"]["code"], "message.stale-receipt");
    assert_eq!(acked.iter().filter(|r| r["status"] == 204).count(), 299);

    // The one left goes back after its deadline, as on any queue: here it is still invisible.
    let again: Value = node
        .client
        .get(node.client_url("/topics/orders/messages?maxMessages=500"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(again["messages"].as_array().unwrap().is_empty());

    let metrics = node.metrics().await;
    assert_eq!(phases(&metrics), 0.0, "no register or commit to itself");
    let coordinated = common::series(
        &metrics,
        "taba_write_coordinated_total{taba_outcome=\"committed\"}",
    );
    assert_eq!(
        coordinated,
        Some(899.0),
        "300 publications, 300 deliveries and 299 acks"
    );
    node.shutdown().await;
}

/// UC: UC-MSG-024
#[tokio::test]
async fn alone_the_single_calls_and_the_visibility_take_the_same_path() {
    let node = active_with_queue("orders").await;
    let published = node
        .client
        .post(node.client_url("/topics/orders/messages"))
        .header("content-type", "application/json")
        .body("{\"n\":1}")
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 201);
    let delivered: Value = node
        .client
        .get(node.client_url("/topics/orders/messages?visibilityTimeout=600"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let message = delivered["messages"][0].clone();
    let (id, receipt) = (
        message["id"].as_str().unwrap().to_owned(),
        message["receiptToken"].as_str().unwrap().to_owned(),
    );
    // Visibility zero: the message is ready again right away, with a new delivery.
    let released = node
        .client
        .put(node.client_url(&format!(
            "/topics/orders/messages/{id}/visibility?receiptToken={receipt}"
        )))
        .json(&json!({ "visibilityTimeout": 0 }))
        .send()
        .await
        .unwrap();
    assert_eq!(released.status(), 200);
    let again: Value = node
        .client
        .get(node.client_url("/topics/orders/messages"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(again["messages"][0]["id"], json!(id));
    assert_eq!(again["messages"][0]["deliveryCount"], 2);
    let receipt = again["messages"][0]["receiptToken"].as_str().unwrap();
    let stale = node
        .client
        .delete(node.client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken=0000000000000000000000000"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(stale, 409).await, "message.stale-receipt");
    let acked = node
        .client
        .delete(node.client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={receipt}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(acked.status(), 204);
    assert_eq!(phases(&node.metrics().await), 0.0);
    node.shutdown().await;
}

async fn eventually_status<F, Fut>(call: &F, expected: u16)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = reqwest::StatusCode>,
{
    for _ in 0..200 {
        if call().await.as_u16() == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the call never answered {expected}");
}

/// UC: UC-MSG-028
#[tokio::test]
async fn alone_a_delayed_publication_in_a_batch_and_in_a_single_call_waits_for_the_deadline() {
    let clock = std::sync::Arc::new(taba_node::core::clock::ManualClock::at_doc_epoch());
    let node = profile().clock(clock.clone()).start().await;
    wait_joined(&node).await;
    let state = node.node.shared().clone();
    eventually("the node to be active", move || {
        state.state() == taba_node::cluster::state::NodeState::Active
    })
    .await;
    let created = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": "orders", "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);

    let single = node
        .client
        .post(node.client_url("/topics/orders/messages?delaySeconds=30"))
        .json(&json!({ "n": "single" }))
        .send()
        .await
        .unwrap();
    assert_eq!(single.status(), 201);
    let batch = post_json(
        &node,
        "/topics/orders/messages/batch",
        json!({ "messages": [
            { "body": { "n": "batch-delayed" }, "delaySeconds": 30 },
            { "body": { "n": "batch-now" } },
        ]}),
    )
    .await;
    assert!(
        batch["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == 201),
        "{batch}"
    );

    let consume = || async {
        let response: Value = node
            .client
            .get(node.client_url("/topics/orders/messages?maxMessages=10"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let mut names: Vec<String> = response["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["body"]["n"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(consume().await, ["batch-now"]);
    clock.advance(std::time::Duration::from_secs(29));
    assert!(consume().await.is_empty(), "29 s: still waiting");
    clock.advance(std::time::Duration::from_secs(1));
    assert_eq!(consume().await, ["batch-delayed", "single"]);
    node.shutdown().await;
}
