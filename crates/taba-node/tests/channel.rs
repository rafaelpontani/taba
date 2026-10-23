//! The channel between the nodes in the simulated cluster (doc 04, section 6.1; doc 13, sections 12.8 and 12.9; doc 06c;
//! doc 20b, level **C**): connections, heartbeat, `seq` and `confirm`, resend, outgoing queue,
//! reconnection and the `taba.cluster.channel.*` metrics.
//!
//! The channel is emulated over the `Transport` (`POST /cluster/channel`): each side calls the other.

mod common;

use std::time::Duration;

use common::cluster::{
    MANY_RETRIES, QUIET, TestCluster, address_of, channels_of, create_queue, eventually,
    eventually_async, host_of, settled, signals_of, total,
};
use common::error_code;
use taba_node::cluster::channel::Channels;
use taba_node::cluster::members::Change;
use taba_node::cluster::protocol::{ChannelBatch, ChannelFrame, Keepalive, ReadyCount, Signal};
use taba_node::cluster::transport::{CallError, ChannelOpen, Transport};
use taba_node::core::error::ErrorCode;
use taba_node::core::id;

fn guid() -> String {
    id::new_at(1_791_288_000_000)
}

fn ready(topic: &str, has_ready: bool) -> Signal {
    Signal::Ready {
        topic: topic.to_owned(),
        has_ready,
    }
}

fn frame(seq: u64, signal: Signal) -> ChannelFrame {
    ChannelFrame { seq, signal }
}

/// Delivers a batch to node 1's channel as if it had come from the peer (what the reception does with each
/// message of the connection) and returns the `confirm`.
fn deliver(cluster: &TestCluster, batch: ChannelBatch) -> Vec<u64> {
    channels_of(cluster, 1).receive(batch).seqs
}

