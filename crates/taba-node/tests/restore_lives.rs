//! The restoration of the past lives (doc 07, sections 10.3 to 10.6; level **C**): the disk of a process that
//! restarted becomes the folder of its life, and every message in it is sent, with its original ID, to the new
//! primary of its block, which checks the topic and the dead before it activates. The `Clock` is manual.

mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use common::TestNode;
use common::cluster::{TestCluster, eventually, eventually_async, host_of, wait_joined};
use taba_node::cluster::protocol::{MessageRecord, Response};
use taba_node::cluster::recovery::Recovery;
use taba_node::cluster::restore::Manifest;
use taba_node::cluster::transport::PeerAddr;
use taba_node::config::DbSynchronous;
use taba_node::core::clock::Clock;
use taba_node::core::id;
use taba_node::engine::messages::Messages;
use taba_node::storage::backup::journal::{self, Header, Record};
use taba_node::storage::backup::{MemoryStore, ObjectStore, keys};
use taba_node::storage::messages::{ExcludedRow, MessageStore};

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

fn record(cluster: &TestCluster, topic: &str, n: u64) -> MessageRecord {
    let now = cluster.clock.now_millis();
    MessageRecord {
        id: id::new_at(now + n),
        topic: topic.into(),
        body: json!({ "n": n }).to_string().into_bytes().into(),
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

fn has(node: &TestNode, topic: &str, id: &str) -> bool {
    messages_of(node).message_info(topic, id).unwrap().is_some()
}

async fn setup(env: &[(&str, &str)]) -> TestCluster {
    let mut all = vec![("TABA_DRAIN_ON_STOP", "false")];
    all.extend_from_slice(env);
    let cluster = TestCluster::start_with(3, &all).await;
    common::cluster::settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), TOPIC).await;
    cluster
}

/// The `topicId` the cluster gave the queue.
fn topic_id(cluster: &TestCluster) -> String {
    messages_of(cluster.node(1))
        .current_topic_id(TOPIC)
        .expect("the topic exists")
}

/// What the process that dies leaves on disk: `(topic, topicId the folder was created for, record)`.
type OnDisk<'a> = (&'a str, Option<&'a str>, &'a MessageRecord);

