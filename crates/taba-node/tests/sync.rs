//! The block synchronization of a node that joins (doc 05, section 7.3; doc 04, sections 6.2 and 6.8; doc 20c,
//! level **C**): the block load, the event database, the final check, the switch to normal
//! mode, the concurrency of member changes and the cleanup of the blocks a node lost.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, eventually, host_of, pause_sync, start_isolated, wait_phase};
use taba_node::cluster::members::MemberState;
use taba_node::cluster::protocol::{Request, Response, StreamQuery};
use taba_node::cluster::state::NodeState;
use taba_node::core::ring::message_position;
use taba_node::engine::messages::Messages;

fn messages_of(node: &TestNode) -> Arc<Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

async fn create_queue(node: &TestNode, name: &str) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
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

async fn ack(node: &TestNode, topic: &str, id: &str, token: &str) -> u16 {
    node.client
        .delete(node.client_url(&format!(
            "/topics/{topic}/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .expect("request")
        .status()
        .as_u16()
}

fn member_state(cluster: &TestCluster, observer: usize, of: usize) -> Option<MemberState> {
    cluster
        .table(observer)?
        .member(&cluster.node_id(of))
        .map(|m| m.state)
}

fn held(node: &TestNode, topic: &str, ids: &[String]) -> usize {
    let messages = messages_of(node);
    ids.iter()
        .filter(|id| messages.message_info(topic, id).unwrap().is_some())
        .count()
}

fn owned(node: &TestNode, topic: &str, ids: &[String]) -> usize {
    let messages = messages_of(node);
    ids.iter()
        .filter(|id| {
            messages
                .message_info(topic, id)
                .unwrap()
                .is_some_and(|info| info.owned)
        })
        .count()
}

async fn two_nodes_with_messages(count: usize, env: &[(&str, &str)]) -> (TestCluster, Vec<String>) {
    let cluster = TestCluster::start_with(2, env).await;
    create_queue(cluster.node(1), "orders").await;
    let mut ids = Vec::new();
    for n in 0..count {
        ids.push(publish(cluster.node(1 + n % 2), "orders", json!({"n": n})).await);
    }
    (cluster, ids)
}

/// UC: UC-SIN-001, UC-SIN-002, UC-SIN-005, UC-SIN-006, UC-SIN-012, UC-ING-021, UC-ING-022,
/// UC: UC-ING-024, UC-SIN-019, UC-ARM-015
#[tokio::test]
async fn a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready() {
    let (mut cluster, ids) = two_nodes_with_messages(40, &[]).await;
    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "replay");
    wait_phase(cluster.node(number), "replay").await;
    let node3 = cluster.node(number);

    // The node is in the table, syncing, and does not serve yet: `/health/ready` says why.
    assert_eq!(node3.node.shared().state(), NodeState::Syncing);
    assert_eq!(
        member_state(&cluster, 1, number),
        Some(MemberState::Syncing),
        "the others see it syncing"
    );
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "syncing");

    // The load already wrote the messages of its blocks (with factor 3 and 3 nodes, that is all of them), but it
    // owns none: it does not deliver.
    assert_eq!(held(node3, "orders", &ids), ids.len());
    assert_eq!(owned(node3, "orders", &ids), 0);
    let definition = node3.client_get("/topics/orders").await;
    assert_eq!(
        definition.status(),
        200,
        "the client call was forwarded to another node"
    );
    let got = consume(node3, "orders", "maxMessages=5&visibilityTimeout=300").await;
    assert_eq!(
        got.len(),
        5,
        "the forwarded GET delivers through the other owners"
    );

    gate.add_permits(1);
    common::cluster::wait_joined(node3).await;
    assert_eq!(node3.node.shared().state(), NodeState::Active);
    assert_eq!(node3.client_get_ops("/health/ready").await.status(), 200);
    // Ready, it is the owner of the messages of the blocks it is primary of.
    eventually("the node to own the messages of its blocks", || {
        owned(node3, "orders", &ids) > 0
    })
    .await;
    let owners: usize = (1..=3)
        .map(|n| owned(cluster.node(n), "orders", &ids))
        .sum();
    assert!(
        owners >= ids.len() - 5,
        "each message has a single owner: {owners}"
    );
    for n in 1..=3 {
        assert_eq!(
            member_state(&cluster, n, number),
            Some(MemberState::Active),
            "node {n} knows it became ready"
        );
    }
    assert!(
        !node3.storage_dir().join("_sync").exists(),
        "the event database is discarded"
    );
    cluster.shutdown().await;
}

/// UC: UC-SIN-010, UC-SIN-011, UC-SIN-013, UC-SIN-014, UC-SIN-015, UC-SIN-017, UC-SIN-018,
/// UC: UC-SIN-007, UC-ING-022
#[tokio::test]
async fn events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load() {
    let (mut cluster, _ids) = two_nodes_with_messages(12, &[]).await;
    // A message delivered (invisible) before the join: the ack will come during the synchronization.
    let delivered = consume(
        cluster.node(1),
        "orders",
        "maxMessages=1&visibilityTimeout=300",
    )
    .await;
    let (acked_id, token) = (
        delivered[0]["id"].as_str().unwrap().to_owned(),
        delivered[0]["receiptToken"].as_str().unwrap().to_owned(),
    );

    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "replay");
    wait_phase(cluster.node(number), "replay").await;
    let node3 = cluster.node(number);
    let syncer = node3
        .node
        .cluster()
        .and_then(|c| c.replication())
        .unwrap()
        .syncer
        .clone();
    assert!(syncer.is_recording());
    assert_eq!(
        held(node3, "orders", std::slice::from_ref(&acked_id)),
        1,
        "the load brought the message that will be confirmed"
    );

    // During the synchronization: an ack, a new publish and a new topic. The syncing node
    // confirms the register by writing the event, so everything answers as if it were a normal node.
    assert_eq!(ack(cluster.node(2), "orders", &acked_id, &token).await, 204);
    let fresh: Vec<String> = {
        let mut out = Vec::new();
        for n in 0..6 {
            out.push(publish(cluster.node(1 + n % 2), "orders", json!({"new": n})).await);
        }
        out
    };
    create_queue(cluster.node(1), "extras").await;
    let metrics = node3.metrics().await;
    assert!(
        common::series(&metrics, "taba_sync_log_events").unwrap() >= 3.0,
        "the events wait in the database: {metrics}"
    );
    assert!(node3.storage_dir().join("_sync").join("sync.db").is_file());
    assert_eq!(
        held(node3, "orders", &fresh),
        0,
        "the syncing node does not process the events: it only stores them"
    );

    gate.add_permits(1);
    common::cluster::wait_joined(node3).await;
    let messages = messages_of(node3);
    // The ack beat the load: the message it brought, now old, disappeared.
    assert!(
        messages
            .message_info("orders", &acked_id)
            .unwrap()
            .is_none()
    );
    // The publish made during the synchronization arrived by event.
    assert_eq!(held(node3, "orders", &fresh), fresh.len());
    // The topic created during the synchronization too.
    assert_eq!(node3.client_get("/topics/extras").await.status(), 200);
    // The database emptied, the node moved to normal mode and already processes in real time.
    assert!(!syncer.is_recording());
    let after = publish(cluster.node(1), "orders", json!({"after": true})).await;
    eventually("the real-time event", || {
        messages.message_info("orders", &after).unwrap().is_some()
    })
    .await;
    assert_eq!(
        common::series(&node3.metrics().await, "taba_sync_log_events"),
        Some(0.0)
    );
    cluster.shutdown().await;
}

