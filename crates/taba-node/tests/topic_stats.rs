//! `GET /metrics/topics`, `/metrics/topics/{topic}` e `/metrics/topics/{topic}/groups` (doc 06b,
//! doc 11).

mod common;

use std::time::Duration;

use common::{TestNode, error_code};
use reqwest::Response;
use serde_json::{Value, json};

async fn create(node: &TestNode, body: Value) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{body}");
}

async fn queue(node: &TestNode, name: &str) {
    create(node, json!({ "name": name, "type": "queue" })).await;
}

async fn publish(node: &TestNode, topic: &str, query: &str, body: &str) -> Response {
    node.client
        .post(node.client_url(&format!("/topics/{topic}/messages{query}")))
        .header("content-type", "text/plain")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

async fn take(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    node.client_get(&format!("/topics/{topic}/messages?{query}"))
        .await
        .json::<Value>()
        .await
        .unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

fn names(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect()
}

fn item<'a>(page: &'a Value, name: &str) -> &'a Value {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("no `{name}` in {page}"))
}

fn conditions(topic: &Value) -> Vec<&str> {
    topic["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["condition"].as_str().unwrap())
        .collect()
}

/// UC: UC-OPS-019, UC-OPS-024, UC-OPS-029
#[tokio::test]
async fn lists_the_topics_by_name_with_their_numbers_and_the_dead_letter_too() {
    let node = TestNode::single().await;
    create(
        &node,
        json!({ "name": "orders", "type": "queue", "tags": { "time": "fin" } }),
    )
    .await;
    queue(&node, "quiet").await;
    publish(&node, "orders", "", "a").await;
    publish(&node, "orders", "", "b").await;
    assert_eq!(
        take(&node, "orders", "visibilityTimeout=600&maxMessages=1")
            .await
            .len(),
        1
    );

    let page = node.stats("/metrics/topics").await;
    assert_eq!(
        names(&page),
        ["orders", "orders-dead-letter", "quiet", "quiet-dead-letter"]
    );
    assert!(page.get("nextCursor").is_none());

    let orders = item(&page, "orders");
    assert_eq!(orders["type"], "queue");
    assert_eq!(orders["tags"], json!({ "time": "fin" }));
    assert_eq!(orders["published"], 2);
    assert_eq!(orders["publishedBytes"], 2);
    assert_eq!(orders["publishErrors"], 0);
    assert_eq!(orders["consumed"], 1);
    assert_eq!(orders["acked"], 0);
    assert_eq!(orders["visibleMessages"], 1);
    assert_eq!(orders["invisibleMessages"], 1);
    assert_eq!(orders["depth"], 2);
    assert_eq!(orders["memoryMessages"], 2);
    assert_eq!(orders["diskMessages"], 0);
    assert_eq!(orders["memoryBytes"], 2);
    assert_eq!(orders["diskBytes"], 0);
    assert_eq!(orders["bytes"], 2);
    assert_eq!(orders["memoryShare"], 1.0);
    assert_eq!(orders["groups"], 1);
    assert_eq!(orders["worstCondition"], "ok");
    assert_eq!(orders["conditions"], json!([]));
    assert_eq!(orders["partial"], false);
    assert!(orders["publishRate"].as_f64().unwrap() > 0.0);
    assert!(orders["publishLatency"]["p50"].is_number());
    assert!(orders["consumeLatency"]["p99"].is_number());
    assert!(orders["oldestMessageAt"].is_string());
    assert!(orders["oldestAge"].as_f64().unwrap() >= 0.0);
    assert!(orders["collectedAt"].is_string());

    let empty = item(&page, "quiet");
    assert_eq!(empty["depth"], 0);
    assert!(empty.get("oldestMessageAt").is_none(), "never zero");
    assert!(empty.get("oldestAge").is_none());
    assert!(empty["publishLatency"]["p50"].is_null());

    let dead = item(&page, "orders-dead-letter");
    assert_eq!(dead["deadLetter"], true);
    assert_eq!(dead["source"], "orders");
    assert_eq!(
        dead["tags"],
        json!({ "time": "fin" }),
        "inherits the origin's tags"
    );
    node.shutdown().await;
}

/// UC: UC-OPS-019
#[tokio::test]
async fn pages_by_name_and_validates_the_parameters() {
    let node = TestNode::single().await;
    create(
        &node,
        json!({ "name": "a", "type": "queue", "tags": { "time": "fin" } }),
    )
    .await;
    queue(&node, "b").await;

    let mut seen = Vec::new();
    let mut path = "/metrics/topics?limit=1".to_owned();
    loop {
        let page = node.stats(&path).await;
        seen.extend(names(&page).into_iter().map(str::to_owned));
        match page.get("nextCursor") {
            Some(cursor) => {
                path = format!(
                    "/metrics/topics?limit=1&cursor={}",
                    cursor.as_str().unwrap()
                );
            }
            None => break,
        }
    }
    assert_eq!(seen, ["a", "a-dead-letter", "b", "b-dead-letter"]);

    let first = node.stats("/metrics/topics?limit=1").await;
    let cursor = first["nextCursor"].as_str().unwrap();
    let stale = node
        .client_get_ops(&format!(
            "/metrics/topics?limit=1&tag.time=fin&cursor={cursor}"
        ))
        .await;
    assert_eq!(error_code(stale, 400).await, "validation.invalid-cursor");

    let tagged = node.stats("/metrics/topics?tag.time=fin").await;
    assert_eq!(names(&tagged), ["a", "a-dead-letter"]);

    let narrow = node
        .stats("/metrics/topics?fields=depth,worstCondition")
        .await;
    let keys: Vec<&String> = narrow["items"][0].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["depth", "id", "name", "worstCondition"]);

    for (query, status, code) in [
        ("sort=name", 400, "validation.unknown-field"),
        ("fields=nothing", 400, "validation.unknown-field"),
        ("scope=mundo", 400, "validation.failed"),
        ("condition=nothing", 400, "validation.failed"),
        ("limit=0", 400, "validation.out-of-range"),
        ("cursor=@@", 400, "validation.invalid-cursor"),
    ] {
        let response = node
            .client_get_ops(&format!("/metrics/topics?{query}"))
            .await;
        assert_eq!(error_code(response, status).await, code, "{query}");
    }
    node.shutdown().await;
}

