//! The scan of S3 at the start (doc 33, the scan at the start; level **C**): the last step of the restoration of a node brings
//! back the messages the bucket holds alive of the blocks the node is primary of. The object store is the in-memory
//! one (`MemoryStore`), the `Clock` is the manual one, and the test loses the messages of the nodes (not the bucket)
//! to see what the scan alone brings.

mod common;

use std::sync::Arc;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, create_queue, eventually, settled};
use taba_node::config::DbSynchronous;
use taba_node::storage::backup::MemoryStore;
use taba_node::storage::messages::{ExcludedRow, MessageStore};

const QUEUE: &str = "orders";

fn messages_of(node: &TestNode) -> Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

async fn publish(node: &TestNode, n: u64) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{QUEUE}/messages")))
        .json(&json!({ "n": n }))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
    let value: Value = response.json().await.unwrap();
    value["id"].as_str().unwrap().to_owned()
}

async fn flush(cluster: &TestCluster) {
    let node = cluster.node(1);
    let response = node
        .client
        .post(node.operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let report: Value = response.json().await.unwrap();
    assert_eq!(report["complete"], true, "{report}");
}

/// Whether some node holds the message.
fn has(cluster: &TestCluster, id: &str) -> bool {
    (1..=cluster.nodes.len()).any(|n| {
        messages_of(cluster.node(n))
            .message_info(QUEUE, id)
            .unwrap()
            .is_some_and(|info| !info.meta.content_type.is_empty())
    })
}

/// The nodes lose the messages they keep (memory and disk) and the bucket stays as it is.
fn lose_messages(cluster: &TestCluster) {
    for number in 1..=cluster.nodes.len() {
        messages_of(cluster.node(number)).reset_all();
    }
}

/// The scan of each node, as the last step of its restoration.
async fn scan_every_node(cluster: &TestCluster) {
    for number in 1..=cluster.nodes.len() {
        let recovery = cluster
            .node(number)
            .node
            .cluster()
            .and_then(|c| c.replication())
            .map(|r| r.recovery.clone())
            .expect("the node is in the cluster");
        let summary = recovery.restore_lives().await;
        assert!(summary.done, "the pass reached its end");
    }
}

/// Consumes one message and deletes it with its receipt token; returns its ID.
async fn consume_and_ack(node: &TestNode) -> String {
    let response = node
        .client
        .get(node.client_url(&format!(
            "/topics/{QUEUE}/messages?maxMessages=1&visibilityTimeout=600"
        )))
        .send()
        .await
        .unwrap();
    let value: Value = response.json().await.unwrap();
    let message = value["messages"][0].clone();
    let (id, token) = (
        message["id"].as_str().unwrap().to_owned(),
        message["receiptToken"].as_str().unwrap().to_owned(),
    );
    let response = node
        .client
        .delete(node.client_url(&format!(
            "/topics/{QUEUE}/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    id
}

/// A cluster of three with S3 and a queue with `count` messages, all in the bucket.
async fn setup(s3: &Arc<MemoryStore>, count: u64) -> (TestCluster, Vec<String>) {
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    settled(&cluster).await;
    create_queue(cluster.node(1), QUEUE).await;
    let mut ids = Vec::new();
    for n in 0..count {
        ids.push(publish(cluster.node(1), n).await);
    }
    flush(&cluster).await;
    (cluster, ids)
}

/// UC: UC-BKP-041
#[tokio::test]
async fn the_scan_brings_back_the_messages_of_the_bucket_that_no_node_has() {
    let s3 = Arc::new(MemoryStore::new());
    let (cluster, ids) = setup(&s3, 20).await;
    lose_messages(&cluster);
    assert!(ids.iter().all(|id| !has(&cluster, id)), "nothing left");

    scan_every_node(&cluster).await;
    eventually("every message back from the bucket", || {
        ids.iter().all(|id| has(&cluster, id))
    })
    .await;

    // A second pass finds everything where it is: it skips what the node has and adds nothing.
    scan_every_node(&cluster).await;
    let ready: u64 = (1..=3)
        .map(|n| messages_of(cluster.node(n)).ready_count_of(QUEUE))
        .sum();
    assert!(ready >= 20, "the messages are ready to be consumed");
    cluster.shutdown().await;
}

/// UC: UC-BKP-042
#[tokio::test]
async fn the_scan_discards_what_the_journal_removed_and_what_the_log_says_is_dead() {
    let s3 = Arc::new(MemoryStore::new());
    let (cluster, ids) = setup(&s3, 12).await;
    // One message is consumed and deleted and the flush takes the removal to the journal; another one is deleted
    // **after** that flush: the bucket still has it alive, with no removal in the journal, but the log of excluded
    // messages of the nodes records the ack.
    let in_journal = consume_and_ack(cluster.node(1)).await;
    flush(&cluster).await;
    let consumed = consume_and_ack(cluster.node(1)).await;
    let position = messages_of(cluster.node(1)).position_for(QUEUE, &consumed, None);
    lose_messages(&cluster);
    // Losing the messages also loses the log of the topic: each node gets back the record of the ack, as the log
    // of a node that kept it would have it.
    for number in 1..=3 {
        let disk = MessageStore::new(
            &cluster.node(number).dir.path().join("taba"),
            DbSynchronous::Off,
        );
        disk.set_topic_id_resolver(Box::new(|_| None));
        disk.import_excluded(
            QUEUE,
            &[ExcludedRow {
                id: consumed.clone(),
                position,
                at_ms: 1,
            }],
        )
        .expect("writes the log");
    }

    scan_every_node(&cluster).await;
    let others: Vec<&String> = ids
        .iter()
        .filter(|id| **id != consumed && **id != in_journal)
        .collect();
    eventually("the live messages back", || {
        others.iter().all(|id| has(&cluster, id))
    })
    .await;
    assert!(
        !has(&cluster, &consumed),
        "the bucket is not proof of life: the log of the nodes says it is dead"
    );
    assert!(!has(&cluster, &in_journal), "the journal removed it");
    cluster.shutdown().await;
}

/// UC: UC-BKP-043
#[tokio::test]
async fn with_s3_down_the_scan_is_skipped_and_the_node_goes_on_with_its_disk() {
    let s3 = Arc::new(MemoryStore::new());
    let (cluster, ids) = setup(&s3, 6).await;
    s3.set_down(true);
    lose_messages(&cluster);

    // It does not wait for the bucket and does not fail: the pass ends.
    scan_every_node(&cluster).await;
    assert!(ids.iter().all(|id| !has(&cluster, id)), "nothing came back");
    // The node keeps serving.
    let id = publish(cluster.node(1), 99).await;
    assert!(has(&cluster, &id));
    cluster.shutdown().await;
}

/// UC: UC-BKP-044
#[tokio::test]
async fn the_scan_reads_the_journal_of_a_topic_once_and_keeps_it_for_the_next_pass() {
    let s3 = Arc::new(MemoryStore::new());
    let (cluster, _) = setup(&s3, 3).await;
    for _ in 0..3 {
        consume_and_ack(cluster.node(1)).await;
    }
    flush(&cluster).await;
    let journals = s3
        .keys(&format!(
            "taba/{}",
            taba_node::storage::backup::keys::topic_journal_prefix(QUEUE)
        ))
        .len() as u64;
    assert!(journals > 0, "the removals are in the journal");

    let recovery = cluster
        .node(1)
        .node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.recovery.clone())
        .expect("the node is in the cluster");
    let before = s3.gets();
    recovery.restore_lives().await;
    let first = s3.gets() - before;
    let before = s3.gets();
    recovery.restore_lives().await;
    let second = s3.gets() - before;
    assert_eq!(
        first - second,
        journals,
        "the second pass found the journal in the cache"
    );
    cluster.shutdown().await;
}
