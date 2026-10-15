//! The new-message notices between the nodes (doc 13, section 12.8; doc 20b, level **C**): the
//! topic owner, the window that groups the ready ones into a single notice, the wait for the `take`, the
//! pass to the next node with a listener and the result of the notice on the node that receives it.

mod common;

use std::time::Duration;

use common::cluster::{
    TestCluster, create_queue, eventually, eventually_async, settled, signals_of, total,
};
use serde_json::json;
use taba_node::cluster::protocol::{Signal, SignalFrame};

async fn publish(cluster: &TestCluster, topic: &str, count: usize) {
    for n in 0..count {
        let node = cluster.node(1);
        let response = node
            .client
            .post(node.client_url(&format!("/topics/{topic}/messages")))
            .json(&json!({ "n": n }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
}

/// The notices of type `outcome` in the direction `direction` that the node has counted so far.
async fn notices(cluster: &TestCluster, number: usize, direction: &str, outcome: &str) -> f64 {
    let text = cluster.node(number).metrics().await;
    total(
        &text,
        "taba_cluster_channel_notifications_total",
        &format!("taba_direction=\"{direction}\",taba_outcome=\"{outcome}\""),
    )
}

/// The nodes tell everyone that they have a listener on the topic (without a real `GET`: no one comes to fetch).
async fn silent_listeners(
    cluster: &TestCluster,
    topic: &str,
    numbers: &[usize],
) -> Vec<impl Drop + use<>> {
    let guards: Vec<_> = numbers
        .iter()
        .map(|n| signals_of(cluster, *n).listener(topic))
        .collect();
    for number in 1..=cluster.nodes.len() {
        let signals = signals_of(cluster, number);
        let topic = topic.to_owned();
        // The node itself does not keep its own interest in the others' table.
        let expected: Vec<String> = numbers
            .iter()
            .filter(|n| **n != number)
            .map(|n| cluster.node_id(*n))
            .collect();
        eventually("the interest to reach all the nodes", move || {
            let known = signals.listener_nodes(&topic);
            expected.iter().all(|id| known.contains(id))
        })
        .await;
    }
    guards
}

/// UC: UC-CAN-015
#[tokio::test]
async fn the_ready_messages_of_a_window_become_one_notice_with_the_quantity() {
    let cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_CLUSTER_NOTIFY_WINDOW", "1500ms"),
            ("TABA_CLUSTER_NOTIFY_WAIT", "50ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "avisos").await;
    settled(&cluster).await;
    let _listener = silent_listeners(&cluster, "avisos", &[2]).await;

    publish(&cluster, "avisos", 24).await;

    let signals = signals_of(&cluster, 2);
    eventually("the notice to reach node 2", {
        let signals = signals.clone();
        move || signals.last_notice_count("avisos").is_some()
    })
    .await;
    // Each owner (nodes 1 and 3) sent a single notice, with the window's ready ones and not one.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let received = notices(&cluster, 2, "in", "received").await;
    assert!(received <= 2.0, "one notice per owner, got {received}");
    let count = signals.last_notice_count("avisos").unwrap();
    assert!(count > 1, "the notice carries the count, got {count}");
    cluster.shutdown().await;
}

/// UC: UC-CAN-016
#[tokio::test]
async fn the_notice_goes_to_one_listener_at_a_time_and_passes_on_when_nobody_comes() {
    let cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_CLUSTER_NOTIFY_WINDOW", "20ms"),
            ("TABA_CLUSTER_NOTIFY_WAIT", "700ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "avisos").await;
    settled(&cluster).await;
    let _listeners = silent_listeners(&cluster, "avisos", &[2, 3]).await;

    // With enough messages, node 1 owns some and opens its cycle.
    publish(&cluster, "avisos", 30).await;

    eventually_async("node 1's first notice", || async {
        notices(&cluster, 1, "out", "sent").await >= 1.0
    })
    .await;
    // One node at a time: while the first has time to come fetch, the second is not notified.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(notices(&cluster, 1, "out", "sent").await, 1.0);
    // No one came: it moves on to the next, and with the nodes run out the cycle ends.
    eventually_async("the notice to the second node", || async {
        notices(&cluster, 1, "out", "sent").await >= 2.0
    })
    .await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(notices(&cluster, 1, "out", "sent").await, 2.0);
    cluster.shutdown().await;
}

/// UC: UC-CAN-017
#[tokio::test]
async fn a_notice_is_a_hint_that_ends_fetched_or_empty() {
    let cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_CLUSTER_NOTIFY_WINDOW", "20ms"),
            ("TABA_CLUSTER_NOTIFY_WAIT", "300ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "avisos").await;
    settled(&cluster).await;
    let query = "waitTime=5&maxMessages=1&visibilityTimeout=60";

    // Empty: node 1's notice (and ready list) is old, and the `take` comes back with nothing.
    let waiting = {
        let node = cluster.node(2);
        let client = node.client.clone();
        let url = node.client_url(&format!("/topics/avisos/messages?{query}"));
        tokio::spawn(async move { client.get(url).send().await })
    };
    let signals = signals_of(&cluster, 2);
    eventually("node 2's listener", {
        let signals = signals.clone();
        move || signals.local_listener_count("avisos") == 1
    })
    .await;
    let from = cluster.node_id(1);
    signals.receive(SignalFrame {
        from: from.clone(),
        signal: Signal::Ready {
            topic: "avisos".into(),
            has_ready: true,
        },
    });
    signals.receive(SignalFrame {
        from,
        signal: Signal::Notify {
            topic: "avisos".into(),
            count: 1,
        },
    });
    eventually_async("the old notice counted as empty", || async {
        notices(&cluster, 2, "in", "empty").await >= 1.0
    })
    .await;
    waiting.abort();

    // Fetched: a `GET` waits on node 2 and the ready message is from another node. If the owner is node 2
    // itself the message does not travel, so it repeats until an outside owner.
    for _ in 0..20 {
        if notices(&cluster, 2, "in", "fetched").await >= 1.0 {
            break;
        }
        let node = cluster.node(2);
        let client = node.client.clone();
        let url = node.client_url(&format!("/topics/avisos/messages?{query}"));
        let get = tokio::spawn(async move { client.get(url).send().await.unwrap().status() });
        eventually("node 2's listener", {
            let signals = signals.clone();
            move || signals.local_listener_count("avisos") == 1
        })
        .await;
        publish(&cluster, "avisos", 1).await;
        assert_eq!(get.await.unwrap(), 200);
    }
    assert!(notices(&cluster, 2, "in", "fetched").await >= 1.0);
    cluster.shutdown().await;
}
