//! `POST` e `GET /topics/{topic}/messages` contra um nó de verdade (doc 06a, "Mensagens").

mod common;

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{TestNode, error_code};
use reqwest::Response;
use serde_json::{Value, json};

async fn create_topic(node: &TestNode, body: Value) {
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
    create_topic(node, json!({ "name": name, "type": "queue" })).await;
}

async fn publish_raw(
    node: &TestNode,
    topic: &str,
    query: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Response {
    let mut request = node
        .client
        .post(node.client_url(&format!("/topics/{topic}/messages{query}")))
        .body(body);
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    request.send().await.unwrap()
}

async fn publish(node: &TestNode, topic: &str, text: &str) -> Value {
    let response = publish_raw(node, topic, "", None, text.as_bytes().to_vec()).await;
    assert_eq!(response.status(), 201);
    response.json().await.unwrap()
}

async fn consume_raw(node: &TestNode, topic: &str, query: &str) -> Response {
    node.client_get(&format!("/topics/{topic}/messages{query}"))
        .await
}

async fn consume(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    let response = consume_raw(node, topic, query).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    body["messages"].as_array().unwrap().clone()
}

/// UC: UC-MSG-001, UC-MSG-018, UC-MSG-019, UC-CNS-001, UC-CNS-008
#[tokio::test]
async fn publishing_json_without_content_type_and_consuming_it_back() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;

    let response = node
        .client
        .post(node.client_url("/topics/pedidos/messages"))
        .header("x-request-id", "meu-pedido-1")
        .body(r#"{ "pedido": 9001, "valor": 129.90, "itens": ["a", "b"] }"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["x-request-id"], "meu-pedido-1");
    let published: Value = response.json().await.unwrap();
    assert_eq!(
        published.as_object().unwrap().len(),
        1,
        "sem `duplicate` numa fila comum"
    );
    let id = published["id"].as_str().unwrap();
    assert!(taba_node::core::id::is_guid_v7(id));

    let messages = consume(&node, "pedidos", "?visibilityTimeout=90").await;
    assert_eq!(messages.len(), 1);
    let m = &messages[0];
    assert_eq!(m["id"], id);
    assert_eq!(
        m["body"],
        json!({ "pedido": 9001, "valor": 129.90, "itens": ["a", "b"] })
    );
    assert_eq!(m["contentType"], "application/json");
    assert_eq!(m["deliveryCount"], 1);
    assert_eq!(m["publishedAt"], "2026-10-06T12:00:00.000Z");
    assert_eq!(m["invisibleUntil"], "2026-10-06T12:01:30.000Z");
    assert!(taba_node::core::id::is_guid_v7(
        m["receiptToken"].as_str().unwrap()
    ));
    assert!(m.get("bodyEncoding").is_none() && m.get("groupId").is_none());
    node.shutdown().await;
}

/// UC: UC-MSG-001, UC-CNS-008
#[tokio::test]
async fn the_json_body_comes_back_exactly_as_it_was_sent() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    let sent = r#"{ "valor": 129.90,  "n": 1e3 }"#;
    publish(&node, "pedidos", sent).await;

    let text = consume_raw(&node, "pedidos", "")
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains(sent), "{text}");
    node.shutdown().await;
}

/// UC: UC-MSG-002, UC-MSG-003, UC-MSG-007, UC-CNS-009, UC-CNS-010, UC-CNS-011, UC-CNS-012, UC-MSG-020
#[tokio::test]
async fn a_mixed_batch_keeps_each_content_type_and_the_exact_bytes() {
    let node = TestNode::development().await;
    queue(&node, "arquivos").await;
    let all_bytes: Vec<u8> = (0..=255).collect();

    for (content_type, body, query) in [
        (
            "text/plain; charset=utf-8",
            "Pedido 9001 aprovado".as_bytes().to_vec(),
            "",
        ),
        ("image/png", all_bytes.clone(), "?groupId=cliente-42"),
        ("application/json", br#"{ "pedido": 9001 }"#.to_vec(), ""),
        ("application/vnd.api+json", b"[1, 2]".to_vec(), ""),
        ("text/plain; charset=iso-8859-1", vec![0xe9], ""),
    ] {
        let response = publish_raw(&node, "arquivos", query, Some(content_type), body).await;
        assert_eq!(response.status(), 201, "{content_type}");
    }

    // Sem garantia de ordem na fila comum (UC-CNS-013): compara em ordem de `id`.
    let mut messages = consume(&node, "arquivos", "?maxMessages=10").await;
    messages.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[0]["body"], "Pedido 9001 aprovado");
    assert_eq!(messages[0]["contentType"], "text/plain; charset=utf-8");
    assert!(messages[0].get("bodyEncoding").is_none());

    assert_eq!(messages[1]["bodyEncoding"], "base64");
    assert_eq!(messages[1]["contentType"], "image/png");
    assert_eq!(messages[1]["groupId"], "cliente-42");
    let bytes = STANDARD
        .decode(messages[1]["body"].as_str().unwrap())
        .unwrap();
    assert_eq!(bytes, all_bytes, "os bytes saem exatamente como entraram");

    assert_eq!(messages[2]["body"], json!({ "pedido": 9001 }));
    assert_eq!(messages[3]["body"], json!([1, 2]));
    assert_eq!(
        messages[4]["bodyEncoding"], "base64",
        "text/* com outro charset"
    );
    assert_eq!(messages[4]["body"], "6Q==");
    node.shutdown().await;
}

