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

/// UC: UC-RTN-045
#[tokio::test]
async fn of_two_copies_the_greater_delivery_count_prevails_and_never_decreases() {
    let (h, _) = clustered(&[]);
    h.topics.create(queue("orders")).await.unwrap();
    let (mut meta, body) = record(&h, "orders", "{\"a\":1}");
    let id = meta.id.clone();
    meta.delivery_count = 1;
    h.messages.apply_load("orders", meta.clone(), body).unwrap();

    assert!(h.messages.raise_delivery_count("orders", &id, 4).unwrap());
    let info = h.messages.message_info("orders", &id).unwrap().unwrap();
    assert_eq!(info.meta.delivery_count, 4);
    assert!(
        !h.messages.raise_delivery_count("orders", &id, 2).unwrap(),
        "a smaller counter changes nothing"
    );
    assert!(!h.messages.raise_delivery_count("orders", &id, 4).unwrap());
    let info = h.messages.message_info("orders", &id).unwrap().unwrap();
    assert_eq!(info.meta.delivery_count, 4);
    assert_eq!(
        info.meta.invisible_until_ms, 0,
        "the invisibility is untouched"
    );
    assert!(
        !h.messages
            .raise_delivery_count("orders", "inexistente", 9)
            .unwrap(),
        "a message the node does not keep is not created"
    );
}

/// Applies `count` messages of one topic one second apart, in the **reverse** order of their creation,
/// and returns the IDs in the order they were created.
fn applied_backwards(h: &Harness, topic: &str, count: usize) -> Vec<String> {
    let mut made = Vec::new();
    for n in 0..count {
        h.clock.advance(Duration::from_secs(1));
        let (meta, body) = record(h, topic, &format!("{{\"n\":{n}}}"));
        made.push((meta, body));
    }
    let ids: Vec<String> = made.iter().map(|(meta, _)| meta.id.clone()).collect();
    for (meta, body) in made.into_iter().rev() {
        h.messages.apply_publish(topic, meta, body).unwrap();
    }
    ids
}

/// UC: UC-STR-002
#[tokio::test]
async fn the_block_streams_come_in_ascending_id_order_whatever_the_order_of_arrival() {
    let (h, _) = clustered(&[("TABA_MEMORY_LIMIT", "20B")]);
    h.topics.create(queue("orders")).await.unwrap();
    let ids = applied_backwards(&h, "orders", 6);
    assert!(h.messages.storage_stats().disk_messages > 0);
    assert!(h.messages.storage_stats().memory_messages > 0);

    // The GUID v7 as text, in the canonical lowercase form, is the order of the publication.
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "text order is the order of creation");
    assert!(ids.iter().all(|id| *id == id.to_ascii_lowercase()));

    let all = [(0, u64::MAX)];
    let listed: Vec<String> = h
        .messages
        .block_entries(&all, None, None, 100)
        .unwrap()
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(listed, sorted, "memory and disk, merged in ID order");

    let (found, more) = h
        .messages
        .block_messages(&all, None, None, 100, u64::MAX)
        .unwrap();
    assert!(!more);
    assert_eq!(
        found.into_iter().map(|m| m.meta.id).collect::<Vec<_>>(),
        sorted
    );
}

/// UC: UC-STR-003
#[tokio::test]
async fn from_and_to_are_inclusive_and_limit_lowers_the_ceiling_of_the_block_streams() {
    let (h, _) = clustered(&[("TABA_MEMORY_LIMIT", "20B")]);
    h.topics.create(queue("orders")).await.unwrap();
    let ids = applied_backwards(&h, "orders", 6);
    let all = [(0, u64::MAX)];
    let entries = |from: Option<&String>, to: Option<&String>, limit: usize| -> Vec<String> {
        h.messages
            .block_entries(
                &all,
                from.map(String::as_str),
                to.map(String::as_str),
                limit,
            )
            .unwrap()
            .into_iter()
            .map(|entry| entry.id)
            .collect()
    };

    assert_eq!(entries(Some(&ids[1]), Some(&ids[3]), 100), ids[1..=3]);
    assert_eq!(entries(Some(&ids[4]), None, 100), ids[4..]);
    assert_eq!(entries(None, Some(&ids[1]), 100), ids[..=1]);
    assert_eq!(entries(Some(&ids[2]), Some(&ids[2]), 100), ids[2..=2]);
    assert_eq!(entries(None, None, 100), ids);
    assert_eq!(entries(None, None, 2), ids[..2], "the limit cuts the tail");

    let (found, more) = h
        .messages
        .block_messages(&all, Some(&ids[2]), Some(&ids[4]), 100, u64::MAX)
        .unwrap();
    assert_eq!(
        found.into_iter().map(|m| m.meta.id).collect::<Vec<_>>(),
        ids[2..=4]
    );
    assert!(!more);
    let (found, more) = h
        .messages
        .block_messages(&all, Some(&ids[2]), Some(&ids[4]), 2, u64::MAX)
        .unwrap();
    assert_eq!(found.len(), 2);
    assert!(more, "the limit left one line for the next stretch");
}

