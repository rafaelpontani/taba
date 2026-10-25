use std::collections::BTreeSet;
use std::sync::Arc;

use super::*;
use crate::config::{Config, MapEnv};
use crate::core::clock::ManualClock;
use crate::storage::topics::MemoryTopicStore;

struct Harness {
    topics: Topics,
    store: Arc<MemoryTopicStore>,
    clock: Arc<ManualClock>,
}

fn rules() -> TopicRules {
    let env = MapEnv::new([("TABA_REPLICATION_FACTOR", "1")]);
    TopicRules::from_config(&Config::parse(None, &env).unwrap())
}

fn harness_with(rules: TopicRules) -> Harness {
    let store = Arc::new(MemoryTopicStore::default());
    let clock = Arc::new(ManualClock::at_doc_epoch());
    let topics = Topics::new(rules, store.clone(), clock.clone(), Vec::new());
    Harness {
        topics,
        store,
        clock,
    }
}

fn harness() -> Harness {
    harness_with(rules())
}

fn queue(name: &str) -> CreateTopic {
    CreateTopic {
        name: name.into(),
        kind: Kind::Queue,
        tags: None,
        prefixes: None,
        volatile: None,
        idle_timeout: None,
        fifo: None,
        max_message_bytes: None,
        ttl: None,
        max_deliveries: None,
    }
}

fn distributor(name: &str, prefixes: &[&str]) -> CreateTopic {
    CreateTopic {
        kind: Kind::Distributor,
        prefixes: Some(prefixes.iter().map(|p| (*p).to_owned()).collect()),
        ..queue(name)
    }
}

fn names(result: &ListResult) -> Vec<String> {
    result
        .items
        .iter()
        .map(|v| v.definition.name.clone())
        .collect()
}

async fn code<T>(result: impl std::future::Future<Output = Result<T, TabaError>>) -> ErrorCode {
    match result.await {
        Err(e) => e.code,
        Ok(_) => panic!("expected an error"),
    }
}

#[tokio::test]
async fn a_new_queue_has_the_defaults_and_the_cluster_values_in_effect() {
    let h = harness();
    let view = h.topics.create(queue("orders")).await.unwrap();
    let json = view.to_json();
    assert_eq!(json["type"], "queue");
    assert_eq!(json["volatile"], false);
    assert!(json["ttl"].is_null() && json["maxDeliveries"].is_null());
    assert_eq!(
        json["effective"],
        serde_json::json!({ "maxMessageBytes": 5_242_880, "ttl": 1_209_600, "maxDeliveries": 10 })
    );
    assert!(crate::core::id::is_guid_v7(&view.definition.id));
    assert_eq!(h.store.names(), ["orders"]);
}

#[tokio::test]
async fn a_volatile_queue_without_idle_timeout_gets_the_default() {
    let h = harness();
    let created = h
        .topics
        .create(CreateTopic {
            volatile: Some(true),
            ..queue("v")
        })
        .await
        .unwrap();
    assert_eq!(created.to_json()["idleTimeout"], 600);
    let own = h
        .topics
        .create(CreateTopic {
            volatile: Some(true),
            idle_timeout: Some(60),
            ..queue("w")
        })
        .await
        .unwrap();
    assert_eq!(own.to_json()["idleTimeout"], 60);
}

#[tokio::test]
async fn creating_an_existing_name_conflicts_and_names_are_case_sensitive() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    assert_eq!(
        code(h.topics.create(queue("orders"))).await,
        ErrorCode::TopicAlreadyExists
    );
    h.topics.create(queue("Pedidos")).await.unwrap();
}

#[tokio::test]
async fn recreating_after_a_delete_gives_a_new_id() {
    let h = harness();
    let first = h.topics.create(queue("orders")).await.unwrap();
    h.topics.delete("orders").await.unwrap();
    let second = h.topics.create(queue("orders")).await.unwrap();
    assert_ne!(first.definition.id, second.definition.id);
}