/// UC: UC-MSG-004, UC-MSG-005, UC-MSG-006
#[tokio::test]
async fn invalid_content_type_and_invalid_bodies_are_400() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;

    let response = publish_raw(
        &node,
        "pedidos",
        "",
        Some("nao-e-media-type"),
        b"x".to_vec(),
    )
    .await;
    assert_eq!(
        error_code(response, 400).await,
        "validation.invalid-content-type"
    );
    let too_long = format!("text/{}", "a".repeat(251));
    let response = publish_raw(&node, "pedidos", "", Some(&too_long), b"x".to_vec()).await;
    assert_eq!(
        error_code(response, 400).await,
        "validation.invalid-content-type"
    );
    let response = publish_raw(
        &node,
        "pedidos",
        "",
        Some("application/json"),
        b"{ nao".to_vec(),
    )
    .await;
    assert_eq!(error_code(response, 400).await, "validation.invalid-body");
    let response = publish_raw(&node, "pedidos", "", Some("text/plain"), vec![0xff, 0xfe]).await;
    assert_eq!(error_code(response, 400).await, "validation.invalid-body");
    let response = publish_raw(&node, "pedidos", "", None, Vec::new()).await;
    assert_eq!(error_code(response, 400).await, "validation.invalid-body");

    assert!(
        consume(&node, "pedidos", "").await.is_empty(),
        "nada foi aceito"
    );
    node.shutdown().await;
}

/// UC: UC-MSG-008, UC-MSG-009, UC-TOP-031
#[tokio::test]
async fn the_size_limit_counts_bytes_and_a_topic_may_exceed_the_general_one() {
    let node = TestNode::development().await;
    create_topic(
        &node,
        json!({ "name": "pequena", "type": "queue", "maxMessageBytes": 100 }),
    )
    .await;
    queue(&node, "padrao").await;
    create_topic(
        &node,
        json!({ "name": "grande", "type": "queue", "maxMessageBytes": 8 * 1024 * 1024 }),
    )
    .await;

    let blob = |n: usize| {
        let mut body = vec![b'x'; n];
        body[0] = b'"';
        body[n - 1] = b'"';
        body
    };
    let response = publish_raw(&node, "pequena", "", None, blob(100)).await;
    assert_eq!(response.status(), 201, "exatamente o limite");
    let response = publish_raw(&node, "pequena", "", None, blob(101)).await;
    assert_eq!(error_code(response, 413).await, "message.too-large");

    let five_mb = 5 * 1024 * 1024;
    let response = publish_raw(&node, "padrao", "", None, blob(five_mb)).await;
    assert_eq!(response.status(), 201, "o geral é 5 MB");
    let response = publish_raw(&node, "padrao", "", None, blob(five_mb + 1)).await;
    assert_eq!(error_code(response, 413).await, "message.too-large");
    let response = publish_raw(&node, "grande", "", None, blob(five_mb + 1)).await;
    assert_eq!(response.status(), 201, "o tópico pode passar do geral");

    // Mudar o limite vale só para as publicações seguintes.
    node.client
        .patch(node.client_url("/topics/pequena"))
        .json(&json!({ "maxMessageBytes": 10 }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        consume(&node, "pequena", "").await.len(),
        1,
        "a antiga segue lá"
    );
    let response = publish_raw(&node, "pequena", "", None, blob(11)).await;
    assert_eq!(error_code(response, 413).await, "message.too-large");
    node.shutdown().await;
}

/// UC: UC-MSG-010, UC-CNS-018, UC-CNS-017
#[tokio::test]
async fn unknown_topics_are_404_and_a_distributor_is_not_consumed() {
    let node = TestNode::development().await;
    create_topic(
        &node,
        json!({ "name": "todos", "type": "distributor", "prefixes": ["a"] }),
    )
    .await;

    let response = publish_raw(&node, "nada", "", None, b"1".to_vec()).await;
    assert_eq!(error_code(response, 404).await, "topic.not-found");
    let response = consume_raw(&node, "nada", "").await;
    assert_eq!(error_code(response, 404).await, "topic.not-found");
    let response = consume_raw(&node, "todos", "").await;
    assert_eq!(error_code(response, 409).await, "topic.wrong-type");
    node.shutdown().await;
}

/// UC: UC-MSG-011, UC-MSG-012, UC-FIF-002
#[tokio::test]
async fn group_and_deduplication_ids_are_validated() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    create_topic(
        &node,
        json!({ "name": "ordem", "type": "queue", "fifo": true }),
    )
    .await;

    let response = publish_raw(&node, "pedidos", "?groupId=cliente-42", None, b"1".to_vec()).await;
    assert_eq!(response.status(), 201);
    let messages = consume(&node, "pedidos", "").await;
    assert_eq!(messages[0]["groupId"], "cliente-42");

    for bad in [
        "?groupId=",
        "?groupId=com%20espaco",
        &format!("?groupId={}", "x".repeat(129)),
    ] {
        let response = publish_raw(&node, "pedidos", bad, None, b"1".to_vec()).await;
        assert_eq!(
            error_code(response, 400).await,
            "validation.failed",
            "{bad}"
        );
    }
    let response = publish_raw(&node, "pedidos", "?deduplicationId=d1", None, b"1".to_vec()).await;
    assert_eq!(error_code(response, 400).await, "validation.not-applicable");

    let response = publish_raw(&node, "ordem", "", None, b"1".to_vec()).await;
    assert_eq!(
        error_code(response, 400).await,
        "validation.group-id-required"
    );
    let response = publish_raw(
        &node,
        "ordem",
        "?groupId=a&deduplicationId=!",
        None,
        b"1".to_vec(),
    )
    .await;
    assert_eq!(error_code(response, 400).await, "validation.failed");
    node.shutdown().await;
}

