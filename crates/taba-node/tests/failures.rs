//! A node's failure in the simulated cluster (doc 04, sections 6.3 and 6.8; doc 05, section 7.3; doc 06c;
//! doc 20c, level **C**): detection, exclusion vote, unregistration by failure, `node-failed`,
//! partition and concurrency between member changes.
//!
//! "Falling" is isolating the node from the in-memory network (`isolate`): to the others it stops answering, and it
//! itself stops reaching the others.

mod common;

use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{
    MANY_RETRIES, NO_VOTE, TestCluster, address_of, channels_of, create_queue, eventually,
    eventually_async, host_of, name_owned_by, pause_sync, settled, start_isolated, total,
    wait_joined, wait_phase,
};
use taba_node::cluster::members::MemberTable;
use taba_node::cluster::protocol::{Ballot, Request, Response, VoteReason, VoteRequest};
use taba_node::cluster::state::NodeState;
use taba_node::cluster::transport::{PeerAddr, Transport};
use taba_node::core::clock::Clock;
use taba_node::core::id;
use taba_node::core::ring::message_position;

const OPENED_EXCLUDED: &str = "taba_outcome=\"excluded\",taba_role=\"opened\"";

fn guid() -> String {
    id::new_at(1_791_288_000_000)
}

/// The tables of nodes `numbers` are equal, without node `gone`, and have `size` members.
fn agree_without(cluster: &TestCluster, numbers: &[usize], gone: usize, size: usize) -> bool {
    let tables: Vec<Option<MemberTable>> = numbers.iter().map(|n| cluster.table(*n)).collect();
    let gone = cluster.node_id(gone);
    tables.iter().all(|t| {
        t.as_ref()
            .is_some_and(|t| t.members.len() == size && t.member(&gone).is_none())
    }) && tables.windows(2).all(|w| w[0] == w[1])
}

/// The sum of series `name` with label `label` on all the nodes `numbers`.
async fn sum_of(cluster: &TestCluster, numbers: &[usize], name: &str, label: &str) -> f64 {
    let mut sum = 0.0;
    for number in numbers {
        sum += total(&cluster.node(*number).metrics().await, name, label);
    }
    sum
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

fn messages_of(node: &TestNode) -> std::sync::Arc<taba_node::engine::messages::Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

/// A queue with some messages, published by different nodes.
async fn queue_with_messages(cluster: &TestCluster, count: usize) -> Vec<String> {
    create_queue(cluster.node(1), "orders").await;
    let mut ids = Vec::new();
    for n in 0..count {
        let via = 1 + n % cluster.nodes.len();
        ids.push(publish(cluster.node(via), "orders", json!({"n": n})).await);
    }
    ids
}

/// UC: UC-FAL-004, UC-FAL-010, UC-FAL-014, UC-FAL-015, UC-FAL-016, UC-FAL-019, UC-FAL-020
#[tokio::test]
async fn a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let before = cluster.table(1).unwrap().version;

    cluster.network.isolate(&host_of(3));

    eventually("the two healthy ones to exclude node 3", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    assert_eq!(
        cluster.table(1).unwrap().version,
        before + 1,
        "a single member change"
    );

    // The unregistration followed the two phases, with the two nodes that remain.
    assert!(cluster.network.count("vote") >= 1, "asked for the vote");
    assert!(cluster.network.count("unregister") >= 1, "fase 1");
    assert!(cluster.network.count("commit") >= 1, "fase 2");

    // The vote and the ballot were counted, and the member change too (by phase and reason).
    eventually_async("the vote and the result in the metrics", || async {
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            OPENED_EXCLUDED,
        )
        .await
            >= 1.0
            && sum_of(
                &cluster,
                &[1, 2],
                "taba_cluster_vote_total",
                "taba_role=\"cast\"",
            )
            .await
                >= 1.0
    })
    .await;
    for phase in ["register", "commit"] {
        let label = format!(
            "taba_operation=\"unregister\",taba_outcome=\"ok\",taba_phase=\"{phase}\",taba_reason=\"vote\""
        );
        eventually_async("the unregistration phase by vote counted", || async {
            sum_of(
                &cluster,
                &[1, 2],
                "taba_cluster_membership_changes_total",
                &label,
            )
            .await
                >= 2.0
        })
        .await;
    }

    // The excluded node's channel disappears from the two that remained, and the signal queue for it with it.
    for number in [1, 2] {
        let channels = channels_of(&cluster, number);
        eventually("the excluded node's channel to disappear", move || {
            channels.peers().len() == 1
        })
        .await;
    }

    // The isolated node sees only itself as healthy: fewer than `min-voters`, it excludes no one.
    let lonely = cluster.node(3);
    eventually_async("the isolated node without enough voters", || async {
        total(
            &lonely.metrics().await,
            "taba_cluster_vote_total",
            "taba_outcome=\"quorum-lost\"",
        ) >= 1.0
    })
    .await;
    assert_eq!(
        cluster.member_ids(3).len(),
        3,
        "the isolated node excludes no one"
    );
    cluster.shutdown().await;
}

