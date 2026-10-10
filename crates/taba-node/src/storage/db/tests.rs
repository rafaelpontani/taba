use super::*;

/// UC: UC-ARM-017
#[test]
fn opens_in_wal_mode_with_the_configured_synchronous() {
    let tmp = tempfile::tempdir().unwrap();
    let db = NodeDb::open(tmp.path(), DbSynchronous::Normal).unwrap();
    assert_eq!(db.pragma("journal_mode").unwrap(), "wal");
    assert_eq!(db.pragma("synchronous").unwrap(), "1");
    drop(db);

    let db = NodeDb::open(tmp.path(), DbSynchronous::Full).unwrap();
    assert_eq!(db.pragma("synchronous").unwrap(), "2");
}

#[test]
fn creates_the_directory_and_the_file_and_reopens_keeping_the_data() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("a").join("taba");
    let db = NodeDb::open(&dir, DbSynchronous::Normal).unwrap();
    db.conn()
        .execute("INSERT INTO topics VALUES ('x', '{}')", [])
        .unwrap();
    drop(db);
    assert!(dir.join(NODE_DB_FILE).is_file());

    let db = NodeDb::open(&dir, DbSynchronous::Normal).unwrap();
    let n: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM topics", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn a_database_from_a_newer_schema_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let db = NodeDb::open(tmp.path(), DbSynchronous::Normal).unwrap();
    db.conn().pragma_update(None, "user_version", 99).unwrap();
    drop(db);

    let err = NodeDb::open(tmp.path(), DbSynchronous::Normal).unwrap_err();
    assert!(err.to_string().contains("versão 99"), "{err}");
}

#[test]
fn a_file_that_is_not_a_database_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(NODE_DB_FILE), "isto não é um banco sqlite").unwrap();
    assert!(NodeDb::open(tmp.path(), DbSynchronous::Normal).is_err());
}

/// Enche o banco até a última página que o SQLite deixa usar, para o próximo `INSERT` falhar
/// com `SQLITE_FULL`.
fn fill_up(db: &NodeDb) {
    let pages: i64 = db
        .conn()
        .pragma_query_value(None, "page_count", |row| row.get(0))
        .unwrap();
    db.conn()
        .pragma_update(None, "max_page_count", pages)
        .unwrap();
}

/// UC: UC-FAL-028
#[test]
fn a_full_disk_in_the_node_database_fails_the_node_from_any_of_its_tables() {
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::core::topic::{QueueSpec, Spec, TopicDefinition};
    use crate::storage::losses::{Loss, LossStore};
    use crate::storage::topics::{SqliteTopicStore, TopicStore};

    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(NodeDb::open(tmp.path(), DbSynchronous::Normal).unwrap());
    let failures = Arc::new(AtomicU32::new(0));
    {
        let failures = failures.clone();
        db.on_failure(Arc::new(move || {
            failures.fetch_add(1, Ordering::SeqCst);
        }));
    }
    let losses = LossStore::new(db.clone());
    let topics = SqliteTopicStore::new(db.clone());

    // Com espaço, nada falha.
    let small = Loss {
        topic: "t".into(),
        id: "0".into(),
        block: "0".repeat(16),
        detected_ms: 1,
    };
    assert!(losses.record(&small).unwrap());
    assert_eq!(failures.load(Ordering::SeqCst), 0);

    fill_up(&db);
    let big = Loss {
        topic: "t".repeat(40_000),
        ..small.clone()
    };
    assert!(losses.record(&big).is_err());
    assert_eq!(failures.load(Ordering::SeqCst), 1, "perdas");

    let definition = TopicDefinition {
        id: crate::core::id::new_at(1_791_288_000_000),
        name: "n".repeat(40_000),
        created_ms: 1_791_288_000_000,
        tags: std::collections::BTreeMap::new(),
        spec: Spec::Queue(QueueSpec {
            volatile: false,
            idle_timeout: None,
            fifo: false,
            max_message_bytes: None,
            ttl: None,
            max_deliveries: None,
        }),
    };
    assert!(topics.save(&definition).is_err());
    assert_eq!(failures.load(Ordering::SeqCst), 2, "definições");
}

/// UC: UC-FAL-028
#[test]
fn an_error_that_is_not_fatal_in_the_node_database_does_not_fail_the_node() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let tmp = tempfile::tempdir().unwrap();
    let db = NodeDb::open(tmp.path(), DbSynchronous::Normal).unwrap();
    let failures = Arc::new(AtomicU32::new(0));
    {
        let failures = failures.clone();
        db.on_failure(Arc::new(move || {
            failures.fetch_add(1, Ordering::SeqCst);
        }));
    }
    let busy =
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), None);
    let error = db.fail(busy);
    assert!(!super::super::is_fatal(&error));
    assert_eq!(failures.load(Ordering::SeqCst), 0);
}
