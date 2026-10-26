//! What the node database (`_node.db`) keeps: the definitions and the tokens, and nothing about
//! consumption (doc 02, sections 3.3 and 3.4).

mod common;

use common::TestNode;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";

fn open_db(node: &TestNode) -> Connection {
    Connection::open_with_flags(
        node.storage_dir().join("_node.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

async fn post(node: &TestNode, path: &str, body: Value) -> Value {
    let response = node
        .client
        .post(node.client_url(path))
        .header("authorization", format!("Bearer {ROOT}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{path}");
    response.json().await.unwrap()
}

/// UC: UC-DEF-010
#[tokio::test]
async fn the_file_keeps_only_the_definition_and_never_consumption_data() {
    let node = TestNode::builder()
        .production()
        .env("TABA_AUTH_ROOT_TOKEN", ROOT)
        .start()
        .await;
    post(
        &node,
        "/topics",
        json!({ "name": "orders", "type": "queue" }),
    )
    .await;
    for i in 0..3 {
        let response = node
            .client
            .post(node.client_url("/topics/orders/messages"))
            .header("authorization", format!("Bearer {ROOT}"))
            .header("content-type", "application/json")
            .body(i.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
    }
    let taken = node
        .client
        .get(node.client_url("/topics/orders/messages?maxMessages=1"))
        .header("authorization", format!("Bearer {ROOT}"))
        .send()
        .await
        .unwrap();
    assert_eq!(taken.status(), 200);

    let db = open_db(&node);
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        tables,
        ["definition_versions", "losses", "state", "tokens", "topics"],
        "no table for messages, depth or rates"
    );
    let definition: String = db
        .query_row(
            "SELECT definition FROM topics WHERE name = 'orders'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let definition: Value = serde_json::from_str(&definition).unwrap();
    for key in definition.as_object().unwrap().keys() {
        let lower = key.to_lowercase();
        for consumption in ["depth", "rate", "histogram", "inflight", "oldest", "stats"] {
            assert!(
                !lower.contains(consumption),
                "`{key}` is consumption data in the definition"
            );
        }
    }
    node.shutdown().await;
}

/// UC: UC-DEF-012
#[tokio::test]
async fn each_token_is_a_row_with_the_salted_hash_and_never_the_secret() {
    let node = TestNode::builder()
        .production()
        .env("TABA_AUTH_ROOT_TOKEN", ROOT)
        .start()
        .await;
    let created = post(
        &node,
        "/tokens",
        json!({ "name": "service", "permissions": ["publish"], "topicPrefixes": ["orders"] }),
    )
    .await;
    let id = created["id"].as_str().unwrap();
    let full = created["token"].as_str().unwrap();

    let db = open_db(&node);
    let columns: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('tokens') ORDER BY cid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        [
            "id",
            "name",
            "permissions",
            "topic_prefixes",
            "created_at",
            "expires_at",
            "salt",
            "secret_hash"
        ]
    );
    let row: Vec<Option<String>> = db
        .query_row(
            "SELECT id, name, permissions, topic_prefixes, CAST(created_at AS TEXT), \
             CAST(expires_at AS TEXT), salt, secret_hash FROM tokens WHERE id = ?1",
            [id],
            |row| (0..8).map(|i| row.get(i)).collect(),
        )
        .unwrap();
    assert_eq!(row[1].as_deref(), Some("service"));
    assert_eq!(row[2].as_deref(), Some("publish"));
    assert!(row[6].as_deref().is_some_and(|s| !s.is_empty()), "the salt");
    assert!(row[7].as_deref().is_some_and(|s| !s.is_empty()), "the hash");
    let secret = full.rsplit('_').next().unwrap();
    for value in row.iter().flatten() {
        assert!(!value.contains(secret), "the secret is not in the row");
        assert_ne!(value, full);
    }
    node.shutdown().await;
}