/// UC: UC-OPS-019
#[tokio::test]
async fn group_by_a_tag_gives_one_row_per_value_with_the_sums() {
    let node = TestNode::single().await;
    for (name, time) in [("a", "fin"), ("b", "fin"), ("c", "ops")] {
        create(
            &node,
            json!({ "name": name, "type": "queue", "tags": { "time": time } }),
        )
        .await;
    }
    queue(&node, "no-tag").await;
    publish(&node, "a", "", "x").await;
    publish(&node, "b", "", "yy").await;
    publish(&node, "b", "", "zz").await;
    publish(&node, "c", "", "w").await;

    let page = node.stats("/metrics/topics?groupBy=time").await;
    let rows = page["items"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        2,
        "the topic without the tag does not enter: {page}"
    );
    assert_eq!(rows[0]["tag"], "time");
    assert_eq!(rows[0]["value"], "fin");
    assert_eq!(rows[0]["topics"], 4, "a, b e as duas dead-letters");
    assert_eq!(rows[0]["published"], 3);
    assert_eq!(rows[0]["publishedBytes"], 5);
    assert_eq!(rows[0]["depth"], 3);
    assert!(rows[0]["oldestMessageAt"].is_string());
    assert_eq!(rows[1]["value"], "ops");
    assert_eq!(rows[1]["depth"], 1);

    let one = node
        .stats("/metrics/topics?groupBy=time&limit=1&fields=depth")
        .await;
    assert_eq!(one["items"][0].as_object().unwrap().len(), 4);
    assert!(one["nextCursor"].is_string());
    node.shutdown().await;
}

/// UC: UC-DST-018
#[tokio::test]
async fn a_distributor_only_shows_what_it_received_copied_and_failed_to_copy() {
    let node = TestNode::single().await;
    queue(&node, "orders-1").await;
    queue(&node, "orders-2").await;
    create(
        &node,
        json!({ "name": "orders-fifo", "type": "queue", "fifo": true }),
    )
    .await;
    create(
        &node,
        json!({ "name": "eventos", "type": "distributor", "prefixes": ["orders"] }),
    )
    .await;
    create(
        &node,
        json!({ "name": "none", "type": "distributor", "prefixes": ["zzz"] }),
    )
    .await;
    assert_eq!(publish(&node, "eventos", "", "a").await.status(), 200);
    assert_eq!(publish(&node, "none", "", "a").await.status(), 200);

    let eventos = node.stats("/metrics/topics/eventos").await;
    assert_eq!(
        eventos["distributor"],
        json!({ "received": 1, "copies": 2, "copyErrors": 1 })
    );
    assert!(
        eventos.get("depth").is_none(),
        "the queue metrics are the destination queues'"
    );
    assert!(eventos.get("topGroups").is_none());
    assert_eq!(eventos["worstCondition"], "ok");
    let none = node.stats("/metrics/topics/none").await;
    assert_eq!(
        none["distributor"],
        json!({ "received": 1, "copies": 0, "copyErrors": 0 })
    );
    let page = node.stats("/metrics/topics").await;
    assert_eq!(item(&page, "orders-1")["published"], 1);
    node.shutdown().await;
}