#[tokio::test]
async fn a_failed_write_leaves_nothing_visible() {
    let h = harness();
    h.topics.create(queue("a")).await.unwrap();
    h.store.set_failing(true);

    assert_eq!(
        code(h.topics.create(queue("b"))).await,
        ErrorCode::InternalError
    );
    assert_eq!(code(h.topics.delete("a")).await, ErrorCode::InternalError);
    let patch = UpdateTopic {
        ttl: Patch::Value(100),
        ..Default::default()
    };
    assert_eq!(
        code(h.topics.update("a", patch)).await,
        ErrorCode::InternalError
    );

    let listed = h.topics.list(ListQuery::default()).unwrap();
    assert_eq!(names(&listed), ["a", "a-dead-letter"]);
    assert!(h.topics.get("a").unwrap().to_json()["ttl"].is_null());
}

#[tokio::test]
async fn the_store_keeps_the_definitions_that_were_created() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    h.topics
        .create(distributor("dist", &["ped"]))
        .await
        .unwrap();

    let stored = h.topics.stored().await.unwrap();
    let orders = stored.iter().find(|d| d.name == "orders").unwrap();
    assert_eq!(orders, &h.topics.get("orders").unwrap().definition);
    assert!(stored.iter().any(|d| d.name == "dist"));

    // A registry built from nothing does not see them: the node takes them over only through the restoration.
    let fresh = Topics::new(rules(), h.store.clone(), h.clock.clone(), Vec::new());
    assert_eq!(
        code(async { fresh.get("orders") }).await,
        ErrorCode::TopicNotFound
    );
}

/// UC: UC-TOP-008, UC-TOP-009, UC-TOP-010
#[tokio::test]
async fn name_and_tags_are_validated_with_the_field_in_details() {
    let h = harness();
    let err = h.topics.create(queue("with space")).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationInvalidName);
    assert_eq!(err.details[0].field, "name");

    let err = h.topics.create(queue("x-dead-letter")).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationReservedName);

    let bad_tags = CreateTopic {
        tags: Some([("k".to_owned(), "a b".to_owned())].into()),
        ..queue("t")
    };
    let err = h.topics.create(bad_tags).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationFailed);
    assert_eq!(err.details[0].field, "tags.k");
}

/// UC: UC-TOP-013
#[tokio::test]
async fn fields_that_do_not_apply_to_the_type_are_refused() {
    let h = harness();
    let with = |f: fn(&mut CreateTopic)| {
        let mut request = distributor("d", &["p"]);
        f(&mut request);
        request
    };
    type Mutate = fn(&mut CreateTopic);
    let cases: [(&str, Mutate); 6] = [
        ("fifo", |r| r.fifo = Some(true)),
        ("volatile", |r| r.volatile = Some(false)),
        ("ttl", |r| r.ttl = Some(100)),
        ("maxDeliveries", |r| r.max_deliveries = Some(5)),
        ("maxMessageBytes", |r| r.max_message_bytes = Some(10)),
        ("idleTimeout", |r| r.idle_timeout = Some(100)),
    ];
    for (field, mutate) in cases {
        let err = h.topics.create(with(mutate)).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationNotApplicable, "{field}");
        assert_eq!(err.details[0].field, field);
    }
    let queue_with_prefixes = CreateTopic {
        prefixes: Some(vec!["p".into()]),
        ..queue("q")
    };
    assert_eq!(
        code(h.topics.create(queue_with_prefixes)).await,
        ErrorCode::ValidationNotApplicable
    );
    let idle_without_volatile = CreateTopic {
        idle_timeout: Some(100),
        ..queue("q")
    };
    assert_eq!(
        code(h.topics.create(idle_without_volatile)).await,
        ErrorCode::ValidationNotApplicable
    );
}