/// UC: UC-CAN-001, UC-CAN-024
#[tokio::test]
async fn every_node_has_one_channel_to_each_other_node_and_measures_it() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    for number in 1..=3 {
        let me = cluster.node_id(number);
        let peers = channels_of(&cluster, number).peers();
        let ids: Vec<String> = peers.iter().map(|p| p.node_id.clone()).collect();
        assert_eq!(peers.len(), 2, "N−1 channels on node {number}");
        assert!(!ids.contains(&me), "no channel with itself");
        for peer in &peers {
            assert_eq!(peer.version, Some(3), "the heartbeat carries the version");
            assert_eq!(peer.state.as_deref(), Some("active"));
        }

        let node = cluster.node(number);
        eventually_async("the channel metrics", || async {
            let text = node.metrics().await;
            total(&text, "taba_cluster_channel_connections", "") == 2.0
                && total(&text, "taba_cluster_channel_rtt_seconds_count", "") > 0.0
        })
        .await;
        let text = node.metrics().await;
        for id in &ids {
            assert_eq!(
                total(
                    &text,
                    "taba_cluster_channel_state",
                    &format!("taba_peer=\"{id}\"")
                ),
                1.0,
                "the channel with {id} is connected"
            );
        }
        assert_eq!(
            total(&text, "taba_cluster_channel_queue_out", ""),
            0.0,
            "nothing pending"
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-CAN-003, UC-CAN-012
#[tokio::test]
async fn the_node_that_joins_calls_every_member_and_only_members_are_answered() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    // Node 3 joined last and opened the channel with each node it received in the response.
    let calls = cluster.network.calls();
    for target in [1, 2] {
        assert!(
            calls.iter().any(|c| c.from == host_of(3)
                && c.to.host == host_of(target)
                && c.request == "channel"),
            "node 3 calls node {target}"
        );
    }

    // Whoever is not in the table is refused by the in-memory network...
    let outsider = cluster.network.transport("10.0.0.50");
    let open = ChannelOpen {
        from: guid(),
        to: cluster.node_id(1),
    };
    let batch = ChannelBatch {
        to: String::new(),
        from: guid(),
        keepalive: None,
        frames: vec![frame(1, ready("x", true))],
    };
    match outsider
        .open_channel(&address_of(&cluster, 1), open.clone())
        .await
    {
        Err(CallError::Remote(e)) => assert_eq!(e.code, ErrorCode::ClusterNotMember),
        Err(other) => panic!("expected the peer's error, got {other}"),
        Ok(_) => panic!("whoever is not a member does not open the channel"),
    }
    let _ = batch;

    // ...and by the cluster's HTTP route (the test calls from 127.0.0.1, which is not in the table): the
    // error comes out before the upgrade, as a status.
    let node = cluster.node(1);
    let upgrade = |to: &str| {
        node.client
            .get(node.cluster_url(&format!("/cluster/channel?from={}&to={to}", guid())))
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
    };
    let response = upgrade(&cluster.node_id(1)).await.unwrap();
    assert_eq!(error_code(response, 403).await, "cluster.not-member");
    // Another process at the address (the `to` is not this node) is refused before looking at who calls.
    let response = upgrade(&guid()).await.unwrap();
    assert_eq!(error_code(response, 421).await, "cluster.wrong-node");
    cluster.shutdown().await;
}

/// UC: UC-CAN-006, UC-CAN-009, UC-CAN-011
#[tokio::test]
async fn frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored() {
    let cluster = TestCluster::start_with(2, &[QUIET]).await;
    settled(&cluster).await;
    let node2 = cluster.node_id(2);
    let signals = signals_of(&cluster, 1);

    // A `seq` well above the ones node 2 really uses.
    let base = 1_000_000;
    let batch = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: None,
        frames: vec![
            frame(base + 1, ready("x", true)),
            frame(base + 2, ready("y", true)),
        ],
    };
    let confirm = deliver(&cluster, batch);
    assert_eq!(
        confirm,
        vec![base + 1, base + 2],
        "the confirm groups the seqs"
    );
    // The confirm only comes back after processing.
    assert_eq!(signals.ready_peers("x"), vec![node2.clone()]);
    assert_eq!(signals.ready_peers("y"), vec![node2.clone()]);

    // The confirmation got lost and the sender resent the same `seq`, now with other content: the
    // receiver returns the confirm and does not process again.
    let repeated = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: None,
        frames: vec![frame(base + 1, ready("x", false))],
    };
    let confirm = deliver(&cluster, repeated);
    assert_eq!(confirm, vec![base + 1]);
    assert_eq!(
        signals.ready_peers("x"),
        vec![node2.clone()],
        "the already processed seq was ignored"
    );
    let next = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: None,
        frames: vec![frame(base + 3, ready("x", false))],
    };
    assert_eq!(deliver(&cluster, next), vec![base + 3]);
    assert!(signals.ready_peers("x").is_empty(), "the new seq counts");

    // The heartbeat has no `seq`: it is neither confirmed nor stays in anyone's queue.
    let alive = ChannelBatch {
        to: String::new(),
        from: node2.clone(),
        keepalive: Some(Keepalive {
            at: "2026-10-05T12:00:00.000Z".into(),
            load: 0,
            version: 99,
            state: "active".into(),
            ready: vec![ReadyCount {
                topic: "z".into(),
                count: 3,
            }],
        }),
        frames: vec![],
    };
    assert!(
        deliver(&cluster, alive).is_empty(),
        "the heartbeat has no confirm"
    );
    let seen = channels_of(&cluster, 1)
        .peers()
        .into_iter()
        .find(|p| p.node_id == node2)
        .unwrap();
    assert_eq!(seen.version, Some(99), "the heartbeat was applied");
    assert_eq!(signals.ready_peers("z"), vec![node2]);
    for number in 1..=2 {
        assert!(
            channels_of(&cluster, number)
                .peers()
                .iter()
                .all(|p| p.queue == 0),
            "the heartbeat does not stay in node {number}'s outgoing queue"
        );
    }
    cluster.shutdown().await;
}

/// A long-polling `GET` on node 1 (the queue is empty): the node tells the others it has a listener.
fn listen(
    cluster: &TestCluster,
    number: usize,
    topic: &str,
    seconds: u32,
) -> tokio::task::JoinHandle<()> {
    let node = cluster.node(number);
    let client = node.client.clone();
    let url = node.client_url(&format!(
        "/topics/{topic}/messages?waitTime={seconds}&maxMessages=1&visibilityTimeout=60"
    ));
    tokio::spawn(async move {
        let _ = client.get(url).send().await;
    })
}