/// UC: UC-OPS-021
#[tokio::test]
async fn the_detail_adds_the_largest_groups_and_404s_for_unknown_topics() {
    let node = TestNode::builder()
        .env("TABA_STATS_TOPICS_TOP_GROUPS", "2")
        .start()
        .await;
    queue(&node, "orders").await;
    for _ in 0..5 {
        publish(&node, "orders", "?groupId=big", "a").await;
    }
    for _ in 0..3 {
        publish(&node, "orders", "?groupId=medio", "a").await;
    }
    publish(&node, "orders", "?groupId=small", "a").await;

    let detail = node.stats("/metrics/topics/orders").await;
    assert_eq!(detail["depth"], 9);
    assert_eq!(detail["groups"], 3);
    let top = detail["topGroups"].as_array().unwrap();
    assert_eq!(top.len(), 2, "stats.topics.top-groups");
    assert_eq!(top[0]["groupId"], "big");
    assert_eq!(top[0]["messages"], 5);
    assert_eq!(top[1]["groupId"], "medio");
    assert!(
        top[0].get("band").is_none(),
        "the range only comes with scope=node"
    );
    assert!((detail["noisyShare"].as_f64().unwrap() - 5.0 / 9.0).abs() < 1e-9);

    let local = node.stats("/metrics/topics/orders?scope=node").await;
    assert!(local["topGroups"][0]["band"].is_number());

    let dead = node.stats("/metrics/topics/orders-dead-letter").await;
    assert_eq!(dead["deadLetter"], true);

    let missing = node.client_get_ops("/metrics/topics/does-not-exist").await;
    assert_eq!(error_code(missing, 404).await, "topic.not-found");
    node.shutdown().await;
}

