//! The FIFO queues while the previous lives are restored (doc 07, section 10.9).

use super::*;

fn fifo_request(group: &str, key: &str, body: &str) -> PublishRequest {
    PublishRequest {
        body: body.as_bytes().to_vec().into(),
        group_id: Some(group.into()),
        deduplication_id: Some(key.into()),
        ..Default::default()
    }
}

/// What the primary of the group does with a publication: the draft and the commit.
fn accept(h: &Harness, topic: &str, request: &PublishRequest) -> Option<String> {
    let view = h.topics.get(topic).unwrap();
    match h.messages.draft_fifo_publish(&view, request).unwrap() {
        Draft::New { meta, body } => {
            let id = meta.id.clone();
            h.messages.apply_publish(topic, meta, body).unwrap();
            Some(id)
        }
        Draft::Duplicate { .. } => None,
    }
}

/// A message that comes from a past life, with the key given.
fn restored(h: &Harness, topic: &str, id: &str, group: &str, key: &str) {
    let meta = MessageMeta {
        id: id.into(),
        content_type: "application/json".into(),
        group_id: Some(group.into()),
        deduplication_id: Some(key.into()),
        published_ms: crate::core::id::unix_millis_of(id).unwrap(),
        delivery_count: 2,
        invisible_until_ms: 0,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
        position: 0,
    };
    assert!(matches!(
        h.messages
            .apply_load(topic, meta, br#"{"restored":true}"#.to_vec().into())
            .unwrap(),
        Loaded::Stored
    ));
}

/// UC: UC-FIF-048
#[tokio::test]
async fn a_held_fifo_queue_receives_and_does_not_deliver_until_it_is_released() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    h.messages.hold_fifo();
    assert!(accept(&h, "f", &fifo_request("g", "k1", "1")).is_some());
    assert!(fifo_get(&h, "f", "5").await.is_empty());
    // The plain queues do not wait.
    h.topics.create(queue("plain")).await.unwrap();
    h.messages.publish("plain", json_body("1")).unwrap();
    assert_eq!(
        consume(&h, "plain", q(Some("5"), None, None)).await.len(),
        1
    );

    h.messages.release_fifo();
    assert!(!h.messages.fifo_held());
    assert_eq!(fifo_get(&h, "f", "5").await.len(), 1);
}

/// UC: UC-FIF-050
#[tokio::test]
async fn while_held_nothing_is_deduplicated_and_the_duplicate_answer_never_comes() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    // Outside the hold the second publication is a duplicate.
    assert!(accept(&h, "f", &fifo_request("g", "k0", "1")).is_some());
    assert!(accept(&h, "f", &fifo_request("g", "k0", "1")).is_none());

    h.messages.hold_fifo();
    let first = accept(&h, "f", &fifo_request("g", "k1", "1")).unwrap();
    let second = accept(&h, "f", &fifo_request("g", "k1", "1")).unwrap();
    assert!(second > first, "the group's order is the order of the ids");
}

/// UC: UC-FIF-051, UC-FIF-053
#[tokio::test]
async fn the_release_lists_as_duplicates_what_repeats_a_restored_key_or_an_earlier_publication() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    h.messages.hold_fifo();
    let first = accept(&h, "f", &fifo_request("g", "k1", "1")).unwrap();
    let second = accept(&h, "f", &fifo_request("g", "k1", "2")).unwrap();
    let alone = accept(&h, "f", &fifo_request("g", "k2", "3")).unwrap();
    let other_group = accept(&h, "f", &fifo_request("h", "k1", "4")).unwrap();
    // Among themselves, the first one stays; the others of the same key are duplicates.
    assert_eq!(
        h.messages.fifo_duplicates(),
        [("f".to_owned(), second.clone())]
    );

    // A message of a past life arrives with the key of "alone": it is older, so it keeps the key, and the
    // restored one is not delivered while the queue is held.
    let old = crate::core::id::new_at(h.clock.now_millis().saturating_sub(10));
    restored(&h, "f", &old, "g", "k2");
    let mut duplicates = h.messages.fifo_duplicates();
    duplicates.sort();
    let mut want = vec![("f".to_owned(), second), ("f".to_owned(), alone)];
    want.sort();
    assert_eq!(duplicates, want);
    assert!(fifo_get(&h, "f", "5").await.is_empty());
    let _ = (first, other_group);

    // The release forgets the list.
    h.messages.release_fifo();
    assert!(h.messages.fifo_duplicates().is_empty());
}

/// UC: UC-FIF-049
#[tokio::test]
async fn the_ids_of_the_new_life_start_above_the_watermark_of_the_old_ones() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let ahead = h.clock.now_millis() + 3_600_000;
    let watermark = crate::core::id::new_at(ahead);
    h.messages.raise_id_floor(&watermark);
    let id = accept(&h, "f", &fifo_request("g", "k", "1")).unwrap();
    assert!(id > watermark, "{id} must be above {watermark}");
    // The manifest records what the node generated.
    assert!(h.messages.watermark().unwrap() >= watermark);
    // Not an ID: ignored.
    h.messages.raise_id_floor("not-an-id");
}
