//! The end of the restoration of the previous lives (doc 37, sections 4.7, 4.8 and 4.10; level **C**): the nodes wait
//! for the expected ones (or for `G`), the coordinator closes the vote when every present node finished, the FIFO
//! queues receive and do not deliver until the cluster is restored from disk, and the operator can force it. The
//! `Clock` is manual: `G` passes when the test says so.
//!
//! The lives are planted in the nodes' `_lives` folder (as a restart leaves them) and the restoration is started on
//! all the nodes at once, which is what a cold start does.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use common::TestNode;
use common::cluster::{TestCluster, eventually, settled};
use taba_node::cluster::protocol::MessageRecord;
use taba_node::cluster::restore::{Manifest, ManifestMember, Status};
use taba_node::config::DbSynchronous;
use taba_node::core::clock::Clock;
use taba_node::core::id;
use taba_node::engine::messages::Messages;
use taba_node::storage::messages::MessageStore;

const PLAIN: &str = "plain";
const FIFO: &str = "orders";

fn messages_of(node: &TestNode) -> Arc<Messages> {
    node.node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.messages.messages().clone())
        .expect("the node is in the cluster")
}

fn status_of(cluster: &TestCluster, number: usize) -> Status {
    cluster
        .node(number)
        .node
        .cluster()
        .expect("the node is in the cluster")
        .restoration()
        .status()
}

fn all_have(cluster: &TestCluster, status: Status) -> bool {
    (1..=cluster.nodes.len()).all(|n| status_of(cluster, n) == status)
}

fn has(cluster: &TestCluster, topic: &str, id: &str) -> bool {
    (1..=cluster.nodes.len()).any(|n| {
        messages_of(cluster.node(n))
            .message_info(topic, id)
            .unwrap()
            .is_some_and(|info| !info.meta.content_type.is_empty())
    })
}

fn plain_record(cluster: &TestCluster, n: u64) -> MessageRecord {
    let now = cluster.clock.now_millis();
    MessageRecord {
        id: id::new_at(now + n),
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
    }
}

fn fifo_record(cluster: &TestCluster, n: u64, group: &str, key: &str) -> MessageRecord {
    MessageRecord {
        topic: FIFO.into(),
        group_id: Some(group.into()),
        deduplication_id: Some(key.into()),
        ..plain_record(cluster, n)
    }
}

/// Leaves a life in the `_lives` folder of the node, as a restart does: the messages it kept and its manifest.
fn plant_life(
    cluster: &TestCluster,
    number: usize,
    old: &str,
    members: &[&str],
    watermark: Option<&str>,
    records: &[&MessageRecord],
) {
    let dir = messages_of(cluster.node(number))
        .storage_root()
        .join(MessageStore::LIVES)
        .join(old);
    let store = MessageStore::new(&dir, DbSynchronous::Off);
    store.set_topic_id_resolver(Box::new(|_| None));
    for record in records {
        store
            .put(&record.topic, &record.to_meta(), &record.body)
            .expect("writes to disk");
    }
    drop(store);
    Manifest {
        node_id: old.into(),
        members: members
            .iter()
            .map(|id| ManifestMember {
                node_id: (*id).into(),
                addresses: Vec::new(),
                port: 0,
            })
            .collect(),
        table_version: 1,
        last_seen_ms: cluster.clock.now_millis(),
        watermark: watermark.map(str::to_owned),
        excluded_since_ms: None,
    }
    .write(&dir)
    .expect("writes the manifest");
}

fn life_dir(cluster: &TestCluster, number: usize, old: &str) -> std::path::PathBuf {
    messages_of(cluster.node(number))
        .storage_root()
        .join(MessageStore::LIVES)
        .join(old)
}

/// A cluster of three with a plain queue and a FIFO queue, whose clock already moved 60 s (the lives planted from
/// now on are inside the window of the log).
async fn setup(env: &[(&str, &str)]) -> TestCluster {
    // The deduplication window is wider than the `G` the tests jump over, as in a cluster that restores in time.
    let mut all = vec![
        ("TABA_DRAIN_ON_STOP", "false"),
        ("TABA_FIFO_DEDUP_WINDOW", "1h"),
    ];
    all.extend_from_slice(env);
    let cluster = TestCluster::start_with(3, &all).await;
    settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), PLAIN).await;
    let response = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({ "name": FIFO, "type": "queue", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    cluster.clock.advance(Duration::from_secs(60));
    cluster
}

fn begin_all(cluster: &TestCluster) {
    // First every node is in restoration (as after a cold start), then the passes start.
    for number in 1..=cluster.nodes.len() {
        cluster
            .node(number)
            .node
            .cluster()
            .unwrap()
            .hold_for_restoration();
    }
    for number in 1..=cluster.nodes.len() {
        cluster
            .node(number)
            .node
            .cluster()
            .unwrap()
            .begin_restoration();
    }
}

async fn publish_fifo(node: &TestNode, group: &str, key: &str, body: &str) -> Value {
    let response = node
        .client
        .post(node.client_url(&format!(
            "/topics/{FIFO}/messages?groupId={group}&deduplicationId={key}"
        )))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "{:?}",
        response.text().await
    );
    response.json::<Value>().await.unwrap()
}

