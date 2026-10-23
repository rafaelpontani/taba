//! The optional S3 backup (doc 23, "S3 backup"; doc 20, level **C**): the object store is the
//! in-memory one (`MemoryStore`), shared among the nodes of the simulated cluster, and the `Clock` is the
//! manual one (the `backup.delay` window passes when the test advances the clock).

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::{TestCluster, eventually, eventually_async};
use common::{TestNode, error_code};
use taba_node::storage::backup::{MemoryStore, ObjectStore, keys};

fn backup_of(node: &TestNode) -> Arc<taba_node::cluster::backup::Backup> {
    node.node.backup().expect("the backup is on").clone()
}

async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
}

async fn publish(node: &TestNode, topic: &str, body: Value) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    let value: Value = response.json().await.unwrap();
    value["id"].as_str().unwrap().to_owned()
}

async fn consume(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    let response = node
        .client
        .get(node.client_url(&format!("/topics/{topic}/messages?{query}")))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 200, "{:?}", response.text().await);
    let value: Value = response.json().await.unwrap();
    value["messages"].as_array().cloned().unwrap_or_default()
}

/// Like `consume`, but an error response (the quorum has not closed yet) is "nothing for now".
async fn try_consume(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    let response = node
        .client
        .get(node.client_url(&format!("/topics/{topic}/messages?{query}")))
        .send()
        .await
        .expect("request");
    if response.status() != 200 {
        return Vec::new();
    }
    let value: Value = response.json().await.unwrap();
    value["messages"].as_array().cloned().unwrap_or_default()
}

async fn ack(node: &TestNode, topic: &str, message: &Value) {
    let id = message["id"].as_str().unwrap();
    let token = message["receiptToken"].as_str().unwrap();
    let response = node
        .client
        .delete(node.client_url(&format!(
            "/topics/{topic}/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 204, "{:?}", response.text().await);
}

/// How many messages the bucket holds in the topic's segments (adding up the footers).
async fn messages_in_bucket(s3: &MemoryStore, topic: &str) -> usize {
    let mut total = 0;
    for key in s3.keys(&format!("taba/{}", keys::topic_messages_prefix(topic))) {
        let body = s3.get(&key).await.unwrap().unwrap().body;
        total += taba_node::storage::backup::segment::decode_all(&body)
            .unwrap()
            .1
            .len();
    }
    total
}

fn outbox_total(cluster: &TestCluster) -> u64 {
    cluster
        .nodes
        .iter()
        .map(|n| backup_of(n).status().outbox_pending)
        .sum()
}

/// UC: UC-BKP-001
#[tokio::test]
async fn without_a_bucket_the_backup_does_not_exist_and_its_routes_say_so() {
    let node = TestNode::single().await;
    assert!(node.node.backup().is_none());
    for (method, path) in [
        ("GET", "/node/backup"),
        ("POST", "/node/backup/flush"),
        ("POST", "/cluster/backup/flush"),
    ] {
        let request = match method {
            "GET" => node.client.get(node.operations_url(path)),
            _ => node.client.post(node.operations_url(path)),
        };
        let response = request.send().await.unwrap();
        assert_eq!(
            error_code(response, 422).await,
            "backup.not-configured",
            "{path}"
        );
    }
    node.shutdown().await;
}

/// UC: UC-BKP-009, UC-BKP-010, UC-BKP-011, UC-BKP-012
#[tokio::test]
async fn a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..10 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    eventually("the owners' outbox to have the 10 messages", || {
        outbox_total(&cluster) == 10
    })
    .await;
    assert_eq!(
        messages_in_bucket(&s3, "orders").await,
        0,
        "nothing goes up before the window"
    );

    // The consumer confirms 6 within the window: they leave the outbox without generating an upload.
    let got = consume(
        cluster.node(1),
        "orders",
        "maxMessages=6&visibilityTimeout=300",
    )
    .await;
    assert_eq!(got.len(), 6);
    for message in &got {
        ack(cluster.node(1), "orders", message).await;
    }
    eventually("the 6 confirmed to leave the outbox", || {
        outbox_total(&cluster) == 4
    })
    .await;

    // The window closes: the remaining 4 go to S3, and only after the PUT do they leave the outbox.
    cluster.clock.advance(Duration::from_secs(61));
    eventually("o outbox esvaziar", || outbox_total(&cluster) == 0).await;
    assert_eq!(messages_in_bucket(&s3, "orders").await, 4);
    cluster.shutdown().await;
}

/// UC: UC-BKP-009, UC-BKP-012
#[tokio::test]
async fn publishing_never_waits_for_s3_and_the_outbox_keeps_the_ids_while_it_is_down() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    s3.set_down(true);
    for i in 0..5 {
        publish(cluster.node(2), "orders", json!({"n": i})).await;
    }
    cluster.clock.advance(Duration::from_secs(61));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        outbox_total(&cluster),
        5,
        "S3 down: the IDs stay in the outbox"
    );
    let consumed = consume(cluster.node(3), "orders", "maxMessages=5").await;
    assert_eq!(consumed.len(), 5, "consumption goes on as without S3");

    s3.set_down(false);
    eventually("the outbox to empty when S3 comes back", || {
        outbox_total(&cluster) == 0
    })
    .await;
    assert_eq!(messages_in_bucket(&s3, "orders").await, 5);
    cluster.shutdown().await;
}

