//! The probe of the old IPs and the regroup of the groups that took over the disk without talking to each other
//! (doc 07, section 10.11; level **C**). The network is the in-memory one (`MemoryNetwork`), which can cut the link
//! between two hosts, and the `Clock` is the manual one.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use common::TestNode;
use common::cluster::{TestCluster, balancer, eventually, host_of, settled, wait_joined};
use taba_node::cluster::protocol::MessageRecord;
use taba_node::cluster::transport::PeerAddr;
use taba_node::config::DbSynchronous;
use taba_node::core::clock::Clock;
use taba_node::core::id;
use taba_node::engine::messages::Messages;
use taba_node::storage::messages::MessageStore;

const PLAIN: &str = "plain";

/// The parameters of every test: no drain (the process falls), the deduplication window wider than the jumps of
/// the clock, a short cold-start wait and a probe every 200 ms.
const ENV: &[(&str, &str)] = &[
    ("TABA_DRAIN_ON_STOP", "false"),
    ("TABA_FIFO_DEDUP_WINDOW", "1h"),
    ("TABA_CLUSTER_RESTORE_COLD_START_WAIT", "300ms"),
    ("TABA_CLUSTER_RESTORE_PROBE_INTERVAL", "200ms"),
    // A group of one node must be able to accept a message while it is apart.
    ("TABA_WRITE_QUORUM", "1"),
];

fn messages_of(node: &TestNode) -> Arc<Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

fn has(cluster: &TestCluster, id: &str) -> bool {
    (1..=cluster.nodes.len()).any(|n| {
        messages_of(cluster.node(n))
            .message_info(PLAIN, id)
            .unwrap()
            .is_some_and(|info| !info.meta.content_type.is_empty())
    })
}

/// A message of the group's disk (what a running node has already spilled to it): written with a second handle on the
/// same folder, as the cold start tests do.
fn leave_on_disk(node: &TestNode, n: u64, clock: &dyn Clock) -> String {
    let id = id::new_at(clock.now_millis() + n);
    let record = MessageRecord {
        id: id.clone(),
        topic: PLAIN.into(),
        body: json!({ "n": n }).to_string().into_bytes(),
        content_type: "application/json".into(),
        group_id: None,
        deduplication_id: None,
        delivery_count: 1,
        invisible_until_ms: None,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
        delete_at_ms: None,
    };
    let disk = MessageStore::new(messages_of(node).storage_root(), DbSynchronous::Off);
    disk.set_topic_id_resolver(Box::new(|_| None));
    disk.put(PLAIN, &record.to_meta(), &record.body)
        .expect("writes to disk");
    id
}

/// A node that fell: where it was and what is left on its disk. The folder stays until the test ends.
struct Fallen {
    number: usize,
    port: u16,
    storage: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fall(cluster: &mut TestCluster) -> Fallen {
    let number = cluster.nodes.len();
    let port = cluster.node(number).node.cluster_addr().port();
    let TestNode { node, dir, .. } = cluster.nodes.pop().unwrap();
    node.shutdown().await;
    cluster
        .network
        .unregister(&PeerAddr::new(host_of(number), port));
    Fallen {
        number,
        port,
        storage: dir.path().join("taba"),
        _dir: dir,
    }
}

async fn come_back(cluster: &mut TestCluster, fallen: &Fallen, env: &[(&str, &str)]) {
    let (port, storage) = (fallen.port, fallen.storage.display().to_string());
    cluster
        .respawn_node(fallen.number, |mut builder| {
            for (name, value) in env {
                builder = builder.env(name, value);
            }
            builder
                .env("TABA_CLUSTER_PORT", &port.to_string())
                .env("TABA_STORAGE_DIR", &storage)
        })
        .await
        .expect("the process restarts");
}

/// A cluster of three with a plain queue that fell whole and came back in two groups: node 1 alone and nodes 2 and 3
/// together, because the network does not let node 1 talk to them. Returns the old ID of node 1 and the disks (kept until the test ends).
async fn two_groups(env: &[(&str, &str)]) -> (TestCluster, String, Vec<Fallen>) {
    let mut cluster = TestCluster::start_with(3, ENV).await;
    settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), PLAIN).await;
    cluster.clock.advance(Duration::from_secs(60));
    let old_id = cluster.node_id(1);
    let mut fallen = Vec::new();
    while !cluster.nodes.is_empty() {
        fallen.push(fall(&mut cluster).await);
    }
    fallen.reverse();
    cluster.network.add_balancer(balancer(), Vec::new());
    cluster.network.partition(&host_of(1), &host_of(2));
    cluster.network.partition(&host_of(1), &host_of(3));
    for node in &fallen {
        come_back(&mut cluster, node, env).await;
    }
    for number in 1..=3 {
        wait_joined(cluster.node(number)).await;
    }
    eventually("two groups: one of one node and one of two", || {
        cluster.member_ids(1).len() == 1
            && cluster.member_ids(2).len() == 2
            && cluster.member_ids(3).len() == 2
    })
    .await;
    // The disks of the groups stay in their folders until the test ends.
    (cluster, old_id, fallen)
}