/// UC: UC-TOP-014
#[tokio::test]
async fn idle_timeout_ttl_and_max_deliveries_have_exact_limits() {
    let h = harness();
    type Set = fn(&mut CreateTopic, i64);
    let cases: [(&str, Set, i64, i64); 3] = [
        (
            "idleTimeout",
            |r, v| {
                r.volatile = Some(true);
                r.idle_timeout = Some(v);
            },
            60,
            604_800,
        ),
        ("ttl", |r, v| r.ttl = Some(v), 60, 31_536_000),
        ("maxDeliveries", |r, v| r.max_deliveries = Some(v), 1, 1000),
    ];
    for (i, (field, set, lo, hi)) in cases.into_iter().enumerate() {
        for (n, (value, ok)) in [(lo - 1, false), (lo, true), (hi, true), (hi + 1, false)]
            .into_iter()
            .enumerate()
        {
            let mut request = queue(&format!("t{i}-{n}"));
            set(&mut request, value);
            let result = h.topics.create(request).await;
            assert_eq!(result.is_ok(), ok, "{field}={value}");
            if let Err(e) = result {
                assert_eq!(e.code, ErrorCode::ValidationOutOfRange, "{field}={value}");
            }
        }
    }
}

/// UC: UC-TOP-015
#[tokio::test]
async fn max_message_bytes_may_exceed_the_default_but_not_the_hard_limit() {
    let h = harness_with(TopicRules {
        hard_limit: Some(10_000_000),
        ..rules()
    });
    let big = CreateTopic {
        max_message_bytes: Some(8_000_000),
        ..queue("a")
    };
    h.topics.create(big).await.unwrap();
    let too_big = CreateTopic {
        max_message_bytes: Some(10_000_001),
        ..queue("b")
    };
    assert_eq!(
        code(h.topics.create(too_big)).await,
        ErrorCode::ValidationMaxBytesAboveHardLimit
    );
    let zero = CreateTopic {
        max_message_bytes: Some(0),
        ..queue("c")
    };
    assert_eq!(
        code(h.topics.create(zero)).await,
        ErrorCode::ValidationOutOfRange
    );
}

/// UC: UC-TOP-016
#[tokio::test]
async fn distributor_prefixes_are_validated() {
    let h = harness();
    assert!(h.topics.create(distributor("ok", &[])).await.is_ok());
    for bad in [vec![""], vec!["a b"]] {
        assert!(h.topics.create(distributor("d", &bad)).await.is_err());
    }
}

#[tokio::test]
async fn patch_changes_only_what_is_sent_and_tags_replace_the_whole_map() {
    let h = harness();
    let created = CreateTopic {
        tags: Some([("a".to_owned(), "1".to_owned())].into()),
        ttl: Some(100),
        ..queue("orders")
    };
    h.topics.create(created).await.unwrap();

    let only_tags = UpdateTopic {
        tags: Some([("b".to_owned(), "2".to_owned())].into()),
        ..Default::default()
    };
    let json = h
        .topics
        .update("orders", only_tags)
        .await
        .unwrap()
        .to_json();
    assert_eq!(json["tags"], serde_json::json!({ "b": "2" }));
    assert_eq!(json["ttl"], 100, "what did not come does not change");

    let json = h
        .topics
        .update("orders", UpdateTopic::default())
        .await
        .unwrap()
        .to_json();
    assert_eq!(json["tags"], serde_json::json!({ "b": "2" }));
}

#[tokio::test]
async fn patch_null_returns_to_the_cluster_default() {
    let h = harness();
    let created = CreateTopic {
        ttl: Some(100),
        max_deliveries: Some(3),
        max_message_bytes: Some(1000),
        ..queue("orders")
    };
    h.topics.create(created).await.unwrap();
    let reset = UpdateTopic {
        ttl: Patch::Null,
        max_deliveries: Patch::Null,
        max_message_bytes: Patch::Null,
        ..Default::default()
    };
    let json = h.topics.update("orders", reset).await.unwrap().to_json();
    assert!(json["ttl"].is_null() && json["maxDeliveries"].is_null());
    assert_eq!(json["effective"]["ttl"], 1_209_600);
    assert_eq!(json["effective"]["maxMessageBytes"], 5_242_880);
}

