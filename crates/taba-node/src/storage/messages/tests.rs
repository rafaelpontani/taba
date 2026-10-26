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
    store.put("orders", &m, &bytes).unwrap();

    assert!(tmp.path().join("orders").join(TOPIC_DB_FILE).is_file());
    assert_eq!(
        std::fs::read(tmp.path().join("orders").join("a1.body")).unwrap(),
        bytes,
        "only the body bytes, as they came"
    );
    assert_eq!(store.get("orders", "a1").unwrap(), Some(m));
    assert_eq!(store.body("orders", "a1").unwrap(), bytes);
    assert_eq!(store.count("orders").unwrap(), 1);
}

#[test]
fn reads_do_not_create_folders_for_topics_without_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    assert_eq!(store.get("nothing", "x").unwrap(), None);
    assert!(store.ready_ids("nothing", 0, 10).unwrap().is_empty());
    assert_eq!(store.count("nothing").unwrap(), 0);
    store.remove("nothing", "x").unwrap();
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

/// UC: UC-ARM-014
#[test]
fn removing_deletes_the_row_and_then_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("a1"), b"x").unwrap();
    store.remove("orders", "a1").unwrap();
    store.remove("orders", "a1").unwrap();
    assert_eq!(store.get("orders", "a1").unwrap(), None);
    assert!(!tmp.path().join("orders").join("a1.body").exists());
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
        "still invisible"
    );
    assert!(store.exhausted_ids("t", 60_000, 3).unwrap().is_empty());
}

#[test]
fn moving_renames_the_body_and_keeps_the_id() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("a1"), b"corpo").unwrap();
    let mut moved = meta("a1");
    moved.dead_lettered_ms = Some(7);
    moved.dead_letter_reason = Some("max-deliveries".into());
    store
        .move_to("orders", "orders-dead-letter", &moved)
        .unwrap();

    assert_eq!(store.get("orders", "a1").unwrap(), None);
    assert_eq!(store.get("orders-dead-letter", "a1").unwrap(), Some(moved));
    assert_eq!(store.body("orders-dead-letter", "a1").unwrap(), b"corpo");
    assert!(!tmp.path().join("orders").join("a1.body").exists());
}

/// UC: UC-DEF-024
#[test]
fn topic_folders_are_safe_for_case_insensitive_disks() {
    assert_eq!(dir_name("orders"), "orders");
    assert_eq!(dir_name("Orders"), "~orders");
    assert_eq!(dir_name("aBc-D_1"), "a~bc-~d_1");
    for name in ["orders", "Orders", "aBc-D_1", "ABC"] {
        assert_eq!(topic_of_dir(&dir_name(name)).as_deref(), Some(name));
    }
    assert_eq!(topic_of_dir("Maiuscula"), None);
    assert_eq!(topic_of_dir("~"), None);

    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store.put("orders", &meta("a"), b"1").unwrap();
    store.put("Orders", &meta("a"), b"2").unwrap();
    assert_eq!(store.body("orders", "a").unwrap(), b"1");
    assert_eq!(store.body("Orders", "a").unwrap(), b"2");
    let mut on_disk = store.topics_on_disk().unwrap();
    on_disk.sort();
    assert_eq!(on_disk, ["Orders", "orders"]);
}

#[test]
fn dropping_a_topic_removes_its_folder_and_survives_a_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let store = store(tmp.path());
        store.put("orders", &meta("a"), b"1").unwrap();
    }
    let store = store(tmp.path());
    assert_eq!(
        store.count("orders").unwrap(),
        1,
        "the disk survives a restart"
    );
    store.drop_topic("orders").unwrap();
    store.drop_topic("orders").unwrap();
    assert_eq!(store.count("orders").unwrap(), 0);
    assert!(!tmp.path().join("orders").exists());
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
    assert_eq!(store.count("f").unwrap(), 0, "the short row does not count");
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
    store.tombstone("f", &meta("old"), 1_000).unwrap();
    store.tombstone("f", &meta("new"), 9_000).unwrap();
    store.put("f", &meta("stays"), b"x").unwrap();
    assert_eq!(store.sweep_tombstones("f", 5_000).unwrap(), 1);
    let ids: Vec<String> = store.scan("f").unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["new", "stays"]);
    assert_eq!(store.sweep_tombstones("missing", 5_000).unwrap(), 0);
}

