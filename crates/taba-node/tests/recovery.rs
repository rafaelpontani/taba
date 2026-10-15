//! The recovery of losses and the node's return (doc 04, section 6.6; doc 20c, UC-RTN and UC-DRN-018;
//! level **C**): the node that finds out it was excluded, or that comes back from the drain, queries the
//! other nodes' loss list, looks for each message in what it still keeps and sends it to the owner of the
//! block, before deleting the local state. The `Clock` is manual; the wait deadlines are real.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, eventually, eventually_async, host_of, total, wait_joined};
use taba_node::cluster::protocol::{LossRecord, MessageRecord, Request, Response, StreamQuery};
use taba_node::cluster::recovery::Recovery;
use taba_node::cluster::state::NodeState;
use taba_node::cluster::transport::{CallError, PeerAddr, Transport};
use taba_node::config::DbSynchronous;
use taba_node::core::clock::Clock;
use taba_node::core::error::ErrorCode;
use taba_node::core::id;
use taba_node::engine::messages::Messages;
use taba_node::storage::losses::Loss;
use taba_node::storage::messages::MessageStore;

/// The 5 minutes of `cluster.freezing.duration`.
const FREEZING: Duration = Duration::from_secs(300);
const TOPIC: &str = "orders";

fn messages_of(node: &TestNode) -> Arc<Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

fn recovery_of(node: &TestNode) -> Arc<Recovery> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.recovery.clone())
        .expect("the node is in the cluster")
}

/// A message as node 3 would keep it: already delivered twice and with a current invisibility.
fn record(cluster: &TestCluster, n: u64, text: &str) -> MessageRecord {
    let now = cluster.clock.now_millis();
    MessageRecord {
        id: id::new_at(now + n),
        topic: TOPIC.into(),
        body: json!({ "text": text }).to_string().into_bytes(),
        content_type: "application/json".into(),
        group_id: None,
        deduplication_id: None,
        delivery_count: 3,
        invisible_until_ms: Some(now + 3_600_000),
        receipt_token: Some(id::new_at(now + 100 + n)),
        dead_lettered_ms: None,
        dead_letter_reason: None,
        delete_at_ms: None,
    }
}

/// The node starts keeping the message (as if it had received it from a publish).
fn hold(node: &TestNode, record: &MessageRecord) {
    messages_of(node)
        .apply_load(&record.topic, record.to_meta(), record.body.clone())
        .expect("writes");
    assert!(messages_of(node).holds(&record.topic, &record.id));
}

/// The node declares the loss of the message (as the synchronization of a new node would declare it).
fn declare_loss(cluster: &TestCluster, number: usize, record: &MessageRecord) {
    recovery_of(cluster.node(number))
        .losses()
        .record(&Loss {
            topic: record.topic.clone(),
            id: record.id.clone(),
            block: "0000000000000001".into(),
            detected_ms: cluster.clock.now_millis(),
        })
        .unwrap();
}

fn has(node: &TestNode, id: &str) -> bool {
    messages_of(node).message_info(TOPIC, id).unwrap().is_some()
}

fn loss_count(cluster: &TestCluster, number: usize) -> u64 {
    recovery_of(cluster.node(number)).losses().count().unwrap()
}

async fn setup(env: &[(&str, &str)]) -> TestCluster {
    let cluster = TestCluster::start_with(3, env).await;
    common::cluster::settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), TOPIC).await;
    cluster
}

/// Isolates node 3 and waits for the other two to exclude it.
async fn exclude_third(cluster: &TestCluster) {
    let id = cluster.node_id(3);
    cluster.network.isolate(&host_of(3));
    eventually("the two healthy ones to exclude node 3", || {
        [1, 2].iter().all(|n| !cluster.member_ids(*n).contains(&id))
    })
    .await;
}