/// UC: UC-TOP-034
#[tokio::test]
async fn patch_refuses_immutable_fields_even_with_the_current_value() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    for field in ["name", "id", "type", "volatile", "fifo"] {
        let patch = UpdateTopic {
            immutable_present: vec![field.to_owned()],
            ..Default::default()
        };
        let err = h.topics.update("orders", patch).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationImmutableField, "{field}");
        assert_eq!(err.details[0].field, field);
    }
}

/// UC: UC-TOP-035
#[tokio::test]
async fn patch_validates_applicability_and_ranges() {
    let h = harness();
    h.topics.create(queue("queue")).await.unwrap();
    h.topics.create(distributor("dist", &["p"])).await.unwrap();

    let ttl_on_distributor = UpdateTopic {
        ttl: Patch::Value(100),
        ..Default::default()
    };
    assert_eq!(
        code(h.topics.update("dist", ttl_on_distributor)).await,
        ErrorCode::ValidationNotApplicable
    );
    let prefixes_on_queue = UpdateTopic {
        prefixes: Some(vec!["x".into()]),
        ..Default::default()
    };
    assert_eq!(
        code(h.topics.update("queue", prefixes_on_queue)).await,
        ErrorCode::ValidationNotApplicable
    );
    let idle_on_plain_queue = UpdateTopic {
        idle_timeout: Some(100),
        ..Default::default()
    };
    assert_eq!(
        code(h.topics.update("queue", idle_on_plain_queue)).await,
        ErrorCode::ValidationNotApplicable
    );
    let out = UpdateTopic {
        ttl: Patch::Value(59),
        ..Default::default()
    };
    assert_eq!(
        code(h.topics.update("queue", out)).await,
        ErrorCode::ValidationOutOfRange
    );
    let new_prefixes = UpdateTopic {
        prefixes: Some(vec!["a".into(), "b".into()]),
        ..Default::default()
    };
    let json = h
        .topics
        .update("dist", new_prefixes)
        .await
        .unwrap()
        .to_json();
    assert_eq!(json["prefixes"], serde_json::json!(["a", "b"]));
}

#[tokio::test]
async fn unknown_topics_are_404_and_dead_letters_are_read_only() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    h.topics.create(distributor("dist", &["p"])).await.unwrap();

    assert_eq!(
        code(h.topics.update("nothing", UpdateTopic::default())).await,
        ErrorCode::TopicNotFound
    );
    assert_eq!(
        code(h.topics.delete("nothing")).await,
        ErrorCode::TopicNotFound
    );
    for dead_letter in ["orders-dead-letter"] {
        assert_eq!(
            code(h.topics.update(dead_letter, UpdateTopic::default())).await,
            ErrorCode::TopicReadOnly
        );
        assert_eq!(
            code(h.topics.delete(dead_letter)).await,
            ErrorCode::TopicReadOnly
        );
    }
    // A distributor has no dead-letter, and neither does the dead-letter of a queue that does not exist.
    for missing in ["dist-dead-letter", "nothing-dead-letter"] {
        assert_eq!(
            code(h.topics.update(missing, UpdateTopic::default())).await,
            ErrorCode::TopicNotFound,
            "{missing}"
        );
    }
}

