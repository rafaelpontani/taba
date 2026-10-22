//! The versions and tombstones of the definitions kept in `_node.db` (doc 07, section 10.2; level **C** of
//! doc 20): several real nodes in the same process, linked by the in-memory network.

mod common;

use std::sync::Arc;

use common::TestNode;
use common::cluster::{TestCluster, address_of, eventually_async, host_of, name_owned_by, settled};
use serde_json::json;
use taba_node::cluster::protocol::{Request, Response, StreamQuery};
use taba_node::cluster::transport::Transport;
use taba_node::config::DbSynchronous;
use taba_node::storage::db::NodeDb;
use taba_node::storage::versions::{VersionRow, VersionStore};

/// The rows this node has on disk.
fn rows_of(node: &TestNode) -> Vec<VersionRow> {
    let db = Arc::new(NodeDb::open(&node.storage_dir(), DbSynchronous::Normal).expect("_node.db"));
    VersionStore::new(db).load_all().expect("rows")
}

fn row_of(node: &TestNode, kind: &str, key: &str) -> Option<VersionRow> {
    rows_of(node)
        .into_iter()
        .find(|r| r.kind == kind && r.key == key)
}

async fn create(cluster: &TestCluster, number: usize, name: &str) {
    let node = cluster.node(number);
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({"name": name, "type": "queue"}))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
}

async fn remove(cluster: &TestCluster, number: usize, name: &str) {
    let node = cluster.node(number);
    let response = node
        .client
        .delete(node.client_url(&format!("/topics/{name}")))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 204);
}

async fn topic_id(cluster: &TestCluster, number: usize, name: &str) -> String {
    let body: serde_json::Value = cluster
        .node(number)
        .client_get(&format!("/topics/{name}"))
        .await
        .json()
        .await
        .expect("json");
    body["id"].as_str().expect("id").to_owned()
}

/// Every node writes to `_node.db` the version of the definition it applied and, when the topic is
/// removed, the tombstone with the version and the `topicId` of the topic that was removed.
/// UC: UC-DEF-039, UC-DEF-040
#[tokio::test]
async fn every_node_writes_the_version_and_the_tombstone_with_the_topic_id_to_its_database() {
    let cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    let name = name_owned_by(&cluster, 1, "kept-");
    create(&cluster, 1, &name).await;
    let first_id = topic_id(&cluster, 1, &name).await;

    for number in 1..=2 {
        eventually_async("the version on disk", || async {
            row_of(cluster.node(number), "topic", &name).is_some_and(|r| {
                r.version == 1 && !r.deleted && r.topic_id.as_deref() == Some(&first_id)
            })
        })
        .await;
    }

    remove(&cluster, 1, &name).await;
    for number in 1..=2 {
        eventually_async("the tombstone on disk", || async {
            row_of(cluster.node(number), "topic", &name).is_some_and(|r| {
                r.version == 2 && r.deleted && r.topic_id.as_deref() == Some(&first_id)
            })
        })
        .await;
    }

    // Recreated with the same name: another `topicId`, and the row describes the new one.
    create(&cluster, 1, &name).await;
    let second_id = topic_id(&cluster, 1, &name).await;
    assert_ne!(first_id, second_id);
    for number in 1..=2 {
        eventually_async("the recreated topic on disk", || async {
            row_of(cluster.node(number), "topic", &name).is_some_and(|r| {
                r.version == 3 && !r.deleted && r.topic_id.as_deref() == Some(&second_id)
            })
        })
        .await;
    }
    cluster.shutdown().await;
}

/// A node that joins reads the tombstones of the source, with the `topicId`, and keeps them on disk.
/// UC: UC-DEF-039, UC-DEF-040
#[tokio::test]
async fn a_new_node_stores_the_tombstones_it_learns_with_the_topic_id() {
    let mut cluster = TestCluster::start(2).await;
    settled(&cluster).await;
    let name = name_owned_by(&cluster, 1, "gone-");
    create(&cluster, 1, &name).await;
    let id = topic_id(&cluster, 1, &name).await;
    remove(&cluster, 1, &name).await;
    eventually_async("the tombstone on the owner", || async {
        row_of(cluster.node(1), "topic", &name).is_some_and(|r| r.deleted)
    })
    .await;

    cluster.add_node(false).await;

    eventually_async("the tombstone on the new node", || async {
        row_of(cluster.node(3), "topic", &name)
            .is_some_and(|r| r.deleted && r.version == 2 && r.topic_id.as_deref() == Some(&id))
    })
    .await;

    // The stream a peer reads says which topic the tombstone is of.
    let peer = cluster.network.transport(host_of(1));
    let Response::Stream(page) = peer
        .call(
            &address_of(&cluster, 3),
            Request::ListTombstones(StreamQuery::default()),
        )
        .await
        .expect("stream")
    else {
        panic!("expected a stream");
    };
    let item = page
        .items
        .iter()
        .find(|i| i["key"] == name.as_str())
        .expect("the tombstone in the stream");
    assert_eq!(item["topicId"], id.as_str());
    cluster.shutdown().await;
}
