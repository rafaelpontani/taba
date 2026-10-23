use std::sync::Arc;

use super::*;
use crate::config::DbSynchronous;

fn store() -> (tempfile::TempDir, NodeStateStore) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(NodeDb::open(dir.path(), DbSynchronous::Normal).unwrap());
    (dir, NodeStateStore::new(db))
}

/// UC: UC-ARM-026
#[test]
fn the_start_of_the_excluded_recording_is_kept_and_survives_reopening_the_database() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = Arc::new(NodeDb::open(dir.path(), DbSynchronous::Normal).unwrap());
        let store = NodeStateStore::new(db);
        assert_eq!(store.excluded_since().unwrap(), None);
        store.set_excluded_since(42).unwrap();
        store.set_excluded_since(77).unwrap();
    }
    let db = Arc::new(NodeDb::open(dir.path(), DbSynchronous::Normal).unwrap());
    assert_eq!(NodeStateStore::new(db).excluded_since().unwrap(), Some(77));
}

/// UC: UC-ARM-026
#[test]
fn the_inheritance_of_the_recording_is_marked_and_cleared() {
    let (_dir, store) = store();
    assert!(!store.excluded_inherited().unwrap());
    store.set_excluded_inherited(true).unwrap();
    assert!(store.excluded_inherited().unwrap());
    store.set_excluded_inherited(false).unwrap();
    assert!(!store.excluded_inherited().unwrap());
}