/// UC: UC-CNS-002, UC-CNS-004, UC-CNS-006, UC-CNS-007
#[tokio::test]
async fn a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;

    let response = consume_raw(&node, "pedidos", "").await;
    assert_eq!(response.status(), 200, "fila vazia é 200, nunca 204");
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({ "messages": [] })
    );

    let id = publish(&node, "pedidos", "1").await["id"].clone();
    let first = consume(&node, "pedidos", "?visibilityTimeout=30").await;
    assert_eq!(
        first.len(),
        1,
        "devolve o que há, sem esperar completar o lote"
    );
    assert!(consume(&node, "pedidos", "").await.is_empty());

    node.clock.advance(Duration::from_secs(30));
    let again = consume(&node, "pedidos", "").await;
    assert_eq!(again[0]["id"], id);
    assert_eq!(again[0]["deliveryCount"], 2);
    assert_ne!(again[0]["receiptToken"], first[0]["receiptToken"]);
    node.shutdown().await;
}

/// UC: UC-CNS-003, UC-CNS-005, UC-CNS-024
#[tokio::test]
async fn consume_parameters_are_checked() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    for i in 0..12 {
        publish(&node, "pedidos", &i.to_string()).await;
    }
    assert_eq!(
        consume(&node, "pedidos", "").await.len(),
        10,
        "o padrão é 10"
    );
    assert_eq!(consume(&node, "pedidos", "?maxMessages=1").await.len(), 1);

    for query in [
        "?maxMessages=0",
        "?maxMessages=-3",
        "?visibilityTimeout=-1",
        "?waitTime=21",
        "?waitTime=-1",
    ] {
        let response = consume_raw(&node, "pedidos", query).await;
        assert_eq!(
            error_code(response, 400).await,
            "validation.out-of-range",
            "{query}"
        );
    }
    let response = consume_raw(&node, "pedidos", "?maxMessages=dez").await;
    assert_eq!(error_code(response, 400).await, "validation.failed");
    node.shutdown().await;
}

/// UC: UC-CNS-015
#[tokio::test]
async fn a_message_past_its_ttl_is_not_delivered() {
    let node = TestNode::development().await;
    create_topic(
        &node,
        json!({ "name": "curta", "type": "queue", "ttl": 60 }),
    )
    .await;
    publish(&node, "curta", "1").await;
    node.clock.advance(Duration::from_secs(60));
    assert!(consume(&node, "curta", "").await.is_empty());
    node.shutdown().await;
}

/// UC: UC-CNS-016
#[tokio::test]
async fn the_dead_letter_can_be_published_to_and_consumed() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos-dead-letter", "1").await;
    assert_eq!(consume(&node, "pedidos-dead-letter", "").await.len(), 1);
    assert!(consume(&node, "pedidos", "").await.is_empty());
    node.shutdown().await;
}

/// UC: UC-CNS-025, UC-CNS-027
#[tokio::test]
async fn long_polling_returns_as_soon_as_a_message_is_published() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;

    let url = node.client_url("/topics/pedidos/messages?waitTime=20&maxMessages=5");
    let client = node.client.clone();
    let waiter = tokio::spawn(async move {
        let started = std::time::Instant::now();
        let body: Value = client.get(url).send().await.unwrap().json().await.unwrap();
        (body, started.elapsed())
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!waiter.is_finished());
    publish(&node, "pedidos", "1").await;

    let (body, elapsed) = waiter.await.unwrap();
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    node.shutdown().await;
}

/// UC: UC-CNS-026
#[tokio::test]
async fn long_polling_ends_with_an_empty_list_when_nothing_arrives() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    let started = std::time::Instant::now();
    assert!(consume(&node, "pedidos", "?waitTime=1").await.is_empty());
    assert!(started.elapsed() >= Duration::from_millis(900));
    node.shutdown().await;
}

/// UC: UC-CNS-028
#[tokio::test]
async fn a_client_that_gives_up_does_not_swallow_the_next_message() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;

    let impatient = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(300))
        .build()
        .unwrap();
    let error = impatient
        .get(node.client_url("/topics/pedidos/messages?waitTime=20"))
        .send()
        .await
        .unwrap_err();
    assert!(error.is_timeout());
    tokio::time::sleep(Duration::from_millis(200)).await;

    publish(&node, "pedidos", "1").await;
    let messages = consume(&node, "pedidos", "").await;
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["deliveryCount"], 1,
        "ninguém a entregou a uma conexão morta"
    );
    node.shutdown().await;
}