/// UC: UC-ARM-011
#[test]
fn a_topic_database_from_before_the_fifo_columns_is_upgraded_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("legacy");
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
                VALUES ('old', 'text/plain', 7, 7);",
        )
        .unwrap();
    }
    let store = store(tmp.path());
    let rows = store.scan("legacy").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].deduplication_id.clone(), rows[0].delete_at_ms),
        (None, None)
    );
    assert_eq!(store.count("legacy").unwrap(), 1);
    store.tombstone("legacy", &meta("new"), 100).unwrap();
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
    // A block that wraps around the ring has two ranges, and the extremes count.
    assert_eq!(
        ids(&[(0, 5), (u64::MAX - 1, u64::MAX)], None, None, 10),
        ["a", "d", "e"]
    );
    assert_eq!(ids(&[(0, 60)], Some("b"), Some("e"), 10), ["b", "e"]);
    assert_eq!(ids(&[(0, 60)], None, None, 2), ["a", "b"]);
    assert!(ids(&[(100, 400)], None, None, 10).is_empty());
    assert!(
        store
            .block_rows("no-folder", &[(0, u64::MAX)], None, None, 10)
            .unwrap()
            .is_empty()
    );
    // The position comes back as it was written.
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

/// UC: UC-RTN-014, UC-RTN-016, UC-ING-036
#[test]
fn the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one() {
    let tmp = tempfile::tempdir().unwrap();
    let old = store(tmp.path());
    old.put("orders", &meta("a1"), b"old").unwrap();
    drop(old);

    let store = store(tmp.path());
    let previous = store
        .set_aside_old_disk(None)
        .unwrap()
        .expect("there were messages on disk");
    assert!(
        store.topics_on_disk().unwrap().is_empty(),
        "the node's disk is left empty"
    );
    assert_eq!(previous.body("orders", "a1").unwrap(), b"old");

    // A second restart does not lose what was already set aside: the new folder disappears and the old one stays.
    drop(previous);
    store.put("orders", &meta("a2"), b"new").unwrap();
    let previous = store
        .set_aside_old_disk(None)
        .unwrap()
        .expect("there still is");
    assert!(previous.get("orders", "a1").unwrap().is_some());
    assert!(previous.get("orders", "a2").unwrap().is_none());

    drop(previous);
    store.drop_old_disk().unwrap();
    assert!(!tmp.path().join(MessageStore::PREVIOUS_LIFE).exists());
}

/// UC: UC-RTN-023, UC-RTN-025
#[test]
fn a_disk_with_a_manifest_becomes_the_folder_of_its_life_and_the_lives_coexist() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = tmp.path().join(MessageStore::LIFE_MANIFEST);

    // First life: its disk and its manifest go to the folder with its ID.
    let first = store(tmp.path());
    first.put("orders", &meta("a1"), b"one").unwrap();
    std::fs::write(&manifest, b"{\"life\":1}").unwrap();
    let previous = first
        .set_aside_old_disk(Some("node-a"))
        .unwrap()
        .expect("there were messages on disk");
    assert_eq!(previous.body("orders", "a1").unwrap(), b"one");
    assert!(
        !manifest.exists(),
        "the root has no manifest of a past life"
    );
    let lives = tmp.path().join(MessageStore::LIVES);
    assert!(
        lives
            .join("node-a")
            .join(MessageStore::LIFE_MANIFEST)
            .is_file()
    );

    // Second life, after another restart: the first one stays, next to it.
    drop(previous);
    first.put("orders", &meta("a2"), b"two").unwrap();
    std::fs::write(&manifest, b"{\"life\":2}").unwrap();
    let previous = first.set_aside_old_disk(Some("node-b")).unwrap().unwrap();
    assert!(previous.get("orders", "a2").unwrap().is_some());
    assert!(lives.join("node-a").join("orders").is_dir());
    assert!(lives.join("node-b").join("orders").is_dir());

    // A life without messages is still a life: the manifest is kept for the nodes that were expected.
    std::fs::write(&manifest, b"{\"life\":3}").unwrap();
    assert!(first.set_aside_old_disk(Some("node-c")).unwrap().is_none());
    assert!(
        lives
            .join("node-c")
            .join(MessageStore::LIFE_MANIFEST)
            .is_file()
    );

    // The end of the recovery of a disk with no manifest does not touch the lives.
    drop(previous);
    first.drop_old_disk().unwrap();
    for life in ["node-a", "node-b", "node-c"] {
        assert!(
            lives.join(life).is_dir(),
            "{life} stays until the restoration ends"
        );
    }

    // The ID of a life is a folder name.
    assert!(first.set_aside_old_disk(Some("../x")).is_err());
}

/// UC: UC-RTN-027
#[test]
fn the_folder_of_a_topic_records_the_topic_id_it_was_created_for_and_keeps_it() {
    let tmp = tempfile::tempdir().unwrap();
    let first = store(tmp.path());
    // No folder, no record; and a folder created before the resolver is set has none either (a disk of a
    // version before this record).
    assert_eq!(first.topic_id("orders").unwrap(), None);
    first.put("legacy", &meta("l1"), b"x").unwrap();
    assert_eq!(first.topic_id("legacy").unwrap(), None);

    first.set_topic_id_resolver(Box::new(|topic| Some(format!("id-of-{topic}"))));
    first.put("orders", &meta("a1"), b"x").unwrap();
    first.put("orders", &meta("a2"), b"x").unwrap();
    assert_eq!(
        first.topic_id("orders").unwrap().as_deref(),
        Some("id-of-orders")
    );

    // A later life of the same folder does not rewrite it: the folder belongs to the topic it was created for.
    let second = store(tmp.path());
    second.set_topic_id_resolver(Box::new(|_| Some("another".into())));
    second.put("orders", &meta("a3"), b"x").unwrap();
    assert_eq!(
        second.topic_id("orders").unwrap().as_deref(),
        Some("id-of-orders")
    );

    // The store over a life reads the same record.
    let life = second.open_life(tmp.path());
    assert_eq!(
        life.topic_id("orders").unwrap().as_deref(),
        Some("id-of-orders")
    );
}

