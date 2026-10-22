//! The upgrade of versions (doc 23, rolling update): the node of the newest version drains, one at a time, the
//! members of an earlier version. The versions are simulated: the nodes announce the one the test gives them.

mod common;

use std::time::Duration;

use common::cluster::{TestCluster, eventually, wait_joined};
use taba_node::cluster::members::MemberState;

const OLD: &str = "0.0.1";
const NEW: &str = "0.0.2";

/// Starts the next node of the cluster announcing `version`, and waits for it to be active.
async fn add_version(cluster: &mut TestCluster, version: &str) {
    let number = cluster.nodes.len() + 1;
    let builder = cluster
        .builder(number, number == 1)
        .program_version(version);
    cluster
        .spawn_node(builder)
        .await
        .expect("the node should start");
    wait_joined(cluster.nodes.last().expect("just added")).await;
    cluster.publish_backends(cluster.nodes.len());
}

fn members_of(cluster: &TestCluster, number: usize) -> Vec<(String, MemberState)> {
    cluster
        .table(number)
        .map(|t| {
            t.members
                .into_iter()
                .map(|m| (m.node_id, m.state))
                .collect()
        })
        .unwrap_or_default()
}

/// UC: UC-ING-029
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nodes_of_the_new_version_retire_the_old_ones_one_at_a_time() {
    let mut cluster = TestCluster::empty();
    for _ in 0..3 {
        add_version(&mut cluster, OLD).await;
    }
    let old: Vec<String> = (1..=3).map(|n| cluster.node_id(n)).collect();
    for _ in 0..3 {
        add_version(&mut cluster, NEW).await;
    }

    // Until the three old nodes have left, never more than one is draining at a time.
    let mut most_draining = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(common::cluster::WAIT);
    loop {
        let members = members_of(&cluster, 6);
        let draining = members
            .iter()
            .filter(|(_, state)| *state == MemberState::Draining)
            .count();
        most_draining = most_draining.max(draining);
        if members.iter().all(|(id, _)| !old.contains(id)) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "waited for the old nodes to leave; the table still has {members:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        most_draining <= 1,
        "{most_draining} members draining at once"
    );
    let table = cluster.table(6).expect("table");
    assert_eq!(table.members.len(), 3, "the three new nodes remain");
    cluster.shutdown().await;
}

/// UC: UC-ING-030
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_upgrade_does_not_drain_if_fewer_than_three_would_stay_active() {
    let mut cluster = TestCluster::empty();
    add_version(&mut cluster, OLD).await;
    add_version(&mut cluster, OLD).await;
    add_version(&mut cluster, NEW).await;

    // Three active nodes: draining one would leave two. It waits several rounds and does nothing.
    tokio::time::sleep(Duration::from_secs(6)).await;
    let members = members_of(&cluster, 3);
    assert_eq!(members.len(), 3, "nobody left: {members:?}");
    assert!(
        members
            .iter()
            .all(|(_, state)| *state == MemberState::Active),
        "nobody entered drain: {members:?}"
    );
    cluster.shutdown().await;
}

/// UC: UC-ING-032
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_is_only_sent_what_its_version_announced_support_for() {
    let mut cluster = TestCluster::empty();
    add_version(&mut cluster, OLD).await;
    add_version(&mut cluster, OLD).await;
    add_version(&mut cluster, NEW).await;
    let (old, new) = (cluster.node_id(1), cluster.node_id(3));
    let service = cluster.node(3).node.cluster().expect("cluster").clone();

    eventually("the versions of the members to be known", || {
        service.peer_supports(&old, OLD)
    })
    .await;
    assert!(service.peer_supports(&old, OLD));
    assert!(
        !service.peer_supports(&old, NEW),
        "the old node did not announce the new version"
    );
    assert!(service.peer_supports(&new, NEW), "the node itself");
    assert!(!service.peer_supports("unknown-node", OLD));
    cluster.shutdown().await;
}

/// UC: UC-ING-033
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_older_node_that_joins_is_retired_and_does_not_drain_the_newer_ones() {
    let mut cluster = TestCluster::empty();
    for _ in 0..3 {
        add_version(&mut cluster, NEW).await;
    }
    let newer: Vec<String> = (1..=3).map(|n| cluster.node_id(n)).collect();
    add_version(&mut cluster, OLD).await;
    let older = cluster.node_id(4);

    // The three newer ones retire the old node that came in; it does not drain any of them.
    eventually("the old node to leave the table", || {
        !members_of(&cluster, 1).iter().any(|(id, _)| *id == older)
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let members = members_of(&cluster, 1);
    let ids: Vec<&String> = members.iter().map(|(id, _)| id).collect();
    assert_eq!(ids.len(), 3, "{members:?}");
    assert!(newer.iter().all(|id| ids.contains(&id)), "{members:?}");
    assert!(
        members
            .iter()
            .all(|(_, state)| *state == MemberState::Active),
        "the newer nodes stay active: {members:?}"
    );
    cluster.shutdown().await;
}