/// UC: UC-DST-001, UC-DST-002, UC-DST-003, UC-DST-009, UC-DST-011
#[tokio::test]
async fn a_distributor_copies_to_matching_queues_and_reports_each_one() {
    let node = TestNode::development().await;
    queue(&node, "pedidos-eu").await;
    queue(&node, "pedidos-us").await;
    queue(&node, "outra").await;
    create_topic(
        &node,
        json!({ "name": "p-fifo", "type": "queue", "fifo": true }),
    )
    .await;
    create_topic(
        &node,
        json!({ "name": "todos", "type": "distributor", "prefixes": ["pedidos-"] }),
    )
    .await;
    create_topic(
        &node,
        json!({ "name": "nenhum", "type": "distributor", "prefixes": ["zzz"] }),
    )
    .await;
    create_topic(
        &node,
        json!({ "name": "com-fifo", "type": "distributor", "prefixes": ["p-"] }),
    )
    .await;

    let response = publish_raw(&node, "todos", "", Some("text/plain"), b"oi".to_vec()).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["matched"], 2);
    let deliveries = body["deliveries"].as_array().unwrap();
    assert_eq!(deliveries[0]["topic"], "pedidos-eu");
    assert_eq!(deliveries[1]["topic"], "pedidos-us");
    assert_ne!(deliveries[0]["id"], deliveries[1]["id"]);

    let eu = consume(&node, "pedidos-eu", "").await;
    let us = consume(&node, "pedidos-us", "").await;
    assert_eq!(eu[0]["id"], deliveries[0]["id"]);
    assert_eq!(
        (eu[0]["body"].clone(), eu[0]["contentType"].clone()),
        (json!("oi"), json!("text/plain"))
    );
    assert_eq!(us[0]["id"], deliveries[1]["id"]);
    assert!(consume(&node, "outra", "").await.is_empty());

    let response = publish_raw(&node, "nenhum", "", None, b"1".to_vec()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({ "matched": 0, "deliveries": [] })
    );

    let response = publish_raw(&node, "com-fifo", "", None, b"1".to_vec()).await;
    assert_eq!(
        response.status(),
        200,
        "falha parcial não é erro do distribuidor"
    );
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["deliveries"][0]["topic"], "p-fifo");
    assert_eq!(
        body["deliveries"][0]["error"]["code"],
        "validation.group-id-required"
    );
    node.shutdown().await;
}

/// UC: UC-DST-011
#[tokio::test]
async fn max_targets_refuses_the_whole_publication() {
    let node = TestNode::builder()
        .env("TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS", "1")
        .start()
        .await;
    queue(&node, "a1").await;
    queue(&node, "a2").await;
    create_topic(
        &node,
        json!({ "name": "todos", "type": "distributor", "prefixes": ["a"] }),
    )
    .await;

    let response = publish_raw(&node, "todos", "", None, b"1".to_vec()).await;
    assert_eq!(
        error_code(response, 422).await,
        "distributor.too-many-targets"
    );
    assert!(consume(&node, "a1", "").await.is_empty());
    node.shutdown().await;
}

/// UC: UC-TOP-039
#[tokio::test]
async fn deleting_a_queue_drops_its_ready_and_in_flight_messages() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    publish(&node, "pedidos", "2").await;
    assert_eq!(consume(&node, "pedidos", "?maxMessages=1").await.len(), 1);

    let response = node
        .client
        .delete(node.client_url("/topics/pedidos"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    queue(&node, "pedidos").await;
    node.clock.advance(Duration::from_secs(600));
    assert!(consume(&node, "pedidos", "").await.is_empty());
    node.shutdown().await;
}

async fn ack_raw(node: &TestNode, topic: &str, id: &str, query: &str) -> Response {
    node.client
        .delete(node.client_url(&format!("/topics/{topic}/messages/{id}{query}")))
        .send()
        .await
        .unwrap()
}

async fn ack(node: &TestNode, message: &Value) -> Response {
    let id = message["id"].as_str().unwrap();
    let receipt = message["receiptToken"].as_str().unwrap();
    ack_raw(node, "pedidos", id, &format!("?receiptToken={receipt}")).await
}

async fn set_visibility(node: &TestNode, m: &Value, body: Value) -> Response {
    let url = format!(
        "/topics/pedidos/messages/{}/visibility?receiptToken={}",
        m["id"].as_str().unwrap(),
        m["receiptToken"].as_str().unwrap()
    );
    node.client
        .put(node.client_url(&url))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn one_delivered(node: &TestNode, visibility: u32) -> Value {
    let mut messages = consume(node, "pedidos", &format!("?visibilityTimeout={visibility}")).await;
    assert_eq!(messages.len(), 1);
    messages.remove(0)
}

/// UC: UC-ACK-001, UC-ACK-003, UC-ACK-011
#[tokio::test]
async fn ack_removes_the_message_for_good() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 30).await;

    assert_eq!(ack(&node, &m).await.status(), 204);
    let again = ack(&node, &m).await;
    assert_eq!(error_code(again, 404).await, "message.not-found");

    node.clock.advance(Duration::from_secs(3600));
    assert!(
        consume(&node, "pedidos", "").await.is_empty(),
        "nem depois do prazo"
    );
    node.shutdown().await;
}

/// UC: UC-ACK-002, UC-ACK-007
#[tokio::test]
async fn ack_validates_the_request_and_the_topic() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    create_topic(
        &node,
        json!({ "name": "todos", "type": "distributor", "prefixes": ["a"] }),
    )
    .await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 30).await;
    let id = m["id"].as_str().unwrap();

    let response = ack_raw(&node, "pedidos", id, "").await;
    assert_eq!(error_code(response, 400).await, "validation.required");
    let response = ack_raw(&node, "todos", id, "?receiptToken=x").await;
    assert_eq!(error_code(response, 409).await, "topic.wrong-type");
    let response = ack_raw(&node, "nada", id, "?receiptToken=x").await;
    assert_eq!(error_code(response, 404).await, "topic.not-found");
    let response = ack_raw(&node, "pedidos", "nao-existe", "?receiptToken=x").await;
    assert_eq!(error_code(response, 404).await, "message.not-found");
    node.shutdown().await;
}