/// UC: UC-CAN-010, UC-FAL-006
#[tokio::test]
async fn a_signal_without_confirm_waits_in_the_queue_and_goes_out_when_the_channel_returns() {
    let cluster = TestCluster::start_with(3, &[MANY_RETRIES]).await;
    create_queue(cluster.node(1), "orders").await;
    settled(&cluster).await;
    let node2 = cluster.node_id(2);
    let channels = channels_of(&cluster, 1);
    let pending = |channels: &Channels| {
        channels
            .peers()
            .into_iter()
            .find(|p| p.node_id == node2)
            .unwrap()
    };

    cluster.network.partition(&host_of(1), &host_of(2));
    let waiting = listen(&cluster, 1, "orders", 3);
    let channels_for_wait = channels.clone();
    let node2_for_wait = node2.clone();
    eventually("the listener's signal in the queue to node 2", move || {
        let peer = channels_for_wait
            .peers()
            .into_iter()
            .find(|p| p.node_id == node2_for_wait)
            .unwrap();
        !peer.up && peer.queue >= 1
    })
    .await;
    // Node 3 is still reachable: the queue to it empties on its own.
    let third = cluster.node_id(3);
    let channels_for_third = channels.clone();
    eventually("node 3 to confirm the signal", move || {
        channels_for_third
            .peers()
            .into_iter()
            .find(|p| p.node_id == third)
            .is_some_and(|p| p.up && p.queue == 0)
    })
    .await;

    cluster.network.heal(&host_of(1), &host_of(2));
    let after = channels.clone();
    let node2_after = node2.clone();
    eventually(
        "the channel to come back and the queue to empty",
        move || {
            after
                .peers()
                .into_iter()
                .find(|p| p.node_id == node2_after)
                .is_some_and(|p| p.up && p.queue == 0)
        },
    )
    .await;
    assert!(pending(&channels).up);
    // No one was excluded: the channel came back before the vote had a reason to open.
    assert_eq!(cluster.member_ids(1).len(), 3);
    assert_eq!(cluster.network.count("vote"), 0);
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// UC: UC-CAN-020
#[tokio::test]
async fn a_queue_that_waits_too_long_opens_the_vote_without_waiting_for_the_retries() {
    let cluster = TestCluster::start_with(
        3,
        &[
            MANY_RETRIES,
            ("TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE", "300ms"),
        ],
    )
    .await;
    create_queue(cluster.node(1), "orders").await;
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    cluster.network.partition(&host_of(1), &host_of(2));
    let waiting = listen(&cluster, 1, "orders", 3);

    // The signal ages in the queue: the vote opens without waiting for the 1000 attempts.
    let node = cluster.node(1);
    eventually_async("the overflowed queue to open the vote", || async {
        let text = node.metrics().await;
        total(
            &text,
            "taba_cluster_channel_queue_dropped_total",
            "taba_reason=\"overflow\"",
        ) >= 1.0
            && total(&text, "taba_cluster_vote_total", "") >= 1.0
    })
    .await;
    assert!(
        cluster.network.count("vote") >= 1,
        "node 1 asked the others for the vote"
    );

    // Node 3 still reaches node 2 and votes no: no one leaves the table.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(cluster.table(1).unwrap().version, version);
    assert_eq!(cluster.member_ids(1).len(), 3);

    cluster.network.heal(&host_of(1), &host_of(2));
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// UC: UC-CAN-021, UC-FAL-003
#[tokio::test]
async fn a_channel_that_returns_before_the_retries_end_changes_nothing() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "orders").await;
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    // The next two calls of the channel to node 2 are lost (fewer than the 3 attempts).
    cluster.network.lose_kind(&host_of(2), "channel", 2);
    let waiting = listen(&cluster, 1, "orders", 2);

    let nodes = [cluster.node(1), cluster.node(3)];
    eventually_async("the counted reconnection", || async {
        let mut ok = 0.0;
        for node in nodes {
            let text = node.metrics().await;
            ok += total(
                &text,
                "taba_cluster_channel_reconnects_total",
                "taba_outcome=\"ok\"",
            );
        }
        ok >= 1.0
    })
    .await;
    settled(&cluster).await;

    assert_eq!(cluster.network.count("vote"), 0, "no vote was opened");
    assert_eq!(cluster.table(1).unwrap().version, version);
    assert_eq!(cluster.member_ids(1).len(), 3);
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// The send with confirmation (what the node that just became ready uses): returns when all the peers
/// confirmed and gives up at the deadline if one does not confirm, leaving the signal in its queue.
#[tokio::test]
async fn a_broadcast_with_confirm_returns_when_every_peer_confirmed_and_gives_up_at_the_deadline() {
    let cluster = TestCluster::start_with(3, &[MANY_RETRIES]).await;
    settled(&cluster).await;
    let channels = channels_of(&cluster, 1);
    let state = || Signal::NodeState {
        state: "active".to_owned(),
    };

    let started = std::time::Instant::now();
    channels
        .broadcast_confirmed(state(), Duration::from_secs(5))
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "all confirmed: does not wait for the deadline"
    );
    assert!(channels.peers().iter().all(|p| p.queue == 0));

    // Node 2 does not confirm: the send waits the whole deadline and the signal stays in its queue.
    cluster.network.partition(&host_of(1), &host_of(2));
    let started = std::time::Instant::now();
    channels
        .broadcast_confirmed(state(), Duration::from_millis(600))
        .await;
    assert!(started.elapsed() >= Duration::from_millis(600));
    let node2 = cluster.node_id(2);
    let waiting = channels
        .peers()
        .into_iter()
        .find(|p| p.node_id == node2)
        .unwrap();
    assert!(
        waiting.queue >= 1,
        "the unconfirmed signal stays in the queue"
    );
    cluster.shutdown().await;
}

