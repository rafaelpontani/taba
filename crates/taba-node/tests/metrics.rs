//! `GET /metrics` and the node's `taba.*` metrics (doc 11).

mod common;

use std::time::Duration;

use common::{TestNode, error_code, series};
use reqwest::Response;
use serde_json::{Value, json};

async fn create_queue(node: &TestNode, body: Value) {
    let response = node
        .client
        .post(node.client_url("/topics"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{body}");
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
    let response = node
        .client_get(&format!("/topics/{topic}/messages?{query}"))
        .await;
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone()
}

fn port(node: &TestNode) -> u16 {
    node.node.client_addr().port()
}

/// UC: UC-OPS-006, UC-OPS-016, UC-OPS-045, UC-DEV-011
#[tokio::test]
async fn metrics_is_the_prometheus_text_of_this_node_only() {
    let node = TestNode::single().await;
    node.clock.advance(Duration::from_secs(90));
    let response = node.client_get_ops("/metrics").await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let text = response.text().await.unwrap();

    assert_eq!(series(&text, "process_uptime_seconds"), Some(90.0));
    assert_eq!(
        series(&text, "taba_node_state{taba_state=\"active\"}"),
        Some(1.0)
    );
    assert_eq!(
        series(&text, "taba_node_state{taba_state=\"joining\"}"),
        Some(0.0)
    );
    assert_eq!(
        series(&text, "taba_node_drain_state{taba_state=\"active\"}"),
        Some(1.0)
    );
    // The scrape came in on the operations port, so that is the port with an open connection.
    let connections = format!(
        "taba_node_connections{{server_port=\"{}\"}}",
        node.node.operations_addr().port()
    );
    assert!(series(&text, &connections).unwrap() >= 1.0, "{text}");
    assert!(text.contains("# TYPE taba_node_state gauge\n"));

    let open = node
        .client
        .get(node.operations_url("/metrics"))
        .header("accept", "application/openmetrics-text")
        .send()
        .await
        .unwrap();
    assert!(
        open.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/openmetrics-text")
    );
    assert!(open.text().await.unwrap().ends_with("# EOF\n"));
    node.shutdown().await;
}

#[cfg(target_os = "linux")]
/// UC: UC-OPS-016
#[tokio::test]
async fn on_linux_the_process_metrics_have_memory_threads_and_files() {
    let node = TestNode::single().await;
    let text = node.metrics().await;
    assert!(series(&text, "process_memory_usage_bytes").unwrap() > 0.0);
    assert!(series(&text, "process_thread_count").unwrap() >= 1.0);
    assert!(series(&text, "process_open_file_descriptor_count").unwrap() >= 1.0);
    node.shutdown().await;
}

/// UC: UC-OPS-007
#[tokio::test]
async fn metrics_is_404_when_off_and_403_for_an_ip_outside_the_list() {
    let off = TestNode::builder()
        .env("TABA_TELEMETRY_PROMETHEUS_ENABLED", "false")
        .start()
        .await;
    assert_eq!(
        error_code(off.client_get_ops("/metrics").await, 404).await,
        "route.not-found"
    );
    off.shutdown().await;

    let listed = TestNode::builder()
        .env("TABA_OPERATIONS_ALLOWED_IPS", "10.0.0.0/8")
        .start()
        .await;
    for path in ["/metrics", "/metrics/topics", "/metrics/topics/x/groups"] {
        assert_eq!(
            error_code(listed.client_get_ops(path).await, 403).await,
            "ops.ip-not-allowed",
            "{path}"
        );
    }
    listed.shutdown().await;
}

/// UC: UC-OPS-012, UC-OPS-041
#[tokio::test]
async fn requests_are_measured_by_route_template_never_by_topic_or_path() {
    let node = TestNode::single().await;
    create_queue(&node, json!({ "name": "orders", "type": "queue" })).await;
    assert_eq!(publish(&node, "orders", "", "a").await.status(), 201);
    assert_eq!(publish(&node, "orders", "", "b").await.status(), 201);
    assert_eq!(node.client_get("/nothing/here").await.status(), 404);
    let text = node.metrics().await;
    let p = port(&node);
    let ops = node.node.operations_addr().port();

    let route = "http_route=\"/topics/{topic}/messages\"";
    let labels = format!(
        "{{http_request_method=\"POST\",http_response_status_code=\"201\",{route},\
         server_port=\"{p}\",url_scheme=\"http\"}}"
    );
    assert_eq!(
        series(
            &text,
            &format!("http_server_request_duration_seconds_count{labels}")
        ),
        Some(2.0),
        "{text}"
    );
    assert_eq!(
        series(
            &text,
            &format!("http_server_request_body_size_bytes_count{labels}")
        ),
        Some(2.0),
        "each publish's body has a known size"
    );
    assert!(text.contains("http_server_request_duration_seconds_bucket{"));
    assert_eq!(
        series(
            &text,
            &format!(
                "http_server_request_duration_seconds_count{{http_request_method=\"GET\",\
                 http_response_status_code=\"404\",server_port=\"{p}\",url_scheme=\"http\"}}"
            )
        ),
        Some(1.0),
        "the route that did not match has no `http.route`"
    );
    assert_eq!(
        series(
            &text,
            &format!(
                "http_server_active_requests{{http_request_method=\"GET\",server_port=\"{ops}\",\
                 url_scheme=\"http\"}}"
            )
        ),
        Some(1.0),
        "the read itself (on the operations port) is in progress"
    );
    assert!(!text.contains("orders"), "the topic is never a node label");
    assert!(!text.contains("/nothing/here"));
    assert!(!text.contains("http_server_requests_total"), "{text}");
    assert!(!text.contains("msg_"));
    node.shutdown().await;
}

/// UC: UC-ARM-009
#[tokio::test]
async fn storage_metrics_count_overflow_and_reads_by_tier() {
    let node = TestNode::builder()
        .env("TABA_MEMORY_LIMIT", "150")
        .start()
        .await;
    create_queue(&node, json!({ "name": "orders", "type": "queue" })).await;
    let body = "x".repeat(100);
    for _ in 0..3 {
        assert_eq!(publish(&node, "orders", "", &body).await.status(), 201);
    }
    let text = node.metrics().await;
    assert_eq!(
        series(&text, "taba_storage_memory_limit_bytes"),
        Some(150.0)
    );
    assert_eq!(
        series(&text, "taba_storage_memory_usage_bytes"),
        Some(100.0)
    );
    assert_eq!(series(&text, "taba_storage_memory_messages"), Some(1.0));
    assert_eq!(series(&text, "taba_storage_disk_messages"), Some(2.0));
    assert_eq!(series(&text, "taba_storage_overflow_total"), Some(2.0));
    assert_eq!(series(&text, "taba_delivery_ready"), Some(3.0));

    assert_eq!(take(&node, "orders", "maxMessages=10").await.len(), 3);
    let text = node.metrics().await;
    assert_eq!(
        series(&text, "taba_storage_reads_total{taba_tier=\"memory\"}"),
        Some(1.0)
    );
    assert_eq!(
        series(&text, "taba_storage_reads_total{taba_tier=\"disk\"}"),
        Some(2.0)
    );
    assert_eq!(series(&text, "taba_delivery_ready"), Some(0.0));
    assert_eq!(series(&text, "taba_storage_disk_errors_total"), Some(0.0));
    node.shutdown().await;
}

/// UC: UC-RET-018, UC-RET-007
#[tokio::test]
async fn redeliveries_dead_letters_and_expirations_are_counted() {
    let node = TestNode::single().await;
    create_queue(
        &node,
        json!({ "name": "orders", "type": "queue", "maxDeliveries": 2 }),
    )
    .await;
    create_queue(
        &node,
        json!({ "name": "short", "type": "queue", "ttl": 60 }),
    )
    .await;
    publish(&node, "orders", "", "a").await;
    publish(&node, "short", "", "b").await;

    assert_eq!(take(&node, "orders", "visibilityTimeout=1").await.len(), 1);
    node.clock.advance(Duration::from_secs(2));
    assert_eq!(take(&node, "orders", "visibilityTimeout=1").await.len(), 1);
    node.clock.advance(Duration::from_secs(60));
    // The background sweep takes the exhausted message to the dead-letter and deletes the
    // one that went past the `ttl`.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let text = node.metrics().await;
    assert_eq!(series(&text, "taba_delivery_redeliveries_total"), Some(1.0));
    assert_eq!(
        series(&text, "taba_delivery_dead_lettered_total"),
        Some(1.0)
    );
    assert_eq!(series(&text, "taba_retention_expired_total"), Some(1.0));
    node.shutdown().await;
}

/// UC: UC-GRP-017
#[tokio::test]
async fn the_wait_is_measured_per_band_and_small_groups_wait_less() {
    let node = TestNode::single().await;
    create_queue(&node, json!({ "name": "orders", "type": "queue" })).await;
    for _ in 0..20 {
        publish(&node, "orders", "?groupId=big", "a").await;
    }
    node.clock.advance(Duration::from_secs(30));
    for _ in 0..2 {
        publish(&node, "orders", "?groupId=small", "b").await;
    }
    node.clock.advance(Duration::from_secs(1));
    assert_eq!(
        take(&node, "orders", "maxMessages=22&visibilityTimeout=600")
            .await
            .len(),
        22
    );
    let text = node.metrics().await;

    let band = |label: &str| {
        let count = series(
            &text,
            &format!("taba_delivery_fair_wait_seconds_count{{taba_band=\"{label}\"}}"),
        )?;
        let sum = series(
            &text,
            &format!("taba_delivery_fair_wait_seconds_sum{{taba_band=\"{label}\"}}"),
        )?;
        Some((count, sum))
    };
    assert_eq!(
        band("0"),
        Some((2.0, 2.0)),
        "the two of the small group: 1 s each"
    );
    let (mut count, mut sum) = (0.0, 0.0);
    for label in (1..15).map(|n| n.to_string()).chain(["15+".to_owned()]) {
        if let Some((c, s)) = band(&label) {
            count += c;
            sum += s;
        }
    }
    assert_eq!(count, 20.0);
    assert_eq!(sum / count, 31.0, "the big group waited 31 s");
    node.shutdown().await;
}

/// UC: UC-LIM-015
#[tokio::test]
async fn rate_limit_rejections_are_counted_by_category_and_key_and_metrics_is_exempt() {
    let node = TestNode::builder()
        .env("TABA_RATELIMIT_PUBLISH_RPS", "1")
        .env("TABA_RATELIMIT_OPS_RPS", "1")
        .env("TABA_RATELIMIT_BURST", "1")
        .start()
        .await;
    create_queue(&node, json!({ "name": "orders", "type": "queue" })).await;
    assert_eq!(publish(&node, "orders", "", "a").await.status(), 201);
    assert_eq!(publish(&node, "orders", "", "b").await.status(), 429);
    assert_eq!(node.client_get_ops("/metrics/topics").await.status(), 200);
    assert_eq!(node.client_get_ops("/metrics/topics").await.status(), 429);

    for _ in 0..5 {
        assert_eq!(
            node.client_get_ops("/metrics").await.status(),
            200,
            "`/metrics` never answers 429"
        );
    }
    let text = node.metrics().await;
    assert_eq!(
        series(
            &text,
            "taba_ratelimit_rejected_total{taba_category=\"publish\",taba_key=\"ip\"}"
        ),
        Some(1.0)
    );
    assert_eq!(
        series(
            &text,
            "taba_ratelimit_rejected_total{taba_category=\"ops\",taba_key=\"ip\"}"
        ),
        Some(1.0)
    );
    node.shutdown().await;
}

mod authentication {
    use super::*;

    const ROOT: &str = "raiz-raiz-raiz-raiz-raiz-raiz-raiz-0001";

    /// UC: UC-AUT-036, UC-AUT-021
    #[tokio::test]
    async fn auth_requests_are_counted_by_outcome_and_the_token_id_is_never_a_label() {
        let node = TestNode::builder()
            .production()
            .env("TABA_AUTH_ROOT_TOKEN", ROOT)
            .start()
            .await;
        let call = |token: Option<String>, method: reqwest::Method, path: &str, body: Value| {
            let mut request = node.client.request(method, node.client_url(path));
            if let Some(token) = token {
                request = request.header("authorization", format!("Bearer {token}"));
            }
            request.json(&body).send()
        };
        let created = call(
            Some(ROOT.to_owned()),
            reqwest::Method::POST,
            "/tokens",
            json!({ "name": "t", "permissions": ["consume"], "topicPrefixes": [] }),
        )
        .await
        .unwrap();
        assert_eq!(created.status(), 201);
        let created: Value = created.json().await.unwrap();
        let (id, token) = (
            created["id"].as_str().unwrap().to_owned(),
            created["token"].as_str().unwrap().to_owned(),
        );

        let none = call(None, reqwest::Method::GET, "/topics", json!({})).await;
        assert_eq!(none.unwrap().status(), 401);
        let forbidden = call(
            Some(token),
            reqwest::Method::POST,
            "/topics",
            json!({ "name": "x", "type": "queue" }),
        )
        .await;
        assert_eq!(forbidden.unwrap().status(), 403);
        let text = node.metrics().await;

        assert_eq!(
            series(
                &text,
                "taba_auth_requests_total{taba_outcome=\"unauthorized\"}"
            ),
            Some(1.0)
        );
        assert_eq!(
            series(
                &text,
                "taba_auth_requests_total{taba_outcome=\"forbidden\"}"
            ),
            Some(1.0)
        );
        assert_eq!(
            series(&text, "taba_auth_requests_total{taba_outcome=\"ok\"}"),
            Some(1.0),
            "only the token's creation by the root"
        );
        assert_eq!(series(&text, "taba_auth_tokens"), Some(1.0));
        assert!(!text.contains(&id), "the token id is never a label");
        node.shutdown().await;
    }
}

/// UC: UC-AUT-036
#[tokio::test]
async fn without_authentication_there_are_no_auth_series() {
    let node = TestNode::single().await;
    node.client_get("/topics").await;
    let text = node.metrics().await;
    assert!(!text.contains("taba_auth_"), "{text}");
    node.shutdown().await;
}

/// UC: UC-ENT-031
#[tokio::test]
async fn long_polling_is_counted_while_waiting_and_by_outcome() {
    let node = TestNode::single().await;
    create_queue(&node, json!({ "name": "orders", "type": "queue" })).await;

    let waiting = {
        let client = node.client.clone();
        let url = node.client_url("/topics/orders/messages?waitTime=5");
        tokio::spawn(async move { client.get(url).send().await.unwrap() })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let text = node.metrics().await;
    assert_eq!(series(&text, "taba_delivery_longpoll_waiting"), Some(1.0));

    publish(&node, "orders", "", "a").await;
    let response = waiting.await.unwrap();
    assert_eq!(
        response.json::<Value>().await.unwrap()["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let empty = node.client_get("/topics/orders/messages?waitTime=1").await;
    assert_eq!(empty.status(), 200);
    let text = node.metrics().await;
    assert_eq!(series(&text, "taba_delivery_longpoll_waiting"), Some(0.0));
    assert_eq!(
        series(
            &text,
            "taba_delivery_longpoll_outcome_total{taba_outcome=\"delivered\"}"
        ),
        Some(1.0)
    );
    assert_eq!(
        series(
            &text,
            "taba_delivery_longpoll_outcome_total{taba_outcome=\"empty\"}"
        ),
        Some(1.0)
    );
    node.shutdown().await;
}

/// UC: UC-OPS-014
#[tokio::test]
async fn topic_metrics_are_off_by_default() {
    let node = TestNode::single().await;
    create_queue(&node, json!({ "name": "orders", "type": "queue" })).await;
    publish(&node, "orders", "", "a").await;
    let text = node.metrics().await;
    assert!(
        !text.contains("taba_topic_"),
        "/metrics does not grow with the topics"
    );
    node.shutdown().await;
}

async fn two_topics(node: &TestNode) {
    create_queue(
        node,
        json!({ "name": "orders", "type": "queue", "tags": { "time": "fin" } }),
    )
    .await;
    create_queue(
        node,
        json!({ "name": "quiet", "type": "queue", "tags": { "time": "ops" } }),
    )
    .await;
    publish(node, "orders", "", "a").await;
    publish(node, "orders", "", "b").await;
    assert_eq!(
        take(node, "orders", "maxMessages=1&visibilityTimeout=600")
            .await
            .len(),
        1
    );
}

/// UC: UC-OPS-014
#[tokio::test]
async fn per_topic_mode_has_one_series_per_topic_and_no_oldest_without_messages() {
    let node = TestNode::builder()
        .env("TABA_TELEMETRY_TOPICS_MODE", "per-topic")
        .env("TABA_TELEMETRY_TOPICS_GROUP_BY_TAGS", "time")
        .env("TABA_STATS_TOPICS_BACKLOG_DEPTH", "1")
        .start()
        .await;
    two_topics(&node).await;
    let text = node.metrics().await;
    let orders = "messaging_destination_name=\"orders\",taba_topic_tag_time=\"fin\"";
    let empty = "messaging_destination_name=\"quiet\",taba_topic_tag_time=\"ops\"";

    assert_eq!(
        series(&text, &format!("taba_topic_published_total{{{orders}}}")),
        Some(2.0)
    );
    assert_eq!(
        series(&text, &format!("taba_topic_consumed_total{{{orders}}}")),
        Some(1.0)
    );
    assert_eq!(
        series(&text, &format!("taba_topic_visible{{{orders}}}")),
        Some(1.0)
    );
    assert_eq!(
        series(&text, &format!("taba_topic_invisible{{{orders}}}")),
        Some(1.0)
    );
    assert_eq!(
        series(&text, &format!("taba_topic_depth{{{orders}}}")),
        Some(2.0)
    );
    assert!(
        series(
            &text,
            &format!("taba_topic_oldest_timestamp_seconds{{{orders}}}")
        )
        .unwrap()
            > 0.0
    );
    assert_eq!(
        series(&text, &format!("taba_topic_depth{{{empty}}}")),
        Some(0.0)
    );
    assert_eq!(
        series(
            &text,
            &format!("taba_topic_oldest_timestamp_seconds{{{empty}}}")
        ),
        None,
        "without messages there is no series for the oldest"
    );
    assert_eq!(
        series(
            &text,
            "taba_topic_condition{messaging_destination_name=\"orders\",\
             taba_condition=\"backlog\",taba_topic_tag_time=\"fin\"}"
        ),
        Some(1.0)
    );
    node.shutdown().await;
}

/// UC: UC-OPS-014
#[tokio::test]
async fn per_topic_mode_folds_the_topics_above_the_ceiling_into_other() {
    let node = TestNode::builder()
        .env("TABA_TELEMETRY_TOPICS_MODE", "per-topic")
        .env("TABA_TELEMETRY_TOPICS_MAX_TOPICS", "1")
        .start()
        .await;
    two_topics(&node).await;
    let text = node.metrics().await;
    assert_eq!(
        series(
            &text,
            "taba_topic_published_total{messaging_destination_name=\"orders\"}"
        ),
        Some(2.0)
    );
    assert_eq!(
        series(
            &text,
            "taba_topic_published_total{messaging_destination_name=\"_other\"}"
        ),
        Some(0.0)
    );
    assert!(!text.contains("messaging_destination_name=\"quiet\""));
    node.shutdown().await;
}

/// UC: UC-OPS-014
#[tokio::test]
async fn by_tags_mode_sums_by_tag_value_and_never_names_a_topic() {
    let node = TestNode::builder()
        .env("TABA_TELEMETRY_TOPICS_MODE", "by-tags")
        .env("TABA_TELEMETRY_TOPICS_GROUP_BY_TAGS", "time")
        .start()
        .await;
    two_topics(&node).await;
    let text = node.metrics().await;
    assert_eq!(
        series(
            &text,
            "taba_topic_published_total{taba_topic_tag_time=\"fin\"}"
        ),
        Some(2.0)
    );
    assert_eq!(
        series(&text, "taba_topic_depth{taba_topic_tag_time=\"ops\"}"),
        Some(0.0)
    );
    assert!(!text.contains("messaging_destination_name"), "{text}");
    node.shutdown().await;
}

/// UC: UC-OPS-040
#[tokio::test]
async fn the_resource_attributes_come_from_the_otel_variables_and_the_node() {
    let node = TestNode::builder()
        .env("OTEL_SERVICE_NAME", "taba-test")
        .env(
            "OTEL_RESOURCE_ATTRIBUTES",
            "service.namespace=cluster-a,deployment.environment.name=staging",
        )
        .start()
        .await;
    let text = node.metrics().await;
    let target = text
        .lines()
        .find(|l| l.starts_with("target_info{"))
        .expect("target_info");
    assert!(target.contains("service_name=\"taba-test\""), "{target}");
    assert!(target.contains("service_namespace=\"cluster-a\""));
    assert!(target.contains("deployment_environment_name=\"staging\""));
    assert!(target.contains(&format!("service_instance_id=\"{}\"", node.node.node_id())));
    assert!(target.contains(&format!(
        "service_version=\"{}\"",
        env!("CARGO_PKG_VERSION")
    )));
    node.shutdown().await;

    let plain = TestNode::single().await;
    let text = plain.metrics().await;
    assert!(text.contains("service_name=\"taba\""));
    plain.shutdown().await;
}

/// UC: UC-ARM-009, UC-OPS-018
#[tokio::test]
async fn free_disk_space_is_a_metric_and_below_the_minimum_is_storage_pressure() {
    let node = TestNode::builder()
        .env("TABA_HEALTH_SINGLE_NODE_CONDITION", "false")
        .start()
        .await;
    let text = node.metrics().await;
    assert!(series(&text, "taba_storage_disk_free_bytes").unwrap() > 0.0);
    let health = node.stats("/health/cluster").await;
    assert!(
        health["nodes"][0]["status"]["storage"]["diskFreeBytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(health["worstCondition"], "ok");
    node.shutdown().await;

    let pressed = TestNode::builder()
        .env("TABA_HEALTH_SINGLE_NODE_CONDITION", "false")
        .env("TABA_STATS_DISK_FREE_MIN", "1000000000000000")
        .start()
        .await;
    let health = pressed.stats("/health/cluster").await;
    assert_eq!(health["worstCondition"], "storage-pressure");
    assert_eq!(health["conditions"][0]["severity"], "warning");
    pressed.shutdown().await;
}