/// UC: UC-GRP-015, UC-OPS-022
#[tokio::test]
async fn the_groups_route_lists_pages_filters_and_tops_the_groups() {
    let node = TestNode::builder()
        .env("TABA_DELIVERY_FAIR_MAX_GROUPS", "3")
        .start()
        .await;
    queue(&node, "orders").await;
    for (group, count) in [("a", 3), ("b", 1), ("c", 2)] {
        for _ in 0..count {
            publish(&node, "orders", &format!("?groupId={group}"), "x").await;
        }
    }
    let all = node.stats("/metrics/topics/orders/groups").await;
    let ids: Vec<&str> = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["groupId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
    assert_eq!(all["items"][0]["messages"], 3);
    assert_eq!(all["items"][0]["visibleMessages"], 3);
    assert_eq!(all["items"][0]["invisibleMessages"], 0);
    assert_eq!(all["items"][0]["share"], 0.5);
    assert!(all["items"][0]["oldestMessageAt"].is_string());
    assert!(all["items"][0]["oldestAge"].is_number());

    let top = node.stats("/metrics/topics/orders/groups?top=2").await;
    let ids: Vec<&str> = top["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["groupId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a", "c"]);
    assert!(top.get("nextCursor").is_none(), "`top` does not paginate");

    let one = node.stats("/metrics/topics/orders/groups?groupId=b").await;
    assert_eq!(one["items"].as_array().unwrap().len(), 1);
    assert_eq!(one["items"][0]["messages"], 1);

    let first = node.stats("/metrics/topics/orders/groups?limit=2").await;
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    let cursor = first["nextCursor"].as_str().unwrap();
    let second = node
        .stats(&format!(
            "/metrics/topics/orders/groups?limit=2&cursor={cursor}"
        ))
        .await;
    assert_eq!(second["items"][0]["groupId"], "c");
    assert!(second.get("nextCursor").is_none());

    let both = node
        .client_get_ops(&format!(
            "/metrics/topics/orders/groups?top=1&cursor={cursor}"
        ))
        .await;
    assert_eq!(error_code(both, 400).await, "validation.failed");
    let zero = node
        .client_get_ops("/metrics/topics/orders/groups?top=0")
        .await;
    assert_eq!(error_code(zero, 400).await, "validation.out-of-range");
    let missing = node.client_get_ops("/metrics/topics/nothing/groups").await;
    assert_eq!(error_code(missing, 404).await, "topic.not-found");
    node.shutdown().await;
}

/// UC: UC-GRP-012, UC-GRP-015
#[tokio::test]
async fn the_shared_counter_shows_as_overflow_and_an_empty_group_disappears() {
    let node = TestNode::builder()
        .env("TABA_DELIVERY_FAIR_MAX_GROUPS", "2")
        .start()
        .await;
    queue(&node, "orders").await;
    for group in ["a", "b", "c", "d"] {
        publish(&node, "orders", &format!("?groupId={group}"), "x").await;
    }
    publish(&node, "orders", "", "no group").await;

    let page = node.stats("/metrics/topics/orders/groups").await;
    let rows = page["items"].as_array().unwrap();
    let last = rows.last().unwrap();
    assert_eq!(last["overflow"], true);
    assert!(last.get("groupId").is_none());
    assert_eq!(last["messages"], 3, "c, d and the message without a group");
    assert_eq!(rows.len(), 3);

    // The group that empties stops being tracked.
    let message = take(&node, "orders", "maxMessages=1&visibilityTimeout=600").await;
    let (id, receipt) = (
        message[0]["id"].as_str().unwrap().to_owned(),
        message[0]["receiptToken"].as_str().unwrap().to_owned(),
    );
    let acked = node
        .client
        .delete(node.client_url(&format!(
            "/topics/orders/messages/{id}?receiptToken={receipt}"
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(acked.status(), 204);
    let page = node.stats("/metrics/topics/orders/groups").await;
    assert_eq!(page["items"].as_array().unwrap().len(), 2);

    let distributor = {
        create(
            &node,
            json!({ "name": "dist", "type": "distributor", "prefixes": ["ped"] }),
        )
        .await;
        node.client_get_ops("/metrics/topics/dist/groups").await
    };
    assert_eq!(error_code(distributor, 409).await, "topic.wrong-type");
    node.shutdown().await;
}

/// UC: UC-GRP-016, UC-OPS-028
#[tokio::test]
async fn noisy_group_fires_with_two_groups_and_the_largest_over_the_share() {
    let node = TestNode::single().await;
    queue(&node, "orders").await;
    queue(&node, "calmo").await;
    for _ in 0..9 {
        publish(&node, "orders", "?groupId=g1", "x").await;
    }
    publish(&node, "orders", "?groupId=g2", "x").await;
    publish(&node, "calmo", "?groupId=g1", "x").await;
    publish(&node, "calmo", "?groupId=g2", "x").await;

    let page = node.stats("/metrics/topics").await;
    let orders = item(&page, "orders");
    assert_eq!(conditions(orders), ["noisy-group"]);
    assert_eq!(orders["worstCondition"], "noisy-group");
    assert_eq!(orders["conditions"][0]["severity"], "info");
    assert!((orders["noisyShare"].as_f64().unwrap() - 0.9).abs() < 1e-9);
    assert_eq!(
        item(&page, "calmo")["worstCondition"],
        "ok",
        "50% does not go over 50%"
    );

    let only = node.stats("/metrics/topics?condition=noisy-group").await;
    assert_eq!(names(&only), ["orders"]);
    let healthy = node.stats("/metrics/topics?condition=ok").await;
    assert!(!names(&healthy).contains(&"orders"));
    assert!(names(&healthy).contains(&"calmo"));
    node.shutdown().await;
}

/// UC: UC-OPS-028
#[tokio::test]
async fn backlog_stalled_and_idle_follow_the_configured_limits_and_the_clock() {
    let node = TestNode::builder()
        .env("TABA_STATS_TOPICS_BACKLOG_DEPTH", "2")
        .env("TABA_STATS_TOPICS_BACKLOG_AGE", "10m")
        .env("TABA_STATS_TOPICS_STALLED_AFTER", "1m")
        .env("TABA_STATS_TOPICS_IDLE_AFTER", "5m")
        .start()
        .await;
    queue(&node, "orders").await;
    queue(&node, "ocioso").await;
    let worst = |page: &Value, name: &str| {
        item(page, name)["worstCondition"]
            .as_str()
            .unwrap()
            .to_owned()
    };

    publish(&node, "orders", "", "a").await;
    let page = node.stats("/metrics/topics").await;
    assert_eq!(worst(&page, "orders"), "ok");

    // Goes over `stats.topics.backlog.depth`.
    publish(&node, "orders", "", "b").await;
    publish(&node, "orders", "", "c").await;
    let page = node.stats("/metrics/topics").await;
    assert_eq!(conditions(item(&page, "orders")), ["backlog"]);

    // No one has consumed for more than `stalled-after`.
    node.clock.advance(Duration::from_secs(61));
    let page = node.stats("/metrics/topics").await;
    assert_eq!(conditions(item(&page, "orders")), ["backlog", "stalled"]);
    assert_eq!(
        item(&page, "orders")["conditions"][1]["severity"],
        "warning"
    );

    // Consuming takes away the `stalled` (and there are no longer too many ready ones).
    assert_eq!(
        take(&node, "orders", "maxMessages=3&visibilityTimeout=600")
            .await
            .len(),
        3
    );
    let page = node.stats("/metrics/topics").await;
    assert_eq!(conditions(item(&page, "orders")), ["backlog"]);

    // The oldest goes over `backlog.age` and the topic with no activity becomes `idle`.
    node.clock.advance(Duration::from_secs(600));
    let page = node.stats("/metrics/topics").await;
    assert_eq!(
        conditions(item(&page, "orders")),
        ["backlog", "stalled", "idle"]
    );
    assert_eq!(conditions(item(&page, "ocioso")), ["idle"]);
    node.shutdown().await;
}

/// UC: UC-OPS-028
#[tokio::test]
async fn memory_pressure_means_most_of_the_topic_is_on_disk() {
    let node = TestNode::builder()
        .env("TABA_MEMORY_LIMIT", "150")
        .start()
        .await;
    queue(&node, "orders").await;
    let body = "x".repeat(100);
    for _ in 0..3 {
        publish(&node, "orders", "", &body).await;
    }
    let page = node.stats("/metrics/topics").await;
    let orders = item(&page, "orders");
    assert_eq!(orders["memoryMessages"], 1);
    assert_eq!(orders["diskMessages"], 2);
    assert_eq!(orders["memoryBytes"], 100);
    assert_eq!(orders["diskBytes"], 200);
    assert_eq!(orders["bytes"], 300);
    assert_eq!(conditions(orders), ["memory-pressure"]);
    node.shutdown().await;
}

/// UC: UC-OPS-028, UC-FIF-044
#[tokio::test]
async fn group_stalled_fires_when_a_fifo_head_is_delivered_too_often_without_ack() {
    let node = TestNode::builder()
        .env("TABA_STATS_TOPICS_GROUP_STALLED_DELIVERIES", "2")
        .start()
        .await;
    create(
        &node,
        json!({ "name": "orders", "type": "queue", "fifo": true }),
    )
    .await;
    publish(&node, "orders", "?groupId=customer-1", "x").await;
    publish(&node, "orders", "?groupId=customer-2", "x").await;

    for _ in 0..2 {
        assert!(
            !take(&node, "orders", "maxMessages=1&visibilityTimeout=0")
                .await
                .is_empty()
        );
    }
    let page = node.stats("/metrics/topics").await;
    assert!(
        !conditions(item(&page, "orders")).contains(&"group-stalled"),
        "two deliveries still do not go over the limit"
    );
    assert!(
        !take(&node, "orders", "maxMessages=1&visibilityTimeout=0")
            .await
            .is_empty()
    );
    let page = node.stats("/metrics/topics?condition=group-stalled").await;
    assert_eq!(names(&page), ["orders"]);
    assert_eq!(
        item(&page, "orders")["conditions"][0]["severity"],
        "warning"
    );
    node.shutdown().await;
}

/// UC: UC-OPS-005, UC-OPS-019
#[tokio::test]
async fn the_json_routes_count_in_the_ops_category_and_metrics_does_not() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_OPS_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .start()
        .await;
    assert_eq!(node.client_get("/metrics/topics").await.status(), 200);
    let limited = node.client_get("/metrics/topics").await;
    assert_eq!(limited.headers()["retry-after"], "1");
    assert_eq!(error_code(limited, 429).await, "rate-limit.exceeded");
    assert_eq!(node.client_get("/metrics").await.status(), 200);
    assert_eq!(node.client_get("/health/live").await.status(), 200);
    node.shutdown().await;
}

/// UC: UC-FIF-045
#[tokio::test]
async fn noisy_group_points_at_the_hot_group_of_a_fifo_queue() {
    let node = TestNode::single().await;
    create(
        &node,
        json!({ "name": "orders", "type": "queue", "fifo": true }),
    )
    .await;
    // Different bodies: on the FIFO the same repeated body is discarded as a duplicate.
    for n in 0..9 {
        publish(&node, "orders", "?groupId=quente", &format!("m{n}")).await;
    }
    publish(&node, "orders", "?groupId=frio", "x").await;

    let page = node.stats("/metrics/topics").await;
    let orders = item(&page, "orders");
    assert_eq!(conditions(orders), ["noisy-group"]);
    assert!((orders["noisyShare"].as_f64().unwrap() - 0.9).abs() < 1e-9);
    // The hot group appears first in `topGroups`.
    let detail = node.stats("/metrics/topics/orders").await;
    assert_eq!(detail["topGroups"][0]["groupId"], "quente");
    node.shutdown().await;
}