async fn publish_json(node: &common::TestNode, topic: &str, body: serde_json::Value) {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages")))
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
}

/// UC: UC-CAN-005
#[tokio::test]
async fn the_keepalive_carries_the_time_the_state_the_version_and_the_busiest_topics_up_to_the_limit()
 {
    let cluster = TestCluster::start(3).await;
    for topic in ["a", "b", "c", "d"] {
        create_queue(cluster.node(1), topic).await;
        for n in 0..12 {
            publish_json(cluster.node(1), topic, serde_json::json!({ "n": n })).await;
        }
    }
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    // Each node owns part of each topic's messages; one of them has ready ones in three or more.
    let (richest, all) = (1..=3)
        .map(|n| (n, signals_of(&cluster, n).keepalive(10)))
        .max_by_key(|(_, keepalive)| keepalive.ready.len())
        .unwrap();
    assert!(
        all.ready.len() >= 3,
        "topics with ready ones: {:?}",
        all.ready
    );
    assert_eq!(all.state, "active");
    assert_eq!(all.version, version);
    assert!(
        all.at.ends_with('Z'),
        "the node's date and time: {}",
        all.at
    );

    // Only the topics with ready ones, the ones with the most messages first, up to the limit
    // (`cluster.channel.keepalive.max-topics`).
    let limited = signals_of(&cluster, richest).keepalive(2);
    assert_eq!(limited.ready.len(), 2);
    assert!(limited.ready[0].count >= limited.ready[1].count);
    assert_eq!(limited.ready, all.ready[..2]);
    assert!(all.ready.iter().all(|r| r.count > 0));
    cluster.shutdown().await;
}

/// UC: UC-CAN-008
#[tokio::test]
async fn a_channel_opens_with_a_snapshot_of_the_state_the_listeners_and_the_ready_topics() {
    let cluster = TestCluster::start(3).await;
    create_queue(cluster.node(1), "a").await;
    create_queue(cluster.node(1), "b").await;
    for n in 0..12 {
        publish_json(cluster.node(1), "a", serde_json::json!({ "n": n })).await;
    }
    // A waiting `GET` on node 2 is a listener of topic `b`.
    let waiting = {
        let url = cluster.node(2).client_url("/topics/b/messages?waitTime=20");
        let client = cluster.node(2).client.clone();
        tokio::spawn(async move { client.get(url).send().await })
    };
    let signals = signals_of(&cluster, 2);
    let probe = signals.clone();
    eventually("node 2's listener to appear", move || {
        probe.local_listener_count("b") == 1
    })
    .await;

    // The snapshot: the state first and then a `listeners` and a `ready` for each topic in which
    // the state is true.
    let snapshot = signals.snapshot();
    assert_eq!(
        snapshot[0],
        Signal::NodeState {
            state: "active".into()
        }
    );
    assert!(snapshot.contains(&Signal::Listeners {
        topic: "b".into(),
        has_listeners: true
    }));
    let with_ready = (1..=3)
        .filter(|n| {
            signals_of(&cluster, *n)
                .snapshot()
                .contains(&ready("a", true))
        })
        .count();
    assert!(with_ready >= 1, "some node owns messages of `a`");
    assert!(
        !snapshot.contains(&ready("b", true)),
        "only what is true goes in the snapshot"
    );
    waiting.abort();
    cluster.shutdown().await;
}

