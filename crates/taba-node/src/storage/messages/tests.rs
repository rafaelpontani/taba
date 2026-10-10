use super::*;

fn meta(id: &str) -> MessageMeta {
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
        position: 0,
    }
}

fn store(dir: &Path) -> MessageStore {
    MessageStore::new(dir, DbSynchronous::Normal)
}

/// UC: UC-ARM-002, UC-ARM-010, UC-ARM-011
#[test]
fn a_message_is_a_body_file_plus_a_row_in_the_topic_database() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    let mut m = meta("a1");
    m.group_id = Some("g".into());
    m.content_type = "image/png".into();
    let bytes = [0u8, 255, 10, 13, 200];
    store.put("pedidos", &m, &bytes).unwrap();

    assert!(tmp.path().join("pedidos").join(TOPIC_DB_FILE).is_file());
    assert_eq!(
        std::fs::read(tmp.path().join("pedidos").join("a1.body")).unwrap(),
        bytes,
        "só os bytes do corpo, como vieram"
    );
    assert_eq!(store.get("pedidos", "a1").unwrap(), Some(m));
    assert_eq!(store.body("pedidos", "a1").unwrap(), bytes);
    assert_eq!(store.count("pedidos").unwrap(), 1);
}

#[test]
fn reads_do_not_create_folders_for_topics_without_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    assert_eq!(store.get("nada", "x").unwrap(), None);
    assert!(store.ready_ids("nada", 0, 10).unwrap().is_empty());
    assert_eq!(store.count("nada").unwrap(), 0);
    store.remove("nada", "x").unwrap();
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

/// UC: UC-ARM-014
#[test]
fn removing_deletes_the_row_and_then_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("pedidos", &meta("a1"), b"x").unwrap();
    store.remove("pedidos", "a1").unwrap();
    store.remove("pedidos", "a1").unwrap();
    assert_eq!(store.get("pedidos", "a1").unwrap(), None);
    assert!(!tmp.path().join("pedidos").join("a1.body").exists());
}

#[test]
fn ready_ids_come_in_id_order_and_skip_the_invisible() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    for (id, until) in [("c", 0), ("a", 0), ("b", 5_000), ("d", 100)] {
        let mut m = meta(id);
        m.invisible_until_ms = until;
        store.put("t", &m, b"x").unwrap();
    }
    assert_eq!(store.ready_ids("t", 100, 10).unwrap(), ["a", "c", "d"]);
    assert_eq!(store.ready_ids("t", 100, 2).unwrap(), ["a", "c"]);
    assert_eq!(store.next_invisible_until("t", 100).unwrap(), Some(5_000));
    assert_eq!(store.next_invisible_until("t", 5_000).unwrap(), None);
}

#[test]
fn delivery_fields_are_updated_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    let mut m = meta("a");
    store.put("t", &m, b"x").unwrap();
    m.delivery_count = 3;
    m.invisible_until_ms = 9_000;
    m.receipt_token = Some("tok".into());
    store.update_delivery("t", &m).unwrap();
    assert_eq!(store.get("t", "a").unwrap(), Some(m));
}

#[test]
fn expired_and_exhausted_queries_use_the_age_base_and_the_counters() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    let mut old = meta("old");
    old.published_ms = 1_000;
    let mut dead = meta("dead");
    dead.published_ms = 1_000;
    dead.dead_lettered_ms = Some(50_000);
    let mut spent = meta("spent");
    spent.published_ms = 40_000;
    spent.delivery_count = 2;
    spent.invisible_until_ms = 60_000;
    for m in [&old, &dead, &spent] {
        store.put("t", m, b"x").unwrap();
    }

    assert_eq!(store.expired_ids("t", 61_000, 60_000).unwrap(), ["old"]);
    assert!(store.expired_ids("t", 10_000, 60_000).unwrap().is_empty());
    assert_eq!(store.exhausted_ids("t", 60_000, 2).unwrap(), ["spent"]);
    assert!(
        store.exhausted_ids("t", 59_999, 2).unwrap().is_empty(),
        "ainda invisível"
    );
    assert!(store.exhausted_ids("t", 60_000, 3).unwrap().is_empty());
}