/// UC: UC-FAL-010
#[tokio::test]
async fn with_only_two_nodes_a_cut_excludes_nobody() {
    let cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    let version = cluster.table(1).unwrap().version;

    cluster.network.isolate(&host_of(2));
    eventually_async("the vote without enough voters", || async {
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            "taba_outcome=\"quorum-lost\"",
        )
        .await
            >= 2.0
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    for number in [1, 2] {
        assert_eq!(cluster.table(number).unwrap().version, version);
        assert_eq!(cluster.member_ids(number).len(), 2);
    }
    assert_eq!(cluster.network.count("unregister"), 0);
    cluster.shutdown().await;
}

/// UC: UC-FAL-011
#[tokio::test]
async fn vote_remove_answers_no_to_who_still_reaches_the_candidate_and_yes_to_who_does_not() {
    let cluster = TestCluster::start_with(3, &[MANY_RETRIES]).await;
    settled(&cluster).await;
    let (second, third) = (cluster.node_id(2), cluster.node_id(3));
    let node1 = address_of(&cluster, 1);
    let ask = |candidate: String| {
        let transport = cluster.network.transport(host_of(2));
        let node1 = node1.clone();
        let opened_by = second.clone();
        async move {
            let request = Request::Vote(VoteRequest {
                vote_id: guid(),
                candidate_node_id: candidate,
                opened_by,
                reason: VoteReason::ChannelDown,
            });
            match transport.call(&node1, request).await {
                Ok(Response::Vote(answer)) => answer.vote,
                other => panic!("expected a vote, got {other:?}"),
            }
        }
    };

    // Node 1 reaches node 3: votes no. Whoever is not a member and the voter itself are also no.
    assert_eq!(ask(third.clone()).await, Ballot::No);
    assert_eq!(ask(guid()).await, Ballot::No);
    assert_eq!(ask(cluster.node_id(1)).await, Ballot::No, "estou vivo");

    // With node 1 cut off from node 3, it no longer reaches it and votes yes.
    cluster.network.partition(&host_of(1), &host_of(3));
    let channels = channels_of(&cluster, 1);
    eventually("node 1's channel with node 3 to fall", move || {
        channels.peers().iter().any(|p| !p.up)
    })
    .await;
    assert_eq!(ask(third.clone()).await, Ballot::Yes);

    // The votes were counted, and giving the vote excludes no one.
    let text = cluster.node(1).metrics().await;
    assert!(total(&text, "taba_cluster_vote_total", "taba_outcome=\"yes\"") >= 1.0);
    assert!(total(&text, "taba_cluster_vote_total", "taba_outcome=\"no\"") >= 3.0);
    assert_eq!(cluster.member_ids(1).len(), 3);
    cluster.shutdown().await;
}

/// UC: UC-FAL-005, UC-FAL-023
#[tokio::test]
async fn a_node_that_announces_its_failure_is_excluded_while_its_channel_is_still_up() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    cluster.node(3).node.shared().set_failed();

    eventually(
        "the two healthy ones to exclude the node that failed",
        || agree_without(&cluster, &[1, 2], 3, 2),
    )
    .await;
    // The channel never fell: it was the node's notice, not the connection, that opened the vote.
    assert_eq!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_channel_reconnects_total",
            ""
        )
        .await,
        0.0
    );
    assert!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            OPENED_EXCLUDED
        )
        .await
            >= 1.0
    );
    // The node stays in `failed`, out of rotation, and does not try to come back on its own.
    let ready = cluster.node(3).client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "failed");
    cluster.shutdown().await;
}

