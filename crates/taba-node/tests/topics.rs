//! `POST`, `GET`, `PATCH` e `DELETE /topics` contra um nó de verdade (doc 06a).

mod common;

use std::collections::BTreeSet;

use common::{TestNode, error_code};
use reqwest::Response;
use serde_json::{Value, json};

async fn create(node: &TestNode, body: Value) -> Response {
    node.client
        .post(node.client_url("/topics"))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn patch(node: &TestNode, name: &str, body: Value) -> Response {
    node.client
        .patch(node.client_url(&format!("/topics/{name}")))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn delete(node: &TestNode, name: &str) -> Response {
    node.client
        .delete(node.client_url(&format!("/topics/{name}")))
        .send()
        .await
        .unwrap()
}

async fn created(node: &TestNode, body: Value) -> Value {
    let response = create(node, body).await;
    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

async fn body(response: Response) -> Value {
    response.json().await.unwrap()
}

fn names(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap().to_owned())
        .collect()
}

/// UC: UC-TOP-001
#[tokio::test]
async fn creating_a_queue_returns_201_with_location_and_the_defaults() {
    let node = TestNode::development().await;
    let response = create(&node, json!({ "name": "pedidos", "type": "queue" })).await;

    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["location"], "/topics/pedidos");
    let topic = body(response).await;
    assert!(taba_node::core::id::is_guid_v7(
        topic["id"].as_str().unwrap()
    ));
    assert_eq!(topic["name"], "pedidos");
    assert_eq!(topic["type"], "queue");
    assert_eq!(topic["tags"], json!({}));
    assert_eq!(topic["volatile"], false);
    assert_eq!(topic["fifo"], false);
    assert_eq!(topic["createdAt"], "2026-10-06T12:00:00.000Z");
    assert_eq!(
        topic["effective"],
        json!({ "maxMessageBytes": 5_242_880, "ttl": 1_209_600, "maxDeliveries": 10 })
    );
    node.shutdown().await;
}

/// UC: UC-TOP-002, UC-TOP-017
#[tokio::test]
async fn a_distributor_shows_prefixes_and_none_of_the_queue_fields() {
    let node = TestNode::development().await;
    let topic = created(
        &node,
        json!({ "name": "dist", "type": "distributor", "prefixes": ["pedidos"] }),
    )
    .await;
    assert_eq!(topic["prefixes"], json!(["pedidos"]));
    for absent in [
        "volatile",
        "fifo",
        "ttl",
        "maxDeliveries",
        "maxMessageBytes",
        "effective",
    ] {
        assert!(topic.get(absent).is_none(), "{absent}");
    }

    let empty = created(&node, json!({ "name": "vazio", "type": "distributor" })).await;
    assert_eq!(empty["prefixes"], json!([]));
    node.shutdown().await;
}

/// UC: UC-TOP-003, UC-TOP-004, UC-TOP-005
#[tokio::test]
async fn volatile_fifo_and_own_limits_come_back_as_defined() {
    let node = TestNode::development().await;
    let v = created(
        &node,
        json!({ "name": "v", "type": "queue", "volatile": true }),
    )
    .await;
    assert_eq!(v["idleTimeout"], 600);
    let w = created(
        &node,
        json!({ "name": "w", "type": "queue", "volatile": true, "idleTimeout": 120 }),
    )
    .await;
    assert_eq!(w["idleTimeout"], 120);

    let f = created(&node, json!({ "name": "f", "type": "queue", "fifo": true })).await;
    assert_eq!(f["fifo"], true);

    let own = created(
        &node,
        json!({ "name": "o", "type": "queue", "ttl": 86400, "maxDeliveries": 3, "maxMessageBytes": 1000 }),
    )
    .await;
    assert_eq!(
        (
            own["ttl"].clone(),
            own["maxDeliveries"].clone(),
            own["maxMessageBytes"].clone()
        ),
        (json!(86400), json!(3), json!(1000))
    );
    assert_eq!(
        own["effective"],
        json!({ "maxMessageBytes": 1000, "ttl": 86400, "maxDeliveries": 3 })
    );
    node.shutdown().await;
}

/// UC: UC-TOP-006, UC-TOP-022, UC-TOP-027
#[tokio::test]
async fn every_queue_comes_with_a_read_only_dead_letter_in_the_listing() {
    let node = TestNode::development().await;
    created(
        &node,
        json!({ "name": "pedidos", "type": "queue", "ttl": 500, "tags": { "time": "fin" } }),
    )
    .await;
    created(&node, json!({ "name": "dist", "type": "distributor" })).await;

    let page = body(node.client_get("/topics").await).await;
    assert_eq!(names(&page), ["dist", "pedidos", "pedidos-dead-letter"]);
    assert!(page.get("nextCursor").is_none());

    let dl = body(node.client_get("/topics/pedidos-dead-letter").await).await;
    assert_eq!(dl["deadLetter"], true);
    assert_eq!(dl["source"], "pedidos");
    assert_eq!(dl["ttl"], 500);
    assert_eq!(dl["tags"], json!({ "time": "fin" }));
    node.shutdown().await;
}

/// UC: UC-TOP-008, UC-TOP-009, UC-TOP-010
#[tokio::test]
async fn invalid_names_and_tags_are_400_with_the_field_in_details() {
    let node = TestNode::development().await;
    for (name, code) in [
        ("", "validation.invalid-name"),
        ("com espaço", "validation.invalid-name"),
        ("a/b", "validation.invalid-name"),
        ("acentuação", "validation.invalid-name"),
        ("pedidos-dead-letter", "validation.reserved-name"),
    ] {
        let response = create(&node, json!({ "name": name, "type": "queue" })).await;
        assert_eq!(error_code(response, 400).await, code, "{name:?}");
    }
    let long = "a".repeat(101);
    let response = create(&node, json!({ "name": long, "type": "queue" })).await;
    assert_eq!(error_code(response, 400).await, "validation.invalid-name");
    assert_eq!(
        create(&node, json!({ "name": "a".repeat(100), "type": "queue" }))
            .await
            .status(),
        201
    );

    let response = create(
        &node,
        json!({ "name": "t", "type": "queue", "tags": { "k": "valor inválido" } }),
    )
    .await;
    assert_eq!(response.status(), 400);
    let error = body(response).await;
    assert_eq!(error["code"], "validation.failed");
    assert_eq!(error["details"][0]["field"], "tags.k");
    node.shutdown().await;
}

/// UC: UC-TOP-011, UC-TOP-012, UC-TOP-020
#[tokio::test]
async fn names_are_case_sensitive_duplicates_conflict_and_recreating_changes_the_id() {
    let node = TestNode::development().await;
    let lower = created(&node, json!({ "name": "pedidos", "type": "queue" })).await;
    let upper = created(&node, json!({ "name": "Pedidos", "type": "queue" })).await;
    assert_ne!(lower["id"], upper["id"]);

    let again = create(&node, json!({ "name": "pedidos", "type": "queue" })).await;
    assert_eq!(error_code(again, 409).await, "topic.already-exists");

    assert_eq!(delete(&node, "pedidos").await.status(), 204);
    let recreated = created(&node, json!({ "name": "pedidos", "type": "queue" })).await;
    assert_ne!(recreated["id"], lower["id"]);
    node.shutdown().await;
}

/// UC: UC-TOP-013, UC-TOP-014
#[tokio::test]
async fn fields_out_of_place_or_out_of_range_are_400() {
    let node = TestNode::development().await;
    let cases = [
        (
            json!({ "name": "d", "type": "distributor", "ttl": 100 }),
            "validation.not-applicable",
        ),
        (
            json!({ "name": "d", "type": "distributor", "fifo": false }),
            "validation.not-applicable",
        ),
        (
            json!({ "name": "q", "type": "queue", "prefixes": ["a"] }),
            "validation.not-applicable",
        ),
        (
            json!({ "name": "q", "type": "queue", "ttl": 59 }),
            "validation.out-of-range",
        ),
        (
            json!({ "name": "q", "type": "queue", "ttl": 31_536_001 }),
            "validation.out-of-range",
        ),
        (
            json!({ "name": "q", "type": "queue", "maxDeliveries": 0 }),
            "validation.out-of-range",
        ),
        (
            json!({ "name": "q", "type": "queue", "maxDeliveries": 1001 }),
            "validation.out-of-range",
        ),
        (
            json!({ "name": "q", "type": "queue", "volatile": true, "idleTimeout": 59 }),
            "validation.out-of-range",
        ),
        (
            json!({ "name": "q", "type": "queue", "volatile": true, "idleTimeout": 604_801 }),
            "validation.out-of-range",
        ),
        (
            json!({ "name": "q", "type": "queue", "ttl": -1 }),
            "validation.out-of-range",
        ),
    ];
    for (request, code) in cases {
        let response = create(&node, request.clone()).await;
        assert_eq!(error_code(response, 400).await, code, "{request}");
    }
    for ok in [
        json!({ "name": "a", "type": "queue", "ttl": 60 }),
        json!({ "name": "b", "type": "queue", "ttl": 31_536_000 }),
        json!({ "name": "c", "type": "queue", "maxDeliveries": 1000 }),
        json!({ "name": "d", "type": "queue", "volatile": true, "idleTimeout": 604_800 }),
    ] {
        assert_eq!(create(&node, ok).await.status(), 201);
    }
    node.shutdown().await;
}

/// UC: UC-TOP-015
#[tokio::test]
async fn max_message_bytes_can_pass_the_default_but_not_the_hard_limit() {
    let node = TestNode::builder()
        .env("TABA_MESSAGE_HARD_LIMIT", "10MB")
        .start()
        .await;
    let big = created(
        &node,
        json!({ "name": "big", "type": "queue", "maxMessageBytes": 8_000_000 }),
    )
    .await;
    assert_eq!(big["effective"]["maxMessageBytes"], 8_000_000);

    let response = create(
        &node,
        json!({ "name": "huge", "type": "queue", "maxMessageBytes": 10_485_761 }),
    )
    .await;
    assert_eq!(
        error_code(response, 400).await,
        "validation.max-bytes-above-hard-limit"
    );
    node.shutdown().await;
}

/// UC: UC-TOP-016
#[tokio::test]
async fn distributor_prefixes_follow_the_limits() {
    let node = TestNode::development().await;
    let too_many: Vec<String> = (0..21).map(|i| format!("p{i}")).collect();
    for prefixes in [
        json!(too_many),
        json!([""]),
        json!(["a b"]),
        json!(["a".repeat(101)]),
    ] {
        let response = create(
            &node,
            json!({ "name": "d", "type": "distributor", "prefixes": prefixes }),
        )
        .await;
        assert_eq!(response.status(), 400, "{prefixes}");
    }
    node.shutdown().await;
}

/// UC: UC-TOP-018, UC-CNV-009
#[tokio::test]
async fn unknown_and_missing_fields_are_400_naming_the_field() {
    let node = TestNode::development().await;

    let response = create(
        &node,
        json!({ "name": "a", "type": "queue", "fifoo": true }),
    )
    .await;
    assert_eq!(response.status(), 400);
    let error = body(response).await;
    assert_eq!(error["code"], "validation.unknown-field");
    assert_eq!(error["details"][0]["field"], "fifoo");

    let response = create(&node, json!({})).await;
    assert_eq!(response.status(), 400);
    let error = body(response).await;
    assert_eq!(error["code"], "validation.required");
    let fields: BTreeSet<_> = error["details"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["field"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(fields, BTreeSet::from(["name".into(), "type".into()]));

    let response = create(&node, json!({ "name": "a", "type": "outro" })).await;
    assert_eq!(error_code(response, 400).await, "validation.failed");
    let response = create(&node, json!({ "name": 5, "type": "queue" })).await;
    assert_eq!(error_code(response, 400).await, "validation.failed");
    node.shutdown().await;
}

/// UC: UC-CNV-003
#[tokio::test]
async fn a_json_route_with_another_content_type_is_415() {
    let node = TestNode::development().await;
    let response = node
        .client
        .post(node.client_url("/topics"))
        .header("content-type", "text/plain")
        .body(r#"{"name":"a","type":"queue"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(error_code(response, 415).await, "media.unsupported");

    let with_charset = node
        .client
        .post(node.client_url("/topics"))
        .header("content-type", "Application/JSON; charset=utf-8")
        .body(r#"{"name":"a","type":"queue"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(with_charset.status(), 201);
    node.shutdown().await;
}

/// UC: UC-TOP-021
#[tokio::test]
async fn listing_without_topics_is_an_empty_page() {
    let node = TestNode::development().await;
    let response = node.client_get("/topics").await;
    assert_eq!(response.status(), 200);
    assert_eq!(body(response).await, json!({ "items": [] }));
    node.shutdown().await;
}

/// UC: UC-TOP-023, UC-TOP-024
#[tokio::test]
async fn listing_sorts_and_filters() {
    let node = TestNode::development().await;
    for name in ["b", "a", "c"] {
        created(&node, json!({ "name": name, "type": "distributor" })).await;
        node.clock.advance(std::time::Duration::from_millis(10));
    }
    let listed = |query: &'static str| {
        let node = &node;
        async move { names(&body(node.client_get(&format!("/topics?{query}")).await).await) }
    };
    assert_eq!(listed("sort=name").await, ["a", "b", "c"]);
    assert_eq!(listed("sort=-name").await, ["c", "b", "a"]);
    assert_eq!(listed("sort=createdAt").await, ["b", "a", "c"]);
    assert_eq!(listed("sort=-createdAt").await, ["c", "a", "b"]);
    assert_eq!(listed("sort=id").await, ["b", "a", "c"]);
    assert_eq!(listed("sort=-id").await, ["c", "a", "b"]);

    let response = node.client_get("/topics?sort=tamanho").await;
    assert_eq!(error_code(response, 400).await, "validation.failed");

    created(
        &node,
        json!({ "name": "pedidos-eu", "type": "distributor" }),
    )
    .await;
    created(
        &node,
        json!({ "name": "Pedidos-us", "type": "distributor" }),
    )
    .await;
    assert_eq!(listed("name=pedidos").await, ["pedidos-eu"]);
    assert_eq!(listed("name=-").await, ["Pedidos-us", "pedidos-eu"]);
    node.shutdown().await;
}

/// UC: UC-TOP-026, UC-TOP-037
#[tokio::test]
async fn reading_a_topic_returns_it_and_an_unknown_one_is_404() {
    let node = TestNode::development().await;
    let made = created(&node, json!({ "name": "pedidos", "type": "queue" })).await;
    assert_eq!(body(node.client_get("/topics/pedidos").await).await, made);

    let response = node.client_get("/topics/nada").await;
    assert_eq!(error_code(response, 404).await, "topic.not-found");
    assert_eq!(
        error_code(patch(&node, "nada", json!({})).await, 404).await,
        "topic.not-found"
    );
    node.shutdown().await;
}

/// UC: UC-TOP-028, UC-TOP-029
#[tokio::test]
async fn patch_is_partial_tags_replace_and_null_goes_back_to_the_default() {
    let node = TestNode::development().await;
    created(
        &node,
        json!({ "name": "p", "type": "queue", "tags": { "a": "1" }, "ttl": 100, "maxDeliveries": 4 }),
    )
    .await;

    let response = patch(&node, "p", json!({ "tags": { "b": "2" } })).await;
    assert_eq!(response.status(), 200);
    let topic = body(response).await;
    assert_eq!(topic["tags"], json!({ "b": "2" }));
    assert_eq!(topic["ttl"], 100);
    assert_eq!(topic["maxDeliveries"], 4);

    let topic = body(patch(&node, "p", json!({ "ttl": null })).await).await;
    assert!(topic["ttl"].is_null());
    assert_eq!(topic["effective"]["ttl"], 1_209_600);
    assert_eq!(topic["maxDeliveries"], 4);

    // O que mudou vale também para quem lê depois.
    let read = body(node.client_get("/topics/p").await).await;
    assert_eq!(read, topic);
    node.shutdown().await;
}

/// UC: UC-TOP-032
#[tokio::test]
async fn patching_distributor_prefixes_replaces_them() {
    let node = TestNode::development().await;
    created(
        &node,
        json!({ "name": "d", "type": "distributor", "prefixes": ["a"] }),
    )
    .await;
    let topic = body(patch(&node, "d", json!({ "prefixes": ["x", "y"] })).await).await;
    assert_eq!(topic["prefixes"], json!(["x", "y"]));
    node.shutdown().await;
}

/// UC: UC-TOP-034, UC-TOP-035, UC-TOP-036
#[tokio::test]
async fn patch_refuses_immutable_misplaced_and_dead_letter_changes() {
    let node = TestNode::development().await;
    created(&node, json!({ "name": "q", "type": "queue" })).await;
    created(&node, json!({ "name": "d", "type": "distributor" })).await;

    for body_ in [
        json!({ "name": "q" }),
        json!({ "id": "x" }),
        json!({ "type": "queue" }),
        json!({ "volatile": false }),
        json!({ "fifo": false }),
    ] {
        let response = patch(&node, "q", body_.clone()).await;
        assert_eq!(
            error_code(response, 400).await,
            "validation.immutable-field",
            "{body_}"
        );
    }
    assert_eq!(
        error_code(patch(&node, "d", json!({ "ttl": 100 })).await, 400).await,
        "validation.not-applicable"
    );
    assert_eq!(
        error_code(patch(&node, "q", json!({ "ttl": 10 })).await, 400).await,
        "validation.out-of-range"
    );
    assert_eq!(
        error_code(patch(&node, "q", json!({ "ttll": 10 })).await, 400).await,
        "validation.unknown-field"
    );
    assert_eq!(
        error_code(
            patch(&node, "q-dead-letter", json!({ "tags": {} })).await,
            409
        )
        .await,
        "topic.read-only"
    );
    node.shutdown().await;
}

/// UC: UC-TOP-038, UC-TOP-040, UC-TOP-041, UC-TOP-042, UC-TOP-043
#[tokio::test]
async fn delete_removes_the_topic_and_its_dead_letter() {
    let node = TestNode::development().await;
    created(&node, json!({ "name": "q", "type": "queue" })).await;
    created(
        &node,
        json!({ "name": "d", "type": "distributor", "prefixes": ["q"] }),
    )
    .await;

    assert_eq!(
        error_code(delete(&node, "q-dead-letter").await, 409).await,
        "topic.read-only"
    );

    let response = delete(&node, "q").await;
    assert_eq!(response.status(), 204);
    assert!(response.bytes().await.unwrap().is_empty());
    for gone in ["q", "q-dead-letter"] {
        let response = node.client_get(&format!("/topics/{gone}")).await;
        assert_eq!(error_code(response, 404).await, "topic.not-found");
    }
    assert_eq!(
        error_code(delete(&node, "q").await, 404).await,
        "topic.not-found"
    );

    // Remover o distribuidor não afeta as filas que ele copiava.
    created(&node, json!({ "name": "q2", "type": "queue" })).await;
    assert_eq!(delete(&node, "d").await.status(), 204);
    assert_eq!(node.client_get("/topics/q2").await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-PAG-001, UC-PAG-002, UC-PAG-008
#[tokio::test]
async fn pages_carry_next_cursor_only_when_there_is_more_and_bad_params_are_400() {
    let node = TestNode::development().await;
    for name in ["a", "b", "c"] {
        created(&node, json!({ "name": name, "type": "distributor" })).await;
    }
    let first = body(node.client_get("/topics?limit=2").await).await;
    assert_eq!(names(&first), ["a", "b"]);
    let cursor = first["nextCursor"].as_str().unwrap();

    let last = body(
        node.client_get(&format!("/topics?limit=2&cursor={cursor}"))
            .await,
    )
    .await;
    assert_eq!(names(&last), ["c"]);
    assert!(
        last.get("nextCursor").is_none(),
        "na última página não existe"
    );
    for forbidden in ["total", "page", "offset"] {
        assert!(first.get(forbidden).is_none());
    }

    for bad in ["limit=0", "limit=1001", "limit=abc"] {
        let response = node.client_get(&format!("/topics?{bad}")).await;
        assert_eq!(error_code(response, 400).await, "validation.out-of-range");
    }
    let response = node.client_get("/topics?cursor=lixo").await;
    assert_eq!(error_code(response, 400).await, "validation.invalid-cursor");
    node.shutdown().await;
}

/// UC: UC-PAG-003, UC-PAG-005, UC-PAG-006
#[tokio::test]
async fn walking_the_pages_returns_everything_once_and_cursors_are_stable_and_bound_to_the_query() {
    let node = TestNode::development().await;
    for i in 0..7 {
        created(&node, json!({ "name": format!("t{i}"), "type": "queue" })).await;
    }

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut first_cursor = None;
    loop {
        let url = match &cursor {
            Some(c) => format!("/topics?limit=3&sort=-createdAt&cursor={c}"),
            None => "/topics?limit=3&sort=-createdAt".to_owned(),
        };
        let page = body(node.client_get(&url).await).await;
        seen.extend(names(&page));
        match page.get("nextCursor").and_then(Value::as_str) {
            Some(next) => {
                first_cursor.get_or_insert(next.to_owned());
                cursor = Some(next.to_owned());
            }
            None => break,
        }
    }
    assert_eq!(seen.len(), 14);
    assert_eq!(seen.iter().collect::<BTreeSet<_>>().len(), 14, "{seen:?}");

    // O mesmo cursor devolve a mesma página, sem estado no servidor.
    let url = format!(
        "/topics?limit=3&sort=-createdAt&cursor={}",
        first_cursor.clone().unwrap()
    );
    let one = body(node.client_get(&url).await).await;
    let two = body(node.client_get(&url).await).await;
    assert_eq!(one, two);

    // Com outro `sort` ou filtro, o cursor é recusado.
    let other_sort = format!(
        "/topics?limit=3&sort=name&cursor={}",
        first_cursor.clone().unwrap()
    );
    assert_eq!(
        error_code(node.client_get(&other_sort).await, 400).await,
        "validation.invalid-cursor"
    );
    let other_filter = format!(
        "/topics?limit=3&sort=-createdAt&name=t&cursor={}",
        first_cursor.unwrap()
    );
    assert_eq!(
        error_code(node.client_get(&other_filter).await, 400).await,
        "validation.invalid-cursor"
    );
    node.shutdown().await;
}

/// UC: UC-DEV-012, UC-DEF-009, UC-DEF-024, UC-ARM-017
#[tokio::test]
async fn definitions_are_saved_in_the_node_database_and_resumed_after_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("taba");
    let storage_str = storage.display().to_string();
    let start = || {
        TestNode::builder()
            .env("TABA_STORAGE_DIR", &storage_str)
            .try_start()
    };

    let node = start().await.unwrap();
    let made = created(
        &node,
        json!({ "name": "Pedidos", "type": "queue", "tags": { "a": "b" }, "ttl": 90 }),
    )
    .await;
    created(
        &node,
        json!({ "name": "pedidos", "type": "distributor", "prefixes": ["x"] }),
    )
    .await;
    assert!(storage.join("_node.db").is_file());
    let wal: String = rusqlite::Connection::open(storage.join("_node.db"))
        .unwrap()
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(wal, "wal");
    node.shutdown().await;

    let node = start().await.unwrap();
    assert_eq!(body(node.client_get("/topics/Pedidos").await).await, made);
    assert_eq!(node.client_get("/topics/pedidos").await.status(), 200);
    assert_eq!(delete(&node, "Pedidos").await.status(), 204);
    assert!(
        !storage.join("_topics").exists(),
        "nada de arquivo por tópico"
    );
    node.shutdown().await;

    // Sem `development.resume-on-start`, o nó começa limpo e apaga o disco.
    let node = TestNode::builder()
        .env("TABA_STORAGE_DIR", &storage_str)
        .env("TABA_DEVELOPMENT_RESUME_ON_START", "false")
        .start()
        .await;
    assert_eq!(
        body(node.client_get("/topics").await).await,
        json!({ "items": [] })
    );
    assert!(!storage.join("_topics").exists());
    assert!(
        storage.join("_node.db").is_file(),
        "o banco é recriado vazio"
    );
    node.shutdown().await;
}
