use super::*;
use crate::config::DbSynchronous;

fn store() -> (tempfile::TempDir, LossStore) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(NodeDb::open(dir.path(), DbSynchronous::Normal).unwrap());
    (dir, LossStore::new(db))
}

fn loss(topic: &str, id: &str, at: u64) -> Loss {
    Loss {
        topic: topic.into(),
        id: id.into(),
        block: "00000000000000ab".into(),
        detected_ms: at,
    }
}

/// UC: UC-RTN-008
#[test]
fn a_loss_is_keyed_by_topic_and_id_and_listed_by_id() {
    let (_dir, store) = store();
    assert!(store.record(&loss("orders", "b", 10)).unwrap());
    assert!(
        !store.record(&loss("orders", "b", 99)).unwrap(),
        "repeating the same key changes nothing"
    );
    assert!(store.record(&loss("orders", "a", 20)).unwrap());
    assert!(store.record(&loss("another", "b", 30)).unwrap());
    let all = store.list(None, 10).unwrap();
    assert_eq!(
        all.iter()
            .map(|l| (l.topic.as_str(), l.id.as_str()))
            .collect::<Vec<_>>(),
        vec![("orders", "a"), ("another", "b"), ("orders", "b")]
    );
    assert_eq!(all[2].detected_ms, 10, "the first record counts");
    assert_eq!(store.list(Some("b"), 10).unwrap().len(), 2);
    assert_eq!(store.list(None, 1).unwrap().len(), 1);
}

/// UC: UC-RTN-008, UC-RTN-005
#[test]
fn a_loss_is_removed_by_the_recovery_and_expires_by_age() {
    let (_dir, store) = store();
    store.record(&loss("t", "a", 100)).unwrap();
    store.record(&loss("t", "b", 500)).unwrap();
    assert!(store.remove("t", "a").unwrap());
    assert!(!store.remove("t", "a").unwrap());
    assert_eq!(store.count().unwrap(), 1);
    assert_eq!(store.expire(400).unwrap(), 0);
    assert_eq!(store.expire(501).unwrap(), 1);
    assert_eq!(store.count().unwrap(), 0);
}
