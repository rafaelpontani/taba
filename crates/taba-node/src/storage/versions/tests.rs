use super::*;
use crate::config::DbSynchronous;

fn open(dir: &std::path::Path) -> (VersionStore, Arc<NodeDb>) {
    let db = Arc::new(NodeDb::open(dir, DbSynchronous::Normal).unwrap());
    (VersionStore::new(db.clone()), db)
}

fn row(kind: &str, key: &str, version: u64, deleted: bool) -> VersionRow {
    VersionRow {
        kind: kind.into(),
        key: key.into(),
        version,
        deleted,
        fingerprint: [version as u8; 32],
        topic_id: (kind == "topic").then(|| format!("id-of-{key}")),
        at_ms: 1_000 + version,
    }
}

/// UC: UC-DEF-039
#[test]
fn versions_and_tombstones_with_the_topic_id_survive_reopening_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, db) = open(tmp.path());
    assert!(store.load_all().unwrap().is_empty());

    let live = row("topic", "orders", 3, false);
    let tombstone = row("topic", "old", 7, true);
    let token = row("token", "t1", 1, false);
    for r in [&live, &tombstone, &token] {
        store.save(r).unwrap();
    }
    drop((store, db));

    let (store, _db) = open(tmp.path());
    // ordered by kind and key ("token" before "topic"), then by key
    assert_eq!(store.load_all().unwrap(), vec![token, tombstone, live]);
}

/// UC: UC-DEF-039
#[test]
fn saving_again_replaces_the_row_and_delete_tolerates_a_missing_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, _db) = open(tmp.path());
    store.save(&row("topic", "orders", 1, false)).unwrap();
    let removed = row("topic", "orders", 2, true);
    store.save(&removed).unwrap();
    assert_eq!(store.load_all().unwrap(), vec![removed]);

    store.delete("topic", "orders").unwrap();
    store.delete("topic", "orders").unwrap();
    assert!(store.load_all().unwrap().is_empty());
}

/// UC: UC-DEF-039
#[test]
fn old_tombstones_are_dropped_and_clear_empties_the_table() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, _db) = open(tmp.path());
    store.save(&row("topic", "a", 1, true)).unwrap(); // at_ms 1001
    store.save(&row("topic", "b", 500, true)).unwrap(); // at_ms 1500
    store.save(&row("topic", "c", 2, false)).unwrap(); // live: never expires

    assert_eq!(store.delete_tombstones_before(1_200).unwrap(), 1);
    let left: Vec<String> = store
        .load_all()
        .unwrap()
        .into_iter()
        .map(|r| r.key)
        .collect();
    assert_eq!(left, ["b", "c"]);

    store.clear().unwrap();
    assert!(store.load_all().unwrap().is_empty());
}

#[test]
fn a_row_with_a_bad_fingerprint_is_an_error_naming_the_definition() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, db) = open(tmp.path());
    db.conn()
        .execute(
            "INSERT INTO definition_versions VALUES ('topic', 'bad', 1, 0, x'ab', NULL, 1)",
            [],
        )
        .unwrap();
    let err = store.load_all().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("topic:bad"), "{err}");
}