/// UC: UC-ACK-004, UC-ACK-005, UC-VIS-006
#[tokio::test]
async fn a_stale_token_is_409_and_changes_nothing() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    let first = one_delivered(&node, 10).await;

    node.clock.advance(Duration::from_secs(11));
    // Vencido mas ainda não reentregue: o token vale.
    let still = set_visibility(&node, &first, json!({ "visibilityTimeout": 10 })).await;
    assert_eq!(still.status(), 200);
    node.clock.advance(Duration::from_secs(10));

    let second = one_delivered(&node, 60).await;
    assert_eq!(second["deliveryCount"], 2);
    let response = ack(&node, &first).await;
    assert_eq!(error_code(response, 409).await, "message.stale-receipt");
    let response = set_visibility(&node, &first, json!({ "visibilityTimeout": 0 })).await;
    assert_eq!(error_code(response, 409).await, "message.stale-receipt");

    // Nada mudou: a mensagem segue com o segundo consumidor, que ainda pode confirmá-la.
    assert!(consume(&node, "pedidos", "").await.is_empty());
    assert_eq!(ack(&node, &second).await.status(), 204);
    node.shutdown().await;
}

/// UC: UC-ACK-005
#[tokio::test]
async fn ack_after_the_deadline_but_before_redelivery_works() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 10).await;
    node.clock.advance(Duration::from_secs(11));
    assert_eq!(ack(&node, &m).await.status(), 204);
    node.shutdown().await;
}

/// UC: UC-ACK-006, UC-VIS-007
#[tokio::test]
async fn a_message_past_its_ttl_cannot_be_acked_or_extended() {
    let node = TestNode::development().await;
    create_topic(
        &node,
        json!({ "name": "pedidos", "type": "queue", "ttl": 60 }),
    )
    .await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 600).await;
    node.clock.advance(Duration::from_secs(60));

    let response = set_visibility(&node, &m, json!({ "visibilityTimeout": 5 })).await;
    assert_eq!(error_code(response, 404).await, "message.not-found");
    let response = ack(&node, &m).await;
    assert_eq!(error_code(response, 404).await, "message.not-found");
    node.shutdown().await;
}

/// UC: UC-VIS-001, UC-VIS-002, UC-VIS-004, UC-VIS-009, UC-VIS-010
#[tokio::test]
async fn visibility_replaces_the_deadline_and_keeps_count_and_token() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 30).await;

    node.clock.advance(Duration::from_secs(20));
    let response = set_visibility(&node, &m, json!({ "visibilityTimeout": 100 })).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["id"], m["id"]);
    assert_eq!(body["invisibleUntil"], "2026-10-06T12:02:00.000Z");
    assert_eq!(body.as_object().unwrap().len(), 2);
    let same = set_visibility(&node, &m, json!({ "visibilityTimeout": 100 })).await;
    assert_eq!(same.json::<Value>().await.unwrap(), body, "idempotente");

    node.clock.advance(Duration::from_secs(99));
    assert!(
        consume(&node, "pedidos", "").await.is_empty(),
        "o prazo novo ainda não venceu"
    );
    let response = set_visibility(&node, &m, json!({ "visibilityTimeout": 1 })).await;
    assert_eq!(response.status(), 200, "o token continua o mesmo");
    node.clock.advance(Duration::from_secs(1));
    let back = one_delivered(&node, 30).await;
    assert_eq!(back["deliveryCount"], 2, "o PUT não conta entrega");
    node.shutdown().await;
}

/// UC: UC-VIS-003
#[tokio::test]
async fn visibility_zero_releases_the_message_for_the_next_get() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 600).await;

    let response = set_visibility(&node, &m, json!({ "visibilityTimeout": 0 })).await;
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["invisibleUntil"], "2026-10-06T12:00:00.000Z");
    let again = consume(&node, "pedidos", "").await;
    assert_eq!(again[0]["id"], m["id"]);
    node.shutdown().await;
}

/// UC: UC-VIS-005
#[tokio::test]
async fn visibility_validates_query_and_body() {
    let node = TestNode::builder()
        .env("TABA_DELIVERY_VISIBILITY_TIMEOUT_MAX", "300s")
        .start()
        .await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 30).await;
    let id = m["id"].as_str().unwrap();
    let token = m["receiptToken"].as_str().unwrap();
    let with_token = format!("?receiptToken={token}");
    let put = |query: &str, body: Value| {
        let url = node.client_url(&format!("/topics/pedidos/messages/{id}/visibility{query}"));
        let request = node.client.put(url).json(&body);
        async move { request.send().await.unwrap() }
    };

    let response = put(&with_token, json!({})).await;
    assert_eq!(error_code(response, 400).await, "validation.required");
    let response = put("", json!({ "visibilityTimeout": 5 })).await;
    assert_eq!(error_code(response, 400).await, "validation.required");
    let response = put(&with_token, json!({ "visibilityTimeout": 301 })).await;
    assert_eq!(error_code(response, 400).await, "validation.out-of-range");
    let response = put(&with_token, json!({ "visibilityTimeout": -1 })).await;
    assert_eq!(error_code(response, 400).await, "validation.out-of-range");
    let response = put(&with_token, json!({ "visibilityTimeout": 5, "x": 1 })).await;
    assert_eq!(error_code(response, 400).await, "validation.unknown-field");
    let response = put(&with_token, json!({ "visibilityTimeout": "5" })).await;
    assert_eq!(error_code(response, 400).await, "validation.failed");
    node.shutdown().await;
}