async fn consume_ids(node: &TestNode) -> Vec<String> {
    let response = node
        .client
        .get(node.client_url(&format!(
            "/topics/{TOPIC}/messages?maxMessages=10&visibilityTimeout=300"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    body["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_owned())
        .collect()
}

/// UC: UC-RTN-001, UC-RTN-002, UC-RTN-003, UC-RTN-004, UC-RTN-005, UC-RTN-007, UC-RTN-009, UC-RTN-010
#[tokio::test]
async fn an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins() {
    let cluster = setup(&[]).await;
    let lost = record(&cluster, 1, "perdida");
    let unlisted = record(&cluster, 2, "ninguem declarou");
    let absent = record(&cluster, 3, "node 3 does not have");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
        declare_loss(&cluster, number, &absent);
    }
    exclude_third(&cluster).await;
    hold(cluster.node(3), &lost);
    hold(cluster.node(3), &unlisted);
    let third_id = cluster.node_id(3);

    // The network returns: node 3 sees it was excluded, recovers what the loss list asks for and only then
    // deletes the local state.
    cluster.network.restore(&host_of(3));
    eventually("the lost message to reach the two owners", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;

    // Writes directly, with the delivery counter; the old invisibility and receipt disappear.
    let info = messages_of(cluster.node(1))
        .message_info(TOPIC, &lost.id)
        .unwrap()
        .unwrap();
    assert_eq!(info.meta.delivery_count, 3);
    assert_eq!(info.meta.invisible_until_ms, 0);
    assert_eq!(info.meta.receipt_token, None);
    let owners: Vec<bool> = [1, 2]
        .iter()
        .map(|n| {
            messages_of(cluster.node(*n))
                .message_info(TOPIC, &lost.id)
                .unwrap()
                .unwrap()
                .owned
        })
        .collect();
    assert_eq!(owners.iter().filter(|o| **o).count(), 1, "a single owner");

    // Only what the list asks for and the node has was sent: the message no one declared lost does not
    // go, and the lost one that node 3 does not have stays in the register.
    assert!(!has(cluster.node(1), &unlisted.id));
    assert!(!has(cluster.node(2), &unlisted.id));
    assert!(!has(cluster.node(1), &absent.id));

    // Clearance: the recovered pair leaves both tables; what was not recovered stays.
    eventually("the clearance of the recovered loss", || {
        loss_count(&cluster, 1) == 1 && loss_count(&cluster, 2) == 1
    })
    .await;

    // The message becomes ready for delivery again.
    let mut got = Vec::new();
    for _ in 0..40 {
        got = consume_ids(cluster.node(1)).await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(got, vec![lost.id.clone()]);

    // Count: one sent by node 3, applied on the two owners.
    let third = cluster.node(3).metrics().await;
    assert_eq!(
        total(
            &third,
            "taba_recover_messages_total",
            "taba_outcome=\"sent\""
        ),
        1.0
    );
    assert_eq!(
        total(
            &third,
            "taba_cluster_recover_total",
            "taba_direction=\"sent\""
        ),
        1.0
    );
    let applied: f64 = [
        cluster.node(1).metrics().await,
        cluster.node(2).metrics().await,
    ]
    .iter()
    .map(|text| {
        total(
            text,
            "taba_recover_messages_total",
            "taba_outcome=\"applied\"",
        )
    })
    .sum();
    assert_eq!(applied, 2.0);

    // Recovered, the node deleted what it had and redoes the join from scratch (the freezing only delays it).
    eventually("node 3 to delete the local state", || {
        !has(cluster.node(3), &unlisted.id)
    })
    .await;
    cluster.clock.advance(FREEZING);
    wait_joined(cluster.node(3)).await;
    assert_eq!(cluster.node(3).node.node_id(), third_id);
    cluster.shutdown().await;
}

/// UC: UC-RTN-006, UC-RTN-010
#[tokio::test]
async fn the_node_keeps_its_old_data_while_a_node_does_not_answer_the_loss_query() {
    let cluster = setup(&[("TABA_CLUSTER_RECOVERY_TIMEOUT", "30s")]).await;
    let lost = record(&cluster, 1, "perdida");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }
    exclude_third(&cluster).await;
    hold(cluster.node(3), &lost);

    // Node 2 does not answer the query: node 3 waits, in `returning`, without deleting anything.
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    cluster.network.restore(&host_of(3));
    eventually("node 3 in returning", || {
        cluster.node(3).node.shared().state() == NodeState::Returning
    })
    .await;
    let ready = cluster.node(3).client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "returning");
    let asked = cluster.network.count("message-loss");
    eventually("the repeated query to the node that did not answer", || {
        cluster.network.count("message-loss") >= asked + 3
    })
    .await;
    assert!(
        messages_of(cluster.node(3)).holds(TOPIC, &lost.id),
        "the old state stays in quarantine"
    );
    assert!(!has(cluster.node(1), &lost.id));

    // Node 2 answers again: the recovery finishes and only then is the state deleted.
    cluster.network.lose_kind("10.0.0.2", "message-loss", 0);
    eventually("the message to reach the owners", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually("node 3 to leave returning", || {
        cluster.node(3).node.shared().state() != NodeState::Returning
    })
    .await;
    assert!(!messages_of(cluster.node(3)).holds(TOPIC, &lost.id));
    cluster.shutdown().await;
}

/// UC: UC-RTN-006, UC-RTN-007
#[tokio::test]
async fn when_the_recovery_timeout_runs_out_the_node_goes_on_with_the_lists_it_has() {
    let cluster = setup(&[("TABA_CLUSTER_RECOVERY_TIMEOUT", "400ms")]).await;
    let lost = record(&cluster, 1, "perdida");
    declare_loss(&cluster, 1, &lost);
    exclude_third(&cluster).await;
    hold(cluster.node(3), &lost);

    // Node 2 never answers: after the deadline, node 1's list counts and the node goes on.
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    cluster.network.restore(&host_of(3));
    eventually("the message to reach the owners", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually(
        "node 3 to delete the local state and leave returning",
        || {
            cluster.node(3).node.shared().state() != NodeState::Returning
                && !messages_of(cluster.node(3)).holds(TOPIC, &lost.id)
        },
    )
    .await;
    cluster.clock.advance(FREEZING);
    wait_joined(cluster.node(3)).await;
    cluster.shutdown().await;
}

/// UC: UC-DRN-018, UC-RTN-001, UC-RTN-003, UC-RTN-013
#[tokio::test]
async fn a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node() {
    let cluster = setup(&[]).await;
    let lost = record(&cluster, 1, "perdida");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }
    let node3 = cluster.node(3);
    hold(node3, &lost);
    let old_id = cluster.node_id(3);
    let drain = node3
        .client
        .post(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(drain.status(), 202);
    eventually_async("drained", || async {
        let body: Value = node3
            .client_get_ops("/node/drain")
            .await
            .json()
            .await
            .unwrap();
        body["drainState"] == "drained"
    })
    .await;
    assert!(!has(cluster.node(1), &lost.id));

    let back = node3
        .client
        .delete(node3.operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(back.status(), 202);

    // Before clearing the state, the node delivers to the owners what the loss list asked for.
    eventually("the message to reach the owners", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually("the node to come back with a new ID", || {
        let id = node3.node.node_id();
        id != old_id
            && cluster
                .table(1)
                .is_some_and(|t| t.members.len() == 3 && t.member(&id).is_some())
    })
    .await;
    assert!(!messages_of(node3).holds(TOPIC, &lost.id));
    assert_eq!(
        total(
            &node3.metrics().await,
            "taba_recover_messages_total",
            "taba_outcome=\"sent\""
        ),
        1.0
    );
    cluster.shutdown().await;
}

/// UC: UC-RTN-001, UC-RTN-008, UC-RTN-012
#[tokio::test]
async fn the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only() {
    let cluster = setup(&[("TABA_CLUSTER_MESSAGE_LOSS_TTL", "1h")]).await;
    let lost = record(&cluster, 1, "perdida");
    let outsider = cluster.network.transport("10.0.0.9");
    let first = common::cluster::address_of(&cluster, 1);

    // The loss list and `recover` accept whoever is not in the table; the register does not.
    declare_loss(&cluster, 1, &lost);
    let listed = outsider
        .call(&first, Request::MessageLoss(StreamQuery::default()))
        .await
        .expect("the list accepts whoever is not a member");
    let Response::Stream(page) = listed else {
        panic!("expected a stream");
    };
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0]["id"], lost.id);
    assert_eq!(page.items[0]["topic"], TOPIC);
    let recovered = outsider
        .call(
            &first,
            Request::Recover {
                records: vec![],
                replicate: true,
            },
        )
        .await
        .expect("the recover accepts whoever is not a member");
    assert!(matches!(recovered, Response::Recovered(r) if r.count == 0));
    let register = Request::LossRegister(LossRecord {
        topic: TOPIC.into(),
        id: lost.id.clone(),
        block: "0000000000000001".into(),
        detected_at: "2026-10-07T12:00:00.000Z".into(),
    });
    match outsider.call(&first, register.clone()).await {
        Err(CallError::Remote(e)) => assert_eq!(e.code, ErrorCode::ClusterNotMember),
        other => panic!("esperava `cluster.not-member`: {other:?}"),
    }

    // A member registers the loss on node 2, which starts listing it, and the clearance removes it.
    let member = cluster.network.transport(host_of(1));
    let second = common::cluster::address_of(&cluster, 2);
    let ack = member.call(&second, register).await.unwrap();
    assert!(matches!(ack, Response::Loss(a) if a.changed));
    assert_eq!(loss_count(&cluster, 2), 1);
    let remove = Request::LossRemove {
        topic: TOPIC.into(),
        id: lost.id.clone(),
    };
    let ack = member.call(&second, remove.clone()).await.unwrap();
    assert!(matches!(ack, Response::Loss(a) if a.changed));
    let again = member.call(&second, remove).await.unwrap();
    assert!(matches!(again, Response::Loss(a) if !a.changed));
    assert_eq!(loss_count(&cluster, 2), 0);

    // The record expires after `cluster.message-loss.ttl`.
    cluster.clock.advance(Duration::from_secs(2 * 3600));
    let Response::Stream(page) = outsider
        .call(&first, Request::MessageLoss(StreamQuery::default()))
        .await
        .unwrap()
    else {
        panic!("expected a stream");
    };
    assert!(page.items.is_empty());
    assert_eq!(loss_count(&cluster, 1), 0);
    cluster.shutdown().await;
}

/// Resending the whole batch is safe: the message already applied is ignored and the line counts as done.
///
/// UC: UC-RTN-009, UC-RTN-003
#[tokio::test]
async fn resending_a_recover_batch_ignores_what_was_already_applied() {
    let cluster = setup(&[]).await;
    let lost = record(&cluster, 1, "perdida");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }
    let outsider = cluster.network.transport("10.0.0.9");
    let target = common::cluster::address_of(&cluster, 1);
    let send = || Request::Recover {
        records: vec![lost.clone()],
        replicate: true,
    };
    for round in 0..2 {
        let reply = outsider.call(&target, send()).await.unwrap();
        assert!(
            matches!(reply, Response::Recovered(r) if r.count == 1),
            "envio {round}"
        );
    }
    assert!(has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id));
    let skipped: f64 = [
        cluster.node(1).metrics().await,
        cluster.node(2).metrics().await,
    ]
    .iter()
    .map(|text| {
        total(
            text,
            "taba_recover_messages_total",
            "taba_outcome=\"skipped\"",
        )
    })
    .sum();
    assert_eq!(
        skipped, 2.0,
        "the second send was ignored on the two owners"
    );
    cluster.shutdown().await;
}

/// UC: UC-RTN-011
#[tokio::test]
async fn in_development_mode_there_is_no_cluster_to_recover_from() {
    let node = TestNode::development().await;
    assert!(node.node.cluster().is_none());
    assert!(node.node.cluster_addr().is_none());
    node.shutdown().await;
}

/// UC: UC-RTN-002, UC-RTN-016
#[tokio::test]
async fn a_process_that_restarts_hands_back_the_lost_messages_its_old_disk_has() {
    let mut cluster = setup(&[("TABA_DRAIN_ON_STOP", "false"), ("TABA_MEMORY_LIMIT", "1B")]).await;
    let lost = record(&cluster, 1, "perdida");
    let unlisted = record(&cluster, 2, "ninguem declarou");
    for number in [1, 2] {
        declare_loss(&cluster, number, &lost);
    }

    // The process falls without drain and comes back at the same address, with the same disk: it is born as a new node.
    let old_id = cluster.node_id(3);
    let (number, _) = restart_third(&mut cluster, &[], &[&lost, &unlisted]).await;
    assert_ne!(cluster.node_id(number), old_id, "the ID is new");

    // Only the message the loss list asks for returns to the owners.
    eventually("the lost message to reach the two owners", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    assert!(!has(cluster.node(1), &unlisted.id));
    assert!(!has(cluster.node(2), &unlisted.id));
    eventually("the clearance of the recovered loss", || {
        loss_count(&cluster, 1) == 0 && loss_count(&cluster, 2) == 0
    })
    .await;
    wait_joined(cluster.node(number)).await;
    assert!(!messages_of(cluster.node(number)).holds(TOPIC, &unlisted.id));
    cluster.shutdown().await;
}

/// Node 3's process falls without drain and comes back at the same address, with the same disk: it is born as a
/// new node. Returns the node number and the disk folder.
async fn restart_third(
    cluster: &mut TestCluster,
    env: &[(&str, &str)],
    on_disk: &[&MessageRecord],
) -> (usize, std::path::PathBuf) {
    let port = cluster.node(3).node.cluster_addr().unwrap().port();
    let TestNode { node, dir, .. } = cluster.nodes.pop().unwrap();
    node.shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));
    let storage = dir.path().join("taba");
    // The previous life's disk is written with the process already dead: no file stays open.
    let old_disk = MessageStore::new(&storage, DbSynchronous::Off);
    for record in on_disk {
        old_disk
            .put(&record.topic, &record.to_meta(), &record.body)
            .expect("writes to disk");
    }
    drop(old_disk);
    let number = cluster
        .respawn_node(3, |builder| {
            let mut builder = builder
                .env("TABA_CLUSTER_PORT", &port.to_string())
                .env("TABA_MEMORY_LIMIT", "1B")
                .env("TABA_STORAGE_DIR", &storage.display().to_string());
            for (key, value) in env {
                builder = builder.env(key, value);
            }
            builder
        })
        .await
        .expect("o processo reinicia");
    // The disk folder stays up: the `TempDir` dies with the test, after the check.
    std::mem::forget(dir);
    (number, storage)
}

/// UC: UC-RTN-014, UC-RTN-015, UC-RTN-016
#[tokio::test]
async fn a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards() {
    let mut cluster = setup(&[
        ("TABA_DRAIN_ON_STOP", "false"),
        ("TABA_MEMORY_LIMIT", "1B"),
        ("TABA_CLUSTER_RECOVERY_TIMEOUT", "30s"),
    ])
    .await;
    let from_one = record(&cluster, 1, "so o no 1 declarou");
    let from_two = record(&cluster, 2, "so o no 2 declarou");
    let unlisted = record(&cluster, 3, "ninguem declarou");
    declare_loss(&cluster, 1, &from_one);
    declare_loss(&cluster, 2, &from_two);
    let old_id = cluster.node_id(3);

    // Node 2 does not answer the loss query: before, the restart waited up to 5 minutes.
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    let (number, storage) = restart_third(
        &mut cluster,
        &[("TABA_CLUSTER_RECOVERY_TIMEOUT", "30s")],
        &[&from_one, &from_two, &unlisted],
    )
    .await;
    assert_ne!(cluster.node_id(number), old_id, "the ID is new");
    wait_joined(cluster.node(number)).await;
    assert_eq!(
        cluster.node(number).node.shared().state(),
        NodeState::Active,
        "joins active, without going through returning"
    );

    // Node 1's list was already handled; node 2 stays mute and the previous life's folder waits.
    eventually("node 1's message to reach the owners", || {
        has(cluster.node(1), &from_one.id) && has(cluster.node(2), &from_one.id)
    })
    .await;
    assert!(!has(cluster.node(1), &from_two.id));
    assert!(storage.join("_previous-life").is_dir());

    // Node 2 answers again: the second loss also comes back, and the folder is deleted.
    cluster.network.lose_kind("10.0.0.2", "message-loss", 0);
    eventually("node 2's message to reach the owners", || {
        has(cluster.node(1), &from_two.id) && has(cluster.node(2), &from_two.id)
    })
    .await;
    eventually("the previous life's folder to be deleted", || {
        !storage.join("_previous-life").exists()
    })
    .await;
    for node in [1, 2] {
        assert!(!has(cluster.node(node), &unlisted.id));
    }
    assert!(!messages_of(cluster.node(number)).holds(TOPIC, &unlisted.id));
    cluster.shutdown().await;
}

/// UC: UC-RTN-015
#[tokio::test]
async fn the_recovery_of_a_restarted_process_ends_at_the_timeout_even_if_a_node_stays_mute() {
    let mut cluster = setup(&[("TABA_DRAIN_ON_STOP", "false"), ("TABA_MEMORY_LIMIT", "1B")]).await;
    let lost = record(&cluster, 1, "perdida");
    declare_loss(&cluster, 1, &lost);
    cluster
        .network
        .lose_kind("10.0.0.2", "message-loss", 100_000);
    let (number, storage) = restart_third(
        &mut cluster,
        &[("TABA_CLUSTER_RECOVERY_TIMEOUT", "400ms")],
        &[&lost],
    )
    .await;
    wait_joined(cluster.node(number)).await;
    eventually("the message to reach the owners", || {
        has(cluster.node(1), &lost.id) && has(cluster.node(2), &lost.id)
    })
    .await;
    eventually("the previous life's folder to be deleted on time", || {
        !storage.join("_previous-life").exists()
    })
    .await;
    assert_eq!(
        cluster.node(number).node.shared().state(),
        NodeState::Active
    );
    cluster.shutdown().await;
}

/// UC: UC-RTN-017
#[tokio::test]
async fn the_first_node_of_a_new_cluster_wipes_the_old_disk_and_keeps_no_previous_life() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join("taba");
    let old_disk = MessageStore::new(&storage, DbSynchronous::Off);
    let cluster = TestCluster::empty();
    let old = record(&cluster, 1, "from a cluster that no longer exists");
    old_disk
        .put(&old.topic, &old.to_meta(), &old.body)
        .expect("writes to disk");
    drop(old_disk);
    assert!(storage.join(TOPIC).is_dir());

    let mut cluster = cluster;
    let builder = cluster
        .builder(1, true)
        .env("TABA_STORAGE_DIR", &storage.display().to_string());
    let number = cluster
        .spawn_node(builder)
        .await
        .expect("the first node starts");
    wait_joined(cluster.node(number)).await;

    // There is no one to recover from: the old disk was deleted, and nothing was set aside.
    eventually("the old disk to be deleted", || {
        !storage.join(TOPIC).exists()
    })
    .await;
    assert!(!storage.join(MessageStore::PREVIOUS_LIFE).exists());
    assert_eq!(cluster.member_ids(number).len(), 1);
    cluster.shutdown().await;
}
