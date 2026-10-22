//! The log of excluded messages in the simulated cluster (doc 37, section 4.4; level **C** of doc 20): every
//! node that keeps a copy records the exclusion when the message is acknowledged or sent to the dead-letter.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, eventually};

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

async fn create_queue(node: &TestNode, body: Value) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
}

async fn publish(node: &TestNode, topic: &str) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&json!({"x": 1}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
    let value: Value = response.json().await.unwrap();
    value["id"].as_str().unwrap().to_owned()
}

/// Consumes one message from the node, waiting for it to be ready.
async fn consume_one(node: &TestNode, topic: &str) -> Value {
    for _ in 0..40 {
        let response = node
            .client
            .get(node.client_url(&format!(
                "/topics/{topic}/messages?maxMessages=1&visibilityTimeout=60"
            )))
            .send()
            .await
            .expect("request");
        let value: Value = response.json().await.unwrap();
        if let Some(message) = value["messages"].as_array().and_then(|m| m.first()) {
            return message.clone();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("no message was delivered");
}

/// The ack through any node: the primary and the replicas each record the exclusion; the id of a message
/// that was never acknowledged is not recorded.
/// UC: UC-ESC-037
#[tokio::test]
async fn an_ack_records_the_exclusion_on_every_node_that_keeps_a_copy() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), json!({"name": "orders", "type": "queue"})).await;
    let acked = publish(cluster.node(1), "orders").await;
    let kept = publish(cluster.node(1), "orders").await;
    for number in 1..=3 {
        assert!(!messages_of(cluster.node(number)).is_excluded("orders", &acked));
    }

    let message = consume_one(cluster.node(2), "orders").await;
    let (id, token) = (
        message["id"].as_str().unwrap().to_owned(),
        message["receiptToken"].as_str().unwrap().to_owned(),
    );
    let ack = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);

    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let wanted = id.clone();
        eventually("the exclusion in the log", move || {
            messages.is_excluded("orders", &wanted)
        })
        .await;
        let other = if id == acked { &kept } else { &acked };
        assert!(
            !messages_of(cluster.node(number)).is_excluded("orders", other),
            "the other message was not acknowledged"
        );
    }
    cluster.shutdown().await;
}

/// The move to the dead-letter: the id is dead in the source queue (on every copy), alive in the dead-letter.
/// UC: UC-ESC-038
#[tokio::test]
async fn the_dead_letter_records_the_exclusion_in_the_source_queue_on_every_node() {
    let cluster = TestCluster::start(3).await;
    create_queue(
        cluster.node(1),
        json!({"name": "orders", "type": "queue", "maxDeliveries": 1}),
    )
    .await;
    let id = publish(cluster.node(1), "orders").await;
    consume_one(cluster.node(2), "orders").await;
    cluster.clock.advance(Duration::from_secs(61));

    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let wanted = id.clone();
        eventually("the exclusion of the source queue", move || {
            messages.is_excluded("orders", &wanted)
        })
        .await;
        let messages = messages_of(cluster.node(number));
        assert!(
            !messages.is_excluded("orders-dead-letter", &id),
            "in the dead-letter it is alive"
        );
    }
    cluster.shutdown().await;
}
