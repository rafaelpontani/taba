//! Volatile queue and `autoCreate` (doc 15, sections 14.3 and 14.4).

mod common;

use std::time::Duration;

use common::{TestNode, error_code};
use reqwest::{Method, Response};
use serde_json::{Value, json};

const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";

async fn consume(node: &TestNode, path_and_query: &str) -> Response {
    node.client_get(path_and_query).await
}

async fn topic(node: &TestNode, name: &str) -> Response {
    node.client_get(&format!("/topics/{name}")).await
}

async fn wait_until_gone(node: &TestNode, name: &str) {
    for _ in 0..120 {
        if topic(node, name).await.status() == 404 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("queue `{name}` was not deleted");
}

/// UC: UC-CNS-031
#[tokio::test]
async fn auto_create_on_get_makes_a_volatile_queue_and_the_get_counts() {
    let node = TestNode::development().await;
    let response = consume(&node, "/topics/new/messages?autoCreate=true").await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({ "messages": [] })
    );
    let created: Value = topic(&node, "new").await.json().await.unwrap();
    assert_eq!(created["type"], "queue");
    assert_eq!(created["volatile"], true);
    assert_eq!(created["idleTimeout"], 600);
    node.shutdown().await;
}

/// UC: UC-CNS-032
#[tokio::test]
async fn auto_create_takes_idle_timeout_fifo_volatile_and_tags_from_the_query() {
    let node = TestNode::development().await;
    let response = consume(
        &node,
        "/topics/a/messages?autoCreate=true&idleTimeout=300&fifo=true&tag.time=financeiro&tag.env=prod",
    )
    .await;
    assert_eq!(response.status(), 200);
    let a: Value = topic(&node, "a").await.json().await.unwrap();
    assert_eq!(
        (a["idleTimeout"].as_u64(), a["fifo"].as_bool()),
        (Some(300), Some(true))
    );
    assert_eq!(a["tags"], json!({ "time": "financeiro", "env": "prod" }));

    consume(&node, "/topics/b/messages?autoCreate=true&volatile=false").await;
    let b: Value = topic(&node, "b").await.json().await.unwrap();
    assert_eq!(b["volatile"], false);
    assert!(b.get("idleTimeout").is_none());
    node.shutdown().await;
}

/// UC: UC-CNS-033, UC-CNS-034, UC-CNS-035
#[tokio::test]
async fn auto_create_is_idempotent_never_makes_a_distributor_and_refuses_bad_names() {
    let node = TestNode::development().await;
    consume(
        &node,
        "/topics/queue/messages?autoCreate=true&idleTimeout=300",
    )
    .await;
    // Again, with other parameters: ignored.
    let again = consume(
        &node,
        "/topics/queue/messages?autoCreate=true&idleTimeout=900&fifo=true",
    )
    .await;
    assert_eq!(again.status(), 200);
    let queue: Value = topic(&node, "queue").await.json().await.unwrap();
    assert_eq!(
        (queue["idleTimeout"].as_u64(), queue["fifo"].as_bool()),
        (Some(300), Some(false))
    );

    let made = node
        .client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": "dist", "type": "distributor", "prefixes": ["a-"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(made.status(), 201);
    assert_eq!(
        error_code(
            consume(&node, "/topics/dist/messages?autoCreate=true").await,
            409
        )
        .await,
        "topic.wrong-type"
    );
    assert_eq!(
        error_code(
            consume(&node, "/topics/x-dead-letter/messages?autoCreate=true").await,
            400
        )
        .await,
        "validation.reserved-name"
    );
    assert_eq!(
        error_code(
            consume(&node, "/topics/with%20space%C3%A7/messages?autoCreate=true").await,
            400
        )
        .await,
        "validation.invalid-name"
    );
    assert_eq!(
        error_code(consume(&node, "/topics/nothing/messages").await, 404).await,
        "topic.not-found",
        "without autoCreate"
    );
    assert_eq!(
        error_code(
            consume(&node, "/topics/nothing/messages?autoCreate=maybe").await,
            400
        )
        .await,
        "validation.failed"
    );
    node.shutdown().await;
}

