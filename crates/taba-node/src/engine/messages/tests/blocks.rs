//! The ring's per-block operations (doc 05, section 7.3): list, read, load and clean.

use std::collections::HashSet;
use std::sync::Mutex;

use super::*;
use crate::core::ring::message_position;

/// The node is only responsible for the positions in the list.
#[derive(Default)]
struct Responsible(Mutex<Option<HashSet<u64>>>);

impl Placement for Responsible {
    fn is_owner(&self, _topic: &str, _id: &str, _group: Option<&str>, _position: u64) -> bool {
        false
    }

    fn is_responsible(&self, position: u64) -> bool {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|set| set.contains(&position))
    }
}

fn clustered(env: &[(&str, &str)]) -> (Harness, Arc<Responsible>) {
    let h = harness_with(env);
    let placement = Arc::new(Responsible::default());
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    h.messages.enable_cluster(ClusterHooks {
        placement: placement.clone(),
        dead_letters: tx,
        ready: Arc::new(|_, _| {}),
    });
    (h, placement)
}

fn record(h: &Harness, topic: &str, body: &str) -> (MessageMeta, Vec<u8>) {
    let view = h.topics.get(topic).unwrap();
    h.messages.draft_publish(&view, &json_body(body)).unwrap()
}

/// UC: UC-SIN-002, UC-SIN-006, UC-SIN-007
#[tokio::test]
async fn the_ids_of_a_block_come_from_memory_and_disk_in_order() {
    // Memory for a single message: the following ones go to disk.
    let (h, _) = clustered(&[("TABA_MEMORY_LIMIT", "20B")]);
    h.topics.create(queue("orders")).await.unwrap();
    h.topics.create(queue("other")).await.unwrap();
    let mut ids = Vec::new();
    for (n, topic) in ["orders", "other", "orders", "other", "orders"]
        .into_iter()
        .enumerate()
    {
        let (meta, body) = record(&h, topic, &format!("{{\"n\":{n}}}"));
        ids.push((meta.id.clone(), topic.to_owned()));
        h.messages.apply_publish(topic, meta, body).unwrap();
    }
    assert!(h.messages.storage_stats().disk_messages > 0);
    assert!(h.messages.storage_stats().memory_messages > 0);

    let all = [(0, u64::MAX)];
    let entries = h.messages.block_entries(&all, None, None, 100).unwrap();
    let listed: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.id.clone(), e.topic.clone()))
        .collect();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(listed, expected, "all the topics, in ID order");

    // The position range chooses the block: only what the position covers goes in.
    let pick = &ids[2].0;
    let position = message_position(pick);
    let only = h
        .messages
        .block_entries(&[(position, position)], None, None, 100)
        .unwrap();
    assert!(only.iter().any(|e| &e.id == pick));
    assert!(only.iter().all(|e| message_position(&e.id) == position));

    // ID range and limit.
    let sorted: Vec<String> = expected.iter().map(|(id, _)| id.clone()).collect();
    let middle = h
        .messages
        .block_entries(&all, Some(&sorted[1]), Some(&sorted[3]), 100)
        .unwrap();
    assert_eq!(middle.len(), 3);
    assert_eq!(
        h.messages.block_entries(&all, None, None, 2).unwrap().len(),
        2
    );
}