/// UC: UC-CNC-001, UC-CNC-002, UC-CNC-003, UC-CNC-006
#[tokio::test]
async fn no_other_membership_change_takes_effect_until_the_syncing_node_is_ready() {
    let (mut cluster, _ids) = two_nodes_with_messages(6, &[]).await;
    let third = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, third, "replay");
    wait_phase(cluster.node(third), "replay").await;

    // A fourth node asks to join, and node 2's drain asks to leave: both wait.
    let fourth = {
        let builder = cluster.builder(4, false);
        cluster.spawn_node(builder).await.expect("starts")
    };
    let drain = cluster
        .node(2)
        .client
        .post(cluster.node(2).operations_url("/node/drain"))
        .send()
        .await
        .unwrap();
    assert_eq!(drain.status(), 202);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let table = cluster.table(1).unwrap();
    assert_eq!(table.members.len(), 3, "nothing changed: {table:?}");
    assert!(table.member(&cluster.node_id(fourth)).is_none());
    assert!(table.member(&cluster.node_id(2)).is_some());
    assert_ne!(
        cluster.node(fourth).node.shared().state(),
        NodeState::Active
    );

    // The node becomes ready and the two changes go on, one at a time.
    gate.add_permits(1);
    eventually("the two member changes", || {
        cluster.table(1).is_some_and(|t| {
            t.member(&cluster.node_id(fourth)).is_some() && t.member(&cluster.node_id(2)).is_none()
        })
    })
    .await;
    common::cluster::wait_joined(cluster.node(fourth)).await;
    cluster.shutdown().await;
}