/// UC: UC-ACK-012, UC-VIS-007
#[tokio::test]
async fn ack_and_visibility_after_the_topic_is_removed_are_404() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 600).await;
    node.client
        .delete(node.client_url("/topics/pedidos"))
        .send()
        .await
        .unwrap();

    let response = ack(&node, &m).await;
    assert_eq!(error_code(response, 404).await, "topic.not-found");
    let response = set_visibility(&node, &m, json!({ "visibilityTimeout": 5 })).await;
    assert_eq!(error_code(response, 404).await, "topic.not-found");
    node.shutdown().await;
}

async fn limited_queue(node: &TestNode, name: &str, max_deliveries: u32) {
    create_topic(
        node,
        json!({ "name": name, "type": "queue", "maxDeliveries": max_deliveries }),
    )
    .await;
}

async fn consume_from(node: &TestNode, topic: &str, query: &str) -> Vec<Value> {
    consume(node, topic, query).await
}

/// UC: UC-RET-008, UC-RET-009
#[tokio::test]
async fn the_message_goes_to_the_dead_letter_after_max_deliveries_without_ack() {
    let node = TestNode::development().await;
    limited_queue(&node, "pedidos", 2).await;
    let id = publish(&node, "pedidos", r#"{ "pedido": 1 }"#).await["id"].clone();

    for expected in [1, 2] {
        let m = one_delivered(&node, 10).await;
        assert_eq!(m["deliveryCount"], expected);
        node.clock.advance(Duration::from_secs(10));
    }
    assert!(
        consume(&node, "pedidos", "").await.is_empty(),
        "a terceira entrega nunca acontece"
    );

    let dead = consume_from(&node, "pedidos-dead-letter", "?visibilityTimeout=30").await;
    assert_eq!(dead.len(), 1);
    let m = &dead[0];
    assert_eq!(m["id"], id, "mantém o mesmo id");
    assert_eq!(m["body"], json!({ "pedido": 1 }));
    assert_eq!(
        m["deliveryCount"], 1,
        "zerou ao entrar; esta é a primeira entrega lá"
    );
    assert_eq!(m["deadLetteredAt"], "2026-10-06T12:00:20.000Z");
    assert_eq!(m["deadLetterReason"], "max-deliveries");
    node.shutdown().await;
}

/// UC: UC-RET-008
#[tokio::test]
async fn an_acked_message_does_not_reach_the_dead_letter() {
    let node = TestNode::development().await;
    limited_queue(&node, "pedidos", 1).await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 10).await;
    assert_eq!(ack(&node, &m).await.status(), 204);
    node.clock.advance(Duration::from_secs(600));
    assert!(consume(&node, "pedidos-dead-letter", "").await.is_empty());
    node.shutdown().await;
}

/// UC: UC-VIS-011
#[tokio::test]
async fn visibility_zero_on_the_last_delivery_sends_the_message_to_the_dead_letter() {
    let node = TestNode::development().await;
    limited_queue(&node, "pedidos", 1).await;
    publish(&node, "pedidos", "1").await;
    let m = one_delivered(&node, 600).await;

    let response = set_visibility(&node, &m, json!({ "visibilityTimeout": 0 })).await;
    assert_eq!(response.status(), 200);
    assert!(consume(&node, "pedidos", "").await.is_empty());
    let dead = consume_from(&node, "pedidos-dead-letter", "").await;
    assert_eq!(dead[0]["id"], m["id"]);
    node.shutdown().await;
}

