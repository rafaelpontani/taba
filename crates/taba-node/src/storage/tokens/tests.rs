use super::*;
use crate::config::DbSynchronous;
use crate::core::token::generate;

fn record(id: &str, expires: Option<u64>) -> TokenRecord {
    generate(
        id.into(),
        "servico".into(),
        vec![Permission::Publish, Permission::Create],
        vec!["pedidos".into(), "fin-".into()],
        1_791_288_000_000,
        expires,
    )
    .record
}

fn open(dir: &std::path::Path) -> (SqliteTokenStore, Arc<NodeDb>) {
    let db = Arc::new(NodeDb::open(dir, DbSynchronous::Normal).unwrap());
    (SqliteTokenStore::new(db.clone()), db)
}

/// UC: UC-AUT-032
#[test]
fn tokens_are_rows_of_the_node_db_with_the_hash_and_the_salt() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, db) = open(tmp.path());
    assert!(store.load_all().unwrap().is_empty());

    let a = record("01927f3a-0000-7000-8000-000000000001", None);
    let b = record(
        "01927f3a-0000-7000-8000-000000000002",
        Some(1_800_000_000_000),
    );
    store.save(&a).unwrap();
    store.save(&b).unwrap();
    drop((store, db));

    let (store, _db) = open(tmp.path());
    assert_eq!(store.load_all().unwrap(), vec![a, b]);
    assert!(
        !tmp.path().join("_tokens").exists(),
        "não há arquivo por token"
    );
}

/// UC: UC-AUT-032
#[test]
fn the_secret_is_never_in_the_database_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, db) = open(tmp.path());
    let new = generate(
        "01927f3a-0000-7000-8000-000000000001".into(),
        "x".into(),
        vec![Permission::Consume],
        vec![],
        1,
        None,
    );
    store.save(&new.record).unwrap();
    drop((store, db));

    let (_, secret) = crate::core::token::parse(&new.token).unwrap();
    for entry in std::fs::read_dir(tmp.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(secret), "o segredo vazou para o disco");
    }
}

#[test]
fn saving_again_replaces_the_row_and_delete_tolerates_a_missing_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, _db) = open(tmp.path());
    let mut token = record("01927f3a-0000-7000-8000-000000000001", None);
    store.save(&token).unwrap();
    token.name = "outro".into();
    token.expires_ms = Some(5);
    store.save(&token).unwrap();
    assert_eq!(store.load_all().unwrap(), vec![token.clone()]);

    store.delete(&token.id).unwrap();
    store.delete(&token.id).unwrap();
    assert!(store.load_all().unwrap().is_empty());
}

#[test]
fn a_corrupt_row_is_an_error_naming_the_token() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, db) = open(tmp.path());
    db.conn()
        .execute(
            "INSERT INTO tokens VALUES ('ruim', 'n', 'publish,voar', '[]', 1, NULL, 's', 'h')",
            [],
        )
        .unwrap();
    let error = store.load_all().unwrap_err().to_string();
    assert!(error.contains("ruim") && error.contains("voar"), "{error}");
}

#[test]
fn the_memory_store_can_be_told_to_fail() {
    let store = MemoryTokenStore::default();
    store.save(&record("a", None)).unwrap();
    store.set_failing(true);
    assert!(store.save(&record("b", None)).is_err());
    assert!(store.delete("a").is_err());
    assert_eq!(store.ids(), ["a"]);
}