/// UC: UC-FAL-001
#[tokio::test]
async fn there_is_no_ping_apart_from_the_channel_between_the_nodes() {
    // The upgrade round (version of the members, `status`) and the reunion are periodic calls of their own, every
    // 10 heartbeats; they are pushed out of the observed second so the test sees only what keeps the nodes alive.
    let cluster = TestCluster::start_with(3, &[("TABA_CLUSTER_REUNION_INTERVAL", "600s")]).await;
    settled(&cluster).await;
    cluster.network.clear_calls();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let kinds: std::collections::BTreeSet<&str> = cluster
        .network
        .calls()
        .iter()
        .map(|call| call.request)
        .collect();
    assert!(!kinds.is_empty(), "o canal conversa sozinho");
    assert_eq!(
        kinds,
        std::collections::BTreeSet::from(["channel"]),
        "at rest only the channel talks: no separate ping or probe"
    );
    cluster.shutdown().await;
}

/// UC: UC-CAN-013
#[tokio::test]
async fn two_connections_at_once_end_with_a_single_channel_per_pair() {
    let cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    let (id1, id2) = (cluster.node_id(1), cluster.node_id(2));

    // Node 2 joined and opened the channel. Node 1 opens another at the same time (like whoever detected the fall
    // first): node 2 has both and keeps the one opened by the node with the highest ID hash.
    let mut extra = cluster
        .network
        .transport(host_of(1))
        .open_channel(
            &address_of(&cluster, 2),
            ChannelOpen {
                from: id1.clone(),
                to: id2.clone(),
            },
        )
        .await
        .expect("node 2 accepts a member's channel");
    let one_wins = taba_node::core::ring::position(id1.as_bytes())
        > taba_node::core::ring::position(id2.as_bytes());

    // If node 1 has the highest hash, its connection stays and node 2's falls (node 1 reopens it, replacing the
    // test's, which is on the same side); otherwise the node with the lowest hash (1) closes the one it opened.
    if !one_wins {
        let closed = tokio::time::timeout(Duration::from_secs(2), extra.rx.recv())
            .await
            .expect("the connection of the lowest hash should close");
        assert!(closed.is_none());
    }
    // In any order, in the end there is a single channel between the two, up and talking.
    eventually_async("the single channel reestablished", || async {
        settled_pair(&cluster)
    })
    .await;
    assert!(one_wins || extra.rx.recv().await.is_none());
    cluster.shutdown().await;
}

fn settled_pair(cluster: &TestCluster) -> bool {
    (1..=2).all(|n| {
        let peers = channels_of(cluster, n).peers();
        peers.len() == 1 && peers[0].up && peers[0].version.is_some()
    })
}

/// UC: UC-CAN-018
#[tokio::test]
async fn each_peer_has_an_out_queue_and_an_in_queue_that_drain() {
    let cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    // A burst of signals from node 1: each goes through the outgoing queue to node 2 and through its
    // incoming queue, and both return to zero.
    let channels = channels_of(&cluster, 1);
    for n in 0..300 {
        channels.broadcast(ready(&format!("t{n}"), true));
    }
    let signals = signals_of(&cluster, 2);
    let node1 = cluster.node_id(1);
    eventually("the burst to reach node 2", || {
        signals.ready_peers("t299") == vec![node1.clone()]
    })
    .await;
    eventually_async("as queues esvaziam", || async {
        let one = cluster.node(1).metrics().await;
        let two = cluster.node(2).metrics().await;
        total(&one, "taba_cluster_channel_queue_out", "") == 0.0
            && total(&two, "taba_cluster_channel_queue_in", "") == 0.0
    })
    .await;
    let text = cluster.node(2).metrics().await;
    assert!(
        text.contains("taba_cluster_channel_queue_in{")
            && text.contains(&format!("taba_peer=\"{node1}\"")),
        "the incoming queue is per origin node"
    );
    cluster.shutdown().await;
}

