use std::collections::BTreeMap;

use super::*;
use crate::config::DbSynchronous;
use crate::core::id;
use crate::core::topic::{QueueSpec, Spec};

fn definition(name: &str) -> TopicDefinition {
    TopicDefinition {
        id: id::new_at(1_791_288_000_000),
        name: name.into(),
        created_ms: 1_791_288_000_000,
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

fn store(dir: &std::path::Path) -> (SqliteTopicStore, Arc<NodeDb>) {
    let db = Arc::new(NodeDb::open(dir, DbSynchronous::Normal).unwrap());
    (SqliteTopicStore::new(db.clone()), db)
}

#[test]
fn saves_one_row_per_topic_and_loads_them_back_after_reopening() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, db) = store(tmp.path());
    assert!(store.load_all().unwrap().is_empty());

    let (a, b) = (definition("orders"), definition("Orders"));
    store.save(&a).unwrap();
    store.save(&b).unwrap();
    drop((store, db));

    let (store, _db) = store_at(tmp.path());
    let mut loaded = store.load_all().unwrap();
    loaded.sort_by(|x, y| x.name.cmp(&y.name));
    assert_eq!(loaded, vec![b, a]);
}

fn store_at(dir: &std::path::Path) -> (SqliteTopicStore, Arc<NodeDb>) {
    store(dir)
}

/// UC: UC-DEF-009
#[test]
fn saving_again_replaces_the_row() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, _db) = store(tmp.path());
    let mut topic = definition("orders");
    store.save(&topic).unwrap();
    topic.tags.insert("a".into(), "b".into());
    store.save(&topic).unwrap();

    assert_eq!(store.load_all().unwrap(), vec![topic]);
}

#[test]
fn delete_removes_the_row_and_tolerates_a_missing_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, _db) = store(tmp.path());
    store.save(&definition("orders")).unwrap();
    store.delete("orders").unwrap();
    store.delete("orders").unwrap();
    assert!(store.load_all().unwrap().is_empty());
}

#[test]
fn a_corrupt_row_is_an_error_naming_the_topic() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, db) = store(tmp.path());
    store.save(&definition("good")).unwrap();
    db.conn()
        .execute("INSERT INTO topics VALUES ('bad', '{ not json')", [])
        .unwrap();

    let err = store.load_all().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("`bad`"), "{err}");
}

/// UC: UC-DEF-009, UC-DEF-024
#[test]
fn definitions_are_not_files_and_names_differing_in_case_are_different_topics() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, _db) = store(tmp.path());
    store.save(&definition("orders")).unwrap();
    store.save(&definition("Orders")).unwrap();
    store.delete("Orders").unwrap();
    let left = store.load_all().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "orders");

    assert!(
        !tmp.path().join("_topics").exists(),
        "there is no file per topic"
    );
}

#[test]
fn memory_store_can_be_told_to_fail() {
    let store = MemoryTopicStore::default();
    store.save(&definition("a")).unwrap();
    store.set_failing(true);
    assert!(store.save(&definition("b")).is_err());
    assert!(store.delete("a").is_err());
    assert_eq!(store.names(), ["a"]);
    store.set_failing(false);
    store.delete("a").unwrap();
    assert!(store.names().is_empty());
}
