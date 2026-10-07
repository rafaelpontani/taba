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
        .execute("INSERT INTO topicos VALUES ('x', '{}')", [])
        .unwrap();
    drop(db);
    assert!(dir.join(NODE_DB_FILE).is_file());

    let db = NodeDb::open(&dir, DbSynchronous::Normal).unwrap();
    let n: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM topicos", [], |r| r.get(0))
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