/// UC: UC-RTN-040
#[tokio::test]
async fn the_smaller_group_that_meets_the_other_one_moves_its_disk_to_a_life_and_joins_with_a_new_id()
 {
    let (cluster, old_id, _disks) = two_groups(&[]).await;
    // The group of node 1 accepted a message while it was apart, and it is on its disk.
    let id = leave_on_disk(cluster.node(1), 1, cluster.clock.as_ref());
    cluster.clock.advance(Duration::from_secs(60));

    cluster.network.heal(&host_of(1), &host_of(2));
    cluster.network.heal(&host_of(1), &host_of(3));
    eventually("the three nodes in one table", || {
        (1..=3).all(|n| cluster.member_ids(n).len() == 3)
    })
    .await;
    assert_ne!(
        cluster.node_id(1),
        old_id,
        "the node that lost joined with a new ID"
    );
    wait_joined(cluster.node(1)).await;
    // What it accepted while apart was restored and is in the cluster; the queue (a definition) is still there.
    eventually("the message of the group that lost", || has(&cluster, &id)).await;
    let response = cluster
        .node(2)
        .client_get(&format!("/topics/{PLAIN}"))
        .await;
    assert_eq!(response.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-RTN-041
#[tokio::test]
async fn the_probe_ends_when_the_table_completes_and_raises_no_condition() {
    let (cluster, _, _disks) = two_groups(&[]).await;
    cluster.network.heal(&host_of(1), &host_of(2));
    cluster.network.heal(&host_of(1), &host_of(3));
    eventually("the table complete", || {
        (1..=3).all(|n| cluster.member_ids(n).len() == 3)
    })
    .await;
    for number in 1..=3 {
        let service = cluster.node(number).node.cluster().unwrap();
        assert!(!service.members_missing(), "node {number}");
    }
    cluster.shutdown().await;
}

/// UC: UC-RTN-042, UC-OPS-054
#[tokio::test]
async fn when_the_probe_time_ends_with_nodes_missing_the_members_missing_condition_shows() {
    let (cluster, _, _disks) = two_groups(&[("TABA_CLUSTER_RESTORE_PROBE_TIMEOUT", "1s")]).await;
    let alone = cluster.node(1);
    eventually("the probe ended with nodes missing", || {
        alone.node.cluster().unwrap().members_missing()
    })
    .await;
    let view = alone.stats("/health/cluster?scope=node").await;
    let names: Vec<&str> = view["conditions"]
        .as_array()
        .expect("list of conditions")
        .iter()
        .filter_map(|c| c["condition"].as_str())
        .collect();
    assert!(names.contains(&"members-missing"), "{view}");
    cluster.shutdown().await;
}