/// UC: UC-FAL-005
#[tokio::test]
async fn a_queue_that_overflows_opens_the_vote_that_excludes_the_unreachable_node() {
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

    cluster.network.isolate(&host_of(3));
    // A listener on node 1 generates a signal for node 3 that does not leave: the queue ages and overflows.
    let node = cluster.node(1);
    let client = node.client.clone();
    let url =
        node.client_url("/topics/orders/messages?waitTime=3&maxMessages=1&visibilityTimeout=60");
    let waiting = tokio::spawn(async move {
        let _ = client.get(url).send().await;
    });

    eventually("node 3 excluded after the queue overflow", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    assert!(
        sum_of(
            &cluster,
            &[1],
            "taba_cluster_channel_queue_dropped_total",
            "taba_reason=\"overflow\""
        )
        .await
            >= 1.0
    );
    waiting.await.unwrap();
    cluster.shutdown().await;
}

/// UC: UC-FAL-002
#[tokio::test]
async fn a_node_that_stops_answering_with_the_connection_open_is_detected_and_excluded() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    // Node 3 receives the calls and does not answer: the connection is open, but nothing comes back.
    cluster.network.set_silent(&host_of(3), true);
    eventually("the mute node to be excluded for not answering", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    cluster.network.set_silent(&host_of(3), false);
    cluster.shutdown().await;
}

/// UC: UC-FAL-017
#[tokio::test]
async fn an_exclusion_that_fails_before_the_commit_rolls_back_and_the_vote_is_held_again() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;

    // Phase 1 of the unregistration is lost in the three attempts of each possible coordinator.
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "unregister", 3);
    }
    cluster.network.isolate(&host_of(3));

    eventually("the exclusion after the rollback", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    assert!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            "taba_outcome=\"failed\",taba_role=\"opened\""
        )
        .await
            >= 1.0,
        "the first vote did not make the exclusion take effect"
    );
    assert!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_membership_changes_total",
            "taba_operation=\"unregister\",taba_outcome=\"ok\",taba_phase=\"rollback\""
        )
        .await
            >= 1.0,
        "the rollback was counted"
    );
    cluster.shutdown().await;
}

/// UC: UC-FAL-017
#[tokio::test]
async fn once_the_exclusion_is_decided_the_commit_is_insisted_until_every_node_confirms() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "commit", 3);
    }
    cluster.network.isolate(&host_of(3));

    eventually("the exclusion despite the lost commits", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;
    // The coordinator insists on the commit; the channel's `membership` frame also carries the change, and whoever
    // arrives first fixes the node (D126), so it is not known how many commits were still needed.
    assert!(
        cluster.network.count("commit") >= 1,
        "the coordinator tried the commit"
    );
    assert_eq!(
        sum_of(
            &cluster,
            &[1, 2],
            "taba_cluster_vote_total",
            "taba_outcome=\"failed\""
        )
        .await,
        0.0,
        "decided, there is no going back"
    );
    cluster.shutdown().await;
}

/// UC: UC-FAL-008, UC-FAL-009, UC-FAL-012
#[tokio::test]
async fn when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time() {
    let cluster = TestCluster::start(5).await;
    settled(&cluster).await;
    let before = cluster.table(1).unwrap().version;

    // A whole zone falls: three of the five. Only two healthy remain, and the three do not enter the base
    // (otherwise 2 of 5 would not close the quorum).
    for number in [3, 4, 5] {
        cluster.network.isolate(&host_of(number));
    }
    eventually(
        "the three nodes out of the table of the two that remain",
        || {
            [1, 2].iter().all(|n| {
                cluster
                    .table(*n)
                    .is_some_and(|t| t.members.len() == 2 && t.version == before + 3)
            }) && cluster.table(1) == cluster.table(2)
        },
    )
    .await;
    // Each isolated node sees only itself and excludes no one.
    for number in [3, 4, 5] {
        assert_eq!(cluster.member_ids(number).len(), 5);
    }
    cluster.shutdown().await;
}