/// UC: UC-CNS-036
#[tokio::test]
async fn the_volatile_ceiling_refuses_auto_create_with_422() {
    let node = TestNode::builder()
        .env("TABA_TOPICS_VOLATILE_MAX_COUNT", "1")
        .start()
        .await;
    assert_eq!(
        consume(&node, "/topics/a/messages?autoCreate=true")
            .await
            .status(),
        200
    );
    assert_eq!(
        error_code(
            consume(&node, "/topics/b/messages?autoCreate=true").await,
            422
        )
        .await,
        "topic.volatile-limit"
    );
    assert_eq!(
        consume(&node, "/topics/a/messages?autoCreate=true")
            .await
            .status(),
        200
    );
    node.shutdown().await;
}

/// UC: UC-VOL-001, UC-VOL-002, UC-VOL-003
#[tokio::test]
async fn a_queue_without_consumers_is_deleted_by_the_sweeper_and_a_get_postpones_it() {
    let node = TestNode::development().await;
    consume(
        &node,
        "/topics/efemera/messages?autoCreate=true&idleTimeout=60",
    )
    .await;
    consume(
        &node,
        "/topics/vizinha/messages?autoCreate=true&idleTimeout=3600",
    )
    .await;
    let publish = |name: &'static str| {
        node.client
            .post(node.client_url(&format!("/topics/{name}/messages")))
            .header("content-type", "application/json")
            .body("1")
            .send()
    };
    assert_eq!(publish("efemera").await.unwrap().status(), 201);

    node.clock.advance(Duration::from_secs(59));
    consume(&node, "/topics/efemera/messages").await; // adia o prazo
    node.clock.advance(Duration::from_secs(59));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        topic(&node, "efemera").await.status(),
        200,
        "the GET postponed the deletion"
    );

    node.clock.advance(Duration::from_secs(2));
    wait_until_gone(&node, "efemera").await;
    assert_eq!(
        error_code(consume(&node, "/topics/efemera/messages").await, 404).await,
        "topic.not-found"
    );
    assert_eq!(
        error_code(
            consume(&node, "/topics/efemera-dead-letter/messages").await,
            404
        )
        .await,
        "topic.not-found",
        "the dead-letter goes along"
    );
    assert_eq!(
        topic(&node, "vizinha").await.status(),
        200,
        "the other one did not expire"
    );
    node.shutdown().await;
}

/// UC: UC-VOL-004, UC-VOL-005
#[tokio::test]
async fn publishing_and_reading_the_dead_letter_do_not_keep_a_volatile_queue_alive() {
    let node = TestNode::development().await;
    consume(
        &node,
        "/topics/queue/messages?autoCreate=true&idleTimeout=60",
    )
    .await;
    node.clock.advance(Duration::from_secs(40));
    node.client
        .post(node.client_url("/topics/queue/messages"))
        .header("content-type", "application/json")
        .body("1")
        .send()
        .await
        .unwrap();
    assert_eq!(
        consume(&node, "/topics/queue-dead-letter/messages")
            .await
            .status(),
        200
    );
    node.clock.advance(Duration::from_secs(20));
    wait_until_gone(&node, "queue").await;
    node.shutdown().await;
}

/// UC: UC-VOL-021
#[tokio::test]
async fn volatile_and_the_type_are_immutable_but_the_idle_timeout_can_change() {
    let node = TestNode::development().await;
    consume(
        &node,
        "/topics/queue/messages?autoCreate=true&idleTimeout=60",
    )
    .await;
    let patch = |body: Value| {
        node.client
            .patch(node.client_url("/topics/queue"))
            .json(&body)
            .send()
    };
    assert_eq!(
        patch(json!({ "idleTimeout": 3600 }))
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        error_code(patch(json!({ "volatile": false })).await.unwrap(), 400).await,
        "validation.immutable-field"
    );
    node.clock.advance(Duration::from_secs(120));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        topic(&node, "queue").await.status(),
        200,
        "the new deadline counts"
    );
    node.shutdown().await;
}