#[test]
fn moving_renames_the_body_and_keeps_the_id() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("pedidos", &meta("a1"), b"corpo").unwrap();
    let mut moved = meta("a1");
    moved.dead_lettered_ms = Some(7);
    moved.dead_letter_reason = Some("max-deliveries".into());
    store
        .move_to("pedidos", "pedidos-dead-letter", &moved)
        .unwrap();

    assert_eq!(store.get("pedidos", "a1").unwrap(), None);
    assert_eq!(store.get("pedidos-dead-letter", "a1").unwrap(), Some(moved));
    assert_eq!(store.body("pedidos-dead-letter", "a1").unwrap(), b"corpo");
    assert!(!tmp.path().join("pedidos").join("a1.body").exists());
}

/// UC: UC-DEF-024
#[test]
fn topic_folders_are_safe_for_case_insensitive_disks() {
    assert_eq!(dir_name("pedidos"), "pedidos");
    assert_eq!(dir_name("Pedidos"), "~pedidos");
    assert_eq!(dir_name("aBc-D_1"), "a~bc-~d_1");
    for name in ["pedidos", "Pedidos", "aBc-D_1", "ABC"] {
        assert_eq!(topic_of_dir(&dir_name(name)).as_deref(), Some(name));
    }
    assert_eq!(topic_of_dir("Maiuscula"), None);
    assert_eq!(topic_of_dir("~"), None);

    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("pedidos", &meta("a"), b"1").unwrap();
    store.put("Pedidos", &meta("a"), b"2").unwrap();
    assert_eq!(store.body("pedidos", "a").unwrap(), b"1");
    assert_eq!(store.body("Pedidos", "a").unwrap(), b"2");
    let mut on_disk = store.topics_on_disk().unwrap();
    on_disk.sort();
    assert_eq!(on_disk, ["Pedidos", "pedidos"]);
}

#[test]
fn dropping_a_topic_removes_its_folder_and_survives_a_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = store(tmp.path());
        store.put("pedidos", &meta("a"), b"1").unwrap();
    }
    let store = store(tmp.path());
    assert_eq!(
        store.count("pedidos").unwrap(),
        1,
        "o disco sobrevive ao reinício"
    );
    store.drop_topic("pedidos").unwrap();
    store.drop_topic("pedidos").unwrap();
    assert_eq!(store.count("pedidos").unwrap(), 0);
    assert!(!tmp.path().join("pedidos").exists());
}

/// UC: UC-FIF-036
#[test]
fn a_tombstone_keeps_a_short_row_and_drops_the_body() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    let mut m = meta("a1");
    m.group_id = Some("g".into());
    m.deduplication_id = Some("k".into());
    store.put("f", &m, b"corpo").unwrap();
    assert_eq!(store.count("f").unwrap(), 1);

    store.tombstone("f", &m, 9_000).unwrap();
    assert!(store.body("f", "a1").is_err(), "o corpo saiu");
    assert_eq!(store.count("f").unwrap(), 0, "a linha curta não conta");
    let rows = store.scan("f").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].delete_at_ms, Some(9_000));
    assert_eq!(rows[0].deduplication_id.as_deref(), Some("k"));
    assert_eq!(rows[0].group_id.as_deref(), Some("g"));
    assert_eq!(rows[0].published_ms, 1_000);
}

/// UC: UC-FIF-036
#[test]
fn a_tombstone_works_for_a_message_that_had_no_row() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.tombstone("f", &meta("so-memoria"), 5_000).unwrap();
    assert_eq!(store.scan("f").unwrap().len(), 1);
    assert_eq!(store.count("f").unwrap(), 0);
}

/// UC: UC-FIF-039
#[test]
fn sweeping_deletes_only_the_rows_whose_delete_at_passed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.tombstone("f", &meta("velha"), 1_000).unwrap();
    store.tombstone("f", &meta("nova"), 9_000).unwrap();
    store.put("f", &meta("viva"), b"x").unwrap();
    assert_eq!(store.sweep_tombstones("f", 5_000).unwrap(), 1);
    let ids: Vec<String> = store.scan("f").unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["nova", "viva"]);
    assert_eq!(store.sweep_tombstones("missing", 5_000).unwrap(), 0);
}