async fn get(node: &TestNode, topic: &str) -> Vec<Value> {
    let response = node
        .client_get(&format!(
            "/topics/{topic}/messages?maxMessages=10&visibilityTimeout=600"
        ))
        .await;
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

/// Waits for the `GET` to bring something (the notice between nodes takes a moment).
async fn get_some(node: &TestNode, topic: &str) -> Vec<Value> {
    for _ in 0..100 {
        let got = get(node, topic).await;
        if !got.is_empty() {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Vec::new()
}

async fn conditions(node: &TestNode) -> String {
    node.client_get_ops("/health/cluster")
        .await
        .text()
        .await
        .unwrap()
}

/// Gives the nodes time to poll each other a few times.
async fn let_them_poll() {
    tokio::time::sleep(Duration::from_millis(1800)).await;
}

/// UC: UC-RTN-031
/// UC: UC-RTN-034
/// UC: UC-FIF-048
/// UC: UC-OPS-053
#[tokio::test]
async fn the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo()
{
    let cluster = setup(&[]).await;
    let (o1, o2, o3) = (id::new_at(1), id::new_at(2), id::new_at(3));
    let members = [o1.as_str(), o2.as_str(), o3.as_str()];
    let (a, b) = (plain_record(&cluster, 1), plain_record(&cluster, 2));
    // The third node of the old cluster did not come back: nobody has its life.
    plant_life(&cluster, 1, &o1, &members, None, &[&a]);
    plant_life(&cluster, 2, &o2, &members, None, &[&b]);

    begin_all(&cluster);
    publish_fifo(cluster.node(1), "g", "k", r#"{"fifo":1}"#).await;
    let_them_poll().await;

    // Waiting for the expected node: the status has not changed, the FIFO receives and does not deliver, and the
    // condition is visible; the old messages did not start to come back.
    assert!(all_have(&cluster, Status::Restoring));
    assert!(get(cluster.node(1), FIFO).await.is_empty());
    assert!(
        conditions(cluster.node(1))
            .await
            .contains("previous-life-pending")
    );
    assert!(!has(&cluster, PLAIN, &a.id) && !has(&cluster, PLAIN, &b.id));
    assert_eq!(
        cluster
            .node(1)
            .node
            .cluster()
            .unwrap()
            .restoration()
            .snapshot()
            .missing,
        vec![o3.clone()]
    );

    // The plain queues do not wait for the vote: they receive and deliver.
    let response = cluster
        .node(2)
        .client
        .post(
            cluster
                .node(2)
                .client_url(&format!("/topics/{PLAIN}/messages")),
        )
        .header("content-type", "application/json")
        .body(r#"{"new":true}"#)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(get_some(cluster.node(2), PLAIN).await.len(), 1);

    // `G` passes: the restoration goes on with the nodes that are present and the vote closes.
    cluster.clock.advance(Duration::from_secs(6 * 60));
    eventually("the cluster to be restored from disk", || {
        all_have(&cluster, Status::Restored)
    })
    .await;
    eventually("the old messages to come back", || {
        has(&cluster, PLAIN, &a.id) && has(&cluster, PLAIN, &b.id)
    })
    .await;
    // The FIFO delivers what it received meanwhile, and the folders of the lives are gone.
    assert_eq!(get_some(cluster.node(1), FIFO).await.len(), 1);
    eventually("the folders of the lives to be deleted", || {
        !life_dir(&cluster, 1, &o1).exists() && !life_dir(&cluster, 2, &o2).exists()
    })
    .await;
    assert!(
        !conditions(cluster.node(1))
            .await
            .contains("previous-life-pending")
    );
    cluster.shutdown().await;
}

/// UC: UC-RTN-032
/// UC: UC-RTN-035
#[tokio::test]
async fn in_strict_mode_the_restoration_stays_held_after_g_until_the_operator_forces_it() {
    let cluster = setup(&[("TABA_CLUSTER_RESTORE_STRICT", "true")]).await;
    let (o1, o2, o3) = (id::new_at(1), id::new_at(2), id::new_at(3));
    let members = [o1.as_str(), o2.as_str(), o3.as_str()];
    let a = plain_record(&cluster, 1);
    plant_life(&cluster, 1, &o1, &members, None, &[&a]);

    begin_all(&cluster);
    publish_fifo(cluster.node(2), "g", "k", r#"{"fifo":1}"#).await;
    cluster.clock.advance(Duration::from_secs(6 * 60));
    let_them_poll().await;

    // `G` passed and the node did not come back: strict mode holds everything.
    assert!(all_have(&cluster, Status::Restoring));
    assert!(!has(&cluster, PLAIN, &a.id));
    assert!(get(cluster.node(2), FIFO).await.is_empty());
    assert!(
        conditions(cluster.node(3))
            .await
            .contains("previous-life-pending")
    );

    // The operator forces (any node, the operations API): the status changes everywhere and what was held goes on.
    let response = cluster
        .node(3)
        .client
        .post(cluster.node(3).operations_url("/cluster/restore/force"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["wasRestoring"], true);
    eventually("the cluster to be restored from disk", || {
        all_have(&cluster, Status::Restored)
    })
    .await;
    eventually("the held restoration to go on", || {
        has(&cluster, PLAIN, &a.id)
    })
    .await;
    assert_eq!(get_some(cluster.node(2), FIFO).await.len(), 1);
    cluster.shutdown().await;
}

/// UC: UC-FIF-049
/// UC: UC-FIF-050
/// UC: UC-FIF-051
/// UC: UC-FIF-052
#[tokio::test]
async fn the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark()
 {
    let cluster = setup(&[]).await;
    let (o1, o2, o3) = (id::new_at(1), id::new_at(2), id::new_at(3));
    let members = [o1.as_str(), o2.as_str(), o3.as_str()];
    let restored = fifo_record(&cluster, 5, "g", "k");
    let own = fifo_record(&cluster, 6, "g", "other");
    plant_life(
        &cluster,
        1,
        &o1,
        &members,
        Some(&own.id),
        &[&restored, &own],
    );

    begin_all(&cluster);
    eventually(
        "the watermark of the old life to reach the generator",
        || {
            // The generator stays above the old life from the first poll.
            messages_of(cluster.node(2)).watermark().is_some()
        },
    )
    .await;

    // Nothing is deduplicated during the restoration: the publication that repeats the restored key is accepted
    // (`duplicate=false`), and its `id` is above the old ones.
    let repeated = publish_fifo(cluster.node(2), "g", "k", r#"{"again":true}"#).await;
    assert_eq!(repeated["duplicate"], false);
    let repeated_id = repeated["id"].as_str().unwrap().to_owned();
    assert!(
        repeated_id > restored.id && repeated_id > own.id,
        "{repeated_id} vs {} and {}",
        restored.id,
        own.id
    );

    cluster.clock.advance(Duration::from_secs(6 * 60));
    eventually("the cluster to be restored from disk", || {
        all_have(&cluster, Status::Restored)
    })
    .await;

    // The release dropped the repeated publication, in the owners as well; the restored one is delivered.
    eventually("the repeated publication to be discarded", || {
        !has(&cluster, FIFO, &repeated_id)
    })
    .await;
    assert!(has(&cluster, FIFO, &restored.id));
    let got = get_some(cluster.node(1), FIFO).await;
    assert_eq!(got.len(), 1, "one message per group at a time: {got:?}");
    assert_eq!(got[0]["id"], restored.id.as_str());
    cluster.shutdown().await;
}

/// UC: UC-RTN-033
#[tokio::test]
async fn a_node_that_comes_back_after_the_release_restores_its_lives_on_its_own_and_holds_nothing()
{
    let cluster = setup(&[]).await;
    let o1 = id::new_at(1);
    let kept = fifo_record(&cluster, 7, "g", "late");
    plant_life(&cluster, 3, &o1, &[o1.as_str()], None, &[&kept]);

    // The cluster is restored (nobody was restoring): the node's pass is its own, with no vote and no hold.
    let recovery = cluster
        .node(3)
        .node
        .cluster()
        .and_then(|c| c.replication())
        .map(|r| r.recovery.clone())
        .unwrap();
    let summary = recovery.restoration().await;
    assert!(summary.complete(), "{summary:?}");
    assert_eq!(status_of(&cluster, 3), Status::Restored);
    assert!(!messages_of(cluster.node(3)).fifo_held());
    eventually("the message of the life to be activated", || {
        has(&cluster, FIFO, &kept.id)
    })
    .await;
    recovery.drop_lives(&summary);
    assert!(!life_dir(&cluster, 3, &o1).exists());
    cluster.shutdown().await;
}