/// UC: UC-RET-014
#[tokio::test]
async fn a_corrected_message_can_be_published_back_and_the_dead_one_acked() {
    let node = TestNode::development().await;
    limited_queue(&node, "pedidos", 1).await;
    publish(&node, "pedidos", r#"{ "ruim": true }"#).await;
    one_delivered(&node, 10).await;
    node.clock.advance(Duration::from_secs(10));

    let dead = consume_from(&node, "pedidos-dead-letter", "?visibilityTimeout=60").await;
    publish(&node, "pedidos", r#"{ "ruim": false }"#).await;
    let response = ack_raw(
        &node,
        "pedidos-dead-letter",
        dead[0]["id"].as_str().unwrap(),
        &format!(
            "?receiptToken={}",
            dead[0]["receiptToken"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(response.status(), 204);

    assert_eq!(
        consume(&node, "pedidos", "").await[0]["body"],
        json!({ "ruim": false })
    );
    assert!(consume(&node, "pedidos-dead-letter", "").await.is_empty());
    node.shutdown().await;
}

/// UC: UC-RET-013
#[tokio::test]
async fn the_dead_letter_never_has_its_own_dead_letter() {
    let node = TestNode::development().await;
    limited_queue(&node, "pedidos", 1).await;
    publish(&node, "pedidos", "1").await;
    one_delivered(&node, 10).await;
    node.clock.advance(Duration::from_secs(10));

    for expected in 1..=4 {
        let m = consume_from(&node, "pedidos-dead-letter", "?visibilityTimeout=10").await;
        assert_eq!(
            m[0]["deliveryCount"], expected,
            "segue na dead-letter, sem limite"
        );
        node.clock.advance(Duration::from_secs(10));
    }
    node.shutdown().await;
}

/// UC: UC-RET-016
#[tokio::test]
async fn a_distributor_never_copies_into_a_dead_letter() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    create_topic(
        &node,
        json!({ "name": "todos", "type": "distributor", "prefixes": ["pedidos"] }),
    )
    .await;

    let response = publish_raw(&node, "todos", "", None, b"1".to_vec()).await;
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["matched"], 1);
    assert_eq!(body["deliveries"][0]["topic"], "pedidos");
    assert!(consume(&node, "pedidos-dead-letter", "").await.is_empty());
    node.shutdown().await;
}

/// UC: UC-RET-001, UC-RET-002, UC-RET-004, UC-TOP-030
#[tokio::test]
async fn a_shorter_ttl_expires_what_is_already_stored_even_in_flight() {
    let node = TestNode::development().await;
    limited_queue(&node, "pedidos", 1).await;
    publish(&node, "pedidos", "1").await;
    publish(&node, "pedidos", "2").await;
    let in_flight = consume(&node, "pedidos", "?maxMessages=1&visibilityTimeout=600").await;
    node.clock.advance(Duration::from_secs(120));
    assert!(
        !consume(&node, "pedidos", "?visibilityTimeout=0")
            .await
            .is_empty()
    );

    let response = node
        .client
        .patch(node.client_url("/topics/pedidos"))
        .json(&json!({ "ttl": 60 }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    assert!(consume(&node, "pedidos", "").await.is_empty());
    let response = ack(&node, &in_flight[0]).await;
    assert_eq!(error_code(response, 404).await, "message.not-found");
    assert!(
        consume(&node, "pedidos-dead-letter", "").await.is_empty(),
        "o que vence por ttl não vai à dead-letter"
    );
    node.shutdown().await;
}

/// UC: UC-RET-005, UC-RET-008
#[tokio::test]
async fn the_background_sweeper_moves_it_even_if_nobody_touches_the_source_queue() {
    let node = TestNode::development().await;
    limited_queue(&node, "pedidos", 1).await;
    publish(&node, "pedidos", "1").await;
    one_delivered(&node, 10).await;
    node.clock.advance(Duration::from_secs(10));

    let started = std::time::Instant::now();
    let dead = consume_from(&node, "pedidos-dead-letter", "?waitTime=10").await;
    assert_eq!(
        dead.len(),
        1,
        "a varredura da tarefa de fundo moveu a mensagem"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    node.shutdown().await;
}

fn body_files(node: &TestNode, topic: &str) -> usize {
    std::fs::read_dir(node.storage_dir().join(topic))
        .map(|dir| {
            dir.filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "body"))
                .count()
        })
        .unwrap_or(0)
}

/// UC: UC-ARM-001, UC-ARM-002, UC-ARM-003, UC-ARM-010, UC-ARM-014
#[tokio::test]
async fn messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact() {
    let node = TestNode::builder()
        .env("TABA_MEMORY_LIMIT", "10")
        .start()
        .await;
    queue(&node, "arquivos").await;
    let all_bytes: Vec<u8> = (0..=255).collect();

    let small = publish_raw(&node, "arquivos", "", Some("text/plain"), b"curta".to_vec()).await;
    assert_eq!(small.status(), 201);
    assert_eq!(body_files(&node, "arquivos"), 0, "coube em memória");
    let big = publish_raw(&node, "arquivos", "", Some("image/png"), all_bytes.clone()).await;
    assert_eq!(big.status(), 201);
    assert_eq!(
        body_files(&node, "arquivos"),
        1,
        "o excedente virou um .body"
    );
    assert!(
        node.storage_dir()
            .join("arquivos")
            .join("_topic.db")
            .is_file()
    );

    // O arquivo guarda só os bytes, sem reserializar nem codificar.
    let id = big.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let on_disk = std::fs::read(
        node.storage_dir()
            .join("arquivos")
            .join(format!("{id}.body")),
    )
    .unwrap();
    assert_eq!(on_disk, all_bytes);

    let messages = consume(&node, "arquivos", "?maxMessages=10&visibilityTimeout=30").await;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["body"], "curta");
    assert_eq!(messages[1]["id"], id.as_str());
    assert_eq!(messages[1]["contentType"], "image/png");
    assert_eq!(
        STANDARD
            .decode(messages[1]["body"].as_str().unwrap())
            .unwrap(),
        all_bytes
    );

    // O ack remove a linha e o arquivo.
    let response = ack_raw(
        &node,
        "arquivos",
        &id,
        &format!(
            "?receiptToken={}",
            messages[1]["receiptToken"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(response.status(), 204);
    assert_eq!(body_files(&node, "arquivos"), 0);
    node.shutdown().await;
}

/// UC: UC-ARM-001
#[tokio::test]
async fn without_a_memory_limit_nothing_goes_to_disk_by_overflow() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    for i in 0..20 {
        publish(&node, "pedidos", &i.to_string()).await;
    }
    assert!(!node.storage_dir().join("pedidos").exists());
    node.shutdown().await;
}

/// UC: UC-DRN-010, UC-DRN-021, UC-DRN-022, UC-DRN-023, UC-DRN-024
#[tokio::test]
async fn stopping_saves_memory_to_disk_and_the_next_start_resumes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("taba").display().to_string();
    let start = || {
        TestNode::builder()
            .env("TABA_STORAGE_DIR", &storage)
            .try_start()
    };

    let node = start().await.unwrap();
    queue(&node, "pedidos").await;
    let mut grouped = publish_raw(
        &node,
        "pedidos",
        "?groupId=g1",
        Some("text/plain"),
        b"em processamento".to_vec(),
    )
    .await
    .json::<Value>()
    .await
    .unwrap();
    let ready = publish(&node, "pedidos", r#"{ "pronta": true }"#).await;
    let in_flight = consume(&node, "pedidos", "?maxMessages=1&visibilityTimeout=600").await;
    assert_eq!(in_flight[0]["id"], grouped["id"]);
    assert_eq!(body_files(&node, "pedidos"), 0, "tudo em memória até aqui");
    node.shutdown().await; // o drain grava tudo em disco
    grouped = in_flight[0].clone();

    let node = start().await.unwrap();
    // O que estava em processamento segue invisível, e o mesmo token ainda confirma.
    let pending = consume(&node, "pedidos", "?maxMessages=10&visibilityTimeout=30").await;
    assert_eq!(pending.len(), 1, "só a pronta");
    assert_eq!(pending[0]["id"], ready["id"]);
    assert_eq!(pending[0]["body"], json!({ "pronta": true }));
    let id = grouped["id"].as_str().unwrap();
    let token = grouped["receiptToken"].as_str().unwrap();
    let response = ack_raw(&node, "pedidos", id, &format!("?receiptToken={token}")).await;
    assert_eq!(response.status(), 204, "o receiptToken sobreviveu ao save");

    // O prazo é um instante absoluto: passado o tempo, a mensagem volta com o contador certo.
    node.clock.advance(Duration::from_secs(60));
    let again = consume(&node, "pedidos", "").await;
    assert_eq!(again[0]["id"], ready["id"]);
    assert_eq!(again[0]["deliveryCount"], 2);
    node.shutdown().await;
}

/// UC: UC-DRN-003, UC-DRN-026
#[tokio::test]
async fn without_drain_on_stop_what_was_only_in_memory_is_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("taba").display().to_string();
    let start = || {
        TestNode::builder()
            .env("TABA_STORAGE_DIR", &storage)
            .env("TABA_DRAIN_ON_STOP", "false")
            .try_start()
    };
    let node = start().await.unwrap();
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    node.shutdown().await;

    let node = start().await.unwrap();
    assert_eq!(
        node.client_get("/topics/pedidos").await.status(),
        200,
        "o tópico sobreviveu"
    );
    assert!(
        consume(&node, "pedidos", "").await.is_empty(),
        "a mensagem só em memória se perdeu"
    );
    node.shutdown().await;
}

/// UC: UC-DRN-027
#[tokio::test]
async fn without_resume_on_start_the_messages_on_disk_are_erased() {
    let tmp = tempfile::tempdir().unwrap();
    let storage = tmp.path().join("taba").display().to_string();
    let node = TestNode::builder()
        .env("TABA_STORAGE_DIR", &storage)
        .start()
        .await;
    queue(&node, "pedidos").await;
    publish(&node, "pedidos", "1").await;
    node.shutdown().await;
    assert!(
        tmp.path().join("taba").join("pedidos").is_dir(),
        "o drain deixou em disco"
    );

    let node = TestNode::builder()
        .env("TABA_STORAGE_DIR", &storage)
        .env("TABA_DEVELOPMENT_RESUME_ON_START", "false")
        .start()
        .await;
    assert!(!tmp.path().join("taba").join("pedidos").exists());
    queue(&node, "pedidos").await;
    assert!(consume(&node, "pedidos", "").await.is_empty());
    node.shutdown().await;
}

/// UC: UC-RET-008, UC-ACK-013
#[tokio::test]
async fn the_dead_letter_receives_the_body_file_of_a_message_on_disk() {
    let node = TestNode::builder()
        .env("TABA_MEMORY_LIMIT", "1")
        .start()
        .await;
    create_topic(
        &node,
        json!({ "name": "pedidos", "type": "queue", "maxDeliveries": 1 }),
    )
    .await;
    let id = publish(&node, "pedidos", r#"{ "d": 1 }"#).await["id"].clone();
    assert_eq!(body_files(&node, "pedidos"), 1);
    one_delivered(&node, 10).await;
    node.clock.advance(Duration::from_secs(10));

    let dead = consume_from(&node, "pedidos-dead-letter", "?waitTime=5").await;
    assert_eq!(dead[0]["id"], id);
    assert_eq!(dead[0]["body"], json!({ "d": 1 }));
    assert_eq!(dead[0]["deadLetterReason"], "max-deliveries");
    assert_eq!(body_files(&node, "pedidos"), 0);
    assert_eq!(body_files(&node, "pedidos-dead-letter"), 1);
    node.shutdown().await;
}

/// UC: UC-GRP-010, UC-GRP-002, UC-GRP-003
#[tokio::test]
async fn a_noisy_producer_does_not_delay_a_small_one_over_http() {
    let node = TestNode::development().await;
    queue(&node, "pedidos").await;
    for i in 0..100 {
        let r = publish_raw(
            &node,
            "pedidos",
            "?groupId=barulhento",
            None,
            i.to_string().into_bytes(),
        )
        .await;
        assert_eq!(r.status(), 201);
    }
    for i in 0..3 {
        let r = publish_raw(
            &node,
            "pedidos",
            "?groupId=pequeno",
            None,
            i.to_string().into_bytes(),
        )
        .await;
        assert_eq!(r.status(), 201);
    }
    let first = consume(&node, "pedidos", "?maxMessages=3&visibilityTimeout=600").await;
    let groups: Vec<&str> = first
        .iter()
        .map(|m| m["groupId"].as_str().unwrap())
        .collect();
    assert_eq!(
        groups,
        ["pequeno", "pequeno", "pequeno"],
        "o grupo vai junto com a mensagem"
    );
    // Numa fila comum o grupo não serializa: várias do mesmo grupo vão juntas.
    let more = consume(&node, "pedidos", "?maxMessages=4&visibilityTimeout=600").await;
    assert_eq!(more.len(), 4);
    node.shutdown().await;
}
