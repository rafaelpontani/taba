//! What the engine notifies the S3 backup of (doc 23): publication, ack, delivery, dead-letter, owner
//! change and body absent from disk. The test's `BackupSink` only records the notices.

use std::sync::Mutex;

use super::*;

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<String>>,
    /// The body S3 would return for a message whose file the eviction deleted.
    s3_body: Mutex<Option<Vec<u8>>>,
    waited_for_disk: Mutex<u32>,
}

impl Recorder {
    fn log(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

impl BackupSink for Recorder {
    fn stored(&self, topic: &str, id: &str, owned: bool) {
        self.log(format!("stored {topic} {id} owned={owned}"));
    }

    fn removed(&self, topic: &str, id: &str, owned: bool) {
        self.log(format!("removed {topic} {id} owned={owned}"));
    }

    fn changed(&self, topic: &str, id: &str, owned: bool) {
        self.log(format!("changed {topic} {id} owned={owned}"));
    }

    fn adopted(&self, topic: &str, id: &str) {
        self.log(format!("adopted {topic} {id}"));
    }

    fn released(&self, topic: &str, id: &str) {
        self.log(format!("released {topic} {id}"));
    }

    fn fetch_body(&self, _topic: &str, _id: &str) -> Option<Vec<u8>> {
        self.s3_body.lock().unwrap().clone()
    }

    fn before_disk_write(&self) -> bool {
        *self.waited_for_disk.lock().unwrap() += 1;
        true
    }
}

fn with_recorder() -> (Harness, Arc<Recorder>) {
    let h = harness();
    let recorder = Arc::new(Recorder::default());
    h.messages.set_backup(recorder.clone());
    (h, recorder)
}

/// UC: UC-BKP-009
#[tokio::test]
async fn publishing_only_tells_the_backup_and_never_waits_for_it() {
    let (h, recorder) = with_recorder();
    h.topics.create(queue("orders")).await.unwrap();
    let id = publish_ok(&h, "orders", "1");
    assert_eq!(
        recorder.events(),
        [format!("stored orders {id} owned=true")],
        "one row in the outbox; no call to S3"
    );
}

/// UC: UC-BKP-013
#[tokio::test]
async fn a_delivery_is_a_state_change_and_an_ack_is_a_removal() {
    let (h, recorder) = with_recorder();
    h.topics.create(queue("orders")).await.unwrap();
    let id = publish_ok(&h, "orders", "1");
    let got = consume(&h, "orders", q(Some("1"), Some("60"), None)).await;
    h.messages
        .ack("orders", &got[0].id, Some(&got[0].receipt_token))
        .unwrap();
    assert_eq!(
        recorder.events(),
        [
            format!("stored orders {id} owned=true"),
            format!("changed orders {id} owned=true"),
            format!("removed orders {id} owned=true"),
        ]
    );
}

/// UC: UC-BKP-015
#[tokio::test]
async fn a_message_that_goes_to_the_dead_letter_is_removed_from_the_queue_and_stored_in_the_other()
{
    let (h, recorder) = with_recorder();
    let mut request = queue("orders");
    request.max_deliveries = Some(1);
    h.topics.create(request).await.unwrap();
    let id = publish_ok(&h, "orders", "1");
    consume(&h, "orders", q(Some("1"), Some("1"), None)).await;
    h.clock.advance(Duration::from_secs(5));
    // Any operation settles the queue and the dead-letter.
    let _ = h
        .messages
        .consume("orders-dead-letter", q(Some("1"), None, None))
        .await;
    let events = recorder.events();
    assert!(
        events.contains(&format!("removed orders {id} owned=true")),
        "{events:?}"
    );
    assert!(
        events.contains(&format!("stored orders-dead-letter {id} owned=true")),
        "{events:?}"
    );
}

/// UC: UC-BKP-037
#[tokio::test]
async fn a_replica_becomes_the_owner_and_adopts_and_the_old_owner_releases() {
    let (h, recorder) = with_recorder();
    let owners = Arc::new(super::cluster::Owners::default());
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    h.messages.enable_cluster(ClusterHooks {
        placement: owners.clone(),
        dead_letters: tx,
        ready: Arc::new(|_, _| {}),
    });
    h.topics.create(queue("orders")).await.unwrap();
    let view = h.topics.get("orders").unwrap();
    let request = json_body("1");
    let (meta, body) = h.messages.draft_publish(&view, &request).unwrap();
    let id = meta.id.clone();
    // Replica: stores, but is not the owner.
    h.messages.apply_publish("orders", meta, body).unwrap();
    assert_eq!(
        recorder.events(),
        [format!("stored orders {id} owned=false")]
    );

    // The node becomes the owner (the primary fell): adopts.
    owners.0.lock().unwrap().insert(id.clone());
    h.messages.reown();
    assert!(recorder.events().contains(&format!("adopted orders {id}")));

    // And stops being: releases.
    owners.0.lock().unwrap().clear();
    h.messages.reown();
    assert!(recorder.events().contains(&format!("released orders {id}")));
}

/// UC: UC-BKP-029
#[tokio::test]
async fn a_body_the_eviction_removed_from_disk_comes_back_from_the_backup() {
    let (h, recorder) = with_recorder();
    h.topics.create(queue("orders")).await.unwrap();
    let id = publish_ok(&h, "orders", "{\"a\":1}");
    // To disk, as in the drain.
    h.messages.drain();
    assert!(h.store.has_body("orders", &id));
    // The eviction deletes only the body file; the row stays.
    assert!(h.store.evict_body("orders", &id).unwrap() > 0);
    assert!(!h.store.has_body("orders", &id));
    assert!(
        h.store.get("orders", &id).unwrap().is_some(),
        "the row stays"
    );

    // Without the backup (or without the message there), the usual behavior applies: a vanished body is a loss.
    // With it, the body comes back from the segment.
    *recorder.s3_body.lock().unwrap() = Some(b"{\"a\":1}".to_vec());
    let got = consume(&h, "orders", q(Some("1"), None, None)).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].id, id);
}

/// UC: UC-BKP-030
#[tokio::test]
async fn the_drain_asks_the_backup_before_each_memory_to_disk_write() {
    let (h, recorder) = with_recorder();
    h.topics.create(queue("orders")).await.unwrap();
    for i in 0..3 {
        publish_ok(&h, "orders", &i.to_string());
    }
    h.messages.drain();
    assert_eq!(*recorder.waited_for_disk.lock().unwrap(), 3);
}

/// UC: UC-BKP-001
#[tokio::test]
async fn without_a_backup_nothing_is_told_and_a_missing_body_is_still_a_loss() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let id = publish_ok(&h, "orders", "1");
    h.messages.drain();
    h.store.evict_body("orders", &id).unwrap();
    assert!(
        consume(&h, "orders", q(Some("1"), None, None))
            .await
            .is_empty(),
        "without S3, a body that vanished from disk is a loss, as it always was"
    );
}