#[tokio::test]
async fn the_dead_letter_is_derived_from_its_queue_and_never_stored() {
    let h = harness();
    let source = CreateTopic {
        tags: Some([("time".to_owned(), "fin".to_owned())].into()),
        ttl: Some(500),
        fifo: Some(true),
        ..queue("orders")
    };
    let source = h.topics.create(source).await.unwrap();

    let dl = h.topics.get("orders-dead-letter").unwrap().to_json();
    assert_eq!(dl["deadLetter"], true);
    assert_eq!(dl["source"], "orders");
    assert_eq!(dl["ttl"], 500);
    assert_eq!(dl["tags"], serde_json::json!({ "time": "fin" }));
    assert_eq!(dl["fifo"], false, "a FIFO's dead-letter is not FIFO");
    assert_ne!(dl["id"], source.to_json()["id"]);
    assert_eq!(h.store.names(), ["orders"], "nothing is stored for it");

    let mut json = h.topics.get("orders").unwrap().to_json();
    assert!(json.get("deadLetter").is_none() && json.get("source").is_none());
    json = h
        .topics
        .get("dist-dead-letter")
        .map(|v| v.to_json())
        .unwrap_or_default();
    assert!(json.is_null());
}

#[tokio::test]
async fn deleting_a_queue_takes_its_dead_letter_with_it() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    h.topics.delete("orders").await.unwrap();
    assert_eq!(
        code(async { h.topics.get("orders-dead-letter") }).await,
        ErrorCode::TopicNotFound
    );
    assert!(h.store.names().is_empty());
}

async fn seeded(h: &Harness, names: &[&str]) {
    for name in names {
        h.topics.create(queue(name)).await.unwrap();
        h.clock.advance(std::time::Duration::from_millis(5));
    }
}

#[tokio::test]
async fn listing_is_sorted_by_name_and_includes_dead_letters() {
    let h = harness();
    seeded(&h, &["b", "a"]).await;
    h.topics.create(distributor("c", &["a"])).await.unwrap();
    let listed = h.topics.list(ListQuery::default()).unwrap();
    assert_eq!(
        names(&listed),
        ["a", "a-dead-letter", "b", "b-dead-letter", "c"]
    );
    assert!(listed.next_cursor.is_none());
}

#[tokio::test]
async fn sort_accepts_name_id_and_created_at_ascending_and_descending() {
    let h = harness();
    seeded(&h, &["b", "a", "c"]).await;
    let by = |sort: &str| {
        let query = ListQuery {
            sort: Some(sort.into()),
            name: Some("-".into()),
            ..Default::default()
        };
        names(&h.topics.list(query).unwrap())
    };
    assert_eq!(
        by("name"),
        ["a-dead-letter", "b-dead-letter", "c-dead-letter"]
    );
    assert_eq!(
        by("-name"),
        ["c-dead-letter", "b-dead-letter", "a-dead-letter"]
    );
    assert_eq!(
        by("createdAt"),
        ["b-dead-letter", "a-dead-letter", "c-dead-letter"]
    );
    assert_eq!(
        by("-createdAt"),
        ["c-dead-letter", "a-dead-letter", "b-dead-letter"]
    );
    let ids = by("id");
    assert_eq!(ids, ["b-dead-letter", "a-dead-letter", "c-dead-letter"]);
    assert_eq!(
        by("-id"),
        ["c-dead-letter", "a-dead-letter", "b-dead-letter"]
    );

    let bad = ListQuery {
        sort: Some("tamanho".into()),
        ..Default::default()
    };
    assert_eq!(
        h.topics.list(bad).err().map(|e| e.code),
        Some(ErrorCode::ValidationFailed)
    );
}

#[tokio::test]
async fn name_filter_matches_substrings_and_is_case_sensitive() {
    let h = harness();
    seeded(&h, &["orders-eu", "orders-us", "Pedidos", "faturas"]).await;
    let filtered = |text: &str| {
        let query = ListQuery {
            name: Some(text.into()),
            ..Default::default()
        };
        names(&h.topics.list(query).unwrap())
            .into_iter()
            .filter(|n| !n.ends_with("-dead-letter"))
            .collect::<Vec<_>>()
    };
    assert_eq!(filtered("orders"), ["orders-eu", "orders-us"]);
    assert_eq!(filtered("Pedidos"), ["Pedidos"]);
    assert_eq!(filtered("eu"), ["orders-eu"]);
    assert!(filtered("zzz").is_empty());
}