/// UC: UC-SIN-025, UC-SIN-026, UC-SIN-027, UC-SIN-028, UC-OPS-038
#[tokio::test]
async fn a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on() {
    // With memory at the minimum, the messages go to disk, and a body can be deleted.
    let (mut cluster, ids) = two_nodes_with_messages(8, &[("TABA_MEMORY_LIMIT", "1B")]).await;
    let target = ids[3].clone();
    for number in 1..=2 {
        let folder = cluster.node(number).storage_dir().join("orders");
        let body = folder.join(format!("{target}.body"));
        assert!(body.is_file(), "the message is on disk on node {number}");
        std::fs::remove_file(body).unwrap();
    }
    let number = start_isolated(&mut cluster, &[("TABA_MEMORY_LIMIT", "1B")]).await;
    cluster.network.restore(&host_of(number));
    common::cluster::wait_joined(cluster.node(number)).await;

    // The other messages were loaded; the one left without content is a loss, recorded in the
    // node's database, and the synchronization did not stop nor did the node enter failure.
    let node3 = cluster.node(number);
    let others: Vec<String> = ids.iter().filter(|id| **id != target).cloned().collect();
    assert_eq!(held(node3, "orders", &others), others.len());
    assert_eq!(held(node3, "orders", std::slice::from_ref(&target)), 0);
    assert_eq!(node3.node.shared().state(), NodeState::Active);
    let db = taba_node::storage::db::NodeDb::open(
        &node3.storage_dir(),
        taba_node::config::DbSynchronous::Normal,
    )
    .unwrap();
    let losses = taba_node::storage::losses::LossStore::new(Arc::new(db))
        .list(None, 10)
        .unwrap();
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(losses[0].id, target);
    assert_eq!(losses[0].topic, "orders");

    // Whoever detects the loss also sends it to the other owners of the block, so the information
    // survives its fall (doc 04, section 6.6, "Loss table").
    for other in 1..=2 {
        let recovery = cluster
            .node(other)
            .node
            .cluster()
            .and_then(|c| c.replication())
            .map(|r| r.recovery.clone())
            .expect("the node is in the cluster");
        eventually("the loss to reach the other owner", || {
            recovery
                .losses()
                .list(None, 10)
                .unwrap()
                .iter()
                .any(|l| l.id == target && l.topic == "orders")
        })
        .await;
    }
    cluster.shutdown().await;
}

/// UC: UC-SIN-020, UC-SIN-023, UC-SIN-030
#[tokio::test]
async fn a_node_with_no_source_for_a_load_fails_after_the_timeout() {
    let (mut cluster, _ids) = two_nodes_with_messages(6, &[]).await;
    let number = start_isolated(&mut cluster, &[("TABA_CLUSTER_SYNC_TIMEOUT", "300ms")]).await;
    // No one serves the ID listing.
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "block-ids", 1_000_000);
    }
    cluster.network.restore(&host_of(number));
    let node3 = cluster.node(number);
    let shared = node3.node.shared().clone();
    eventually("the node to enter failure", move || {
        shared.state() == NodeState::Failed
    })
    .await;
    let ready = node3.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "failed");
    let metrics = node3.metrics().await;
    assert!(
        common::series(
            &metrics,
            "taba_sync_failures_total{taba_reason=\"no-source\"}"
        )
        .is_some_and(|n| n >= 1.0),
        "{metrics}"
    );
    // The node in failure still appears as syncing and would block the others' exit (until the
    // exclusion vote exists): it leaves first.
    cluster.nodes.pop().unwrap().shutdown().await;
    cluster.shutdown().await;
}

