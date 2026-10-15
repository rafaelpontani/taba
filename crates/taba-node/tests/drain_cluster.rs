//! The drain in a cluster (doc 04, sections 6.9 and 6.10; doc 20c, DRN and FWD; level **C**): the node notifies the
//! peers, forwards the client calls to the freest node, leaves the cluster without a vote and can come back
//! as a new node.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::cluster::{NO_VOTE, TestCluster, eventually, eventually_async};
use common::{TestNode, error_code};

async fn drain_state(node: &TestNode) -> String {
    let response = node.client_get_ops("/node/drain").await;
    let body: Value = response.json().await.unwrap();
    body["drainState"].as_str().unwrap().to_owned()
}

async fn start_drain(node: &TestNode) {
    let response = node
        .client
        .post(node.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
}

async fn wait_drain_state(node: &TestNode, wanted: &str) {
    eventually_async(&format!("drainState {wanted}"), || async {
        drain_state(node).await == wanted
    })
    .await;
}

async fn publish(node: &TestNode, topic: &str, body: Value) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{:?}", response.text().await);
    response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn consume_all(node: &TestNode, topic: &str, expected: usize) -> Vec<Value> {
    let mut got: Vec<Value> = Vec::new();
    for _ in 0..80 {
        let response = node
            .client
            .get(node.client_url(&format!(
                "/topics/{topic}/messages?maxMessages=10&visibilityTimeout=300"
            )))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{:?}", response.text().await);
        let body: Value = response.json().await.unwrap();
        got.extend(body["messages"].as_array().cloned().unwrap_or_default());
        if got.len() >= expected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    got
}

/// UC: UC-DRN-007, UC-DRN-008, UC-DRN-011, UC-DRN-012, UC-FRZ-006, UC-FWD-001, UC-FWD-004, UC-FWD-009
#[tokio::test]
async fn a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote() {
    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    node3
        .client
        .post(node3.client_url("/topics"))
        .json(&json!({"name": "orders", "type": "queue"}))
        .send()
        .await
        .unwrap();
    let mut published = Vec::new();
    for n in 0..12 {
        published.push(publish(cluster.node(1), "orders", json!({"n": n})).await);
    }
    let version_before = cluster.table(1).unwrap().version;

    start_drain(node3).await;
    wait_drain_state(node3, "drained").await;

    // It left the others' table by a two-phase unregistration (`reason: drain`), without a vote.
    for number in [1, 2] {
        let table = cluster.table(number).unwrap();
        assert_eq!(table.members.len(), 2, "node {number}");
        assert_eq!(table.version, version_before + 1);
        assert!(!cluster.member_ids(number).contains(&cluster.node_id(3)));
    }
    assert!(cluster.network.count("unregister") >= 1);
    assert_eq!(cluster.network.count("rollback"), 0);

    // The peers were notified before anything else: no `take` goes to it anymore.
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "drained");

    // The client calls that still reach it are forwarded, the publish included.
    let listed = node3
        .client
        .get(node3.client_url("/topics/orders"))
        .send()
        .await
        .unwrap();
    assert_eq!(listed.status(), 200);
    let id = publish(node3, "orders", json!({"after": true})).await;
    published.push(id);
    assert!(cluster.network.count("forward") >= 2);

    // The messages it owned stayed with the new owner, which already had the copy.
    cluster.network.clear_calls();
    let got = consume_all(cluster.node(1), "orders", published.len()).await;
    let mut ids: Vec<String> = got
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    let mut expected = published.clone();
    expected.sort();
    assert_eq!(ids, expected);
    assert!(
        cluster
            .network
            .calls()
            .iter()
            .all(|c| !(c.request == "take" && c.to.host == "10.0.0.3")),
        "no one asks a `take` of a node outside the cluster"
    );

    // The forwarded `GET` also works through the node in drain (the destination delivers).
    let via_drained = node3
        .client
        .get(node3.client_url("/topics/orders/messages?maxMessages=1"))
        .send()
        .await
        .unwrap();
    assert_eq!(via_drained.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-016
#[tokio::test]
async fn canceling_before_the_node_left_keeps_it_in_the_cluster() {
    let cluster = TestCluster::start_with(3, NO_VOTE).await;
    // The others do not reach node 3: the unregistration does not close and the drain stays in `draining`.
    cluster.network.isolate("10.0.0.3");
    let node3 = cluster.node(3);
    start_drain(node3).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(drain_state(node3).await, "draining");
    assert_eq!(cluster.table(3).unwrap().members.len(), 3);

    let cancel = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status(), 200);
    assert_eq!(drain_state(node3).await, "active");
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 200);
    cluster.network.restore("10.0.0.3");
    cluster.shutdown().await;
}

/// UC: UC-DRN-018, UC-FRZ-006
#[tokio::test]
async fn after_leaving_the_node_returns_as_a_new_node_with_a_new_id() {
    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    let old_id = cluster.node_id(3);
    start_drain(node3).await;
    wait_drain_state(node3, "drained").await;

    let back = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(back.status(), 202);

    // New ID, full bootstrap and the return to the table, without freezing.
    eventually("the node to come back to the cluster", || {
        let id = node3.node.node_id();
        id != old_id
            && cluster
                .table(1)
                .is_some_and(|t| t.members.len() == 3 && t.member(&id).is_some())
    })
    .await;
    assert_eq!(drain_state(node3).await, "active");
    assert!(!cluster.member_ids(1).contains(&old_id));
    eventually("the node ready again", || {
        node3.node.shared().state() == taba_node::cluster::state::NodeState::Active
    })
    .await;
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-017
#[tokio::test]
async fn canceling_a_drain_that_is_leaving_is_a_conflict() {
    let cluster = TestCluster::start(3).await;
    // The unregistration commit gets stuck: the exit is decided but does not finish.
    cluster.network.set_delay(Duration::from_millis(150));
    let node3 = cluster.node(3);
    start_drain(node3).await;
    let mut conflict = false;
    for _ in 0..40 {
        let response = node3
            .client
            .delete(node3.operations_url("/node/drain"))
            .send()
            .await
            .unwrap();
        if response.status() == 409 {
            assert_eq!(error_code(response, 409).await, "drain.conflict");
            conflict = true;
            break;
        }
        // The cancellation went through (the drain was still emptying the memory): it starts again.
        if drain_state(node3).await == "active" {
            start_drain(node3).await;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        conflict,
        "there should have been a window with the exit in progress"
    );
    cluster.network.set_delay(Duration::ZERO);
    cluster.shutdown().await;
}

/// UC: UC-DRN-008, UC-ENT-022, UC-CAN-023
#[tokio::test]
async fn a_draining_node_answers_take_empty_and_the_peers_stop_asking() {
    use taba_node::cluster::protocol::{Request, Response, TakeRequest};
    use taba_node::cluster::transport::{PeerAddr, Transport};

    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    node3
        .client
        .post(node3.client_url("/topics"))
        .json(&json!({"name": "orders", "type": "queue"}))
        .send()
        .await
        .unwrap();
    for n in 0..9 {
        publish(cluster.node(1), "orders", json!({"n": n})).await;
    }
    // The exit does not close (the unregistration does not reach the peers): the node stays in `draining`, still in the
    // table and reachable.
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 1_000_000);
    }
    start_drain(node3).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(drain_state(node3).await, "draining");

    // A `take` that still arrives is answered empty, even with ready messages.
    let asker = cluster.network.transport("10.0.0.1");
    let port = node3.node.cluster_addr().unwrap().port();
    let reply = asker
        .call(
            &PeerAddr::new("10.0.0.3", port),
            Request::Take {
                topic: "orders".into(),
                request: TakeRequest {
                    max_messages: 10,
                    visibility_timeout: 60,
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(reply, Response::Taken(t) if t.messages.is_empty()));

    // The peers were notified by signal and no longer ask it for a `take`.
    assert!(cluster.network.count("channel") >= 2);
    cluster.network.clear_calls();
    let got = consume_all(cluster.node(1), "orders", 1).await;
    assert!(!got.is_empty());
    assert!(
        cluster
            .network
            .calls()
            .iter()
            .all(|c| !(c.request == "take" && c.to.host == "10.0.0.3"))
    );
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 0);
    }
    // Cancels the drain: the node goes back to active and the test's shutdown does not wait for `drain.timeout`.
    let cancel = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-009, UC-DRN-010
#[tokio::test]
async fn while_it_drains_the_node_writes_every_new_replica_straight_to_disk() {
    let cluster = TestCluster::start(3).await;
    let node3 = cluster.node(3);
    node3
        .client
        .post(node3.client_url("/topics"))
        .json(&json!({"name": "orders", "type": "queue"}))
        .send()
        .await
        .unwrap();
    for n in 0..6 {
        publish(cluster.node(1), "orders", json!({"n": n})).await;
    }
    let messages = node3
        .node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .unwrap();
    eventually("node 3's memory with the replicas", || {
        messages.pending() > 0
    })
    .await;

    // The exit does not close: the node stays in `draining`, receiving registers and commits from the peers.
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 1_000_000);
    }
    start_drain(node3).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(messages.pending(), 0, "the memory all went to disk");
    let disk_before = messages.storage_stats().disk_messages;
    assert!(disk_before > 0);
    for n in 0..6 {
        publish(cluster.node(1), "orders", json!({"after": n})).await;
    }
    eventually("the new replicas on disk", || {
        messages.storage_stats().disk_messages > disk_before
    })
    .await;
    assert_eq!(messages.pending(), 0, "nothing new stayed in memory");

    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "unregister", 0);
    }
    let cancel = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-DRN-013, UC-CNC-003
#[tokio::test]
async fn two_nodes_draining_at_once_leave_one_after_the_other() {
    let cluster = TestCluster::start(3).await;
    let (node2, node3) = (cluster.node(2), cluster.node(3));
    start_drain(node2).await;
    start_drain(node3).await;
    wait_drain_state(node2, "drained").await;
    wait_drain_state(node3, "drained").await;

    // Both exits took effect, one after the other: only node 1 remained, in epoch +2.
    let table = cluster.table(1).unwrap();
    assert_eq!(table.members.len(), 1);
    assert_eq!(table.version, 3 + 2);
    assert_eq!(cluster.member_ids(1), [cluster.node_id(1)]);
    cluster.shutdown().await;
}
