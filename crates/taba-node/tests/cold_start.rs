//! The cold start (doc 07, sections 10.4 and 10.5; level **C**): every node of a cluster that fell comes back at once,
//! with its disk, and nobody answers the entry address. The nodes look for each other through the table of their
//! previous life, the one with the lowest old ID takes over the disk and starts alone, the others join it, and the
//! messages of the lives are restored. The `Clock` is manual; the wait of the cold start is short.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use common::TestNode;
use common::cluster::{
    PortHold, TestCluster, balancer, definitions_of, eventually, eventually_async, host_of,
    settled, wait_joined,
};
use taba_node::cluster::protocol::MessageRecord;
use taba_node::cluster::restore::Status;
use taba_node::cluster::state::NodeState;
use taba_node::cluster::transport::PeerAddr;
use taba_node::config::DbSynchronous;
use taba_node::core::clock::Clock;
use taba_node::core::id;
use taba_node::engine::messages::Messages;
use taba_node::storage::db::NodeDb;
use taba_node::storage::messages::MessageStore;
use taba_node::storage::topics::{SqliteTopicStore, TopicStore};
use taba_node::storage::versions::{VersionRow, VersionStore};

const PLAIN: &str = "plain";

/// The parameters of every test: no drain (the process falls), the deduplication window wider than the jumps of
/// the clock and a short cold-start wait.
const ENV: &[(&str, &str)] = &[
    ("TABA_DRAIN_ON_STOP", "false"),
    ("TABA_FIFO_DEDUP_WINDOW", "1h"),
    ("TABA_CLUSTER_RESTORE_COLD_START_WAIT", "300ms"),
];

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
        body: json!({ "n": n }).to_string().into_bytes().into(),
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

/// A node that fell: where it was and what is left on its disk. The folder stays until the test ends.
struct Fallen {
    number: usize,
    port: u16,
    storage: PathBuf,
    hold: PortHold,
    _dir: tempfile::TempDir,
}

/// The process of node `number` falls (the last one of the cluster) and its address stops answering.
async fn fall(cluster: &mut TestCluster) -> Fallen {
    let number = cluster.nodes.len();
    let port = cluster.node(number).node.cluster_addr().port();
    // The disk of a node that already came back is the one it was told to use, not its own temporary folder.
    let storage = messages_of(cluster.node(number))
        .storage_root()
        .to_path_buf();
    let TestNode { node, dir, .. } = cluster.nodes.pop().unwrap();
    node.shutdown().await;
    cluster
        .network
        .unregister(&PeerAddr::new(host_of(number), port));
    Fallen {
        number,
        port,
        storage,
        hold: PortHold::new(port),
        _dir: dir,
    }
}

/// Writes a message on the disk of a node that fell, as the life that wrote it would have left it.
fn leave_on_disk(fallen: &Fallen, record: &MessageRecord) {
    let disk = MessageStore::new(&fallen.storage, DbSynchronous::Off);
    disk.set_topic_id_resolver(Box::new(|_| None));
    disk.put(&record.topic, &record.to_meta(), &record.body)
        .expect("writes to disk");
}

/// The node comes back at the same address, with the same disk, and **without** `cluster.first-node`.
async fn come_back(cluster: &mut TestCluster, fallen: &Fallen) {
    come_back_with(cluster, fallen, &[]).await;
}

/// The same, with parameters of this start only.
async fn come_back_with(cluster: &mut TestCluster, fallen: &Fallen, env: &[(&str, &str)]) {
    let (port, storage) = (fallen.port, fallen.storage.display().to_string());
    fallen.hold.release();
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

/// A cluster of three, with a plain queue, after a minute (so the lives it leaves are inside the log's window).
async fn setup() -> TestCluster {
    let cluster = TestCluster::start_with(3, ENV).await;
    settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), PLAIN).await;
    cluster.clock.advance(Duration::from_secs(60));
    cluster
}