/// UC: UC-SIN-002, UC-SIN-009, UC-SIN-003, UC-SIN-004
#[tokio::test]
async fn the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages() {
    // Plenty of messages: with few, the ID draw might not put two in the same block.
    let (cluster, ids) = two_nodes_with_messages(150, &[]).await;
    let service = cluster.node(1).node.cluster().unwrap().clone();
    let ring = service.membership().ring();
    let caller = host_of(2);

    // All the blocks together list all the messages, each in the block of its position.
    let mut listed = Vec::new();
    for block in ring.blocks() {
        let reply = service
            .handle(
                &caller,
                Request::BlockIds {
                    block: block.to_string(),
                    query: StreamQuery::default(),
                },
            )
            .await
            .unwrap();
        let Response::Stream(page) = reply else {
            panic!("expected a stream")
        };
        let block_ids: Vec<String> = page
            .items
            .iter()
            .map(|i| i["id"].as_str().unwrap().to_owned())
            .collect();
        assert!(block_ids.windows(2).all(|w| w[0] < w[1]), "in ID order");
        for id in &block_ids {
            assert_eq!(ring.block_of(message_position(id)), Some(block));
        }
        assert!(page.items.iter().all(|i| i["topic"] == "orders"));
        assert!(!page.end.has_more);
        listed.extend(block_ids);
    }
    listed.sort();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(listed, expected);

    // A block with several messages: pagination by `limit`, resumption by `from` and range by `to`.
    let busiest = ring
        .blocks()
        .max_by_key(|b| {
            ids.iter()
                .filter(|id| ring.block_of(message_position(id)) == Some(*b))
                .count()
        })
        .unwrap();
    let in_block: Vec<String> = {
        let mut v: Vec<String> = ids
            .iter()
            .filter(|id| ring.block_of(message_position(id)) == Some(busiest))
            .cloned()
            .collect();
        v.sort();
        v
    };
    assert!(in_block.len() >= 2);
    let Response::Stream(first) = service
        .handle(
            &caller,
            Request::BlockMessages {
                block: busiest.to_string(),
                query: StreamQuery {
                    from: None,
                    to: None,
                    limit: Some(2),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected a stream")
    };
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.end.has_more, in_block.len() > 2);
    assert_eq!(first.items[0]["id"], in_block[0].as_str());
    assert!(
        first.items[0]["body"].is_object(),
        "the JSON body goes native"
    );
    let Response::Stream(range) = service
        .handle(
            &caller,
            Request::BlockMessages {
                block: busiest.to_string(),
                query: StreamQuery {
                    from: Some(in_block[1].clone()),
                    to: Some(in_block[1].clone()),
                    limit: None,
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected a stream")
    };
    assert_eq!(range.items.len(), 1);
    assert_eq!(range.items[0]["id"], in_block[1].as_str());

    // The single message: `200` in the right block, `404` in the other and on an ID that does not exist.
    let Response::Record(record) = service
        .handle(
            &caller,
            Request::BlockMessage {
                block: busiest.to_string(),
                id: in_block[0].clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected the message")
    };
    assert_eq!(record.id, in_block[0]);
    assert_eq!(record.topic, "orders");
    let other_block = ring.blocks().find(|b| *b != busiest).unwrap();
    for (block, id) in [
        (other_block, in_block[0].clone()),
        (busiest, taba_node::core::id::new_at(1)),
    ] {
        let error = service
            .handle(
                &caller,
                Request::BlockMessage {
                    block: block.to_string(),
                    id,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code.as_str(), "message.not-found");
    }
    // A malformed block or one the ring does not have; and whoever is not a member.
    let bad = service
        .handle(
            &caller,
            Request::BlockIds {
                block: "xyz".into(),
                query: StreamQuery::default(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(bad.status(), 400);
    let unknown = service
        .handle(
            &caller,
            Request::BlockIds {
                block: "0000000000000001".into(),
                query: StreamQuery::default(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.status(), 409);
    let stranger = service
        .handle(
            "192.0.2.9",
            Request::BlockIds {
                block: busiest.to_string(),
                query: StreamQuery::default(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(stranger.code.as_str(), "cluster.not-member");
    cluster.shutdown().await;
}

/// UC: UC-ARM-022, UC-ARM-023, UC-SIN-021
#[tokio::test]
async fn a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready() {
    // Factor 2: with the third node joining, part of the blocks stop being the first two's.
    let (mut cluster, ids) = two_nodes_with_messages(40, &[("TABA_REPLICATION_FACTOR", "2")]).await;
    assert_eq!(held(cluster.node(1), "orders", &ids), 40);
    assert_eq!(held(cluster.node(2), "orders", &ids), 40);

    let number = start_isolated(&mut cluster, &[("TABA_REPLICATION_FACTOR", "2")]).await;
    let gate = pause_sync(&cluster, number, "replay");
    wait_phase(cluster.node(number), "replay").await;
    // With the new node syncing, no one deletes anything: if it fails, the ring goes back to the previous one.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(held(cluster.node(1), "orders", &ids), 40);
    assert_eq!(held(cluster.node(2), "orders", &ids), 40);

    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(number)).await;
    // With the cluster sound, each node keeps only the messages of the blocks it is responsible for.
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    for n in 1..=3 {
        let me = cluster.node_id(n);
        let wanted: Vec<String> = ids
            .iter()
            .filter(|id| ring.owners(message_position(id), 2).contains(&me.as_str()))
            .cloned()
            .collect();
        let node = cluster.node(n);
        let ids_all = ids.clone();
        eventually("the node to keep only what is its own", || {
            held(node, "orders", &ids_all) == wanted.len()
        })
        .await;
        assert_eq!(held(node, "orders", &wanted), wanted.len());
    }
    let total: usize = (1..=3).map(|n| held(cluster.node(n), "orders", &ids)).sum();
    assert_eq!(total, 80, "each message on 2 nodes");
    cluster.shutdown().await;
}

/// UC: UC-SIN-004, UC-SIN-005
#[tokio::test]
async fn the_load_is_the_union_of_the_lists_of_the_sources_not_their_intersection() {
    let (mut cluster, mut ids) = two_nodes_with_messages(10, &[]).await;
    // A copy that only node 1 received (for example, the commit never reached node 2).
    let only_on_one = {
        let node = cluster.node(1);
        let messages = messages_of(node);
        let view = messages.queue_view("orders").unwrap();
        let request = taba_node::engine::messages::PublishRequest {
            body: br#"{"so":"no-1"}"#.to_vec().into(),
            ..Default::default()
        };
        let (meta, body) = messages.draft_publish(&view, &request).unwrap();
        let id = meta.id.clone();
        messages.apply_publish("orders", meta, body).unwrap();
        id
    };
    assert_eq!(
        held(
            cluster.node(2),
            "orders",
            std::slice::from_ref(&only_on_one)
        ),
        0
    );
    ids.push(only_on_one.clone());

    let number = start_isolated(&mut cluster, &[]).await;
    cluster.network.restore(&host_of(number));
    common::cluster::wait_joined(cluster.node(number)).await;
    assert_eq!(
        held(cluster.node(number), "orders", &ids),
        ids.len(),
        "what only one origin has is also loaded"
    );
    cluster.shutdown().await;
}

/// UC: UC-SIN-007, UC-SIN-012
#[tokio::test]
async fn a_message_of_a_topic_created_after_the_topic_load_waits_for_the_definition() {
    let (mut cluster, _ids) = two_nodes_with_messages(4, &[]).await;
    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "blocks");
    wait_phase(cluster.node(number), "blocks").await;

    // The node already pulled the definitions; this topic (and its message) is born later.
    create_queue(cluster.node(1), "new").await;
    let late = publish(cluster.node(2), "new", json!({"late": true})).await;

    gate.add_permits(1);
    common::cluster::wait_joined(cluster.node(number)).await;
    let node3 = cluster.node(number);
    assert_eq!(node3.client_get("/topics/new").await.status(), 200);
    assert_eq!(held(node3, "new", std::slice::from_ref(&late)), 1);
    cluster.shutdown().await;
}

/// UC: UC-SIN-016, UC-SIN-029
#[tokio::test]
async fn the_final_check_drops_what_the_load_brought_and_no_source_lists_anymore() {
    let (mut cluster, ids) = two_nodes_with_messages(12, &[]).await;
    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "verification");
    wait_phase(cluster.node(number), "verification").await;
    let node3 = cluster.node(number);
    assert_eq!(
        held(node3, "orders", &ids),
        ids.len(),
        "the load brought everything"
    );

    // A confirmation whose request never reached the new node: it disappears from the origins without going through the event
    // database.
    let gone = ids[5].clone();
    for n in 1..=2 {
        messages_of(cluster.node(n))
            .apply_ack("orders", &gone)
            .unwrap();
    }
    gate.add_permits(1);
    common::cluster::wait_joined(node3).await;
    assert_eq!(held(node3, "orders", std::slice::from_ref(&gone)), 0);
    let others: Vec<String> = ids.iter().filter(|id| **id != gone).cloned().collect();
    assert_eq!(held(node3, "orders", &others), others.len());
    cluster.shutdown().await;
}

/// UC: UC-SIN-020, UC-SIN-030
#[tokio::test]
async fn a_node_whose_join_commit_reaches_the_sources_late_waits_for_them_and_stays_in() {
    let mut cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    for n in 0..12 {
        publish(cluster.node(1), "orders", json!({ "n": n })).await;
    }
    common::cluster::settled(&cluster).await;

    // The commit reaches the other nodes well after `write.commit.timeout` (400 ms): the coordinator
    // answers `cluster.commit-timeout`, the new node starts syncing and the origins do not yet
    // know it (`cluster.not-member`). It waits for them instead of failing.
    cluster
        .network
        .set_delay_kind("commit", Duration::from_millis(1500));
    let builder = cluster.builder(4, false);
    let number = cluster.spawn_node(builder).await.expect("the node starts");
    common::cluster::wait_joined(cluster.node(number)).await;
    assert_eq!(
        cluster.node(number).node.cluster().unwrap().node_state(),
        NodeState::Active
    );
    for observer in 1..=3 {
        assert_eq!(cluster.member_ids(observer).len(), 4);
    }
    cluster.network.set_delay_kind("commit", Duration::ZERO);
    cluster.shutdown().await;
}

/// UC: UC-CNC-009
#[tokio::test]
async fn two_joins_coordinated_by_different_members_have_a_winner_and_neither_fails() {
    // The two new nodes draw the coordinator among the three members: repeats until they land on
    // different coordinators (2 out of 3 times).
    for _ in 0..12 {
        let mut cluster = TestCluster::start(3).await;
        common::cluster::settled(&cluster).await;
        // The register takes a while, so the two coordinators leave the change pending before
        // crossing paths.
        cluster
            .network
            .set_delay_kind("register", Duration::from_millis(150));
        let builders = [cluster.builder(4, false), cluster.builder(5, false)];
        let mut numbers = Vec::new();
        for builder in builders {
            numbers.push(cluster.spawn_node(builder).await.expect("the node starts"));
        }
        for number in &numbers {
            common::cluster::wait_joined(cluster.node(*number)).await;
        }
        cluster.network.set_delay_kind("register", Duration::ZERO);
        let coordinators: std::collections::HashSet<String> = cluster
            .network
            .calls()
            .iter()
            .filter(|call| call.request == "join" && [host_of(4), host_of(5)].contains(&call.from))
            .map(|call| call.to.host.clone())
            .collect();
        let mut refused = 0.0;
        for number in &numbers {
            refused += common::cluster::total(
                &cluster.node(*number).metrics().await,
                "taba_cluster_bootstrap_failures_total",
                "taba_reason=\"phase1\"",
            );
        }
        let table = cluster.table(1).unwrap();
        let all_in = table.members.len() == 5;
        cluster.shutdown().await;
        if coordinators.len() >= 2 {
            assert!(all_in, "the five nodes stayed in the table");
            assert_eq!(refused, 0.0, "no join was refused because of the crossing");
            return;
        }
    }
    panic!("the two joins never landed on different coordinators");
}