/// UC: UC-INV-001
#[test]
fn the_delivery_metadata_lives_in_the_message_row_and_survives_a_reopening() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    let mut m = meta("a");
    store.put("t", &m, b"x").unwrap();
    // Delivery count, absolute deadline and receipt token are written together, in one commit.
    m.delivery_count = 2;
    m.invisible_until_ms = 123_456;
    m.receipt_token = Some("receipt-1".into());
    store.update_delivery("t", &m).unwrap();

    let reopened = self::store(tmp.path());
    let read = reopened.get("t", "a").unwrap().unwrap();
    assert_eq!(read.delivery_count, 2);
    assert_eq!(read.invisible_until_ms, 123_456, "an absolute instant");
    assert_eq!(read.receipt_token.as_deref(), Some("receipt-1"));
    assert_eq!(read, m);
    assert_eq!(reopened.count("t").unwrap(), 1, "no side record");
}

/// UC: UC-ARM-012
#[test]
fn the_expiry_table_keeps_the_pairs_that_did_not_fit_ordered_by_deadline() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    store
        .expiry_apply(
            "orders",
            &[
                (true, 300, "c".into()),
                (true, 100, "a".into()),
                (true, 200, "b".into()),
                (true, 100, "z".into()),
            ],
        )
        .unwrap();
    assert_eq!(store.expiry_count("orders").unwrap(), 4);
    // The earliest first, with a limit, by the table's own key.
    assert_eq!(
        store.expiry_earliest("orders", 3).unwrap(),
        [
            (100, "a".to_owned()),
            (100, "z".to_owned()),
            (200, "b".to_owned())
        ]
    );
    // Operations apply in order: a delete then a put of the same pair leaves it there.
    store
        .expiry_apply(
            "orders",
            &[
                (false, 100, "a".into()),
                (true, 100, "a".into()),
                (false, 300, "c".into()),
            ],
        )
        .unwrap();
    assert_eq!(store.expiry_count("orders").unwrap(), 3);
    // The table is derived: clearing it leaves the messages alone.
    store.expiry_clear("orders").unwrap();
    assert_eq!(store.expiry_count("orders").unwrap(), 0);
    assert!(store.expiry_earliest("orders", 10).unwrap().is_empty());
    // A topic without a folder has an empty table and reading creates nothing.
    assert_eq!(store.expiry_count("nothing").unwrap(), 0);
    assert!(!tmp.path().join(dir_name("nothing")).exists());
}

/// UC: UC-ENT-015, UC-ARM-005
#[test]
fn the_end_of_a_group_is_read_by_an_index_in_id_order() {
    let tmp = tempfile::tempdir().unwrap();
    let store = store(tmp.path());
    for i in 0..30 {
        let mut m = meta(&format!("m{i:02}"));
        m.group_id = (i % 3 != 0).then(|| format!("g{}", i % 3));
        store.put("t", &m, b"{}").unwrap();
    }
    let group = |name: &str, desc: bool, limit: usize| -> Vec<String> {
        store
            .page(
                "t",
                &PageQuery {
                    group: Some(name.into()),
                    desc,
                    limit,
                    ..PageQuery::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    };
    assert_eq!(group("g1", false, 3), ["m01", "m04", "m07"]);
    assert_eq!(group("g2", true, 2), ["m29", "m26"]);
    assert_eq!(
        group("", false, 2),
        ["m00", "m03"],
        "the messages without a group"
    );

    // The plan walks the index of the group, with no sort and no scan of the table.
    let db = tmp.path().join(dir_name("t")).join(TOPIC_DB_FILE);
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(
            "EXPLAIN QUERY PLAN SELECT id FROM messages WHERE delete_at IS NULL AND group_id = ?1              AND id > ?2 AND NOT EXISTS (SELECT 1 FROM expiry WHERE expiry.id = messages.id)              ORDER BY id LIMIT 64",
        )
        .unwrap();
    let plan: Vec<String> = stmt
        .query_map(["g1", "a"], |row| row.get::<_, String>(3))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let plan = plan.join(" | ");
    assert!(plan.contains("messages_group"), "plan: {plan}");
    assert!(!plan.contains("TEMP B-TREE"), "plan: {plan}");
}