/// Node 3's process falls without drain, long after the others recorded the start of their logs (the clock
/// moves 60 s, so the last activity of the life is inside the window), and comes back at the same address with
/// the same disk: it is born as a new node. Returns the number of the node and the folder of the old life.
async fn restart_third(
    cluster: &mut TestCluster,
    env: &[(&str, &str)],
    on_disk: &[OnDisk<'_>],
) -> (usize, std::path::PathBuf) {
    cluster.clock.advance(Duration::from_secs(60));
    let port = cluster.node(3).node.cluster_addr().port();
    let old_id = cluster.node_id(3);
    let TestNode { node, dir, .. } = cluster.nodes.pop().unwrap();
    node.shutdown().await;
    cluster.network.unregister(&PeerAddr::new(host_of(3), port));
    let storage = dir.path().join("taba");
    for (topic, id, record) in on_disk {
        // The folder records the `topicId` it was created for, as the running node does (the first message of a
        // topic creates the folder).
        let old_disk = MessageStore::new(&storage, DbSynchronous::Off);
        let id = id.map(str::to_owned);
        old_disk.set_topic_id_resolver(Box::new(move |_| id.clone()));
        old_disk
            .put(topic, &record.to_meta(), &record.body)
            .expect("writes to disk");
    }
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
    std::mem::forget(dir);
    (number, storage.join(MessageStore::LIVES).join(old_id))
}

fn left_in_life(life: &std::path::Path, topic: &str, id: &str) -> bool {
    MessageStore::new(life, DbSynchronous::Off)
        .get(topic, id)
        .unwrap()
        .is_some()
}

/// UC: UC-RTN-026
#[tokio::test]
async fn a_life_inside_the_window_is_sent_whole_to_the_new_primary_with_its_original_id() {
    let mut cluster = setup(&[]).await;
    let id = topic_id(&cluster);
    let first = record(&cluster, TOPIC, 1);
    let second = record(&cluster, TOPIC, 2);
    let known = record(&cluster, TOPIC, 3);
    // The cluster already has this one: the primary skips it (`Existing`).
    for number in [1, 2] {
        messages_of(cluster.node(number))
            .apply_load(TOPIC, known.to_meta(), known.body.clone())
            .unwrap();
    }

    let (number, life) = restart_third(
        &mut cluster,
        &[],
        &[
            (TOPIC, Some(&id), &first),
            (TOPIC, Some(&id), &second),
            (TOPIC, Some(&id), &known),
        ],
    )
    .await;
    wait_joined(cluster.node(number)).await;

    // Nobody declared any loss: the whole life comes back, with the IDs it had.
    eventually(
        "the two messages of the life to reach the other nodes",
        || {
            [&first, &second]
                .iter()
                .all(|r| has(cluster.node(1), TOPIC, &r.id) && has(cluster.node(2), TOPIC, &r.id))
        },
    )
    .await;
    let info = messages_of(cluster.node(1))
        .message_info(TOPIC, &first.id)
        .unwrap()
        .unwrap();
    assert_eq!(info.meta.delivery_count, 3, "the delivery count stays");
    assert_eq!(
        info.meta.invisible_until_ms, 0,
        "the invisibility is zeroed"
    );
    assert_eq!(info.meta.receipt_token, None, "the old receipt is gone");

    // What the primary confirmed is marked as sent: it leaves the folder of the life.
    eventually_async("the sent rows to leave the folder of the life", || async {
        [&first, &second, &known]
            .iter()
            .all(|r| !left_in_life(&life, TOPIC, &r.id))
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-RTN-027
#[tokio::test]
async fn messages_of_a_deleted_or_recreated_topic_are_discarded_without_asking_the_dead() {
    let mut cluster = setup(&[]).await;
    common::cluster::create_queue(cluster.node(1), "ghosts").await;
    let ghost_id = messages_of(cluster.node(1))
        .current_topic_id("ghosts")
        .unwrap();
    // The topic "ghosts" is deleted while the node is down; "reborn" is deleted and created again, with another
    // `topicId`. Each folder records the one topic it was created for.
    common::cluster::create_queue(cluster.node(1), "reborn").await;
    let older_id = messages_of(cluster.node(1))
        .current_topic_id("reborn")
        .unwrap();
    let gone = record(&cluster, "ghosts", 1);
    let recreated = record(&cluster, "reborn", 2);
    let alive = record(&cluster, TOPIC, 3);
    let id = topic_id(&cluster);
    for name in ["ghosts", "reborn"] {
        let response = cluster
            .node(1)
            .client
            .delete(cluster.node(1).client_url(&format!("/topics/{name}")))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "{}", response.status());
    }
    common::cluster::create_queue(cluster.node(1), "reborn").await;
    assert_ne!(
        messages_of(cluster.node(1)).current_topic_id("reborn"),
        Some(older_id.clone())
    );

    let (number, life) = restart_third(
        &mut cluster,
        &[],
        &[
            ("ghosts", Some(&ghost_id), &gone),
            ("reborn", Some(&older_id), &recreated),
            (TOPIC, Some(&id), &alive),
        ],
    )
    .await;
    wait_joined(cluster.node(number)).await;

    eventually("the live message to be restored", || {
        has(cluster.node(1), TOPIC, &alive.id) && has(cluster.node(2), TOPIC, &alive.id)
    })
    .await;
    // The discarded ones are resolved too (they leave the folder), and nobody keeps them.
    eventually_async(
        "the discarded rows to leave the folder of the life",
        || async {
            !left_in_life(&life, "ghosts", &gone.id)
                && !left_in_life(&life, "reborn", &recreated.id)
        },
    )
    .await;
    for number in [1, 2] {
        assert!(!has(cluster.node(number), "reborn", &recreated.id));
        assert!(!has(cluster.node(number), "ghosts", &gone.id));
    }
    cluster.shutdown().await;
}

/// UC: UC-RTN-028
#[tokio::test]
async fn the_new_primary_discards_what_its_own_log_or_the_log_of_any_present_node_says_is_dead() {
    // Two copies of each block: the third node is not an owner of some blocks and still gets asked.
    let cluster = setup(&[("TABA_REPLICATION_FACTOR", "2")]).await;
    let dead_here = record(&cluster, TOPIC, 1);
    let dead_there = record(&cluster, TOPIC, 2);
    let alive = record(&cluster, TOPIC, 3);
    let log = |number: usize, record: &MessageRecord| {
        let messages = messages_of(cluster.node(number));
        let position = messages.position_for(TOPIC, &record.id, None);
        messages
            .import_excluded(
                TOPIC,
                &[ExcludedRow {
                    id: record.id.clone(),
                    position,
                    at_ms: cluster.clock.now_millis(),
                }],
            )
            .unwrap();
    };
    // One ID is in the log of the node that receives it; the other is only in the log of node 3.
    log(1, &dead_here);
    log(3, &dead_there);

    let answer = recovery_of(cluster.node(1))
        .accept_restore(
            TOPIC.into(),
            None,
            vec![dead_here.clone(), dead_there.clone(), alive.clone()],
        )
        .await
        .expect("accepts");
    let Response::Recovered(result) = answer else {
        panic!("expected the count of resolved messages");
    };
    assert_eq!(result.count, 3, "two discarded and one activated");
    // Two copies of each block: the live message lives on its two owners, wherever they are.
    eventually("the live message to be activated", || {
        (1..=3)
            .filter(|n| has(cluster.node(*n), TOPIC, &alive.id))
            .count()
            == 2
    })
    .await;
    for number in 1..=3 {
        assert!(!has(cluster.node(number), TOPIC, &dead_here.id));
        assert!(!has(cluster.node(number), TOPIC, &dead_there.id));
    }

    // The query itself: a node tells which of the IDs its log has.
    let asked = recovery_of(cluster.node(3))
        .serve_dead(TOPIC.into(), vec![dead_there.id.clone(), alive.id.clone()])
        .await
        .expect("answers");
    let Response::Json(value) = asked else {
        panic!("expected JSON");
    };
    assert_eq!(value["dead"], json!([dead_there.id]));
    cluster.shutdown().await;
}

/// The journal in S3 registers the removal of `id`, as the upload of the owner does after an ack.
async fn remove_in_s3(s3: &Arc<MemoryStore>, topic: &str, topic_id: &str, ids: &[&str]) {
    let header = Header {
        topic: topic.into(),
        topic_id: topic_id.into(),
        writer: "a-writer".into(),
        at_ms: 1,
    };
    let records: Vec<Record> = ids.iter().map(|id| Record::removal(id)).collect();
    s3.put(
        &format!("taba/{}", keys::journal(topic, topic_id, 1, "a-writer", 1)),
        journal::encode(&header, &records),
    )
    .await
    .expect("writes to the bucket");
}

/// UC: UC-RTN-030
#[tokio::test]
async fn a_restored_message_is_on_the_disk_of_every_owner_even_with_memory_to_spare() {
    // The default memory limit: a published message would stay in memory. A restored one goes to disk.
    let mut cluster = setup(&[]).await;
    let id = topic_id(&cluster);
    let message = record(&cluster, TOPIC, 1);
    let (number, _) = restart_third(&mut cluster, &[], &[(TOPIC, Some(&id), &message)]).await;
    wait_joined(cluster.node(number)).await;
    eventually("the message to reach the other nodes", || {
        has(cluster.node(1), TOPIC, &message.id) && has(cluster.node(2), TOPIC, &message.id)
    })
    .await;
    for owner in [1, 2] {
        let disk = MessageStore::new(
            &cluster.node(owner).dir.path().join("taba"),
            DbSynchronous::Off,
        );
        assert!(
            disk.get(TOPIC, &message.id).unwrap().is_some(),
            "node {owner} has it on disk"
        );
    }
    cluster.shutdown().await;
}

/// UC: UC-RTN-029
#[tokio::test]
async fn the_journal_in_s3_discards_what_it_removed_and_a_silent_s3_only_leaves_a_warning() {
    let s3 = Arc::new(MemoryStore::new());
    let cluster = TestCluster::start_with_backup(
        3,
        &[("TABA_DRAIN_ON_STOP", "false"), ("TABA_MEMORY_LIMIT", "1B")],
        s3.clone(),
    )
    .await;
    common::cluster::settled(&cluster).await;
    common::cluster::create_queue(cluster.node(1), TOPIC).await;
    let id = topic_id(&cluster);
    let removed = record(&cluster, TOPIC, 1);
    let alive = record(&cluster, TOPIC, 2);
    remove_in_s3(&s3, TOPIC, &id, &[&removed.id]).await;

    let answer = recovery_of(cluster.node(1))
        .accept_restore(TOPIC.into(), None, vec![removed.clone(), alive.clone()])
        .await
        .expect("accepts");
    let Response::Recovered(result) = answer else {
        panic!("expected the count of resolved messages");
    };
    assert_eq!(result.count, 2);
    assert!(
        !has(cluster.node(1), TOPIC, &removed.id),
        "the journal says it died"
    );
    eventually("the live message to be activated", || {
        has(cluster.node(1), TOPIC, &alive.id)
    })
    .await;

    // S3 does not answer: the primary decides with the other two sources and goes on (the message is activated).
    s3.set_down(true);
    let another = record(&cluster, TOPIC, 3);
    let answer = recovery_of(cluster.node(2))
        .accept_restore(TOPIC.into(), None, vec![another.clone()])
        .await
        .expect("accepts without S3");
    assert!(matches!(answer, Response::Recovered(r) if r.count == 1));
    s3.set_down(false);
    eventually("the message to be activated without S3", || {
        has(cluster.node(2), TOPIC, &another.id)
    })
    .await;
    cluster.shutdown().await;
}

/// UC: UC-RTN-030
#[tokio::test]
async fn a_batch_the_primary_did_not_confirm_stays_in_the_life_and_is_sent_again() {
    let mut cluster = setup(&[("TABA_CLUSTER_BOOTSTRAP_RETRY_INTERVAL", "200ms")]).await;
    let id = topic_id(&cluster);
    let records: Vec<MessageRecord> = (1..=6).map(|n| record(&cluster, TOPIC, n)).collect();
    // The two primaries do not receive the restoration (the third node may be the primary of some, which is
    // resolved without the network).
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "restore", 1_000_000);
    }
    let on_disk: Vec<OnDisk<'_>> = records
        .iter()
        .map(|r| (TOPIC, Some(id.as_str()), r))
        .collect();
    let (number, life) = restart_third(&mut cluster, &[], &on_disk).await;
    wait_joined(cluster.node(number)).await;

    eventually_async(
        "the rows of the remote primaries to stay in the life",
        || async { records.iter().any(|r| left_in_life(&life, TOPIC, &r.id)) },
    )
    .await;
    // Nothing the primary did not confirm is marked as sent: whatever stayed is not on the other nodes.
    for r in records.iter().filter(|r| left_in_life(&life, TOPIC, &r.id)) {
        assert!(!has(cluster.node(1), TOPIC, &r.id));
        assert!(!has(cluster.node(2), TOPIC, &r.id));
    }

    // The primaries answer again: the next try settles everything, and the life is empty.
    for host in ["10.0.0.1", "10.0.0.2"] {
        cluster.network.lose_kind(host, "restore", 0);
    }
    eventually_async("the whole life to be settled", || async {
        records.iter().all(|r| !left_in_life(&life, TOPIC, &r.id))
    })
    .await;
    eventually("every message to reach the nodes", || {
        records
            .iter()
            .all(|r| has(cluster.node(1), TOPIC, &r.id) && has(cluster.node(2), TOPIC, &r.id))
    })
    .await;
    // Marked as sent, nothing is sent again.
    let summary = recovery_of(cluster.node(number)).restore_lives().await;
    assert_eq!(summary.sent, 0);
    cluster.shutdown().await;
}

/// UC: UC-ING-037
#[tokio::test]
async fn no_life_is_restorable_while_a_member_does_not_tell_when_its_log_started() {
    let mut cluster = setup(&[]).await;
    let id = topic_id(&cluster);
    let first = record(&cluster, TOPIC, 1);
    let late = record(&cluster, TOPIC, 2);
    let (number, life) = restart_third(&mut cluster, &[], &[(TOPIC, Some(&id), &first)]).await;
    wait_joined(cluster.node(number)).await;
    // The first pass (in the background) settles the life.
    eventually_async("the first pass to settle the life", || async {
        !left_in_life(&life, TOPIC, &first.id)
    })
    .await;

    // The end of the restoration deletes the folder of the life (doc 07, section 10.8).
    eventually("the folder of the settled life to be deleted", || {
        !life.exists()
    })
    .await;

    // A life shows up again with a message; a member does not tell when its log started (an earlier version does not
    // publish `excludedSince`; to the others it is as if it did not answer).
    let folder = MessageStore::new(&life, DbSynchronous::Off);
    folder.put(TOPIC, &late.to_meta(), &late.body).unwrap();
    drop(folder);
    Manifest {
        node_id: life.file_name().unwrap().to_string_lossy().into_owned(),
        members: Vec::new(),
        table_version: 1,
        last_seen_ms: cluster.clock.now_millis(),
        watermark: None,
        excluded_since_ms: None,
    }
    .write(&life)
    .unwrap();
    cluster.network.lose_kind("10.0.0.2", "status", 1_000_000);
    let recovery = recovery_of(cluster.node(number));
    assert_eq!(recovery.cluster_cutoff().await, None);
    let summary = recovery.restore_lives().await;
    assert_eq!(summary.sent, 0, "nothing is sent");
    assert_eq!(summary.outside_window, 1);
    assert!(left_in_life(&life, TOPIC, &late.id));
    assert!(!has(cluster.node(1), TOPIC, &late.id));

    // When everybody tells, the life is inside the window again and the message goes.
    cluster.network.lose_kind("10.0.0.2", "status", 0);
    assert!(recovery.cluster_cutoff().await.is_some());
    let summary = recovery.restore_lives().await;
    assert_eq!(summary.sent, 1);
    assert!(!left_in_life(&life, TOPIC, &late.id));
    cluster.shutdown().await;
}

/// UC: UC-RTN-045
#[tokio::test]
async fn a_message_the_primary_already_has_takes_the_greater_delivery_count_of_the_two_copies() {
    let mut cluster = setup(&[]).await;
    let id = topic_id(&cluster);
    // The message already exists on the two nodes that stay up, with a small counter.
    let mut copy = record(&cluster, TOPIC, 1);
    copy.delivery_count = 1;
    copy.invisible_until_ms = None;
    copy.receipt_token = None;
    for number in [1, 2] {
        messages_of(cluster.node(number))
            .apply_load(TOPIC, copy.to_meta(), copy.body.clone())
            .unwrap();
    }
    // The life of the third node has the same message, delivered more times.
    let mut old = copy.clone();
    old.delivery_count = 7;
    let (number, _) = restart_third(&mut cluster, &[], &[(TOPIC, Some(&id), &old)]).await;
    wait_joined(cluster.node(number)).await;
    eventually(
        "the greater counter on the nodes that already had the message",
        || {
            [1, 2].iter().all(|n| {
                messages_of(cluster.node(*n))
                    .message_info(TOPIC, &copy.id)
                    .unwrap()
                    .is_some_and(|info| info.meta.delivery_count == 7)
            })
        },
    )
    .await;
    cluster.shutdown().await;
}

/// UC: UC-RTN-049
#[tokio::test]
async fn a_life_outside_the_window_is_not_restored_and_its_folder_goes_with_the_end_of_the_pass() {
    let cluster = setup(&[]).await;
    cluster.clock.advance(Duration::from_secs(60));
    let old = id::new_at(1);
    let stale = record(&cluster, TOPIC, 1);
    let dir = messages_of(cluster.node(1))
        .storage_root()
        .join(MessageStore::LIVES)
        .join(&old);
    let folder = MessageStore::new(&dir, DbSynchronous::Off);
    folder.set_topic_id_resolver(Box::new(|_| None));
    folder
        .put(TOPIC, &stale.to_meta(), &stale.body)
        .expect("writes to disk");
    drop(folder);
    // Last seen before every log of the cluster started: nobody recorded what happened to it afterwards.
    Manifest {
        node_id: old.clone(),
        members: Vec::new(),
        table_version: 1,
        last_seen_ms: 1,
        watermark: None,
        excluded_since_ms: None,
    }
    .write(&dir)
    .expect("writes the manifest");

    let recovery = recovery_of(cluster.node(1));
    let summary = recovery.restore_lives().await;
    assert_eq!(summary.lives, 1);
    assert_eq!(summary.outside_window, 1);
    assert_eq!(summary.sent, 0, "nothing of it is sent");
    assert!(left_in_life(&dir, TOPIC, &stale.id));
    assert!(
        (1..=3).all(|n| !has(cluster.node(n), TOPIC, &stale.id)),
        "no node took the message"
    );

    // The pass is complete (there was nothing to send): the folder is deleted, and the message follows the path of
    // today, where only the listed losses come back.
    recovery.drop_lives(&summary);
    assert!(!dir.exists());
    cluster.shutdown().await;
}

/// UC: UC-RTN-050
#[tokio::test]
async fn a_message_the_primary_already_has_is_skipped_and_keeps_its_invisibility_and_receipt() {
    let mut cluster = setup(&[]).await;
    let id = topic_id(&cluster);
    // The cluster already has the message, delivered to a consumer that still holds it.
    let held = record(&cluster, TOPIC, 1);
    for number in [1, 2] {
        messages_of(cluster.node(number))
            .apply_load(TOPIC, held.to_meta(), held.body.clone())
            .unwrap();
    }
    // The life has the same message with the same counter, but visible and with no receipt.
    let mut old = held.clone();
    old.invisible_until_ms = None;
    old.receipt_token = None;
    let (number, life) = restart_third(&mut cluster, &[], &[(TOPIC, Some(&id), &old)]).await;
    wait_joined(cluster.node(number)).await;
    eventually_async("the row to leave the folder of the life", || async {
        !left_in_life(&life, TOPIC, &held.id)
    })
    .await;

    // Skipped: it was not activated again, so the consumer's hold is intact on both copies.
    for n in [1, 2] {
        let info = messages_of(cluster.node(n))
            .message_info(TOPIC, &held.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            info.meta.invisible_until_ms,
            held.invisible_until_ms.unwrap()
        );
        assert_eq!(info.meta.receipt_token, held.receipt_token);
        assert_eq!(info.meta.delivery_count, 3);
    }
    cluster.shutdown().await;
}
