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
    Config::parse(None, &MapEnv::new([("TABA_REPLICATION_FACTOR", "1")])).unwrap()
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

/// UC: UC-STR-002
#[test]
fn the_definition_streams_list_by_ascending_id_and_not_by_name() {
    // The name that comes last in the alphabet is the oldest `id`.
    let oldest = topic("zebra", NOW_MS);
    let newest = topic("alpha", NOW_MS + 5_000);
    let token_old = token(&id::new_at(NOW_MS + 1_000));
    let token_new = token(&id::new_at(NOW_MS + 9_000));
    let (topics, tokens) = registries(
        vec![newest.clone(), oldest.clone()],
        vec![token_new.clone(), token_old.clone()],
    );

    let (listed, more) = topics.definitions_from(None, 10);
    assert_eq!(
        listed.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
        ["zebra", "alpha"]
    );
    assert!(!more);
    let (listed, more) = tokens.records_from(None, 10);
    assert_eq!(
        listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        [token_old.id.as_str(), token_new.id.as_str()]
    );
    assert!(!more);
}

/// UC: UC-STR-003
#[test]
fn from_is_inclusive_and_limit_lowers_the_ceiling_of_the_definition_streams() {
    let first = topic("a", NOW_MS);
    let second = topic("b", NOW_MS + 1_000);
    let third = topic("c", NOW_MS + 2_000);
    let (topics, _) = registries(vec![third.clone(), first.clone(), second.clone()], vec![]);

    let names = |from: Option<&str>, limit: usize| -> (Vec<String>, bool) {
        let (found, more) = topics.definitions_from(from, limit);
        (found.into_iter().map(|d| d.name).collect(), more)
    };
    assert_eq!(
        names(Some(second.id.as_str()), 10),
        (vec!["b".to_owned(), "c".to_owned()], false)
    );
    assert_eq!(
        names(Some(third.id.as_str()), 10),
        (vec!["c".to_owned()], false)
    );
    assert_eq!(names(None, 2), (vec!["a".to_owned(), "b".to_owned()], true));

    // `limit` of the query lowers the ceiling of 500 lines, never raises it; never below 2.
    let page = |limit: Option<u32>| {
        Definitions::page(&StreamQuery {
            limit,
            ..StreamQuery::default()
        })
    };
    assert_eq!(page(None), STREAM_PAGE);
    assert_eq!(page(Some(10)), 10);
    assert_eq!(page(Some(1)), 2);
    assert_eq!(page(Some(100_000)), STREAM_PAGE);
}

/// UC: UC-STR-011
#[test]
fn the_topic_and_token_lines_are_the_record_plus_the_version_this_node_applied() {
    let orders = topic("orders", NOW_MS);
    let line = with_version(orders.to_json(), 7);
    let mut expected = orders.to_json();
    expected["version"] = json!(7);
    assert_eq!(line, expected, "the TopicDefinition and one more field");
    assert_eq!(
        TopicDefinition::from_json(&line).unwrap(),
        orders,
        "the version does not disturb the definition"
    );
    assert_eq!(version_of_item(&line), 7);

    let record = token(&id::new_at(NOW_MS));
    let line = with_version(token_to_json(&record), 3);
    let mut expected = token_to_json(&record);
    expected["version"] = json!(3);
    assert_eq!(line, expected, "the TokenRecord and one more field");
    assert_eq!(token_from_json(&line).unwrap(), record);
    assert!(line.get("secret").is_none(), "never the secret");
    assert_eq!(version_of_item(&line), 3);
}
