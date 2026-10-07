use super::*;

fn meta(id: &str) -> MessageMeta {
    MessageMeta {
        id: id.into(),
        content_type: "application/json".into(),
        group_id: None,
        published_ms: 1_000,
        delivery_count: 0,
        invisible_until_ms: 0,
        receipt_token: None,
        dead_lettered_ms: None,
        dead_letter_reason: None,
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