/// UC: UC-CAN-026
#[tokio::test]
async fn a_channel_call_without_the_destination_id_is_not_checked_against_it() {
    let cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    let node = cluster.node(1);
    let upgrade = |query: String| {
        node.client
            .get(node.cluster_url(&format!("/cluster/channel?{query}")))
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
    };
    // Without `to` (absent or empty) the call goes on to the other checks: whoever is not a member
    // is refused for that, not for being the wrong node.
    let response = upgrade(format!("from={}", guid())).await.unwrap();
    assert_eq!(error_code(response, 403).await, "cluster.not-member");
    let response = upgrade(format!("from={}&to=", guid())).await.unwrap();
    assert_eq!(error_code(response, 403).await, "cluster.not-member");
    // With a `to` that is not this node, the error is another.
    let response = upgrade(format!("from={}&to={}", guid(), guid()))
        .await
        .unwrap();
    assert_eq!(error_code(response, 421).await, "cluster.wrong-node");
    cluster.shutdown().await;
}

fn membership_batch(from: &str, seq: u64, version: u64, change: Change) -> ChannelBatch {
    ChannelBatch {
        from: from.to_owned(),
        to: String::new(),
        keepalive: None,
        frames: vec![frame(
            seq,
            Signal::Membership {
                operation_id: guid(),
                version,
                change,
            },
        )],
    }
}

/// Two peers tell consecutive version changes, each through its own queue, and the one with the higher version arrives
/// first: the node waits for the earlier one and applies both in order (D126).
/// UC: UC-CNC-015
#[tokio::test]
async fn membership_changes_from_two_peers_arriving_out_of_order_apply_in_version_order() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let start = cluster.table(1).unwrap().version;
    let (two, three) = (cluster.node_id(2), cluster.node_id(3));

    // The next version removes node 3 (told by node 2); the later one removes node 2 (told by node 3).
    let ahead = membership_batch(
        &three,
        1_000_000,
        start + 2,
        Change::Remove {
            node_id: two.clone(),
        },
    );
    assert_eq!(
        deliver(&cluster, ahead),
        [1_000_000],
        "the frame is confirmed"
    );
    assert_eq!(
        cluster.table(1).unwrap().version,
        start,
        "the version ahead waits for the earlier one"
    );
    assert_eq!(cluster.member_ids(1).len(), 3);

    let next = membership_batch(
        &two,
        1_000_000,
        start + 1,
        Change::Remove {
            node_id: three.clone(),
        },
    );
    assert_eq!(deliver(&cluster, next), [1_000_000]);
    assert_eq!(
        cluster.table(1).unwrap().version,
        start + 2,
        "both applied, in order"
    );
    assert_eq!(cluster.member_ids(1), [cluster.node_id(1)]);
    cluster.shutdown().await;
}

/// The frame that removes the node itself from the table is not applied: whoever handles its exit is the
/// exclusion rule (the peers say it is not a member), and not a change it receives (D126).
/// UC: UC-CNC-015
#[tokio::test]
async fn a_membership_frame_that_removes_the_receiving_node_is_ignored() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let start = cluster.table(1).unwrap().version;
    let (me, two) = (cluster.node_id(1), cluster.node_id(2));

    let excluded = membership_batch(
        &two,
        1_000_000,
        start + 1,
        Change::Exclude {
            node_id: me.clone(),
        },
    );
    assert_eq!(
        deliver(&cluster, excluded),
        [1_000_000],
        "it is confirmed anyway"
    );
    let drained = membership_batch(
        &two,
        1_000_001,
        start + 1,
        Change::Remove {
            node_id: me.clone(),
        },
    );
    assert_eq!(deliver(&cluster, drained), [1_000_001]);

    assert_eq!(
        cluster.table(1).unwrap().version,
        start,
        "the table did not change"
    );
    assert!(
        cluster.member_ids(1).contains(&me),
        "the node stays in its own table"
    );
    cluster.shutdown().await;
}
