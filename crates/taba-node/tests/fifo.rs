//! The FIFO queue: order per group, one in-flight message per group and deduplication (doc 17).

mod common;

use std::time::Duration;

use common::{TestNode, error_code};
use reqwest::Response;
use serde_json::{Value, json};

async fn fifo_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.json::<Value>().await.unwrap()["fifo"], true);
}

async fn publish(node: &TestNode, topic: &str, query: &str, body: &str) -> Response {
    node.client
        .post(node.client_url(&format!("/topics/{topic}/messages{query}")))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

async fn get(node: &TestNode, topic: &str, max: u32) -> Vec<Value> {
    let response = node
        .client_get(&format!(
            "/topics/{topic}/messages?maxMessages={max}&visibilityTimeout=600"
        ))
        .await;
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

async fn ack(node: &TestNode, topic: &str, message: &Value) -> Response {
    node.client
        .delete(node.client_url(&format!(
            "/topics/{topic}/messages/{}?receiptToken={}",
            message["id"].as_str().unwrap(),
            message["receiptToken"].as_str().unwrap()
        )))
        .send()
        .await
        .unwrap()
}

/// UC: UC-FIF-002, UC-FIF-003, UC-FIF-004, UC-FIF-006
#[tokio::test]
async fn publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id() {
    let node = TestNode::development().await;
    fifo_queue(&node, "orders").await;

    assert_eq!(
        error_code(publish(&node, "orders", "", "1").await, 400).await,
        "validation.group-id-required"
    );
    assert_eq!(
        error_code(
            publish(&node, "orders", "?groupId=with%20space", "1").await,
            400
        )
        .await,
        "validation.failed"
    );

    let first = publish(
        &node,
        "orders",
        "?groupId=customer-42&deduplicationId=order-9001",
        "1",
    )
    .await;
    assert_eq!(first.status(), 201);
    let first: Value = first.json().await.unwrap();
    assert_eq!(first["duplicate"], false);

    let repeat = publish(
        &node,
        "orders",
        "?groupId=customer-42&deduplicationId=order-9001",
        "2",
    )
    .await;
    assert_eq!(repeat.status(), 200, "a duplicate is not 201");
    let repeat: Value = repeat.json().await.unwrap();
    assert_eq!(repeat, json!({ "id": first["id"], "duplicate": true }));

    let messages = get(&node, "orders", 10).await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["deduplicationId"], "order-9001");
    assert_eq!(messages[0]["groupId"], "customer-42");
    assert_eq!(messages[0]["body"], 1);
    node.shutdown().await;
}

/// UC: UC-FIF-005, UC-FIF-007, UC-FIF-008
#[tokio::test]
async fn the_body_hash_is_the_default_key_the_scope_is_the_group_and_the_window_ends() {
    let node = TestNode::development().await;
    fifo_queue(&node, "orders").await;
    let id = |response: Response| async move { response.json::<Value>().await.unwrap() };

    let a = id(publish(&node, "orders", "?groupId=g1", r#"{ "n": 1 }"#).await).await;
    let same = publish(&node, "orders", "?groupId=g1", r#"{ "n": 1 }"#).await;
    assert_eq!(same.status(), 200);
    assert_eq!(id(same).await["id"], a["id"]);
    let other_group = publish(&node, "orders", "?groupId=g2", r#"{ "n": 1 }"#).await;
    assert_eq!(
        other_group.status(),
        201,
        "another group is not a duplicate"
    );

    node.clock.advance(Duration::from_secs(301));
    let later = publish(&node, "orders", "?groupId=g1", r#"{ "n": 1 }"#).await;
    assert_eq!(
        later.status(),
        201,
        "once the window has passed, it is a new message"
    );

    let messages = get(&node, "orders", 10).await;
    let hash = messages[0]["deduplicationId"].as_str().unwrap();
    assert_eq!(hash.len(), 64, "the SHA-256 in hexadecimal");
    node.shutdown().await;
}

/// UC: UC-FIF-012, UC-FIF-013, UC-FIF-014, UC-FIF-015, UC-FIF-018
#[tokio::test]
async fn one_message_in_flight_per_group_in_order_and_groups_in_parallel() {
    let node = TestNode::development().await;
    fifo_queue(&node, "orders").await;
    for n in 1..=3 {
        for group in ["a", "b"] {
            let r = publish(
                &node,
                "orders",
                &format!("?groupId={group}"),
                &n.to_string(),
            )
            .await;
            assert_eq!(r.status(), 201);
        }
    }
    let batch = get(&node, "orders", 10).await;
    let mut groups: Vec<&str> = batch
        .iter()
        .map(|m| m["groupId"].as_str().unwrap())
        .collect();
    groups.sort_unstable();
    assert_eq!(groups, ["a", "b"], "one from each group in the same batch");
    assert!(
        get(&node, "orders", 10).await.is_empty(),
        "the two groups are busy"
    );

    for message in &batch {
        assert_eq!(ack(&node, "orders", message).await.status(), 204);
    }
    let next = get(&node, "orders", 10).await;
    let bodies: Vec<&Value> = next.iter().map(|m| &m["body"]).collect();
    assert_eq!(bodies, [&json!(2), &json!(2)], "each group's order is kept");
    node.shutdown().await;
}

/// UC: UC-FIF-011
#[tokio::test]
async fn deduplication_still_holds_after_the_ack() {
    let node = TestNode::development().await;
    fifo_queue(&node, "orders").await;
    let first: Value = publish(&node, "orders", "?groupId=g&deduplicationId=k", "1")
        .await
        .json()
        .await
        .unwrap();
    let message = get(&node, "orders", 1).await.remove(0);
    assert_eq!(ack(&node, "orders", &message).await.status(), 204);
    assert_eq!(
        error_code(ack(&node, "orders", &message).await, 404).await,
        "message.not-found"
    );

    let repeat = publish(&node, "orders", "?groupId=g&deduplicationId=k", "1").await;
    assert_eq!(repeat.status(), 200);
    assert_eq!(repeat.json::<Value>().await.unwrap()["id"], first["id"]);
    assert!(
        get(&node, "orders", 10).await.is_empty(),
        "the ack already removed the message"
    );
    node.shutdown().await;
}

/// UC: UC-FIF-021
#[tokio::test]
async fn the_dead_letter_of_a_fifo_is_a_common_queue() {
    let node = TestNode::development().await;
    let made = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": "orders", "type": "queue", "fifo": true, "maxDeliveries": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(made.status(), 201);
    publish(&node, "orders", "?groupId=g&deduplicationId=a", "1").await;
    publish(&node, "orders", "?groupId=g&deduplicationId=b", "2").await;
    let first = node
        .client_get("/topics/orders/messages?visibilityTimeout=5")
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(first["messages"][0]["body"], 1);
    node.clock.advance(Duration::from_secs(5));
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let dead = get(&node, "orders-dead-letter", 10).await;
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0]["body"], 1);
    assert_eq!(dead[0]["deadLetterReason"], "max-deliveries");
    assert!(dead[0].get("deduplicationId").is_none());
    assert_eq!(
        get(&node, "orders", 10).await[0]["body"],
        2,
        "the group went on"
    );
    let topic: Value = node
        .client_get("/topics/orders-dead-letter")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(topic["fifo"], false);
    node.shutdown().await;
}

/// UC: UC-FIF-001, UC-FIF-042, UC-FIF-046
#[tokio::test]
async fn fifo_is_immutable_works_with_auto_create_and_in_development() {
    let node = TestNode::development().await;
    let patch = node
        .client
        .patch(node.client_url("/topics/x"))
        .json(&json!({ "fifo": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch.status(), 404);
    fifo_queue(&node, "orders").await;
    let patch = node
        .client
        .patch(node.client_url("/topics/orders"))
        .json(&json!({ "fifo": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(patch, 400).await, "validation.immutable-field");

    assert_eq!(
        node.client_get("/topics/auto/messages?autoCreate=true&fifo=true")
            .await
            .status(),
        200
    );
    let auto: Value = node.client_get("/topics/auto").await.json().await.unwrap();
    assert_eq!(auto["fifo"], true);
    let dist = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": "dist", "type": "distributor", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(dist, 400).await, "validation.not-applicable");
    node.shutdown().await;
}

/// UC: UC-FIF-046, UC-DRN-023
#[tokio::test]
async fn a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("taba").display().to_string();
    let start = || {
        TestNode::builder()
            .env("TABA_STORAGE_DIR", &storage)
            .try_start()
    };
    let node = start().await.unwrap();
    fifo_queue(&node, "orders").await;
    publish(&node, "orders", "?groupId=g1&deduplicationId=a", "1").await;
    publish(&node, "orders", "?groupId=g1&deduplicationId=b", "2").await;
    publish(&node, "orders", "?groupId=g2&deduplicationId=c", "3").await;
    let flight = get(&node, "orders", 10).await;
    assert_eq!(flight.len(), 2);
    let done = flight.iter().find(|m| m["groupId"] == "g2").unwrap();
    assert_eq!(ack(&node, "orders", done).await.status(), 204);
    node.shutdown().await;

    let node = start().await.unwrap();
    assert!(
        get(&node, "orders", 10).await.is_empty(),
        "g1 stays busy because of the one that was in flight"
    );
    let in_flight = flight.iter().find(|m| m["groupId"] == "g1").unwrap();
    assert_eq!(
        ack(&node, "orders", in_flight).await.status(),
        204,
        "o receiptToken sobreviveu"
    );
    assert_eq!(get(&node, "orders", 10).await[0]["body"], 2);
    let repeat = publish(&node, "orders", "?groupId=g2&deduplicationId=c", "3").await;
    assert_eq!(repeat.status(), 200, "the ack's short row survived");
    node.shutdown().await;
}
