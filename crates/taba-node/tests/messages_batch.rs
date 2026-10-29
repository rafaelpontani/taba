//! The messages in batch against real nodes (doc 06a, "Messages in batch"): `POST /topics/{topic}/messages/batch`,
//! `/ack` and `/visibility`. Each item is answered as its single call; one that fails does not touch the others.

mod common;

use std::collections::HashSet;

use common::cluster::TestCluster;
use common::{TestNode, error_code};
use serde_json::{Value, json};

async fn create(node: &TestNode, body: Value) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{body}");
}

async fn post(node: &TestNode, path: &str, body: &Value) -> reqwest::Response {
    node.client
        .post(node.client_url(path))
        .json(body)
        .send()
        .await
        .unwrap()
}

/// `POST` of a batch that must be `200`: the `results`.
async fn batch(node: &TestNode, path: &str, body: Value) -> Vec<Value> {
    let response = post(node, path, &body).await;
    assert_eq!(response.status(), 200, "{path}");
    let body: Value = response.json().await.unwrap();
    body["results"].as_array().unwrap().clone()
}

fn statuses(results: &[Value]) -> Vec<u64> {
    results
        .iter()
        .map(|r| r["status"].as_u64().unwrap())
        .collect()
}

async fn consume(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    let response = node
        .client_get(&format!("/topics/{topic}/messages{query}"))
        .await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    body["messages"].as_array().unwrap().clone()
}

/// UC: UC-MSG-021, UC-MSG-022
#[tokio::test]
async fn a_batch_publishes_one_message_per_item_and_a_bad_item_fails_alone() {
    let node = TestNode::single().await;
    create(&node, json!({ "name": "orders", "type": "queue" })).await;
    let results = batch(
        &node,
        "/topics/orders/messages/batch",
        json!({ "messages": [
            { "body": { "n": 1, "a": [true] } },
            { "body": "olá", "contentType": "text/plain; charset=utf-8" },
            { "body": "iVBORw0KGgo=", "bodyEncoding": "base64", "contentType": "image/png" },
            { "body": "iVBORw0KGgo=", "contentType": "image/png" },
            { "body": "x", "contentType": "not a type" },
        ] }),
    )
    .await;
    assert_eq!(statuses(&results), [201, 201, 201, 400, 400]);
    assert_eq!(results[3]["error"]["code"], "validation.invalid-body");
    assert_eq!(
        results[4]["error"]["code"],
        "validation.invalid-content-type"
    );
    assert!(results[3]["id"].is_null());

    let mut got = consume(&node, "orders", "?maxMessages=10").await;
    got.sort_by_key(|m| m["id"].as_str().unwrap().to_owned());
    assert_eq!(got.len(), 3);
    let ids: Vec<&str> = results[..3]
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    for (message, id) in got.iter().zip(&ids) {
        assert_eq!(message["id"], *id);
    }
    assert_eq!(got[0]["body"], json!({ "n": 1, "a": [true] }));
    assert_eq!(got[1]["body"], "olá");
    assert_eq!(got[1]["contentType"], "text/plain; charset=utf-8");
    assert_eq!(got[2]["body"], "iVBORw0KGgo=");
    assert_eq!(got[2]["bodyEncoding"], "base64");
    node.shutdown().await;
}