fn walk(h: &Harness, sort: Option<&str>, limit: usize) -> Vec<String> {
    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let query = ListQuery {
            limit: Some(limit.to_string()),
            cursor: cursor.take(),
            sort: sort.map(str::to_owned),
            ..Default::default()
        };
        let page = h.topics.list(query).unwrap();
        assert!(page.items.len() <= limit);
        seen.extend(names(&page));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return seen,
        }
    }
}

/// UC: UC-PAG-007
#[tokio::test]
async fn walking_every_page_returns_each_item_once_even_with_equal_creation_times() {
    let h = harness();
    // Without advancing the clock: all have the same `createdAt` and only the `id` breaks the tie.
    for name in ["a", "b", "c", "d", "e"] {
        h.topics.create(queue(name)).await.unwrap();
    }
    for sort in [
        None,
        Some("-name"),
        Some("createdAt"),
        Some("-createdAt"),
        Some("id"),
        Some("-id"),
    ] {
        for limit in [1, 2, 3, 10] {
            let seen = walk(&h, sort, limit);
            let unique: BTreeSet<_> = seen.iter().collect();
            assert_eq!(seen.len(), 10, "{sort:?} limit {limit}: {seen:?}");
            assert_eq!(unique.len(), 10, "repeated in {sort:?} limit {limit}");
        }
    }
}