/// No UC: each side of the partition excludes the other (UC-FAL-013, first half). The return of the
/// excluded is proved in 15b3 and that of both sides is PRB-040.
#[tokio::test]
async fn a_partition_with_enough_healthy_nodes_on_both_sides_makes_each_side_exclude_the_other() {
    let cluster = TestCluster::start(4).await;
    settled(&cluster).await;

    for left in [1, 2] {
        for right in [3, 4] {
            cluster.network.partition(&host_of(left), &host_of(right));
        }
    }
    eventually("each side to be left with only its two nodes", || {
        let (a, b) = (cluster.member_ids(1), cluster.member_ids(3));
        a.len() == 2
            && b.len() == 2
            && cluster.table(1) == cluster.table(2)
            && cluster.table(3) == cluster.table(4)
            && a.iter().all(|id| !b.contains(id))
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-021, UC-FAL-022
#[tokio::test]
async fn the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary() {
    // No automatic vote: the test opens the vote when it wants.
    let cluster = TestCluster::start_with(4, NO_VOTE).await;
    let ids = queue_with_messages(&cluster, 40).await;
    let victim = cluster.node_id(4);
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    let owned_by_victim: Vec<String> = ids
        .iter()
        .filter(|id| ring.primary(message_position(id)) == Some(victim.as_str()))
        .cloned()
        .collect();
    assert!(!owned_by_victim.is_empty(), "node 4 owns some");
    let others: Vec<String> = ids
        .iter()
        .filter(|id| !owned_by_victim.contains(id))
        .cloned()
        .collect();

    // Node 4 falls: its messages are left without delivery, and the others' keep going out.
    cluster.network.isolate(&host_of(4));
    let channels = channels_of(&cluster, 1);
    let victim_for_wait = victim.clone();
    eventually("the channel with node 4 to fall", move || {
        channels
            .peers()
            .iter()
            .any(|p| p.node_id == victim_for_wait && !p.up)
    })
    .await;
    // Deadline of 10 s, like `eventually`: slow runners (arm64 on CI) take more than 5 s.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut delivered: Vec<String> = Vec::new();
    while tokio::time::Instant::now() < deadline {
        for message in consume(
            cluster.node(1),
            "orders",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await
        {
            delivered.push(message["id"].as_str().unwrap().to_owned());
        }
        if others.iter().all(|id| delivered.contains(id)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        others.iter().all(|id| delivered.contains(id)),
        "the other nodes' messages go out"
    );
    assert!(
        owned_by_victim.iter().all(|id| !delivered.contains(id)),
        "the fallen node's wait for the unregistration"
    );

    // The unregistration: the ring is recalculated and the messages go to a new primary.
    let voting = cluster
        .node(1)
        .node
        .cluster()
        .and_then(|c| c.replication())
        .unwrap()
        .voting
        .clone();
    voting.open(victim.clone(), VoteReason::ChannelDown).await;
    eventually("node 4 out of the table", || {
        agree_without(&cluster, &[1, 2, 3], 4, 3)
    })
    .await;
    let ring = cluster.node(1).node.cluster().unwrap().membership().ring();
    assert!(ring.nodes().all(|n| n != victim));
    assert_eq!(
        (1..=3)
            .map(|n| ring.primary_blocks_of(&cluster.node_id(n)))
            .sum::<usize>(),
        ring.block_count(),
        "the removed node's blocks went to the ones that remained"
    );

    // Each message remains on at least `replication.factor − 1` = 2 nodes.
    for id in &ids {
        let copies = (1..=3)
            .filter(|n| {
                messages_of(cluster.node(*n))
                    .message_info("orders", id)
                    .unwrap()
                    .is_some()
            })
            .count();
        assert!(copies >= 2, "message {id} has {copies} copy(ies)");
    }

    // The ones that were waiting now go out, delivered by the new primary.
    for _ in 0..200 {
        for message in consume(
            cluster.node(1),
            "orders",
            "maxMessages=10&visibilityTimeout=300",
        )
        .await
        {
            delivered.push(message["id"].as_str().unwrap().to_owned());
        }
        if owned_by_victim.iter().all(|id| delivered.contains(id)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        owned_by_victim.iter().all(|id| delivered.contains(id)),
        "the new primary delivers the removed node's messages"
    );
    cluster.shutdown().await;
}

/// UC: UC-CNC-002, UC-CNC-007, UC-FRZ-005, UC-CAN-014
#[tokio::test]
async fn a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node() {
    let mut cluster = TestCluster::start_with(3, &[("TABA_DRAIN_ON_STOP", "false")]).await;
    settled(&cluster).await;
    let old_id = cluster.node_id(3);
    let port = cluster.node(3).node.cluster_addr().port();
    cluster.network.clear_calls();

    // The unregistration takes a while: the vote stays in progress long enough for the new node to arrive.
    cluster
        .network
        .set_delay_kind("unregister", Duration::from_millis(1500));
    cluster.nodes.pop().unwrap().shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));
    eventually("the exclusion vote to open", || {
        [1, 2].iter().any(|n| {
            cluster
                .node(*n)
                .node
                .cluster()
                .is_some_and(|c| !c.open_votes().is_empty())
        })
    })
    .await;

    // With the vote open, the node restarts at the same IP and port and asks to join.
    let number = cluster
        .respawn_node(3, |builder| {
            builder.env("TABA_CLUSTER_PORT", &port.to_string())
        })
        .await
        .expect("the node restarts");
    let new_id = cluster.node_id(number);
    assert_ne!(new_id, old_id, "the ID is new");

    // At no instant does the table have the old and the new node together.
    let mut both = false;
    let mut joined = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !joined {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the new node did not join"
        );
        for observer in [1, 2] {
            if let Some(table) = cluster.table(observer) {
                both |= table.member(&old_id).is_some() && table.member(&new_id).is_some();
                joined |= table.member(&new_id).is_some() && table.member(&old_id).is_none();
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        !both,
        "the new node's request waited for the end of the vote"
    );
    // The restart is another ID: only the old one, excluded, goes into freezing, and the new one joined without waiting.
    let membership = cluster.node(1).node.cluster().unwrap().membership();
    let freezing = Duration::from_secs(300);
    let now = cluster.clock.now();
    assert!(
        membership
            .frozen_remaining(&old_id, now, freezing)
            .is_some()
    );
    assert!(
        membership
            .frozen_remaining(&new_id, now, freezing)
            .is_none()
    );
    // The load balancer also repeats the request to the new node itself (same address): only the join
    // that reached a member counts, and it arrived after the vote opened and before the exclusion commit.
    let calls = cluster.network.calls();
    let first = |name: &str| calls.iter().position(|c| c.request == name);
    let join = calls
        .iter()
        .position(|c| c.request == "join" && c.to.host != host_of(3));
    assert!(
        first("vote") < join && join < first("commit"),
        "the join request arrived in the middle of the vote"
    );
    assert_eq!(cluster.member_ids(1).len(), 3);
    wait_joined(cluster.node(number)).await;
    cluster.shutdown().await;
}

/// UC: UC-CNC-005
#[tokio::test]
async fn a_node_that_falls_while_another_syncs_is_excluded_only_when_the_sync_ends() {
    let mut cluster = TestCluster::start(2).await;
    queue_with_messages(&cluster, 6).await;
    let third = start_isolated(&mut cluster, &[("TABA_CLUSTER_SYNC_TIMEOUT", "300ms")]).await;
    let gate = pause_sync(&cluster, third, "replay");
    wait_phase(cluster.node(third), "replay").await;

    // Node 2 falls with node 3 syncing: the vote closes, but the unregistration waits.
    cluster.network.isolate(&host_of(2));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        cluster.member_ids(1).len(),
        3,
        "nothing changed during the load"
    );
    assert_eq!(
        cluster.node(third).node.shared().state(),
        NodeState::Syncing
    );

    gate.add_permits(1);
    eventually("node 2 to leave after node 3 is ready", || {
        cluster
            .table(1)
            .is_some_and(|t| t.members.len() == 2 && t.member(&cluster.node_id(2)).is_none())
    })
    .await;
    wait_joined(cluster.node(third)).await;
    cluster.shutdown().await;
}

/// UC: UC-SIN-020, UC-CNC-004, UC-FAL-023
#[tokio::test]
async fn a_syncing_node_that_fails_is_excluded_by_the_others() {
    let mut cluster = TestCluster::start(2).await;
    queue_with_messages(&cluster, 6).await;
    let number = start_isolated(&mut cluster, &[("TABA_CLUSTER_SYNC_TIMEOUT", "300ms")]).await;
    // No one serves the ID listing: the node runs out the deadline and enters failure.
    for host in [host_of(1), host_of(2)] {
        cluster.network.lose_kind(&host, "block-ids", 1_000_000);
    }
    cluster.network.restore(&host_of(number));

    let shared = cluster.node(number).node.shared().clone();
    eventually("the node to enter failure", move || {
        shared.state() == NodeState::Failed
    })
    .await;
    // The failure signal releases the vote, and the exit of a syncing node is the exception to the
    // one-change-at-a-time rule.
    eventually("the others to exclude the node that failed", || {
        agree_without(&cluster, &[1, 2], number, 2)
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-026
#[tokio::test]
async fn a_syncing_node_that_the_sources_excluded_redoes_the_join_instead_of_failing() {
    let mut cluster = TestCluster::start(2).await;
    queue_with_messages(&cluster, 6).await;
    let number = start_isolated(&mut cluster, &[]).await;
    let gate = pause_sync(&cluster, number, "blocks");
    wait_phase(cluster.node(number), "blocks").await;
    let shared = cluster.node(number).node.shared().clone();

    // The sources move on to a newer table without the node (a reunion that undid its join).
    let mut newer = cluster.table(1).unwrap();
    newer
        .members
        .retain(|m| m.node_id != cluster.node_id(number));
    newer.version += 10;
    for source in [1, 2] {
        cluster
            .node(source)
            .node
            .cluster()
            .unwrap()
            .membership()
            .install(newer.clone());
    }
    // The sources now answer `cluster.not-member` to the node's block listing.
    let joins = cluster.network.count("join");
    gate.add_permits(100);
    eventually("the node to redo the join", || {
        cluster.network.count("join") > joins
    })
    .await;
    assert_ne!(
        shared.state(),
        NodeState::Failed,
        "an excluded node redoes the join; it does not enter failure"
    );

    wait_joined(cluster.node(number)).await;
    eventually("the three nodes in a single table", || {
        (1..=3).all(|n| cluster.member_ids(n).len() == 3)
    })
    .await;
    cluster.shutdown().await;
}

/// The node that restarts at the same address waits for the cluster port to become free, instead of failing with
/// `AddrInUse`: the port is ephemeral and, in that interval, another connection or another test may take it.
#[tokio::test]
async fn a_node_restarting_on_the_same_address_waits_for_a_port_someone_else_holds() {
    let mut cluster = TestCluster::start(3).await;
    let port = cluster.node(3).node.cluster_addr().port();
    cluster.nodes.pop().unwrap().shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));

    // Someone takes the port and only releases it after a while.
    let mut holder = None;
    for _ in 0..100 {
        if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            holder = Some(listener);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let holder = holder.expect("the port becomes free after the shutdown");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        drop(holder);
    });

    let number = cluster
        .respawn_node(3, |builder| {
            builder.env("TABA_CLUSTER_PORT", &port.to_string())
        })
        .await
        .expect("the node restarts when the port is released");
    release.join().unwrap();
    assert_eq!(cluster.node(number).node.cluster_addr().port(), port);
    cluster.shutdown().await;
}

/// UC: UC-CAN-025, UC-CAN-014, UC-CNC-007
#[tokio::test]
async fn a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced() {
    // 40 attempts of 50 ms: the node restarts well before the fall is declared.
    let mut cluster = TestCluster::start_with(
        3,
        &[
            ("TABA_DRAIN_ON_STOP", "false"),
            ("TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES", "40"),
        ],
    )
    .await;
    settled(&cluster).await;
    let old_id = cluster.node_id(3);
    let port = cluster.node(3).node.cluster_addr().port();

    cluster.nodes.pop().unwrap().shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));
    let number = cluster
        .respawn_node(3, |builder| {
            builder.env("TABA_CLUSTER_PORT", &port.to_string())
        })
        .await
        .expect("the node restarts");
    let new_id = cluster.node_id(number);
    assert_ne!(new_id, old_id);

    // Before the vote opens, the new process no longer goes through the old one: the other sides' channel
    // receives `cluster.wrong-node` and counts as fallen.
    eventually("the old node's vote", || {
        [1, 2].iter().any(|n| {
            cluster
                .node(*n)
                .node
                .cluster()
                .is_some_and(|c| !c.open_votes().is_empty())
                || cluster.member_ids(*n).contains(&new_id)
        })
    })
    .await;
    eventually("the new node to replace the old one in the table", || {
        [1, 2].iter().all(|n| {
            let ids = cluster.member_ids(*n);
            ids.contains(&new_id) && !ids.contains(&old_id)
        })
    })
    .await;
    wait_joined(cluster.node(number)).await;
    assert_eq!(cluster.member_ids(1).len(), 3);
    cluster.shutdown().await;
}

/// UC: UC-FAL-026, UC-FAL-031
#[tokio::test]
async fn after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again() {
    let cluster = TestCluster::start(4).await;
    settled(&cluster).await;
    for left in [1, 2] {
        for right in [3, 4] {
            cluster.network.partition(&host_of(left), &host_of(right));
        }
    }
    eventually("each side to be left with only its two nodes", || {
        cluster.member_ids(1).len() == 2
            && cluster.member_ids(3).len() == 2
            && cluster.table(1) == cluster.table(2)
            && cluster.table(3) == cluster.table(4)
    })
    .await;

    // The network returns, but the two tables have the same version and no channel links one side to the other.
    // Each side's freezing has already passed.
    for left in [1, 2] {
        for right in [3, 4] {
            cluster.network.heal(&host_of(left), &host_of(right));
        }
    }
    cluster.network.clear_calls();
    cluster.clock.advance(Duration::from_secs(301));

    // Each side queries the table of whoever excluded it: the loser (the one with the lowest `nodeId`, in a tie)
    // redoes the join and the four tables become one again.
    eventually("the four nodes in a single table", || {
        let tables: Vec<_> = (1..=4).map(|n| cluster.table(n)).collect();
        tables.windows(2).all(|w| w[0] == w[1])
            && tables[0].as_ref().is_some_and(|t| t.members.len() == 4)
    })
    .await;
    for number in 1..=4 {
        wait_joined(cluster.node(number)).await;
    }

    // Only the losing side redid the join: the one with the lowest `nodeId` of all (a 2-to-2 tie).
    let smallest = (1..=4).min_by_key(|n| cluster.node_id(*n)).unwrap();
    let losers: Vec<String> = if smallest <= 2 {
        vec![host_of(1), host_of(2)]
    } else {
        vec![host_of(3), host_of(4)]
    };
    let joiners: std::collections::BTreeSet<String> = cluster
        .network
        .calls()
        .iter()
        .filter(|call| call.request == "join" && call.from != "balancer")
        .map(|call| call.from.clone())
        .collect();
    assert!(!joiners.is_empty(), "o perdedor refez o ingresso");
    assert!(
        joiners.iter().all(|host| losers.contains(host)),
        "the winner did not redo: {joiners:?}"
    );
    cluster.shutdown().await;
}

/// UC: UC-FAL-027
#[tokio::test]
async fn a_failed_node_forwards_the_client_calls_to_a_healthy_one_until_it_leaves() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    // The exclusion takes a while: the failed node stays in the table and receives calls for a moment.
    cluster
        .network
        .set_delay_kind("unregister", Duration::from_millis(1500));
    // The definition's owner cannot be the failed node (it only stops being owner when it is excluded).
    let name = name_owned_by(&cluster, 1, "orders-");
    cluster.node(3).node.shared().set_failed();

    let created = cluster
        .node(3)
        .client
        .post(cluster.node(3).client_url("/topics"))
        .json(&json!({ "name": name, "type": "queue" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        created.status(),
        201,
        "the call was forwarded to a healthy node"
    );
    assert_eq!(
        total(
            &cluster.node(3).metrics().await,
            "taba_cluster_forwarded_total",
            "taba_outcome=\"ok\""
        ),
        1.0,
        "the failed node forwarded, it did not serve"
    );
    // The topic was created in the cluster, but the failed node does not serve it on its own.
    let listed = cluster
        .node(1)
        .client
        .get(cluster.node(1).client_url(&format!("/topics/{name}")))
        .send()
        .await
        .unwrap();
    assert_eq!(listed.status(), 200);
    cluster.network.set_delay_kind("unregister", Duration::ZERO);
    cluster.shutdown().await;
}

/// UC: UC-FAL-027
#[tokio::test]
async fn a_failed_single_node_refuses_the_client_calls_with_503() {
    let node = TestNode::single().await;
    node.node.shared().set_failed();
    let response = node.client_get("/topics").await;
    assert_eq!(response.status(), 503);
    assert!(response.headers().contains_key("retry-after"));
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "node.failed");
    // The operations routes continue: that is how the failure is seen.
    let ready = node.client_get_ops("/health/ready").await;
    assert_eq!(ready.status(), 503);
    assert_eq!(ready.json::<Value>().await.unwrap()["reason"], "failed");
    node.shutdown().await;
}

/// UC: UC-CAN-022
#[tokio::test]
async fn once_the_exclusion_is_decided_the_nodes_drop_the_channel_and_the_return_starts_with_empty_queues()
 {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let third = cluster.node_id(3);
    cluster.network.isolate(&host_of(3));
    eventually("the two healthy ones to exclude node 3", || {
        agree_without(&cluster, &[1, 2], 3, 2)
    })
    .await;

    // The channel and the queues to the excluded were discarded: they no longer accumulate.
    for number in [1, 2] {
        let peers = channels_of(&cluster, number).peers();
        assert!(
            peers.iter().all(|p| p.node_id != third),
            "node {number}: the channel with the excluded was discarded"
        );
    }
    let label = format!("taba_peer=\"{third}\"");
    assert_eq!(
        sum_of(&cluster, &[1, 2], "taba_cluster_channel_queue_out", &label).await,
        0.0
    );

    // If the node comes back, it redoes the bootstrap and the channels are born new and empty.
    cluster.network.restore(&host_of(3));
    cluster.clock.advance(Duration::from_secs(301));
    wait_joined(cluster.node(3)).await;
    eventually("the new channels, up and empty", || {
        [1, 2].iter().all(|n| {
            channels_of(&cluster, *n)
                .peers()
                .iter()
                .any(|p| p.node_id == third && p.up && p.queue == 0)
        })
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-FAL-029
#[tokio::test]
async fn only_the_lowest_node_id_among_the_healthy_coordinates_the_exclusion() {
    let cluster = TestCluster::start(5).await;
    settled(&cluster).await;
    let lowest = (1..=4)
        .min_by_key(|n| cluster.node_id(*n))
        .expect("four healthy");
    cluster.network.clear_calls();

    cluster.network.isolate(&host_of(5));
    eventually("the four healthy ones to exclude node 5", || {
        agree_without(&cluster, &[1, 2, 3, 4], 5, 4)
    })
    .await;

    // All opened the vote, but only the one with the lowest ID registered the unregistration on the others: the rest
    // waited their turn and found the node already out of the table.
    let coordinators: std::collections::BTreeSet<String> = cluster
        .network
        .calls()
        .iter()
        .filter(|call| call.request == "unregister")
        .map(|call| call.from.clone())
        .collect();
    assert_eq!(
        coordinators,
        std::collections::BTreeSet::from([host_of(lowest)]),
        "the exclusion's coordinator"
    );
    assert!(
        cluster.network.count("vote") >= 4,
        "all the healthy ones voted"
    );
    cluster.shutdown().await;
}
