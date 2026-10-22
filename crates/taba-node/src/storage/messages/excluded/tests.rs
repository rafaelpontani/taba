use std::path::Path;

use super::*;
use crate::config::DbSynchronous;
use crate::storage::messages::TOPIC_DB_FILE;

fn meta(id: &str, position: u64) -> MessageMeta {
    MessageMeta {
        id: id.into(),
        content_type: "application/json".into(),
        group_id: None,
        deduplication_id: None,
        published_ms: 1_000,
        delivery_count: 0,
        invisible_until_ms: 0,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
        position,
    }
}

fn store(dir: &Path, limits: ExcludedLimits) -> MessageStore {
    MessageStore::new(dir, DbSynchronous::Normal).with_excluded(limits, None)
}

fn ids(rows: Vec<ExcludedRow>) -> Vec<String> {
    rows.into_iter().map(|r| r.id).collect()
}

/// The ack: the message leaves the topic and the log gets its row, with the ring position.
/// UC: UC-ESC-037, UC-ARM-029
#[test]
fn removing_with_exclusion_deletes_the_message_and_records_it_in_the_same_transaction() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path(), ExcludedLimits::default());
    store.put("orders", &meta("a1", 77), b"x").unwrap();

    store.remove_excluded("orders", "a1", 5_000).unwrap();

    assert_eq!(store.get("orders", "a1").unwrap(), None);
    assert!(!tmp.path().join("orders").join("a1.body").exists());
    assert_eq!(
        store.excluded_all("orders").unwrap(),
        vec![ExcludedRow {
            id: "a1".into(),
            position: 77,
            at_ms: 5_000
        }]
    );
    assert!(store.is_excluded("orders", "a1").unwrap());
    assert!(!store.is_excluded("orders", "other").unwrap());
}

/// The log lives in the database of the topic that had the message; the cost is one insert in the
/// transaction that already existed, and `storage.db.synchronous` stays as configured.
/// UC: UC-ARM-029
#[test]
fn recording_adds_no_synchronization_beyond_the_configured_one() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path(), ExcludedLimits::default());
    store.put("orders", &meta("a1", 1), b"x").unwrap();
    store.remove_excluded("orders", "a1", 1).unwrap();

    let conn = rusqlite::Connection::open(tmp.path().join("orders").join(TOPIC_DB_FILE)).unwrap();
    // A new connection reads the default of the file; the store sets NORMAL (1) on its own.
    let log: i64 = conn
        .query_row("SELECT COUNT(*) FROM excluded", [], |r| r.get(0))
        .unwrap();
    assert_eq!(log, 1);
    let disk = store.disk("orders", false).unwrap().unwrap();
    let synchronous: i64 = disk
        .conn()
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .unwrap();
    assert_eq!(synchronous, 1, "NORMAL: no fsync per transaction");
}

/// A message that was only in memory has no row: the exclusion is recorded by itself.
/// UC: UC-ESC-037
#[test]
fn a_message_that_was_only_in_memory_is_recorded_by_its_id_and_position() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path(), ExcludedLimits::default());
    store.remove_excluded("orders", "gone", 1).unwrap();
    assert!(
        !tmp.path().join("orders").exists(),
        "nothing to remove, nothing created"
    );

    store.record_excluded("orders", "m1", 9, 2_000).unwrap();
    store.record_excluded("orders", "m1", 9, 3_000).unwrap();
    let rows = store.excluded_all("orders").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at_ms, 2_000, "the first exclusion stays");
}

/// The dead-letter move: the id is dead in the source topic, alive in the dead-letter.
/// UC: UC-ESC-038
#[test]
fn moving_to_the_dead_letter_records_the_exclusion_in_the_source_only() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path(), ExcludedLimits::default());
    store.put("orders", &meta("a1", 5), b"x").unwrap();
    let mut moved = meta("a1", 5);
    moved.dead_lettered_ms = Some(9_000);

    store.move_to("orders", "orders-dlq", &moved).unwrap();

    assert!(store.is_excluded("orders", "a1").unwrap());
    assert_eq!(store.excluded_all("orders").unwrap()[0].at_ms, 9_000);
    assert!(!store.is_excluded("orders-dlq", "a1").unwrap());
    assert!(store.get("orders-dlq", "a1").unwrap().is_some());
}