/// UC: UC-VOL-001
#[tokio::test]
async fn an_ordinary_queue_never_expires() {
    let node = TestNode::development().await;
    node.client
        .post(node.client_url("/topics"))
        .json(&json!({ "name": "orders", "type": "queue" }))
        .send()
        .await
        .unwrap();
    node.clock.advance(Duration::from_secs(30 * 86_400));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(topic(&node, "orders").await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-DRN-023
#[tokio::test]
async fn a_resumed_node_gives_the_volatile_queue_a_fresh_idle_period() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("taba").display().to_string();
    let start = || {
        TestNode::builder()
            .env("TABA_STORAGE_DIR", &storage)
            .try_start()
    };
    let node = start().await.unwrap();
    consume(
        &node,
        "/topics/queue/messages?autoCreate=true&idleTimeout=60",
    )
    .await;
    node.shutdown().await;

    // The new node's clock starts at another instant: the queue's creation is left in the past.
    let node = start().await.unwrap();
    node.clock.advance(Duration::from_secs(3600));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    // The manual clock advanced after the start: only then does the 60 s deadline count.
    wait_until_gone(&node, "queue").await;
    node.shutdown().await;
}

fn auth_builder() -> common::TestNodeBuilder {
    TestNode::builder()
        .production()
        .env("TABA_AUTH_ROOT_TOKEN", ROOT)
}

async fn make_token(node: &TestNode, permissions: &[&str], prefixes: &[&str]) -> String {
    let created: Value = node
        .client
        .request(Method::POST, node.client_url("/tokens"))
        .header("authorization", format!("Bearer {ROOT}"))
        .json(&json!({ "name": "t", "permissions": permissions, "topicPrefixes": prefixes }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    created["token"].as_str().unwrap().to_owned()
}

async fn get_with(node: &TestNode, path: &str, token: &str) -> Response {
    node.client
        .get(node.client_url(path))
        .header("authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
}

/// UC: UC-CNS-040
#[tokio::test]
async fn auto_create_needs_consume_and_create_but_a_plain_consume_only_needs_consume() {
    let node = auth_builder().start().await;
    let consumer = make_token(&node, &["consume"], &[]).await;
    let both = make_token(&node, &["consume", "create"], &[]).await;
    let only_create = make_token(&node, &["create"], &[]).await;

    // Without `create`, the `autoCreate` that would create the queue is `403`, and nothing is created.
    assert_eq!(
        error_code(
            get_with(&node, "/topics/new/messages?autoCreate=true", &consumer).await,
            403
        )
        .await,
        "auth.forbidden"
    );
    assert_eq!(get_with(&node, "/topics/new", ROOT).await.status(), 404);

    // With `consume` and `create`, it creates.
    assert_eq!(
        get_with(&node, "/topics/new/messages?autoCreate=true", &both)
            .await
            .status(),
        200
    );
    assert_eq!(get_with(&node, "/topics/new", ROOT).await.status(), 200);
    // The queue already exists: the normal consumption only asks for `consume`.
    assert_eq!(
        get_with(&node, "/topics/new/messages?autoCreate=true", &consumer)
            .await
            .status(),
        200
    );
    // Only `create` does not consume: the route requires `consume`.
    assert_eq!(
        error_code(
            get_with(&node, "/topics/new/messages?autoCreate=true", &only_create).await,
            403
        )
        .await,
        "auth.forbidden"
    );
    node.shutdown().await;
}

/// UC: UC-CNS-040, UC-AUT-012
#[tokio::test]
async fn auto_create_stays_inside_the_token_prefixes() {
    let node = auth_builder().start().await;
    let token = make_token(&node, &["consume", "create"], &["orders"]).await;
    assert_eq!(
        get_with(&node, "/topics/orders-br/messages?autoCreate=true", &token)
            .await
            .status(),
        200
    );
    assert_eq!(
        error_code(
            get_with(&node, "/topics/estoque/messages?autoCreate=true", &token).await,
            403
        )
        .await,
        "auth.out-of-scope"
    );
    assert_eq!(get_with(&node, "/topics/estoque", ROOT).await.status(), 404);
    node.shutdown().await;
}