/// UC: UC-MSG-023
#[tokio::test]
async fn a_batch_outside_the_limits_is_refused_whole_and_a_large_item_fails_alone() {
    let node = TestNode::builder()
        .env("TABA_API_BATCH_MAX_MESSAGES", "3")
        .env("TABA_API_BATCH_MAX_BYTES", "1KB")
        .start()
        .await;
    create(
        &node,
        json!({ "name": "small", "type": "queue", "maxMessageBytes": 10 }),
    )
    .await;
    let path = "/topics/small/messages/batch";
    let item = json!({ "body": "x", "contentType": "text/plain" });
    for count in [0, 4] {
        let body = json!({ "messages": vec![item.clone(); count] });
        let code = error_code(post(&node, path, &body).await, 400).await;
        assert_eq!(code, "validation.out-of-range", "{count} items");
    }
    let big = json!({ "messages": [{ "body": "y".repeat(2000), "contentType": "text/plain" }] });
    assert_eq!(
        error_code(post(&node, path, &big).await, 413).await,
        "message.too-large"
    );
    let unknown = json!({ "messages": [{ "body": 1, "priority": 9 }] });
    assert_eq!(
        error_code(post(&node, path, &unknown).await, 400).await,
        "validation.unknown-field"
    );
    let missing = json!({ "messages": [{ "contentType": "text/plain" }] });
    assert_eq!(
        error_code(post(&node, path, &missing).await, 400).await,
        "validation.required"
    );
    let not_json = node
        .client
        .post(node.client_url(path))
        .header("content-type", "text/plain")
        .body(r#"{"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(not_json, 415).await, "media.unsupported");
    let nowhere = post(
        &node,
        "/topics/nowhere/messages/batch",
        &json!({ "messages": [item] }),
    )
    .await;
    assert_eq!(error_code(nowhere, 404).await, "topic.not-found");

    // Above the topic's `maxMessageBytes`, only that item is `413`.
    let results = batch(
        &node,
        path,
        json!({ "messages": [
            { "body": "short", "contentType": "text/plain" },
            { "body": "longer than ten", "contentType": "text/plain" },
        ] }),
    )
    .await;
    assert_eq!(statuses(&results), [201, 413]);
    assert_eq!(results[1]["error"]["code"], "message.too-large");
    node.shutdown().await;
}

/// UC: UC-FIF-054
#[tokio::test]
async fn on_a_fifo_queue_a_batch_keeps_the_order_of_each_group_and_deduplicates() {
    let node = TestNode::single().await;
    create(
        &node,
        json!({ "name": "orders", "type": "queue", "fifo": true }),
    )
    .await;
    let item = |group: &str, text: &str| json!({ "body": text, "contentType": "text/plain", "groupId": group });
    let results = batch(
        &node,
        "/topics/orders/messages/batch",
        json!({ "messages": [
            item("g1", "a"), item("g2", "x"), item("g1", "b"), item("g1", "a"),
            { "body": "no group", "contentType": "text/plain" },
        ] }),
    )
    .await;
    assert_eq!(statuses(&results), [201, 201, 201, 200, 400]);
    assert_eq!(results[3]["duplicate"], true);
    assert_eq!(results[3]["id"], results[0]["id"]);
    assert_eq!(results[0]["duplicate"], false);
    assert_eq!(results[4]["error"]["code"], "validation.group-id-required");
    // The ids are GUID v7: the order of publication inside g1 is the order of the list.
    assert!(results[0]["id"].as_str() < results[2]["id"].as_str());

    // The group delivers one at a time, in order: a, then b.
    let mut seen = Vec::new();
    for _ in 0..2 {
        let got = consume(&node, "orders", "?maxMessages=10").await;
        let first = got.iter().find(|m| m["groupId"] == "g1").unwrap().clone();
        seen.push(first["body"].as_str().unwrap().to_owned());
        let acked = batch(
            &node,
            "/topics/orders/messages/ack",
            json!({ "messages": got.iter().map(|m| json!({
                "id": m["id"], "receiptToken": m["receiptToken"] })).collect::<Vec<_>>() }),
        )
        .await;
        assert!(statuses(&acked).iter().all(|s| *s == 204));
    }
    assert_eq!(seen, ["a", "b"]);
    node.shutdown().await;
}

/// UC: UC-ACK-014
#[tokio::test]
async fn a_batch_of_acks_removes_each_message_and_a_stale_or_unknown_item_fails_alone() {
    let node = TestNode::single().await;
    create(&node, json!({ "name": "orders", "type": "queue" })).await;
    let published = batch(
        &node,
        "/topics/orders/messages/batch",
        json!({ "messages": [{ "body": 1 }, { "body": 2 }, { "body": 3 }] }),
    )
    .await;
    assert_eq!(statuses(&published), [201, 201, 201]);
    let mut got = consume(&node, "orders", "?maxMessages=10").await;
    got.sort_by_key(|m| m["body"].as_u64().unwrap());
    let receipt = |m: &Value| json!({ "id": m["id"], "receiptToken": m["receiptToken"] });
    let results = batch(
        &node,
        "/topics/orders/messages/ack",
        json!({ "messages": [
            receipt(&got[0]),
            { "id": got[1]["id"], "receiptToken": "not-the-token" },
            { "id": "01900000-0000-7000-8000-000000000099", "receiptToken": "x" },
            receipt(&got[2]),
            receipt(&got[0]),
            { "id": got[1]["id"] },
        ] }),
    )
    .await;
    assert_eq!(statuses(&results), [204, 409, 404, 204, 404, 400]);
    assert_eq!(results[0]["id"], got[0]["id"]);
    assert_eq!(results[1]["error"]["code"], "message.stale-receipt");
    assert_eq!(results[5]["error"]["code"], "validation.required");

    // Only the message 2 is left, and its receipt still acks it.
    let results = batch(
        &node,
        "/topics/orders/messages/ack",
        json!({ "messages": [receipt(&got[1])] }),
    )
    .await;
    assert_eq!(statuses(&results), [204]);
    let nowhere = post(
        &node,
        "/topics/nowhere/messages/ack",
        &json!({ "messages": [receipt(&got[1])] }),
    )
    .await;
    assert_eq!(error_code(nowhere, 404).await, "topic.not-found");
    node.shutdown().await;
}

/// UC: UC-VIS-013
#[tokio::test]
async fn a_batch_of_invisibility_changes_each_message_and_a_stale_item_fails_alone() {
    let node = TestNode::single().await;
    create(&node, json!({ "name": "orders", "type": "queue" })).await;
    batch(
        &node,
        "/topics/orders/messages/batch",
        json!({ "messages": [{ "body": 1 }, { "body": 2 }] }),
    )
    .await;
    let mut got = consume(&node, "orders", "?maxMessages=10&visibilityTimeout=120").await;
    got.sort_by_key(|m| m["body"].as_u64().unwrap());
    let results = batch(
        &node,
        "/topics/orders/messages/visibility",
        json!({ "messages": [
            { "id": got[0]["id"], "receiptToken": got[0]["receiptToken"], "visibilityTimeout": 0 },
            { "id": got[1]["id"], "receiptToken": "not-the-token", "visibilityTimeout": 0 },
            { "id": got[1]["id"], "receiptToken": got[1]["receiptToken"], "visibilityTimeout": -1 },
        ] }),
    )
    .await;
    assert_eq!(statuses(&results), [200, 409, 400]);
    assert_eq!(results[0]["id"], got[0]["id"]);
    assert!(results[0]["invisibleUntil"].is_string());
    assert_eq!(results[2]["error"]["code"], "validation.out-of-range");

    // Message 1 is visible again; message 2 is still invisible.
    let again = consume(&node, "orders", "?maxMessages=10").await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0]["id"], got[0]["id"]);
    node.shutdown().await;
}