/// The FIFO ack keeps the short row for deduplication and records the exclusion.
/// UC: UC-ESC-037
#[test]
fn the_fifo_ack_records_the_exclusion_together_with_the_short_row() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path(), ExcludedLimits::default());
    let mut m = meta("f1", 3);
    m.group_id = Some("g".into());
    store.put("fifo", &m, b"x").unwrap();

    store.tombstone_excluded("fifo", &m, 99_000, 4_000).unwrap();

    assert!(store.is_excluded("fifo", "f1").unwrap());
    assert!(!tmp.path().join("fifo").join("f1.body").exists());
    assert!(
        store.get("fifo", "f1").unwrap().is_some(),
        "the short row stays"
    );
}

/// The block is found by the position, in id order, between `from` and `to`.
/// UC: UC-ESC-037
#[test]
fn rows_are_listed_by_the_ranges_of_a_block_in_id_order() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path(), ExcludedLimits::default());
    for (id, position) in [("c", 10), ("a", 20), ("b", 500), ("d", 30)] {
        store.record_excluded("orders", id, position, 1).unwrap();
    }
    let block = [(0, 40)];
    assert_eq!(
        ids(store
            .excluded_rows("orders", &block, None, None, 10)
            .unwrap()),
        ["a", "c", "d"]
    );
    assert_eq!(
        ids(store
            .excluded_rows("orders", &block, Some("c"), Some("d"), 10)
            .unwrap()),
        ["c", "d"]
    );
    assert_eq!(
        ids(store
            .excluded_rows("orders", &block, None, None, 2)
            .unwrap()),
        ["a", "c"]
    );
    assert!(
        store
            .excluded_rows("orders", &[], None, None, 10)
            .unwrap()
            .is_empty()
    );
}

/// The age limit drops the old rows; the size limit drops the oldest until it fits; what was
/// discarded moves the start of the recording forward.
/// UC: UC-ARM-026, UC-ARM-027
#[test]
fn the_limits_drop_the_oldest_rows_first_and_move_the_start_forward() {
    let tmp = tempfile::tempdir().unwrap();
    let limits = ExcludedLimits {
        max_bytes: u64::MAX,
        max_age_ms: 10_000,
    };
    let store = store(tmp.path(), limits);
    assert_eq!(store.excluded_since(100).unwrap(), 100);
    store.record_excluded("orders", "old", 1, 1_000).unwrap();
    store.record_excluded("orders", "new", 1, 19_000).unwrap();

    // by age: at 20 s, only the last 10 s stay
    assert_eq!(store.trim_excluded(20_000).unwrap(), 1);
    assert_eq!(ids(store.excluded_all("orders").unwrap()), ["new"]);
    assert_eq!(
        store.excluded_since(0).unwrap(),
        1_000,
        "the newest discarded"
    );

    // by size: three rows of the same weight, a limit for two
    let tmp = tempfile::tempdir().unwrap();
    let weight = (2 + ROW_OVERHEAD) as u64;
    let store = self::store(
        tmp.path(),
        ExcludedLimits {
            max_bytes: weight * 2,
            max_age_ms: u64::MAX / 2,
        },
    );
    store.excluded_since(1).unwrap();
    store.record_excluded("orders", "m1", 1, 100).unwrap();
    store.record_excluded("orders", "m2", 1, 200).unwrap();
    store.record_excluded("orders", "m3", 1, 300).unwrap();
    assert_eq!(store.excluded_bytes().unwrap(), weight * 3);
    assert_eq!(store.trim_excluded(400).unwrap(), 1);
    assert_eq!(ids(store.excluded_all("orders").unwrap()), ["m2", "m3"]);
    assert_eq!(store.excluded_since(0).unwrap(), 100);
}

/// The size limit is over all the topics: the oldest rows go, from whichever topic.
/// UC: UC-ARM-026
#[test]
fn the_size_limit_counts_every_topic_and_drops_the_oldest_of_all() {
    let tmp = tempfile::tempdir().unwrap();
    let weight = (2 + ROW_OVERHEAD) as u64;
    let store = store(
        tmp.path(),
        ExcludedLimits {
            max_bytes: weight * 2,
            max_age_ms: u64::MAX / 2,
        },
    );
    store.record_excluded("a", "m1", 1, 100).unwrap();
    store.record_excluded("b", "m2", 1, 200).unwrap();
    store.record_excluded("a", "m3", 1, 300).unwrap();

    assert_eq!(store.trim_excluded(400).unwrap(), 1);
    assert!(
        store
            .excluded_all("a")
            .unwrap()
            .iter()
            .all(|r| r.id == "m3")
    );
    assert_eq!(ids(store.excluded_all("b").unwrap()), ["m2"]);
}

