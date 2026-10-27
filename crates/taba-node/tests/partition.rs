//! R1 of doc 37: a partition with enough healthy nodes on both sides, so each side excludes the other and goes on
//! accepting writes. When the sides meet again the one that loses must not wipe what it accepted while apart
//! (PRB-078): its disk becomes the folder of a life and the restoration sends it to the merged cluster.

mod common;

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::{TestCluster, create_queue, eventually, host_of, settled};

const QUEUE: &str = "split";

/// The reunion check and the probe run every second (real time) instead of every `keepalive × 10`.
const ENV: &[(&str, &str)] = &[
    ("TABA_CLUSTER_REUNION_INTERVAL", "1s"),
    ("TABA_CLUSTER_RESTORE_PROBE_INTERVAL", "200ms"),
    ("TABA_CLUSTER_RESTORE_COLD_START_WAIT", "300ms"),
];

async fn publish(cluster: &TestCluster, number: usize, seq: u64) -> Option<String> {
    let node = cluster.node(number);
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{QUEUE}/messages")))
        .json(&json!({"seq": seq}))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if response.status() != 201 {
        return None;
    }
    response.json::<Value>().await.ok()?["id"]
        .as_str()
        .map(str::to_owned)
}

fn cut(cluster: &TestCluster, up: bool) {
    for a in [1, 2] {
        for b in [3, 4] {
            if up {
                cluster.network.heal(&host_of(a), &host_of(b));
            } else {
                cluster.network.partition(&host_of(a), &host_of(b));
            }
        }
    }
}

/// Receives and acks what it finds on the four nodes; returns how many times each `seq` came.
async fn drain(cluster: &TestCluster, delivered: &mut HashMap<u64, u32>) -> usize {
    let mut got = 0;
    for n in 1..=4 {
        let node = cluster.node(n);
        let Ok(response) = node
            .client
            .get(node.client_url(&format!(
                "/topics/{QUEUE}/messages?maxMessages=10&visibilityTimeout=30"
            )))
            .timeout(Duration::from_secs(5))
            .send()
            .await
        else {
            continue;
        };
        if response.status() != 200 {
            continue;
        }
        let body: Value = response.json().await.unwrap_or_default();
        for m in body["messages"].as_array().cloned().unwrap_or_default() {
            got += 1;
            if let Some(seq) = m["body"]["seq"].as_u64() {
                *delivered.entry(seq).or_default() += 1;
            }
            let (id, token) = (
                m["id"].as_str().unwrap_or_default(),
                m["receiptToken"].as_str().unwrap_or_default(),
            );
            let _ = node
                .client
                .delete(node.client_url(&format!(
                    "/topics/{QUEUE}/messages/{id}?receiptToken={token}"
                )))
                .send()
                .await;
        }
    }
    got
}

/// UC: UC-FAL-026
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_half_that_loses_the_reunion_keeps_what_it_accepted_while_apart() {
    let cluster = TestCluster::start_with(4, ENV).await;
    settled(&cluster).await;
    create_queue(cluster.node(1), QUEUE).await;

    cut(&cluster, false);
    eventually("each half excludes the other", || {
        cluster.clock.advance(Duration::from_secs(5));
        (1..=4).all(|n| cluster.member_ids(n).len() == 2)
    })
    .await;

    // Both halves keep accepting writes: nodes 1 and 2 on one side, 3 and 4 on the other.
    let mut accepted = HashMap::new();
    for seq in 0..20u64 {
        let number = (seq % 4) as usize + 1;
        if let Some(id) = publish(&cluster, number, seq).await {
            accepted.insert(seq, id);
        }
    }
    assert_eq!(accepted.len(), 20, "both halves accept the writes");

    cut(&cluster, true);
    let mut merged = false;
    for _ in 0..400 {
        cluster.clock.advance(Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(100)).await;
        if (1..=4).all(|n| cluster.member_ids(n).len() == 4) {
            merged = true;
            break;
        }
    }
    assert!(merged, "the halves became one cluster of four nodes");

    // The restoration of the life runs in the background: receive until every message has come.
    let mut delivered: HashMap<u64, u32> = HashMap::new();
    for _ in 0..300 {
        drain(&cluster, &mut delivered).await;
        if delivered.len() == accepted.len() {
            break;
        }
        cluster.clock.advance(Duration::from_secs(1));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut lost: Vec<_> = accepted
        .keys()
        .filter(|seq| !delivered.contains_key(seq))
        .collect();
    lost.sort();
    assert!(lost.is_empty(), "accepted and never delivered: {lost:?}");
    let duplicated: Vec<_> = delivered.iter().filter(|(_, c)| **c > 1).collect();
    assert!(duplicated.is_empty(), "delivered twice: {duplicated:?}");
}