/// UC: UC-STR-006
#[tokio::test]
async fn resuming_from_the_last_id_repeats_that_line_and_the_receiver_ignores_it() {
    let (source, _) = clustered(&[]);
    source.topics.create(queue("orders")).await.unwrap();
    let ids = applied_backwards(&source, "orders", 5);
    let (receiver, _) = clustered(&[]);
    receiver.topics.create(queue("orders")).await.unwrap();
    let all = [(0, u64::MAX)];

    // First stretch: cut at three lines.
    let (first, more) = source
        .messages
        .block_messages(&all, None, None, 3, u64::MAX)
        .unwrap();
    assert!(more);
    for line in first {
        let loaded = receiver
            .messages
            .apply_load(&line.topic, line.meta, line.body)
            .unwrap();
        assert_eq!(loaded, Loaded::Stored);
    }

    // The receiver repeats from the last ID it read: the sender gives that line again...
    let (second, more) = source
        .messages
        .block_messages(&all, Some(&ids[2]), None, 10, u64::MAX)
        .unwrap();
    assert!(!more);
    assert_eq!(
        second.iter().map(|m| m.meta.id.clone()).collect::<Vec<_>>(),
        ids[2..]
    );
    // ... and the receiver ignores the repeated one and stores only the new ones.
    let outcomes: Vec<Loaded> = second
        .into_iter()
        .map(|line| {
            receiver
                .messages
                .apply_load(&line.topic, line.meta, line.body)
                .unwrap()
        })
        .collect();
    assert_eq!(outcomes, [Loaded::Existing, Loaded::Stored, Loaded::Stored]);
    for id in &ids {
        assert!(
            receiver
                .messages
                .message_info("orders", id)
                .unwrap()
                .is_some()
        );
    }
    let (all_copies, _) = receiver
        .messages
        .block_messages(&all, None, None, 100, u64::MAX)
        .unwrap();
    assert_eq!(all_copies.len(), ids.len(), "no duplicate line was created");
}

/// UC: UC-OPS-023
#[tokio::test]
async fn only_the_owner_counts_a_message_in_the_visible_invisible_and_oldest_numbers() {
    let config = Config::parse(None, &MapEnv::new([("TABA_REPLICATION_FACTOR", "1")])).unwrap();
    let rules = StatsRules::from_config(&config);

    // A node that only keeps a copy (it is not the delivery owner) does not count it.
    let (replica, _) = clustered(&[]);
    replica.topics.create(queue("orders")).await.unwrap();
    let (meta, body) = record(&replica, "orders", "{\"a\":1}");
    let id = meta.id.clone();
    replica.messages.apply_load("orders", meta, body).unwrap();
    assert!(
        !replica
            .messages
            .message_info("orders", &id)
            .unwrap()
            .unwrap()
            .owned
    );
    let copy = replica.messages.topic_snapshot("orders", &rules).unwrap();
    assert_eq!((copy.visible, copy.invisible, copy.oldest_ms), (0, 0, None));
    assert_eq!(copy.depth(), 0);

    // The owner counts it, once.
    let owner = harness();
    owner.topics.create(queue("orders")).await.unwrap();
    publish_ok(&owner, "orders", "1");
    let counted = owner.messages.topic_snapshot("orders", &rules).unwrap();
    assert_eq!((counted.visible, counted.invisible), (1, 0));
    assert_eq!(counted.depth(), 1);
    assert!(counted.oldest_ms.is_some());
}
