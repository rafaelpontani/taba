//! The excluded node that comes back and the freezing period (doc 04, sections 6.6 and 6.7; doc 20c, level **C**):
//! the same process, with the same ID, is refused with `403 cluster.node-frozen` until the freezing ends
//! and then redoes the join from scratch. The `Clock` is manual: the test advances the clock instead of waiting.

mod common;

use std::time::Duration;

use common::cluster::{
    TestCluster, address_of, eventually, eventually_async, host_of, settled, signals_of, total,
    wait_joined,
};
use taba_node::cluster::protocol::{JoinRequest, Request, Signal};
use taba_node::cluster::state::NodeState;
use taba_node::cluster::transport::{CallError, Transport};
use taba_node::core::clock::Clock;
use taba_node::core::error::ErrorCode;

/// The 5 minutes of `cluster.freezing.duration`.
const FREEZING: Duration = Duration::from_secs(300);

/// A join request from node `number`, as it would make it (same ID, same address).
fn join_as(cluster: &TestCluster, number: usize) -> Request {
    let local = cluster
        .node(number)
        .node
        .cluster()
        .expect("the node is in the cluster")
        .local();
    Request::Join(JoinRequest {
        node_id: local.node_id,
        addresses: local.addresses,
        port: local.port,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// What the cluster answers to a join request from node `number` made to node `to`.
async fn join_error(cluster: &TestCluster, number: usize, to: usize) -> Option<(ErrorCode, u64)> {
    let transport = cluster.network.transport(host_of(number));
    match transport
        .call(&address_of(cluster, to), join_as(cluster, number))
        .await
    {
        Err(CallError::Remote(error)) => Some((error.code, error.retry_after.unwrap_or(0))),
        _ => None,
    }
}

/// Nodes `numbers` keep node `gone` in freezing and have the table without it.
fn frozen_and_gone(cluster: &TestCluster, numbers: &[usize], gone: usize) -> bool {
    let id = cluster.node_id(gone);
    numbers.iter().all(|n| {
        let service = cluster.node(*n).node.cluster().unwrap();
        !service.membership().is_member(&id)
            && service
                .membership()
                .frozen_remaining(&id, cluster.clock.now(), FREEZING)
                .is_some()
    })
}

/// UC: UC-FAL-013, UC-FRZ-001, UC-FRZ-002, UC-FRZ-003, UC-FRZ-004
#[tokio::test]
async fn an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let id = cluster.node_id(3);

    cluster.network.isolate(&host_of(3));
    eventually("the two healthy ones to exclude node 3", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;
    // The cluster keeps the excluded one's ID and when it left; an ID that never left is not in freezing.
    let service = cluster.node(1).node.cluster().unwrap();
    assert!(
        service
            .membership()
            .frozen_remaining(&cluster.node_id(1), cluster.clock.now(), FREEZING)
            .is_none()
    );

    // The network returns: the peers answer `not-member`, node 3 sees it was excluded, deletes what it had and
    // redoes the join from scratch (the same process, the same ID).
    cluster.network.restore(&host_of(3));
    let third = cluster.node(3);
    eventually_async("node 3 to be refused by freezing", || async {
        total(
            &third.metrics().await,
            "taba_cluster_bootstrap_failures_total",
            "taba_reason=\"frozen\"",
        ) >= 2.0
    })
    .await;
    assert_ne!(
        third.node.shared().state(),
        NodeState::Active,
        "out of the cluster while the freezing lasts"
    );
    assert_eq!(third.node.shared().node_id(), id, "still the same ID");

    // The refusal is `403 cluster.node-frozen` with the remaining time in `Retry-After`; the clock
    // advances 2 minutes and 3 remain.
    assert_eq!(
        join_error(&cluster, 3, 1).await,
        Some((ErrorCode::ClusterNodeFrozen, 300))
    );
    cluster.clock.advance(Duration::from_secs(120));
    assert_eq!(
        join_error(&cluster, 3, 2).await,
        Some((ErrorCode::ClusterNodeFrozen, 180))
    );

    // There is no waiting list: the cluster kept no request. The tables stay without node 3,
    // with no pending change, and node 3 is the one that insists (the bootstrap loop).
    for number in [1, 2] {
        let membership = cluster.node(number).node.cluster().unwrap().membership();
        assert!(!membership.has_pending(cluster.clock.now()));
        assert_eq!(cluster.member_ids(number).len(), 2);
    }
    let attempts = cluster.network.count("join");
    eventually("node 3 to repeat the request", || {
        cluster.network.count("join") > attempts + 1
    })
    .await;

    // With the freezing over the node comes back, with the same ID, and the three tables become equal again.
    cluster.clock.advance(FREEZING);
    wait_joined(cluster.node(3)).await;
    eventually("the three tables equal with node 3", || {
        let tables: Vec<_> = (1..=3).map(|n| cluster.table(n)).collect();
        tables.windows(2).all(|w| w[0] == w[1])
            && tables[0]
                .as_ref()
                .is_some_and(|t| t.members.len() == 3 && t.member(&id).is_some())
    })
    .await;
    assert_eq!(cluster.node(3).node.shared().node_id(), id);
    cluster.shutdown().await;
}

/// UC: UC-FRZ-007
#[tokio::test]
async fn the_nodes_in_freezing_show_in_the_metric_and_in_the_node_frozen_condition() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let conditions = |status: &serde_json::Value| -> Vec<String> {
        status["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["condition"].as_str().unwrap().to_owned())
            .collect()
    };
    let frozen_metric = |text: &str| total(text, "taba_cluster_freezing", "");

    assert_eq!(frozen_metric(&cluster.node(1).metrics().await), 0.0);
    assert!(
        !conditions(&cluster.node(1).stats("/health/cluster?scope=node").await)
            .contains(&"node-frozen".to_owned())
    );

    cluster.network.isolate(&host_of(3));
    eventually("node 3 to be excluded", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;

    // Node 3 stays isolated: it does not ask to join again and the freezing is only what the others keep.
    for number in [1, 2] {
        let node = cluster.node(number);
        assert_eq!(frozen_metric(&node.metrics().await), 1.0);
        let status = node.stats("/health/cluster?scope=node").await;
        assert!(conditions(&status).contains(&"node-frozen".to_owned()));
        assert_eq!(status["nodes"][0]["status"]["signals"]["frozenNodes"], 1);
        let entry = status["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["condition"] == "node-frozen")
            .unwrap()
            .clone();
        assert_eq!(entry["severity"], "warning");
    }

    // The period passes and the node stops counting.
    cluster.clock.advance(FREEZING + Duration::from_secs(1));
    for number in [1, 2] {
        let node = cluster.node(number);
        assert_eq!(frozen_metric(&node.metrics().await), 0.0);
        let status = node.stats("/health/cluster?scope=node").await;
        assert!(!conditions(&status).contains(&"node-frozen".to_owned()));
    }
    cluster.shutdown().await;
}

/// UC: UC-FRZ-009
#[tokio::test]
async fn a_node_that_joins_after_the_exclusion_inherits_the_freezing_of_the_excluded() {
    let mut cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let excluded = cluster.node_id(3);
    cluster.network.isolate(&host_of(3));
    eventually("the two healthy ones to exclude node 3", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;

    // Node 4 joins after the exclusion commit: it did not see it, and receives the freezing in the table.
    cluster.add_node(false).await;
    let membership = cluster.node(4).node.cluster().unwrap().membership();
    assert!(
        membership
            .frozen_remaining(&excluded, cluster.clock.now(), FREEZING)
            .is_some(),
        "the new node inherited the freezing"
    );
    // The stored table does not carry it, and the nodes' tables stay equal.
    assert!(membership.table().unwrap().frozen.is_empty());
    assert_eq!(cluster.table(4), cluster.table(1));

    // That is why the excluded one's request that lands on node 4 is also refused.
    let transport = cluster.network.transport(host_of(1));
    let reply = transport
        .call(&address_of(&cluster, 4), join_as(&cluster, 3))
        .await;
    match reply {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::ClusterNodeFrozen),
        other => panic!("expected cluster.node-frozen, got {other:?}"),
    }
    cluster.shutdown().await;
}

/// UC: UC-FRZ-010, UC-FRZ-011, UC-CNC-013
#[tokio::test]
async fn a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel() {
    let cluster = TestCluster::start(4).await;
    settled(&cluster).await;
    let excluded = cluster.node_id(4);

    // The node with the highest ID among the three healthy is never the one that coordinates the exclusion (the one with the
    // lowest ID coordinates), and it is left without receiving the commit: it is the "down" member in the commit.
    let lagging = (1..=3)
        .max_by_key(|n| cluster.node_id(*n))
        .expect("three healthy nodes");
    let others: Vec<usize> = (1..=3).filter(|n| *n != lagging).collect();
    cluster
        .network
        .lose_kind(&host_of(lagging), "commit", 10_000);
    cluster.network.isolate(&host_of(4));
    eventually("the other two healthy ones to exclude node 4", || {
        frozen_and_gone(&cluster, &others, 4)
    })
    .await;

    // The commit did not reach the laggard: its table still has node 4. Even so it
    // learns of the freezing, through the channel's `frozen` frame.
    let membership = cluster.node(lagging).node.cluster().unwrap().membership();
    eventually("the laggard to learn of the freezing", || {
        membership
            .frozen_remaining(&excluded, cluster.clock.now(), FREEZING)
            .is_some()
    })
    .await;
    // Its table also gets fixed: the channel's `membership` frame carries the exclusion, without depending
    // on the register or the commit it lost (D126).
    eventually(
        "the laggard to remove the excluded one from the table",
        || !membership.is_member(&excluded),
    )
    .await;
    assert_eq!(
        membership.table().unwrap().version,
        cluster.table(others[0]).unwrap().version,
        "the laggard's table became equal to the others'"
    );
    assert!(membership.table().unwrap().frozen.is_empty());

    // The excluded one's request that lands on it is refused, and not accepted as "the same node repeating".
    let transport = cluster.network.transport(host_of(others[0]));
    let reply = transport
        .call(&address_of(&cluster, lagging), join_as(&cluster, 4))
        .await;
    match reply {
        Err(CallError::Remote(error)) => assert_eq!(error.code, ErrorCode::ClusterNodeFrozen),
        other => panic!("expected cluster.node-frozen, got {other:?}"),
    }
    // Without losing more commits, the commit that was going on in the background finishes and the shutdown does not wait.
    cluster.network.lose_kind(&host_of(lagging), "commit", 0);
    cluster.shutdown().await;
}

/// UC: UC-FRZ-014
#[tokio::test]
async fn the_channel_snapshot_carries_the_nodes_still_in_freezing() {
    let cluster = TestCluster::start(3).await;
    settled(&cluster).await;
    let excluded = cluster.node_id(3);
    assert!(
        !signals_of(&cluster, 1)
            .snapshot()
            .iter()
            .any(|s| matches!(s, Signal::Frozen { .. })),
        "without an exclusion, the snapshot carries no freezing"
    );
    cluster.network.isolate(&host_of(3));
    eventually("the two healthy ones to exclude node 3", || {
        frozen_and_gone(&cluster, &[1, 2], 3)
    })
    .await;
    let snapshot = signals_of(&cluster, 1).snapshot();
    assert!(
        snapshot
            .iter()
            .any(|s| matches!(s, Signal::Frozen { node_id, .. } if *node_id == excluded)),
        "the snapshot carries the node in freezing: {snapshot:?}"
    );

    // With the freezing over, it leaves the snapshot.
    cluster.clock.advance(FREEZING + Duration::from_secs(1));
    assert!(
        !signals_of(&cluster, 1)
            .snapshot()
            .iter()
            .any(|s| matches!(s, Signal::Frozen { .. }))
    );
    cluster.shutdown().await;
}