/// UC: UC-SIN-005, UC-SIN-009
#[tokio::test]
async fn block_messages_carry_the_body_and_stop_at_the_item_and_byte_ceilings() {
    let (h, _) = clustered(&[("TABA_MEMORY_LIMIT", "40B")]);
    h.topics.create(queue("orders")).await.unwrap();
    let mut ids = Vec::new();
    for n in 0..6 {
        let (meta, body) = record(&h, "orders", &format!("{{\"n\":{n},\"x\":\"abcdefghij\"}}"));
        ids.push(meta.id.clone());
        h.messages.apply_publish("orders", meta, body).unwrap();
    }
    let all = [(0, u64::MAX)];
    let (found, more) = h
        .messages
        .block_messages(&all, None, None, 4, u64::MAX)
        .unwrap();
    assert_eq!(found.len(), 4);
    assert!(more);
    assert!(
        found
            .iter()
            .all(|m| !m.body.is_empty() && m.topic == "orders")
    );
    // By the byte ceiling: the first always goes in, and the rest waits for the next stretch.
    let (found, more) = h.messages.block_messages(&all, None, None, 10, 1).unwrap();
    assert_eq!(found.len(), 1);
    assert!(more);
    // The continuation by the last ID read returns the same row again (the resumption ignores it).
    let last = found[0].meta.id.clone();
    let (next, _) = h
        .messages
        .block_messages(&all, Some(&last), None, 10, u64::MAX)
        .unwrap();
    assert_eq!(next[0].meta.id, last);
    let first_index = ids.iter().position(|i| *i == last).unwrap();
    assert_eq!(next.len(), 6 - first_index);
    // A loose message.
    let one = h.messages.block_message(&all, None, &ids[2]).unwrap();
    assert_eq!(one.meta.id, ids[2]);
    assert!(h.messages.block_message(&all, None, "inexistente").is_err());
    let elsewhere = [(0, 0)];
    assert!(h.messages.block_message(&elsewhere, None, &ids[2]).is_err());
}

/// UC: UC-SIN-008, UC-SIN-007, UC-SIN-014
#[tokio::test]
async fn loading_skips_what_expired_and_what_the_node_already_has_and_notes_unknown_topics() {
    let (h, _) = clustered(&[]);
    h.topics.create(queue("orders")).await.unwrap();
    let (meta, body) = record(&h, "orders", "{\"a\":1}");
    let id = meta.id.clone();

    assert_eq!(
        h.messages
            .apply_load("orders", meta.clone(), body.clone())
            .unwrap(),
        Loaded::Stored
    );
    let info = h.messages.message_info("orders", &id).unwrap().unwrap();
    assert!(
        !info.owned,
        "the load stores copies: the node in synchronization is not an owner"
    );
    assert_eq!(info.meta.position, message_position(&id));
    assert_eq!(
        h.messages
            .apply_load("orders", meta.clone(), body.clone())
            .unwrap(),
        Loaded::Existing,
        "repeating the load changes nothing"
    );
    assert_eq!(
        h.messages
            .apply_load("no-topic", meta.clone(), body.clone())
            .unwrap(),
        Loaded::UnknownTopic
    );

    // Past the topic's `ttl`: not worth copying.
    let (old, old_body) = record(&h, "orders", "{\"b\":2}");
    h.clock.advance(Duration::from_secs(30 * 24 * 3600));
    assert_eq!(
        h.messages.apply_load("orders", old, old_body).unwrap(),
        Loaded::Expired
    );
}

/// UC: UC-ARM-022, UC-SIN-021
#[tokio::test]
async fn copies_of_blocks_the_node_no_longer_serves_are_forgotten_and_the_rest_stay() {
    let (h, placement) = clustered(&[("TABA_MEMORY_LIMIT", "40B")]);
    h.topics.create(queue("orders")).await.unwrap();
    let mut ids = Vec::new();
    for n in 0..8 {
        let (meta, body) = record(&h, "orders", &format!("{{\"n\":{n}}}"));
        ids.push(meta.id.clone());
        h.messages.apply_publish("orders", meta, body).unwrap();
    }
    // Everything is the node's responsibility: nothing is deleted.
    assert_eq!(h.messages.forget_foreign(&|_| true), 0);
    let present = |h: &Harness| {
        ids.iter()
            .filter(|id| h.messages.message_info("orders", id).unwrap().is_some())
            .count()
    };
    assert_eq!(present(&h), 8);
    assert!(h.messages.pending() > 0 && h.messages.storage_stats().disk_messages > 0);

    // The node stops being responsible for half the positions (memory and disk).
    let keep: HashSet<u64> = ids
        .iter()
        .step_by(2)
        .map(|id| message_position(id))
        .collect();
    *placement.0.lock().unwrap() = Some(keep.clone());
    let removed = h
        .messages
        .forget_foreign(&|position| keep.contains(&position));
    assert_eq!(removed, 4);
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(
            h.messages.message_info("orders", id).unwrap().is_some(),
            n % 2 == 0,
            "message {n}"
        );
    }
    assert!(h.messages.responsible_for(message_position(&ids[0])));
    assert!(!h.messages.responsible_for(message_position(&ids[1])));
    assert_eq!(present(&h), 4);
}