/// UC: UC-BKP-016, UC-BKP-018
#[tokio::test]
async fn definitions_and_tokens_go_to_the_bucket_and_a_failure_never_refuses_the_change() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    eventually("the definition in the bucket", || {
        s3.peek("taba/topics/orders.json").is_some()
    })
    .await;
    let stored: Value =
        serde_json::from_slice(&s3.peek("taba/topics/orders.json").unwrap()).unwrap();
    assert_eq!(stored["definition"]["name"], "orders");

    // S3 down: the creation takes effect in the cluster, and the write waits for the return.
    s3.set_down(true);
    create_queue(cluster.node(2), "faturas").await;
    assert!(s3.peek("taba/topics/faturas.json").is_none());
    s3.set_down(false);
    eventually("the definition that failed to be rewritten", || {
        s3.peek("taba/topics/faturas.json").is_some()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-017
#[tokio::test]
async fn removing_a_topic_leaves_a_tombstone_and_clears_its_messages_from_the_bucket() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    publish(cluster.node(1), "orders", json!({"n": 1})).await;
    cluster.clock.advance(Duration::from_secs(61));
    eventually("the message in the bucket", || {
        !s3.keys("taba/messages/orders/").is_empty()
    })
    .await;
    let response = cluster
        .node(1)
        .client
        .delete(cluster.node(1).client_url("/topics/orders"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    eventually("the tombstone and the deleted prefix", || {
        s3.keys("taba/messages/orders/").is_empty()
            && s3
                .peek("taba/topics/orders.json")
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .is_some_and(|v| v["deleted"] == true)
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-019
#[tokio::test]
async fn every_node_beats_in_the_bucket_with_a_growing_sequence_and_leaves_on_drain() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let id = cluster.node_id(2);
    let key = format!("taba/nodes/{id}.json");
    eventually("node 2's heartbeat", || s3.peek(&key).is_some()).await;
    let first: Value = serde_json::from_slice(&s3.peek(&key).unwrap()).unwrap();
    assert_eq!(first["id"], id.as_str());
    assert!(first["addresses"].as_array().is_some_and(|a| !a.is_empty()));
    let seq = first["seq"].as_u64().unwrap();
    eventually("o seq crescer", || {
        s3.peek(&key)
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|v| v["seq"].as_u64().unwrap() > seq)
    })
    .await;

    // A node that leaves cleanly (drain) removes its own file.
    let response = cluster
        .node(2)
        .client
        .post(cluster.node(2).operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    eventually("node 2's file to disappear", || s3.peek(&key).is_none()).await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-032
#[tokio::test]
async fn the_node_flush_sends_everything_without_stopping_the_node() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..6 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    eventually("the outbox to have the 6", || outbox_total(&cluster) == 6).await;

    // Without waiting for the window: the flush sends everything now.
    for number in 1..=3 {
        let response = cluster
            .node(number)
            .client
            .post(cluster.node(number).operations_url("/node/backup/flush"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let report: Value = response.json().await.unwrap();
        assert_eq!(report["complete"], true, "{report}");
        assert_eq!(report["outboxPending"], 0);
    }
    assert_eq!(messages_in_bucket(&s3, "orders").await, 6);
    // The node stays up and serves.
    assert_eq!(
        consume(cluster.node(1), "orders", "maxMessages=6")
            .await
            .len(),
        6
    );
    cluster.shutdown().await;
}

/// UC: UC-BKP-033, UC-BKP-038
#[tokio::test]
async fn the_cluster_flush_asks_every_node_and_a_slow_one_shows_up_as_pending() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..4 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    let response = cluster
        .node(2)
        .client
        .post(cluster.node(2).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let report: Value = response.json().await.unwrap();
    assert_eq!(report["complete"], true, "{report}");
    assert_eq!(report["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(messages_in_bucket(&s3, "orders").await, 4);
    eventually_async("the status to show everything sent", || async {
        let status: Value = cluster
            .node(1)
            .client
            .get(cluster.node(1).operations_url("/node/backup"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        status["pending"] == 0
    })
    .await;
    cluster.shutdown().await;
}

async fn stop_cluster(cluster: &TestCluster) -> Value {
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/stop"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

/// UC: UC-BKP-023, UC-BKP-026, UC-BKP-031, UC-BKP-034, UC-BKP-035
#[tokio::test]
async fn a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..8 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    // 3 confirmed before the stop: they cannot come back.
    let got = consume(
        cluster.node(1),
        "orders",
        "maxMessages=3&visibilityTimeout=300",
    )
    .await;
    for message in &got {
        ack(cluster.node(1), "orders", message).await;
    }

    let stopped = stop_cluster(&cluster).await;
    assert_eq!(stopped["stopList"], true, "{stopped}");
    assert_eq!(stopped["stopped"].as_array().unwrap().len(), 3);
    let list: Value = serde_json::from_slice(&s3.peek("taba/cluster/stop.json").unwrap()).unwrap();
    assert_eq!(
        list["nodes"].as_array().unwrap().len(),
        3,
        "the stop's list"
    );
    assert!(
        s3.keys("taba/nodes/").is_empty(),
        "whoever leaves cleanly removes its own heartbeat"
    );
    for number in 1..=3 {
        assert_eq!(
            backup_of(cluster.node(number)).status().outbox_pending,
            0,
            "the drain only ends with the outbox sent"
        );
    }
    cluster.shutdown().await;

    // The processes start again (empty disk, new nodes): the first restores on its own.
    let again = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    eventually_async("the 5 messages to come back", || async {
        let got = consume(
            again.node(2),
            "orders",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await;
        got.len() == 5
    })
    .await;
    assert!(
        s3.peek("taba/restore/lock.json").is_none(),
        "the lock is released at the end of the restoration"
    );
    assert!(
        s3.peek("taba/cluster/stop.json").is_none(),
        "the stop's list disappears after being used"
    );
    again.shutdown().await;
}

/// UC: UC-BKP-022, UC-BKP-023, UC-BKP-026
#[tokio::test]
async fn after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..6 {
        publish(cluster.node(2), "orders", json!({"n": i})).await;
    }
    // The backup of a cluster that is going to fall: the flush before the fall.
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    cluster.shutdown().await; // without drain: the heartbeats stay in the bucket, stopped
    assert!(!s3.keys("taba/nodes/").is_empty(), "fall: the files stay");
    assert!(s3.peek("taba/cluster/stop.json").is_none());

    let again = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    eventually_async("the 6 messages to come back", || async {
        consume(
            again.node(2),
            "orders",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await
        .len()
            == 6
    })
    .await;
    again.shutdown().await;
}

/// The node really dies: leaves the network (no one reaches it anymore) and stops (the heartbeat in the bucket stops).
async fn kill_last(cluster: &mut TestCluster) {
    let number = cluster.nodes.len();
    cluster.network.isolate(&common::cluster::host_of(number));
    cluster.nodes.pop().unwrap().shutdown().await;
}

fn members_of(cluster: &TestCluster, number: usize) -> usize {
    cluster.table(number).map_or(0, |t| t.members.len())
}

/// UC: UC-BKP-020, UC-BKP-021
#[tokio::test]
async fn an_unreachable_node_with_a_fresh_heartbeat_is_not_excluded_until_the_heartbeat_expires() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;

    // Node 3 becomes unreachable, but keeps beating in the bucket: alive in another partition.
    cluster.network.isolate(&common::cluster::host_of(3));
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(
        members_of(&cluster, 1),
        3,
        "does not take over the blocks of whoever is alive"
    );
    let victim = cluster.node_id(3);
    assert_eq!(
        backup_of(cluster.node(1)).liveness(&victim),
        taba_node::cluster::backup::Liveness::Fresh
    );

    // Now it really dies: the heartbeat stops, expires, and only then does it leave the table.
    let dead = cluster.nodes.pop().unwrap();
    dead.shutdown().await;
    let died_at = std::time::Instant::now();
    eventually("node 3 to leave the table", || members_of(&cluster, 1) == 2).await;
    assert!(
        died_at.elapsed() >= Duration::from_millis(900),
        "the exclusion waited for the heartbeat to expire ({:?})",
        died_at.elapsed()
    );
    // The two survivors watch the bucket on their own: the observer that voted sees it dead
    // right after (there may be half a heartbeat of difference between the two reads).
    let observer = backup_of(cluster.node(1));
    eventually("the dead heartbeat for whoever observes", || {
        observer.liveness(&victim) == taba_node::cluster::backup::Liveness::Dead
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-022
#[tokio::test]
async fn three_of_five_nodes_fall_and_the_survivors_find_the_missing_messages_in_s3() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(5, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..30 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(messages_in_bucket(&s3, "orders").await, 30);

    // Three fall at the same time (without drain: the heartbeats stay stopped in the bucket).
    for _ in 0..3 {
        kill_last(&mut cluster).await;
    }
    eventually("the two survivors to exclude the dead", || {
        members_of(&cluster, 1) == 2 && members_of(&cluster, 2) == 2
    })
    .await;
    let seen = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    eventually_async("the 30 messages to be delivered again", || {
        let seen = seen.clone();
        let cluster = &cluster;
        async move {
            for message in try_consume(
                cluster.node(1),
                "orders",
                "maxMessages=10&visibilityTimeout=600",
            )
            .await
            {
                seen.lock()
                    .unwrap()
                    .insert(message["id"].as_str().unwrap().to_owned());
            }
            let delivered = seen.lock().unwrap().len();
            delivered == 30
        }
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-023
#[tokio::test]
async fn a_single_survivor_excludes_the_dead_even_below_the_minimum_of_voters() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;
    for _ in 0..2 {
        kill_last(&mut cluster).await;
    }
    eventually("the only node to exclude the two dead", || {
        members_of(&cluster, 1) == 1
    })
    .await;
    cluster.shutdown().await;
}

/// The records of all the topic's journals in the bucket, from oldest to newest.
async fn journal_records(
    s3: &MemoryStore,
    topic: &str,
) -> Vec<taba_node::storage::backup::journal::Record> {
    let mut out = Vec::new();
    for key in s3.keys(&format!("taba/{}", keys::topic_journal_prefix(topic))) {
        let body = s3.get(&key).await.unwrap().unwrap().body;
        out.extend(
            taba_node::storage::backup::journal::decode(&body)
                .unwrap()
                .1,
        );
    }
    out
}

async fn flush_all(cluster: &TestCluster) {
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/cluster/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let report: Value = response.json().await.unwrap();
    assert_eq!(report["complete"], true, "{report}");
}

/// UC: UC-BKP-013, UC-BKP-014
#[tokio::test]
async fn the_journal_keeps_the_last_state_and_an_ack_is_a_removal_that_empties_the_segment() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..3 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    flush_all(&cluster).await;
    assert!(!s3.keys("taba/messages/orders/").is_empty());

    // Delivered in the window: the journal keeps the message's last state.
    let first = consume(
        cluster.node(1),
        "orders",
        "maxMessages=1&visibilityTimeout=300",
    )
    .await;
    let id = first[0]["id"].as_str().unwrap().to_owned();
    eventually_async("the delivery state in the journal", || async {
        journal_records(&s3, "orders")
            .await
            .iter()
            .any(|r| r.id == id && !r.removed && r.delivery_count == 1)
    })
    .await;

    // The ack is a removal; no one deletes a loose message from the bucket.
    ack(cluster.node(1), "orders", &first[0]).await;
    eventually_async("the removal in the journal", || async {
        journal_records(&s3, "orders")
            .await
            .iter()
            .any(|r| r.id == id && r.removed)
    })
    .await;
    assert!(
        !s3.keys("taba/messages/orders/").is_empty(),
        "the segment with live messages stays in the bucket"
    );

    // When all of a segment's messages die, the primary deletes it.
    for message in consume(
        cluster.node(1),
        "orders",
        "maxMessages=5&visibilityTimeout=300",
    )
    .await
    {
        ack(cluster.node(1), "orders", &message).await;
    }
    eventually("the dead segments to leave the bucket", || {
        s3.keys("taba/messages/orders/").is_empty()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-015
#[tokio::test]
async fn volatile_queues_and_dead_letters_are_backed_up_too() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "efemera", "type": "queue", "volatile": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    publish(cluster.node(1), "efemera", json!({"v": 1})).await;

    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({"name": "orders", "type": "queue", "maxDeliveries": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    publish(cluster.node(1), "orders", json!({"n": 1})).await;
    let got = consume(
        cluster.node(1),
        "orders",
        "maxMessages=1&visibilityTimeout=1",
    )
    .await;
    assert_eq!(got.len(), 1);
    // The invisibility expires with the delivery limit spent: the message goes to the dead-letter.
    cluster.clock.advance(Duration::from_secs(5));
    eventually_async("the message in the dead-letter", || async {
        !try_consume(
            cluster.node(2),
            "orders-dead-letter",
            "maxMessages=1&visibilityTimeout=1",
        )
        .await
        .is_empty()
    })
    .await;
    flush_all(&cluster).await;
    assert!(
        !s3.keys("taba/messages/efemera/").is_empty(),
        "volatile queue"
    );
    assert!(
        !s3.keys("taba/messages/orders-dead-letter/").is_empty(),
        "dead-letter"
    );
    cluster.shutdown().await;
}

/// UC: UC-BKP-001
#[tokio::test]
async fn with_s3_down_at_start_the_nodes_start_with_a_warning_and_the_cluster_works() {
    let s3 = Arc::new(MemoryStore::new());
    s3.set_down(true);
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    publish(cluster.node(2), "orders", json!({"n": 1})).await;
    assert_eq!(
        consume(cluster.node(3), "orders", "maxMessages=1")
            .await
            .len(),
        1
    );
    let status: Value = cluster
        .node(1)
        .client
        .get(cluster.node(1).operations_url("/node/backup"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["s3Reachable"], false);
    // S3 comes back: the feature turns on by itself.
    s3.set_down(false);
    eventually("o batimento no bucket", || {
        !s3.keys("taba/nodes/").is_empty()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-036
#[tokio::test]
async fn the_primary_tells_the_replicas_which_messages_are_already_in_the_bucket() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..5 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    flush_all(&cluster).await;
    // Replication factor 3 on 3 nodes: all keep the 5 and, with the notice, all know where they are.
    for number in 1..=3 {
        let node = backup_of(cluster.node(number));
        let watching = node.clone();
        eventually("the replica to know where the 5 are", move || {
            watching.status().saved == 5
        })
        .await;
        // The reference carries the instant of the PUT (the primary's clock), on any replica.
        let first = node.db.saved_of_topic("orders").unwrap().remove(0);
        assert!(first.1.saved_at_ms > 0, "saved_at ausente");
        assert_eq!(
            node.saved_info("orders", &first.0).unwrap().saved_at_ms,
            first.1.saved_at_ms
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-037, UC-BKP-003
#[tokio::test]
async fn when_the_primary_dies_the_new_primary_sends_what_was_still_pending() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_DRAIN_ON_STOP", "false")];
    let mut cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    common::cluster::settled(&cluster).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..9 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    eventually("the owners' outbox", || outbox_total(&cluster) == 9).await;
    // Node 3 falls before sending what it had in the outbox.
    kill_last(&mut cluster).await;
    eventually("node 3 to leave the table", || members_of(&cluster, 1) == 2).await;
    // The heir adopts the messages when the table changes; its window counts from there.
    let s3_check = s3.clone();
    let clock = cluster.clock.clone();
    eventually_async("the heirs to send the 9", || {
        let s3 = s3_check.clone();
        clock.advance(Duration::from_secs(61));
        async move { messages_in_bucket(&s3, "orders").await == 9 }
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-028, UC-BKP-029
#[tokio::test]
async fn a_full_disk_evicts_the_bodies_already_in_s3_and_reading_one_goes_back_to_the_bucket() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    let mut ids = Vec::new();
    for i in 0..6 {
        ids.push(
            publish(
                cluster.node(1),
                "orders",
                json!({"n": i, "text": "x".repeat(200)}),
            )
            .await,
        );
    }
    // Everything to disk (step 1 of the flush) and to S3.
    flush_all(&cluster).await;
    let bodies = |number: usize| -> usize {
        let dir = cluster
            .node(number)
            .storage_dir()
            .join(taba_node::storage::messages::dir_name("orders"));
        ids.iter()
            .filter(|id| dir.join(format!("{id}.body")).is_file())
            .count()
    };
    // The flush only moved node 1's memory; the others also take it to disk.
    for number in 2..=3 {
        let response = cluster
            .node(number)
            .client
            .post(cluster.node(number).operations_url("/node/backup/flush"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }
    for number in 1..=3 {
        eventually("the replica to know where the 6 are", || {
            backup_of(cluster.node(number)).status().saved == 6
        })
        .await;
        assert_eq!(
            bodies(number),
            6,
            "before the eviction the bodies are on disk"
        );
    }

    // The disk goes over 85%: the body of what is already in S3 leaves the disk; the row stays.
    cluster.set_disk_usage(90);
    for number in 1..=3 {
        eventually("the bodies to leave the disk", || bodies(number) == 0).await;
    }
    assert!(backup_of(cluster.node(1)).status().bodies_evicted > 0);

    // The read finds the file missing and fetches the body from the segment by `Range`.
    cluster.set_disk_usage(10);
    let got = consume(
        cluster.node(2),
        "orders",
        "maxMessages=6&visibilityTimeout=300",
    )
    .await;
    assert_eq!(got.len(), 6);
    for message in &got {
        assert_eq!(message["body"]["text"], "x".repeat(200));
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-030
#[tokio::test]
async fn at_ninety_percent_the_memory_to_disk_pass_pauses_until_the_disk_has_room() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..3 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    cluster.set_disk_usage(95);
    let started = std::time::Instant::now();
    let usage = cluster.disk_usage.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        usage.store(10, std::sync::atomic::Ordering::SeqCst);
    });
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).operations_url("/node/backup/flush"))
        .send()
        .await
        .unwrap();
    let report: Value = response.json().await.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(600),
        "the flush waited for the disk ({:?}): {report}",
        started.elapsed()
    );
    assert_eq!(report["memoryRemaining"], 0, "{report}");
    cluster.shutdown().await;
}

/// UC: UC-BKP-011
#[tokio::test]
async fn a_segment_closes_by_message_count_before_it_gets_old() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [
        ("TABA_BACKUP_SEGMENT_MAX_MESSAGES", "2"),
        // The age never closes: only the count.
        ("TABA_BACKUP_SEGMENT_MAX_AGE", "1h"),
    ];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..12 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    cluster.clock.advance(Duration::from_secs(61));
    // Each owner closes the segment on reaching 2 messages; the rest wait for the age (1 h): at least
    // half of the 12 has already gone up, in segments of at most 2.
    eventually_async("segments closed by count", || async {
        messages_in_bucket(&s3, "orders").await >= 6
    })
    .await;
    for key in s3.keys("taba/messages/orders/") {
        let body = s3.get(&key).await.unwrap().unwrap().body;
        let (footer, _) = taba_node::storage::backup::segment::decode_all(&body).unwrap();
        assert!(
            footer.entries.len() <= 2,
            "{key}: {} messages",
            footer.entries.len()
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-018
#[tokio::test]
async fn tokens_go_to_the_bucket_one_file_each_and_a_revoked_token_disappears() {
    let s3 = Arc::new(MemoryStore::new());
    let root = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";
    let env = [
        ("TABA_AUTH_ROOT_TOKEN", root),
        ("TABA_AUTH_ENABLED", "true"),
    ];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    let node = cluster.node(1);
    node.clock.advance(Duration::from_secs(1));
    let response = node
        .client
        .post(node.client_url("/tokens"))
        .header("authorization", format!("Bearer {root}"))
        .json(&json!({"name": "ci", "permissions": ["publish"], "topicPrefixes": ["orders"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    let created: Value = response.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let key = format!("taba/tokens/{id}.json");
    eventually("o token no bucket", || s3.peek(&key).is_some()).await;
    let stored: Value = serde_json::from_slice(&s3.peek(&key).unwrap()).unwrap();
    assert_eq!(stored["token"]["name"], "ci");
    assert!(
        stored["token"]["secretHash"].is_string() && stored["token"].get("token").is_none(),
        "only the salted hash goes to the bucket, never the secret"
    );

    node.clock.advance(Duration::from_secs(1));
    let response = node
        .client
        .delete(node.client_url(&format!("/tokens/{id}")))
        .header("authorization", format!("Bearer {root}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    eventually("the revoked token to leave the bucket", || {
        s3.peek(&key).is_none()
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-BKP-039, UC-BKP-034
#[tokio::test]
async fn with_s3_down_the_stop_still_drains_every_node_and_warns_that_the_backup_was_not_sent() {
    let s3 = Arc::new(MemoryStore::new());
    let env = [("TABA_BACKUP_DRAIN_TIMEOUT", "1s")];
    let cluster = TestCluster::start_with_backup(3, &env, s3.clone()).await;
    create_queue(cluster.node(1), "orders").await;
    for i in 0..6 {
        publish(cluster.node(1), "orders", json!({"n": i})).await;
    }
    s3.set_down(true);
    cluster.clock.advance(Duration::from_secs(61));
    let started = std::time::Instant::now();
    let report = stop_cluster(&cluster).await;
    assert_eq!(report["stopList"], false, "{report}");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("S3")),
        "warns that the backup was not sent: {report}"
    );
    assert_eq!(
        report["stopped"].as_array().unwrap().len(),
        3,
        "the drain to disk was done"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "the send waited until the backup deadline before giving up ({:?})",
        started.elapsed()
    );
    for number in 1..=3 {
        assert_eq!(
            cluster.node(number).node.shared().drain_state(),
            taba_node::cluster::state::DrainState::Drained
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-BKP-040
#[tokio::test]
async fn a_single_node_owns_everything_and_backs_up_too() {
    let s3 = Arc::new(MemoryStore::new());
    let node = TestNode::builder()
        .backup_store(s3.clone())
        .env("TABA_BACKUP_DISK_EVICT_TO", "98")
        .env("TABA_BACKUP_DISK_EVICT_ABOVE", "99")
        .env("TABA_BACKUP_DISK_PAUSE_AT", "100")
        .start()
        .await;
    create_queue(&node, "orders").await;
    for i in 0..3 {
        publish(&node, "orders", json!({"n": i})).await;
    }
    let response = node
        .client
        .post(node.operations_url("/node/backup/flush"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let report: Value = response.json().await.unwrap();
    assert_eq!(report["outboxPending"], 0, "{report}");
    assert_eq!(messages_in_bucket(&s3, "orders").await, 3);
    assert!(s3.peek("taba/topics/orders.json").is_some());
    node.shutdown().await;
}

/// The publish latency with and without the backup (and with S3 down): S3 must not show up in it.
/// It does not run in `cargo test` (it measures time); run it with
/// `cargo test --test backup publish_latency -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "measures time; run by hand"]
async fn publish_latency_with_and_without_s3() {
    async fn run(label: &str, cluster: &TestCluster) -> f64 {
        create_queue(cluster.node(1), "medida").await;
        // Aquece.
        for i in 0..30 {
            publish(cluster.node(1), "medida", json!({"w": i})).await;
        }
        let mut samples = Vec::new();
        for i in 0..300 {
            let started = std::time::Instant::now();
            publish(cluster.node(2), "medida", json!({"n": i})).await;
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        let p50 = samples[samples.len() / 2];
        let p99 = samples[samples.len() * 99 / 100];
        eprintln!("{label:<28} p50 {p50:6.2} ms   p99 {p99:6.2} ms");
        p50
    }

    let without = TestCluster::start(3).await;
    let base = run("without backup", &without).await;
    without.shutdown().await;

    let s3 = Arc::new(MemoryStore::new());
    let with = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let up = run("with backup (S3 up)", &with).await;
    with.shutdown().await;

    let s3 = Arc::new(MemoryStore::new());
    s3.set_down(true);
    let down_cluster = TestCluster::start_with_backup(3, &[], s3.clone()).await;
    let down = run("with backup (S3 down)", &down_cluster).await;
    down_cluster.shutdown().await;

    // The backup only inserts a row in the outbox: no S3 on the publish. Wide margin (development
    // machine, three nodes in the same process).
    assert!(
        up < base * 2.0 + 5.0,
        "with backup {up} ms against {base} ms without"
    );
    assert!(
        down < base * 2.0 + 5.0,
        "with S3 down {down} ms against {base} ms without"
    );
}
