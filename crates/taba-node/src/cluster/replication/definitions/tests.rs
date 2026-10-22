use std::collections::BTreeMap;

use super::*;
use crate::config::{Config, MapEnv};
use crate::core::clock::ManualClock;
use crate::core::id;
use crate::core::token::{Permission, generate};
use crate::core::topic::{QueueSpec, Spec};
use crate::engine::tokens::TokenRules;
use crate::engine::topics::TopicRules;
use crate::storage::tokens::MemoryTokenStore;
use crate::storage::topics::MemoryTopicStore;

const NOW_MS: u64 = 1_791_288_000_000;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(NOW_MS)
}

fn config() -> Config {
    Config::parse(None, &MapEnv::new([("TABA_MODE", "development")])).unwrap()
}

fn topic(name: &str, id_ms: u64) -> TopicDefinition {
    TopicDefinition {
        id: id::new_at(id_ms),
        name: name.into(),
        created_ms: id_ms,
        tags: BTreeMap::new(),
        spec: Spec::Queue(QueueSpec {
            volatile: false,
            idle_timeout: None,
            fifo: false,
            max_message_bytes: None,
            ttl: None,
            max_deliveries: None,
        }),
    }
}

fn registries(topics: Vec<TopicDefinition>, tokens: Vec<TokenRecord>) -> (Topics, Tokens) {
    let clock = Arc::new(ManualClock::at_doc_epoch());
    (
        Topics::new(
            TopicRules::from_config(&config()),
            Arc::new(MemoryTopicStore::default()),
            clock.clone(),
            topics,
        ),
        Tokens::new(
            TokenRules::from_config(&config()),
            Arc::new(MemoryTokenStore::default()),
            clock,
            tokens,
        ),
    )
}

fn row(kind: &str, key: &str, version: u64, deleted: bool, topic_id: Option<&str>) -> VersionRow {
    VersionRow {
        kind: kind.into(),
        key: key.into(),
        version,
        deleted,
        fingerprint: [version as u8; 32],
        topic_id: topic_id.map(str::to_owned),
        at_ms: NOW_MS - 1_000,
    }
}

fn token(id: &str) -> TokenRecord {
    generate(
        id.into(),
        "svc".into(),
        vec![Permission::Publish],
        vec![],
        NOW_MS,
        None,
    )
    .record
}

/// UC: UC-DEF-039
#[test]
fn the_stored_versions_of_the_definitions_that_were_loaded_come_back_with_their_topic_id() {
    let orders = topic("orders", NOW_MS);
    let (topics, tokens) = registries(vec![orders.clone()], vec![token("t1")]);
    let stored = vec![
        row("topic", "orders", 4, false, Some(&orders.id)),
        row("token", "t1", 2, false, None),
    ];

    let (kept, dropped) = reconcile(stored, &topics, &tokens, now());

    assert!(dropped.is_empty());
    let entry = &kept[&(DefinitionKind::Topic, "orders".to_owned())];
    assert_eq!(entry.version, 4);
    assert_eq!(entry.topic_id.as_deref(), Some(orders.id.as_str()));
    assert_eq!(kept[&(DefinitionKind::Token, "t1".to_owned())].version, 2);
}

/// A tombstone is kept with its version: a late creation of a lower version does not bring the topic back.
/// UC: UC-DEF-039
#[test]
fn a_tombstone_is_kept_until_it_expires() {
    let (topics, tokens) = registries(vec![], vec![]);
    let fresh = row("topic", "gone", 5, true, Some("old-id"));
    let mut old = row("topic", "older", 9, true, Some("x"));
    old.at_ms = NOW_MS - TOMBSTONE_TTL.as_millis() as u64 - 1;

    let (kept, dropped) = reconcile(vec![fresh, old], &topics, &tokens, now());

    assert_eq!(kept.len(), 1);
    let entry = &kept[&(DefinitionKind::Topic, "gone".to_owned())];
    assert!(entry.deleted);
    assert_eq!(entry.version, 5);
    assert_eq!(dropped, [("topic".to_owned(), "older".to_owned())]);
}

/// The tombstone is of the topic with that `id`: it neither erases nor describes a topic recreated with
/// the same name and another `id`, and a live row of the old topic does not describe the new one either.
/// UC: UC-DEF-040
#[test]
fn a_row_of_another_life_of_the_topic_does_not_describe_the_recreated_one() {
    let recreated = topic("orders", NOW_MS);
    let (topics, tokens) = registries(vec![recreated.clone()], vec![]);
    let stale_tombstone = row("topic", "orders", 2, true, Some("the-first-topic-id"));

    let (kept, dropped) = reconcile(vec![stale_tombstone], &topics, &tokens, now());
    assert!(
        kept.is_empty(),
        "the tombstone is not the recreated topic's"
    );
    assert_eq!(dropped, [("topic".to_owned(), "orders".to_owned())]);
    assert!(topics.definition("orders").is_some(), "the topic stays");

    let stale_live = row("topic", "orders", 1, false, Some("the-first-topic-id"));
    let (kept, dropped) = reconcile(vec![stale_live], &topics, &tokens, now());
    assert!(kept.is_empty());
    assert_eq!(dropped.len(), 1);
}

/// A row whose definition is not on the node (it was not loaded) is dropped, and so is one of an unknown kind.
/// UC: UC-DEF-039
#[test]
fn a_live_row_without_its_definition_is_dropped() {
    let (topics, tokens) = registries(vec![], vec![]);
    let stored = vec![
        row("topic", "missing", 3, false, Some("id")),
        row("token", "missing", 3, false, None),
        row("widget", "w", 1, false, None),
    ];

    let (kept, dropped) = reconcile(stored, &topics, &tokens, now());

    assert!(kept.is_empty());
    assert_eq!(dropped.len(), 3);
}