/// UC: UC-RTN-036, UC-RTN-037, UC-DEF-041
#[tokio::test]
async fn every_node_comes_back_at_once_and_the_one_with_the_lowest_old_id_starts_the_cluster() {
    let mut cluster = setup().await;
    let (one, two) = (plain_record(&cluster, 1), plain_record(&cluster, 2));
    let mut fallen = Vec::new();
    while !cluster.nodes.is_empty() {
        fallen.push(fall(&mut cluster).await);
    }
    fallen.reverse();
    leave_on_disk(&fallen[0], &one);
    leave_on_disk(&fallen[1], &two);
    // Nobody answers the entry address any more.
    cluster.network.add_balancer(balancer(), Vec::new());

    // None of them is the `first-node`: they all come back together.
    for node in &fallen {
        come_back(&mut cluster, node).await;
    }
    for number in 1..=3 {
        wait_joined(cluster.node(number)).await;
    }
    eventually("the three nodes in one table", || {
        (1..=3).all(|n| cluster.member_ids(n).len() == 3)
    })
    .await;

    // The definitions of the disk were taken over: the queue is there, with no one having created it again.
    let response = cluster
        .node(1)
        .client_get(&format!("/topics/{PLAIN}"))
        .await;
    assert_eq!(response.status(), 200);
    // The messages of the lives were restored and the folders deleted.
    eventually("the messages of the lives restored", || {
        has(&cluster, PLAIN, &one.id) && has(&cluster, PLAIN, &two.id)
    })
    .await;
    eventually("the vote closed on every node", || {
        (1..=3).all(|n| status_of(&cluster, n) == Status::Restored)
    })
    .await;
    eventually("the folders of the lives deleted", || {
        (1..=3).all(|n| {
            !messages_of(cluster.node(n))
                .storage_root()
                .join(MessageStore::LIVES)
                .read_dir()
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
        })
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-RTN-037
#[tokio::test]
async fn a_restarted_node_finds_the_cluster_through_the_old_table_when_the_entry_address_is_dead() {
    let mut cluster = setup().await;
    let fallen = fall(&mut cluster).await;
    // The load balancer has no one behind it: only the IPs of the previous life's table lead to the cluster.
    cluster.network.add_balancer(balancer(), Vec::new());
    come_back(&mut cluster, &fallen).await;
    wait_joined(cluster.node(3)).await;
    let new_id = cluster.node_id(3);
    eventually("the others see the node that came back", || {
        cluster.member_ids(1).contains(&new_id) && cluster.member_ids(2).contains(&new_id)
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-RTN-038
#[tokio::test]
async fn a_node_with_no_previous_life_never_starts_alone_without_first_node() {
    let mut cluster = TestCluster::empty();
    let mut builder = cluster.builder(1, false);
    for (name, value) in ENV {
        builder = builder.env(name, value);
    }
    cluster.spawn_node(builder).await.unwrap();
    // Much longer than the cold-start wait.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_ne!(
        cluster.node(1).node.shared().state(),
        NodeState::Active,
        "only `cluster.first-node` starts a cluster that never existed"
    );
    cluster.shutdown().await;
}

/// UC: UC-DEF-042
#[tokio::test]
async fn a_node_that_joins_a_running_cluster_does_not_impose_the_definitions_of_its_disk() {
    let mut cluster = setup().await;
    common::cluster::create_queue(cluster.node(1), "stale").await;
    settled(&cluster).await;
    let fallen = fall(&mut cluster).await;
    // While it is down, the queue is deleted in the cluster; its disk still has it.
    // The node that fell is still in the table until the others exclude it: the delete waits for that.
    eventually_async("the queue deleted in the cluster", || async {
        let node = cluster.node(1);
        node.client
            .delete(node.client_url("/topics/stale"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    })
    .await;
    come_back(&mut cluster, &fallen).await;
    wait_joined(cluster.node(3)).await;
    let response = cluster.node(3).client_get("/topics/stale").await;
    assert_eq!(response.status(), 404, "the cluster's definitions win");
    let response = cluster
        .node(3)
        .client_get(&format!("/topics/{PLAIN}"))
        .await;
    assert_eq!(response.status(), 200);
    cluster.shutdown().await;
}

/// Writes on the disk of a node that fell a queue the cluster never heard of, with a version: the definition the
/// node would have received (and written) while the others were away.
fn leave_definition_on_disk(fallen: &Fallen, name: &str, version: u64) {
    leave_tagged_definition_on_disk(fallen, name, version, "");
}

/// The same, with a tag `owner` that tells which disk the definition came from (empty: no tag).
fn leave_tagged_definition_on_disk(fallen: &Fallen, name: &str, version: u64, owner: &str) {
    let db = Arc::new(
        NodeDb::open(&fallen.storage, DbSynchronous::Off).expect("opens the node database"),
    );
    let topics = SqliteTopicStore::new(db.clone());
    let mut definition = topics
        .load_all()
        .unwrap()
        .into_iter()
        .find(|d| d.name == PLAIN)
        .expect("the disk has the queue");
    definition.name = name.into();
    definition.id = id::new_at(version);
    if !owner.is_empty() {
        definition.tags.insert("owner".into(), owner.into());
    }
    topics.save(&definition).unwrap();
    VersionStore::new(db)
        .save(&VersionRow {
            kind: "topic".into(),
            key: name.into(),
            version,
            deleted: false,
            fingerprint: [0; 32],
            topic_id: None,
            at_ms: 0,
        })
        .unwrap();
}

/// UC: UC-RTN-039
#[tokio::test]
async fn a_node_that_joins_offers_the_definitions_of_its_disk_and_the_highest_version_wins() {
    let mut cluster = setup().await;
    let mut fallen = Vec::new();
    while !cluster.nodes.is_empty() {
        fallen.push(fall(&mut cluster).await);
    }
    fallen.reverse();
    // The node with the highest old ID has a queue the others do not know: the one that starts the cluster
    // (the lowest old ID) has an older disk, and the offer is what brings the queue to the cluster.
    leave_definition_on_disk(&fallen[2], "offered", 5);
    cluster.network.add_balancer(balancer(), Vec::new());
    for node in &fallen {
        come_back(&mut cluster, node).await;
    }
    for number in 1..=3 {
        wait_joined(cluster.node(number)).await;
    }
    eventually_async("the offered queue in the cluster", || async {
        cluster.node(1).client_get("/topics/offered").await.status() == 200
    })
    .await;
    // The ones the cluster already had are still there.
    let response = cluster
        .node(1)
        .client_get(&format!("/topics/{PLAIN}"))
        .await;
    assert_eq!(response.status(), 200);
    cluster.shutdown().await;
}

/// UC: UC-RTN-058
#[tokio::test]
async fn an_offered_definition_goes_to_every_member_of_the_table_and_not_only_to_the_peers_the_channel_knows()
 {
    let cluster = TestCluster::start_with(3, ENV).await;
    settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), PLAIN).await;

    // What the disk of a node that joins would offer: a queue (the one that exists, under another name and ID) at a
    // version above any the cluster knows.
    let mut offered = definitions_of(&cluster, 1)
        .stored_changes()
        .await
        .into_iter()
        .find(|s| s.change.key == PLAIN)
        .expect("the disk has the queue");
    offered.change.key = "offered".into();
    offered.change.version = 9;
    offered.change.body["name"] = json!("offered");
    offered.change.body["id"] = json!(id::new_at(9));

    cluster.network.clear_calls();
    definitions_of(&cluster, 2).offer(vec![offered]).await;

    // The frame of the channel only reaches the peers the node knows at that instant; a node that registered a few
    // milliseconds later, and is still synchronizing, would never get it. The member is told with a request, like a
    // change decided by an owner, so the synchronizing node records it with the events.
    let calls = cluster.network.calls();
    for target in [1, 3] {
        assert!(
            calls.iter().any(|c| c.from == host_of(2)
                && c.to.host == host_of(target)
                && c.request == "definition-apply"),
            "node 2 tells node {target} with a definition-apply"
        );
    }
    for number in 1..=3 {
        eventually_async("the offered queue in every node", || async {
            cluster
                .node(number)
                .client_get("/topics/offered")
                .await
                .status()
                == 200
        })
        .await;
    }
    cluster.shutdown().await;
}

/// Publishes a plain message through the node and returns its `id`.
async fn publish(node: &TestNode, body: serde_json::Value) -> String {
    let response = node
        .client
        .post(node.client_url(&format!("/topics/{PLAIN}/messages")))
        .json(&body)
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 201);
    let value: serde_json::Value = response.json().await.unwrap();
    value["id"].as_str().unwrap().to_owned()
}

/// The `GET` of one message: the message, if there is one ready.
async fn consume(node: &TestNode) -> Option<serde_json::Value> {
    let response = node
        .client
        .get(node.client_url(&format!(
            "/topics/{PLAIN}/messages?maxMessages=1&visibilityTimeout=60"
        )))
        .send()
        .await
        .expect("request");
    let value: serde_json::Value = response.json().await.unwrap();
    value["messages"]
        .as_array()
        .and_then(|m| m.first())
        .cloned()
}

/// Every node of the cluster falls at once; the nodes come back in the order they were.
async fn fall_all(cluster: &mut TestCluster) -> Vec<Fallen> {
    let mut fallen = Vec::new();
    while !cluster.nodes.is_empty() {
        fallen.push(fall(cluster).await);
    }
    fallen.reverse();
    fallen
}

/// Every fallen node comes back at once, with nobody answering the entry address, and the cluster settles.
async fn come_back_all(cluster: &mut TestCluster, fallen: &[Fallen]) {
    cluster.network.add_balancer(balancer(), Vec::new());
    for node in fallen {
        come_back(cluster, node).await;
    }
    for number in 1..=fallen.len() {
        wait_joined(cluster.node(number)).await;
    }
    eventually("the vote closed on every node", || {
        (1..=fallen.len()).all(|n| status_of(cluster, n) == Status::Restored)
    })
    .await;
}

/// UC: UC-RTN-046
#[tokio::test]
async fn what_was_only_in_memory_on_every_node_is_lost_when_the_whole_cluster_falls() {
    let mut cluster = setup().await;
    // The default memory limit: the message stays in memory on the three nodes and reaches no disk.
    let lost = publish(cluster.node(1), json!({ "only": "in memory" })).await;
    let fallen = fall_all(&mut cluster).await;
    come_back_all(&mut cluster, &fallen).await;
    assert!(!has(&cluster, PLAIN, &lost), "no node has what no disk had");
    assert!(consume(cluster.node(1)).await.is_none());
    cluster.shutdown().await;
}

/// UC: UC-RTN-047
#[tokio::test]
async fn a_fall_that_loses_the_last_entries_of_the_log_delivers_the_acknowledged_message_again() {
    let mut cluster = setup().await;
    let id = publish(cluster.node(1), json!({ "n": 1 })).await;
    let message = loop {
        if let Some(message) = consume(cluster.node(2)).await {
            break message;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(message["id"], id.as_str());
    let token = message["receiptToken"].as_str().unwrap().to_owned();
    let ack = cluster
        .node(3)
        .client
        .delete(cluster.node(3).client_url(&format!(
            "/topics/{PLAIN}/messages/{id}?receiptToken={token}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), 204);
    for number in 1..=3 {
        let messages = messages_of(cluster.node(number));
        let wanted = id.clone();
        eventually("the exclusion in the log", move || {
            messages.is_excluded(PLAIN, &wanted)
        })
        .await;
    }

    let fallen = fall_all(&mut cluster).await;
    // The disks are left as they were before the last writes reached them: the message is on one disk (the
    // delete that the ack made was lost) and no log has the exclusion.
    let mut record = plain_record(&cluster, 1);
    record.id = id.clone();
    leave_on_disk(&fallen[0], &record);
    for node in &fallen {
        let database = node
            .storage
            .join(taba_node::storage::messages::dir_name(PLAIN))
            .join(taba_node::storage::messages::TOPIC_DB_FILE);
        if database.is_file() {
            rusqlite::Connection::open(database)
                .unwrap()
                .execute("DELETE FROM excluded WHERE id = ?1", [&id])
                .unwrap();
        }
    }

    come_back_all(&mut cluster, &fallen).await;
    eventually("the message delivered again", || has(&cluster, PLAIN, &id)).await;
    let mut delivered = None;
    for number in 1..=3 {
        delivered = delivered.or(consume(cluster.node(number)).await);
    }
    assert_eq!(
        delivered.expect("the message came back")["id"],
        id.as_str(),
        "accepted: the consumer is idempotent"
    );
    cluster.shutdown().await;
}

/// UC: UC-BKP-045
#[tokio::test]
async fn a_slow_bucket_holds_only_the_fifo_queues_the_rest_of_the_cluster_is_available() {
    use taba_node::storage::backup::MemoryStore;
    let s3 = Arc::new(MemoryStore::new());
    let mut cluster = TestCluster::start_with_backup(3, ENV, s3.clone()).await;
    settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), PLAIN).await;
    let created = cluster
        .node(1)
        .client
        .post(cluster.node(1).client_url("/topics"))
        .json(&json!({ "name": "ordered", "type": "queue", "fifo": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    publish(cluster.node(1), json!({ "n": 1 })).await;
    cluster.clock.advance(Duration::from_secs(60));

    let fallen = fall_all(&mut cluster).await;
    // The bucket answers, but slowly: the scan at the end of the pass does not finish.
    s3.set_slow(true);
    cluster.network.add_balancer(balancer(), Vec::new());
    for node in &fallen {
        come_back(&mut cluster, node).await;
    }
    for number in 1..=3 {
        wait_joined(cluster.node(number)).await;
    }

    // The cluster is available: the plain queue takes and serves messages.
    let id = publish(cluster.node(1), json!({ "n": 2 })).await;
    let message = loop {
        if let Some(message) = consume(cluster.node(2)).await {
            break message;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(message["id"], id.as_str());
    // The vote does not close while the scan is going: the FIFO queue accepts but does not deliver.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!((1..=3).all(|n| status_of(&cluster, n) == Status::Restoring));
    let accepted = cluster
        .node(1)
        .client
        .post(
            cluster
                .node(1)
                .client_url("/topics/ordered/messages?groupId=g&deduplicationId=k1"),
        )
        .json(&json!({ "n": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), 201);
    let held = cluster
        .node(1)
        .client
        .get(
            cluster
                .node(1)
                .client_url("/topics/ordered/messages?maxMessages=1"),
        )
        .send()
        .await
        .unwrap();
    let held: serde_json::Value = held.json().await.unwrap();
    assert!(held["messages"].as_array().is_none_or(Vec::is_empty));

    // The bucket answers again: the scan ends, the cluster is restored and the FIFO queue is released.
    s3.set_slow(false);
    eventually("the vote closed on every node", || {
        (1..=3).all(|n| status_of(&cluster, n) == Status::Restored)
    })
    .await;
    cluster.shutdown().await;
}

/// Confirms the message (`ack`) through the node.
async fn ack(node: &TestNode, message: &serde_json::Value) {
    let response = node
        .client
        .delete(node.client_url(&format!(
            "/topics/{PLAIN}/messages/{}?receiptToken={}",
            message["id"].as_str().unwrap(),
            message["receiptToken"].as_str().unwrap()
        )))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), 204);
}

/// Consumes (and confirms) everything the cluster delivers, asking each node, until a round brings nothing.
async fn drain(cluster: &TestCluster) -> Vec<String> {
    let mut ids = Vec::new();
    let mut quiet = 0;
    while quiet < 8 {
        let mut got = false;
        for number in 1..=cluster.nodes.len() {
            if let Some(message) = consume(cluster.node(number)).await {
                ack(cluster.node(number), &message).await;
                ids.push(message["id"].as_str().unwrap().to_owned());
                got = true;
            }
        }
        quiet = if got { 0 } else { quiet + 1 };
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    ids
}

/// UC: UC-RTN-053
#[tokio::test]
async fn with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls()
{
    // The messages go to disk (no memory), so what the nodes publish survives the falls.
    let mut env = ENV.to_vec();
    env.push(("TABA_MEMORY_LIMIT", "1B"));
    let mut cluster = TestCluster::start_with(3, &env).await;
    settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), PLAIN).await;
    cluster.clock.advance(Duration::from_secs(60));

    let mut confirmed: Vec<String> = Vec::new();
    let mut delivered_after_falls = 0;
    // The folders of the disks the nodes came back to must outlive the rounds.
    let mut disks = Vec::new();
    for round in 0..3u64 {
        // Published through a different node each round; some are confirmed before the fall, the others are left.
        let mut published = Vec::new();
        for n in 0..4 {
            let node = cluster.node(1 + (round as usize + n) % 3);
            published.push(publish(node, json!({ "round": round, "n": n })).await);
        }
        let mut waiting = published.len();
        while waiting > 2 {
            let node = cluster.node(1 + (round as usize + waiting) % 3);
            if let Some(message) = consume(node).await {
                ack(node, &message).await;
                confirmed.push(message["id"].as_str().unwrap().to_owned());
                waiting -= 1;
            } else {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
        for id in &confirmed {
            for number in 1..=3 {
                let (messages, id) = (messages_of(cluster.node(number)), id.clone());
                eventually("the exclusion in the log", move || {
                    messages.is_excluded(PLAIN, &id)
                })
                .await;
            }
        }
        cluster.clock.advance(Duration::from_secs(30));

        let fallen = fall_all(&mut cluster).await;
        come_back_all(&mut cluster, &fallen).await;
        disks.extend(fallen);
        // What the cluster delivers after the return is never something that was confirmed before.
        let again = drain(&cluster).await;
        for id in &again {
            assert!(!confirmed.contains(id), "round {round}: {id} came back");
        }
        delivered_after_falls += again.len();
        confirmed.extend(again);
    }
    assert!(
        delivered_after_falls > 0,
        "the property is not vacuous: the cluster delivered what was left"
    );
    cluster.shutdown().await;
}

/// The tag \`owner\` of a queue, read from the node (None: the queue is not there).
async fn owner_of(node: &TestNode, name: &str) -> Option<String> {
    let response = node.client_get(&format!("/topics/{name}")).await;
    if response.status() != 200 {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    Some(
        body["tags"]["owner"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    )
}

/// UC: UC-RTN-055
#[tokio::test]
async fn when_several_nodes_connect_the_highest_version_of_each_definition_wins_even_over_the_disk_of_the_coordinator()
 {
    let mut cluster = setup().await;
    let mut fallen = Vec::new();
    while !cluster.nodes.is_empty() {
        fallen.push(fall(&mut cluster).await);
    }
    fallen.reverse();
    // The coordinator (the lowest old ID) is the node that was left out before: its disk is the oldest. The others
    // hold the same queue at higher versions, and the last one holds another queue that the others never saw.
    leave_tagged_definition_on_disk(&fallen[0], "shared", 1, "node-1");
    leave_tagged_definition_on_disk(&fallen[1], "shared", 4, "node-2");
    leave_tagged_definition_on_disk(&fallen[2], "shared", 6, "node-3");
    leave_tagged_definition_on_disk(&fallen[2], "only-3", 2, "node-3");
    cluster.network.add_balancer(balancer(), Vec::new());
    for node in &fallen {
        come_back(&mut cluster, node).await;
    }
    for number in 1..=3 {
        wait_joined(cluster.node(number)).await;
    }
    for number in 1..=3 {
        eventually_async("the highest version in every node", || async {
            let node = cluster.node(number);
            owner_of(node, "shared").await.as_deref() == Some("node-3")
                && owner_of(node, "only-3").await.is_some()
        })
        .await;
    }
    // The queue every disk had is still there.
    assert!(owner_of(cluster.node(1), PLAIN).await.is_some());
    cluster.shutdown().await;
}

/// UC: UC-RTN-056
#[tokio::test]
async fn a_late_node_that_starts_alone_serves_what_it_has_and_the_higher_version_wins_when_another_answers()
 {
    let mut cluster = setup().await;
    let mut fallen = Vec::new();
    while !cluster.nodes.is_empty() {
        fallen.push(fall(&mut cluster).await);
    }
    fallen.reverse();
    leave_tagged_definition_on_disk(&fallen[0], "late", 1, "node-1");
    leave_tagged_definition_on_disk(&fallen[1], "late", 5, "node-2");
    cluster.network.add_balancer(balancer(), Vec::new());

    // Only the node with the lowest old ID comes back: nobody answers, so it starts alone with the disk it has.
    come_back(&mut cluster, &fallen[0]).await;
    wait_joined(cluster.node(1)).await;
    assert_eq!(cluster.member_ids(1).len(), 1, "it is alone");
    assert_eq!(
        owner_of(cluster.node(1), "late").await.as_deref(),
        Some("node-1")
    );
    assert!(owner_of(cluster.node(1), PLAIN).await.is_some());

    // A node with a higher version of the definition arrives: the window of risk ends and the higher version wins.
    come_back(&mut cluster, &fallen[1]).await;
    wait_joined(cluster.node(2)).await;
    eventually_async("the higher version in both", || async {
        owner_of(cluster.node(1), "late").await.as_deref() == Some("node-2")
            && owner_of(cluster.node(2), "late").await.as_deref() == Some("node-2")
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-RTN-057
#[tokio::test]
async fn the_node_that_was_missing_when_the_probe_ended_joins_later_by_the_normal_join_and_the_condition_goes_away()
 {
    let mut cluster = setup().await;
    let mut fallen = Vec::new();
    while !cluster.nodes.is_empty() {
        fallen.push(fall(&mut cluster).await);
    }
    fallen.reverse();
    cluster.network.add_balancer(balancer(), Vec::new());
    let probe = [
        ("TABA_CLUSTER_RESTORE_PROBE_INTERVAL", "200ms"),
        ("TABA_CLUSTER_RESTORE_PROBE_TIMEOUT", "1s"),
    ];

    // Two of the three come back; the third does not, and the probe gives up.
    come_back_with(&mut cluster, &fallen[0], &probe).await;
    come_back_with(&mut cluster, &fallen[1], &probe).await;
    for number in 1..=2 {
        wait_joined(cluster.node(number)).await;
    }
    eventually("the probe ended with a node missing", || {
        cluster.node(1).node.cluster().unwrap().members_missing()
    })
    .await;
    let view = cluster.node(1).stats("/health/cluster?scope=node").await;
    assert!(view.to_string().contains("members-missing"), "{view}");

    // The missing node arrives: it is a node like any other that joins a running cluster.
    come_back_with(&mut cluster, &fallen[2], &probe).await;
    wait_joined(cluster.node(3)).await;
    eventually("the three in one table", || {
        (1..=3).all(|n| cluster.member_ids(n).len() == 3)
    })
    .await;
    eventually("the condition to go away", || {
        (1..=3).all(|n| !cluster.node(n).node.cluster().unwrap().members_missing())
    })
    .await;
    cluster.shutdown().await;
}