/// UC: UC-ESC-041
#[tokio::test(flavor = "multi_thread")]
async fn a_batch_on_one_node_reaches_the_owners_of_every_message_and_travels_in_few_calls() {
    let cluster = TestCluster::start(3).await;
    let node = cluster.node(1);
    create(node, json!({ "name": "orders", "type": "queue" })).await;
    let count = 200;
    cluster.network.clear_calls();
    let results = batch(
        node,
        "/topics/orders/messages/batch",
        json!({ "messages": (0..count).map(|n| json!({ "body": n })).collect::<Vec<_>>() }),
    )
    .await;
    assert!(statuses(&results).iter().all(|s| *s == 201));
    let operations =
        cluster.network.count("write-register") + cluster.network.count("write-commit");
    let calls = cluster.network.count("batch");
    assert!(
        operations >= 2 * count,
        "each publication registers and commits on other nodes: {operations}"
    );
    // The in-memory network has no latency, so the channel joins less than over TCP (UC-ESC-039).
    assert!(
        calls < operations,
        "{operations} operations went in {calls} calls"
    );

    // Another node takes everything and a third acks it in one request.
    let mut taken = Vec::new();
    let mut seen = HashSet::new();
    while taken.len() < count {
        let got = consume(cluster.node(2), "orders", "?maxMessages=100&waitTime=1").await;
        assert!(!got.is_empty(), "the queue emptied at {}", taken.len());
        for message in got {
            assert!(seen.insert(message["id"].as_str().unwrap().to_owned()));
            taken.push(json!({ "id": message["id"], "receiptToken": message["receiptToken"] }));
        }
    }
    let results = batch(
        cluster.node(3),
        "/topics/orders/messages/ack",
        json!({ "messages": taken }),
    )
    .await;
    assert!(statuses(&results).iter().all(|s| *s == 204), "{results:?}");
    assert!(
        consume(cluster.node(1), "orders", "?maxMessages=10")
            .await
            .is_empty()
    );
    cluster.shutdown().await;
}
