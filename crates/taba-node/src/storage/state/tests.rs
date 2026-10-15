use std::sync::Arc;

use super::*;
use crate::config::DbSynchronous;

fn store() -> (tempfile::TempDir, NodeStateStore) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(NodeDb::open(dir.path(), DbSynchronous::Normal).unwrap());
    (dir, NodeStateStore::new(db))
}

/// UC: UC-DRN-022
#[test]
fn the_save_mark_is_written_and_consumed_once() {
    let (_dir, store) = store();
    assert_eq!(store.take_save_mark().unwrap(), None);

    store.mark_save_completed(1234).unwrap();
    store.mark_save_completed(5678).unwrap();
    assert_eq!(store.take_save_mark().unwrap(), Some(5678));
    assert_eq!(
        store.take_save_mark().unwrap(),
        None,
        "reading deletes the mark"
    );
}

/// UC: UC-DRN-023
#[test]
fn clearing_the_save_mark_removes_it() {
    let (_dir, store) = store();
    store.mark_save_completed(1).unwrap();
    store.clear_save_mark().unwrap();
    assert_eq!(store.take_save_mark().unwrap(), None);
}

/// UC: UC-DRN-022
#[test]
fn the_mark_survives_reopening_the_database() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = Arc::new(NodeDb::open(dir.path(), DbSynchronous::Normal).unwrap());
        NodeStateStore::new(db).mark_save_completed(42).unwrap();
    }
    let db = Arc::new(NodeDb::open(dir.path(), DbSynchronous::Normal).unwrap());
    assert_eq!(NodeStateStore::new(db).take_save_mark().unwrap(), Some(42));
}