#[tokio::test]
async fn inserting_or_removing_before_the_cursor_does_not_shift_the_next_page() {
    let h = harness();
    seeded(&h, &["b", "d", "f"]).await;
    let first = h
        .topics
        .list(ListQuery {
            limit: Some("2".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(names(&first), ["b", "b-dead-letter"]);

    seeded(&h, &["a"]).await; // comes before the cursor's key
    h.topics.delete("b").await.unwrap(); // goes away, and the cursor keeps applying
    let second = h
        .topics
        .list(ListQuery {
            limit: Some("2".into()),
            cursor: first.next_cursor.clone(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(names(&second), ["d", "d-dead-letter"]);

    // Repeating the cursor returns the same page while nothing changes.
    let again = h
        .topics
        .list(ListQuery {
            limit: Some("2".into()),
            cursor: first.next_cursor,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(names(&again), names(&second));
}

/// UC: UC-PAG-005, UC-PAG-002
#[tokio::test]
async fn a_cursor_only_works_with_the_sort_and_filter_that_made_it() {
    let h = harness();
    seeded(&h, &["a", "b", "c"]).await;
    let page = h
        .topics
        .list(ListQuery {
            limit: Some("1".into()),
            ..Default::default()
        })
        .unwrap();
    let cursor = page.next_cursor.unwrap();

    let other = |sort: Option<&str>, name: Option<&str>| ListQuery {
        limit: Some("1".into()),
        cursor: Some(cursor.clone()),
        sort: sort.map(str::to_owned),
        name: name.map(str::to_owned),
        ..Default::default()
    };
    for query in [other(Some("-name"), None), other(None, Some("a"))] {
        assert_eq!(
            h.topics.list(query).err().map(|e| e.code),
            Some(ErrorCode::ValidationInvalidCursor)
        );
    }
    assert!(h.topics.list(other(None, None)).is_ok());

    let garbage = ListQuery {
        cursor: Some("lixo".into()),
        ..Default::default()
    };
    assert_eq!(
        h.topics.list(garbage).err().map(|e| e.code),
        Some(ErrorCode::ValidationInvalidCursor)
    );
}

/// UC: UC-RET-003
#[tokio::test]
async fn changing_the_default_ttl_affects_the_queues_without_their_own_ttl_only() {
    let h = harness();
    h.topics.create(queue("plain")).await.unwrap();
    let own = CreateTopic {
        ttl: Some(100),
        ..queue("own")
    };
    h.topics.create(own).await.unwrap();
    let ttl = |topics: &Topics, name: &str| {
        topics.get(name).unwrap().to_json()["effective"]["ttl"].clone()
    };
    assert_eq!(ttl(&h.topics, "plain"), 1_209_600, "14 days by default");
    assert_eq!(ttl(&h.topics, "own"), 100);

    // The default is read when the value is checked: the same definitions under another `retention.ttl.default`.
    let env = MapEnv::new([
        ("TABA_REPLICATION_FACTOR", "1"),
        ("TABA_RETENTION_TTL_DEFAULT", "1h"),
    ]);
    let rules = TopicRules::from_config(&Config::parse(None, &env).unwrap());
    assert_eq!(rules.ttl_default, 3600);
    let definitions = vec![
        h.topics.get("plain").unwrap().definition,
        h.topics.get("own").unwrap().definition,
    ];
    let changed = Topics::new(rules, h.store.clone(), h.clock.clone(), definitions);
    assert_eq!(
        ttl(&changed, "plain"),
        3600,
        "the queue without `ttl` follows the new default"
    );
    assert_eq!(
        ttl(&changed, "own"),
        100,
        "the one that defined it does not"
    );
    assert!(changed.get("plain").unwrap().to_json()["ttl"].is_null());
}

/// UC: UC-RET-015
#[tokio::test]
async fn the_dead_letter_suffix_is_configurable_and_becomes_the_reserved_name() {
    let env = MapEnv::new([
        ("TABA_REPLICATION_FACTOR", "1"),
        ("TABA_TOPICS_DEAD_LETTER_SUFFIX", "-dlq"),
    ]);
    let rules = TopicRules::from_config(&Config::parse(None, &env).unwrap());
    assert_eq!(rules.dead_letter_suffix, "-dlq");
    let h = harness_with(rules);
    h.topics.create(queue("orders")).await.unwrap();

    // The new suffix is reserved; the old one is an ordinary name again.
    assert_eq!(
        code(h.topics.create(queue("x-dlq"))).await,
        ErrorCode::ValidationReservedName
    );
    h.topics.create(queue("y-dead-letter")).await.unwrap();

    // The dead-letter appears under the new name and no longer under the old one.
    let dl = h.topics.get("orders-dlq").unwrap().to_json();
    assert_eq!(dl["deadLetter"], true);
    assert_eq!(dl["source"], "orders");
    assert_eq!(
        code(async { h.topics.get("orders-dead-letter") }).await,
        ErrorCode::TopicNotFound
    );
}

/// UC: UC-CLR-006
#[tokio::test]
async fn the_cluster_definition_is_the_topic_json_without_effective_dead_letter_and_source() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    h.topics
        .create(distributor("dist", &["ped"]))
        .await
        .unwrap();
    let stored = h.topics.stored().await.unwrap();

    for name in ["orders", "orders-dead-letter", "dist"] {
        let view = h.topics.get(name).unwrap();
        let mut api = view.to_json();
        let object = api.as_object_mut().unwrap();
        for derived in ["effective", "deadLetter", "source"] {
            object.remove(derived);
        }
        assert_eq!(
            api,
            view.definition.to_json(),
            "{name}: the Topic fields, without what is derived"
        );
    }

    // What the nodes exchange (and the table keeps) is exactly that JSON, and it reads back to the definition.
    let orders = stored.iter().find(|d| d.name == "orders").unwrap();
    let wire = orders.to_json();
    for derived in ["effective", "deadLetter", "source"] {
        assert!(wire.get(derived).is_none(), "{derived}");
    }
    assert_eq!(&TopicDefinition::from_json(&wire).unwrap(), orders);
    assert!(h.topics.get("orders").unwrap().to_json().get("effective").is_some());
    let dead = h.topics.get("orders-dead-letter").unwrap().to_json();
    assert_eq!(dead["deadLetter"], true);
    assert_eq!(dead["source"], "orders");
}
