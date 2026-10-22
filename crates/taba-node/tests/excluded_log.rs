//! The log of excluded messages in the simulated cluster (doc 07, section 10.3; level **C** of doc 20): every
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

/// A node that joins copies, block by block, the log its sources recorded, and records from when they
/// record (the latest start among them), not from its own birth.
/// UC: UC-SIN-031
#[tokio::test]
async fn a_new_node_copies_the_log_of_its_sources_and_inherits_their_start() {
    let mut cluster = TestCluster::start(2).await;
    create_queue(cluster.node(1), json!({"name": "orders", "type": "queue"})).await;
    publish(cluster.node(1), "orders").await;
    let message = consume_one(cluster.node(2), "orders").await;
    let id = message["id"].as_str().unwrap().to_owned();
    let token = message["receiptToken"].as_str().unwrap().to_owned();
    let ack = cluster
        .node(1)
        .client
        .delete(cluster.node(1).client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    for number in 1..=2 {
        let messages = messages_of(cluster.node(number));
        let wanted = id.clone();
        eventually("the exclusion on the sources", move || {
            messages.is_excluded("orders", &wanted)
        })
        .await;
    }
    let sources_since = messages_of(cluster.node(1)).excluded_since();

    cluster.clock.advance(Duration::from_secs(3600));
    cluster.add_node(false).await;

    let newcomer = messages_of(cluster.node(3));
    let wanted = id.clone();
    eventually("the exclusion copied to the new node", {
        let newcomer = newcomer.clone();
        move || newcomer.is_excluded("orders", &wanted)
    })
    .await;
    assert_eq!(
        newcomer.excluded_since(),
        sources_since,
        "the new node records from when its sources do, not from the hour it joined"
    );
    cluster.shutdown().await;
}

/// The cluster's cut-off is the most recent start among the active members; the operator reads it in
/// `GET /health/cluster` (`summary.excludedCutoff`) and each node's own in its `NodeStatus`.
/// UC: UC-SIN-032
/// UC: UC-OPS-052
#[tokio::test]
async fn the_cluster_cut_off_is_the_latest_start_among_the_active_members() {
    let cluster = TestCluster::start(3).await;
    let first = messages_of(cluster.node(1)).excluded_since();
    let later = first + 5_000;
    // The seed never synchronized, so it has not inherited yet: it takes the later date.
    assert!(
        messages_of(cluster.node(1))
            .inherit_excluded_since(later)
            .unwrap()
    );

    let view = cluster.node(2).stats("/health/cluster").await;
    let wanted = taba_node::core::time::format_ms(later);
    assert_eq!(view["summary"]["excludedCutoff"], wanted);
    let nodes = view["nodes"].as_array().unwrap();
    let seed = nodes
        .iter()
        .find(|n| n["status"]["excludedSince"] == wanted)
        .expect("the seed shows its own start");
    assert_eq!(seed["reachable"], true);
    assert!(
        nodes
            .iter()
            .all(|n| n["status"]["excludedSince"].as_str().is_some()),
        "every node shows its start"
    );
    cluster.shutdown().await;
}