/// A cluster without exclusions for days still covers those days: nothing is discarded, the start stays.
/// UC: UC-ARM-027
#[test]
fn the_start_of_the_recording_does_not_move_without_a_discarded_row() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(
        tmp.path(),
        ExcludedLimits {
            max_bytes: u64::MAX,
            max_age_ms: 1_000,
        },
    );
    assert_eq!(store.excluded_since(5_000).unwrap(), 5_000);
    assert_eq!(store.trim_excluded(5_000_000).unwrap(), 0);
    assert_eq!(store.excluded_since(5_000_000).unwrap(), 5_000);
}

/// The start survives a restart when the node database keeps it.
/// UC: UC-ARM-027
#[test]
fn the_start_of_the_recording_is_kept_in_the_node_database() {
    use crate::storage::db::NodeDb;
    use std::sync::Arc;

    let tmp = tempfile::tempdir().unwrap();
    let node_db = Arc::new(NodeDb::open(tmp.path(), DbSynchronous::Normal).unwrap());
    let state = NodeStateStore::new(node_db.clone());
    let first = MessageStore::new(tmp.path(), DbSynchronous::Normal)
        .with_excluded(ExcludedLimits::default(), Some(state.clone()));
    assert_eq!(first.excluded_since(7_000).unwrap(), 7_000);
    first.advance_excluded_since(9_000).unwrap();
    drop(first);

    let second = MessageStore::new(tmp.path(), DbSynchronous::Normal)
        .with_excluded(ExcludedLimits::default(), Some(state));
    assert_eq!(second.excluded_since(50_000).unwrap(), 9_000);
    second.advance_excluded_since(8_000).unwrap();
    assert_eq!(
        second.excluded_since(0).unwrap(),
        9_000,
        "it never goes back by advancing"
    );
    second.set_excluded_since(4_000).unwrap();
    assert_eq!(second.excluded_since(0).unwrap(), 4_000);
}

/// The log of a past life adds evidence; the start goes back only if the intervals meet.
/// UC: UC-ARM-028
#[test]
fn a_past_life_adds_its_rows_and_moves_the_start_back_only_if_there_is_no_hole() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path(), ExcludedLimits::default());
    store.record_excluded("orders", "x", 1, 5_000).unwrap();
    store
        .import_excluded(
            "orders",
            &[
                ExcludedRow {
                    id: "x".into(),
                    position: 1,
                    at_ms: 3_000,
                },
                ExcludedRow {
                    id: "y".into(),
                    position: 2,
                    at_ms: 4_000,
                },
            ],
        )
        .unwrap();
    let rows = store.excluded_all("orders").unwrap();
    assert_eq!(ids(rows.clone()), ["x", "y"]);
    assert_eq!(rows[0].at_ms, 3_000, "the oldest instant of an id stays");

    // the past life recorded from 1_000 to 8_000; the current one started at 6_000
    assert_eq!(merged_since(6_000, 1_000, 8_000), 1_000);
    // they meet exactly
    assert_eq!(merged_since(6_000, 1_000, 6_000), 1_000);
    // the node was down for longer than the log of the sources remembers: a hole
    assert_eq!(merged_since(6_000, 1_000, 5_999), 6_000);
}

/// An old database without the table opens and gets it.
/// UC: UC-ARM-029
#[test]
fn a_topic_database_from_before_the_log_gets_the_table() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("orders")).unwrap();
    {
        let conn =
            rusqlite::Connection::open(tmp.path().join("orders").join(TOPIC_DB_FILE)).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (id TEXT PRIMARY KEY NOT NULL, content_type TEXT NOT NULL, \
             group_id TEXT, published_at INTEGER NOT NULL, delivery_count INTEGER NOT NULL DEFAULT 0, \
             invisible_until INTEGER NOT NULL DEFAULT 0, receipt_token TEXT, dead_lettered_at INTEGER, \
             dead_letter_reason TEXT, age_base INTEGER NOT NULL) WITHOUT ROWID;",
        )
        .unwrap();
    }
    let store = store(tmp.path(), ExcludedLimits::default());
    assert!(store.excluded_all("orders").unwrap().is_empty());
    store.record_excluded("orders", "m", 1, 1).unwrap();
    assert!(store.is_excluded("orders", "m").unwrap());
}