/// UC: UC-ARM-011
#[test]
fn a_topic_database_from_before_the_fifo_columns_is_upgraded_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("antigo");
    std::fs::create_dir_all(&dir).unwrap();
    {
        let conn = rusqlite::Connection::open(dir.join(TOPIC_DB_FILE)).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY NOT NULL, content_type TEXT NOT NULL, group_id TEXT,
                published_at INTEGER NOT NULL, delivery_count INTEGER NOT NULL DEFAULT 0,
                invisible_until INTEGER NOT NULL DEFAULT 0, receipt_token TEXT,
                dead_lettered_at INTEGER, dead_letter_reason TEXT, age_base INTEGER NOT NULL
             ) WITHOUT ROWID;
             INSERT INTO messages (id, content_type, published_at, age_base)
                VALUES ('velha', 'text/plain', 7, 7);",
        )
        .unwrap();
    }
    let store = store(tmp.path());
    let rows = store.scan("antigo").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].deduplication_id.clone(), rows[0].delete_at_ms),
        (None, None)
    );
    assert_eq!(store.count("antigo").unwrap(), 1);
    store.tombstone("antigo", &meta("nova"), 100).unwrap();
}

/// UC: UC-SIN-002, UC-ARM-005
#[test]
fn the_rows_of_a_block_come_by_position_in_id_order_with_range_and_limit() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    for (id, position) in [
        ("a", 5u64),
        ("b", 50),
        ("c", 500),
        ("d", u64::MAX),
        ("e", 0),
        ("f", 51),
    ] {
        let mut m = meta(id);
        m.position = position;
        store.put("t", &m, b"x").unwrap();
    }
    let ids = |ranges: &[(u64, u64)], from: Option<&str>, to: Option<&str>, limit| {
        store
            .block_rows("t", ranges, from, to, limit)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&[(0, 60)], None, None, 10), ["a", "b", "e", "f"]);
    // Um bloco que dá a volta no anel tem duas faixas, e os extremos contam.
    assert_eq!(
        ids(&[(0, 5), (u64::MAX - 1, u64::MAX)], None, None, 10),
        ["a", "d", "e"]
    );
    assert_eq!(ids(&[(0, 60)], Some("b"), Some("e"), 10), ["b", "e"]);
    assert_eq!(ids(&[(0, 60)], None, None, 2), ["a", "b"]);
    assert!(ids(&[(100, 400)], None, None, 10).is_empty());
    assert!(
        store
            .block_rows("sem-pasta", &[(0, u64::MAX)], None, None, 10)
            .unwrap()
            .is_empty()
    );
    // A posição volta como foi gravada.
    assert_eq!(store.get("t", "d").unwrap().unwrap().position, u64::MAX);
    assert_eq!(store.get("t", "e").unwrap().unwrap().position, 0);
}

/// UC: UC-FIF-041
#[test]
fn the_short_fifo_row_is_listed_with_its_delete_at() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut m = meta("a");
    m.position = 7;
    store.tombstone("f", &m, 9_000).unwrap();
    let rows = store.block_rows("f", &[(0, 10)], None, None, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].delete_at_ms, Some(9_000));
}

/// UC: UC-RTN-014, UC-RTN-016
#[test]
fn the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one() {
    let tmp = tempfile::tempdir().unwrap();
    let old = store(tmp.path());
    old.put("pedidos", &meta("a1"), b"antigo").unwrap();
    drop(old);

    let store = store(tmp.path());
    let previous = store
        .set_aside_old_disk()
        .unwrap()
        .expect("havia mensagens no disco");
    assert!(
        store.topics_on_disk().unwrap().is_empty(),
        "o disco do nó fica vazio"
    );
    assert_eq!(previous.body("pedidos", "a1").unwrap(), b"antigo");

    // Um segundo reinício não perde o que já estava de lado: a pasta nova some e a velha fica.
    drop(previous);
    store.put("pedidos", &meta("a2"), b"novo").unwrap();
    let previous = store.set_aside_old_disk().unwrap().expect("ainda há");
    assert!(previous.get("pedidos", "a1").unwrap().is_some());
    assert!(previous.get("pedidos", "a2").unwrap().is_none());

    drop(previous);
    store.drop_old_disk().unwrap();
    assert!(!tmp.path().join(MessageStore::PREVIOUS_LIFE).exists());
}
