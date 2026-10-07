use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::config::{Config, MapEnv};
use crate::core::clock::ManualClock;
use crate::core::id;
use crate::engine::topics::{CreateTopic, Kind, TopicRules};
use crate::storage::topics::MemoryTopicStore;

struct Harness {
    topics: Arc<Topics>,
    messages: Arc<Messages>,
    clock: Arc<ManualClock>,
}

fn harness_with(env: &[(&str, &str)]) -> Harness {
    let mut vars = vec![("TABA_MODE", "development")];
    vars.extend_from_slice(env);
    let config = Config::parse(
        None,
        &MapEnv(
            vars.into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        ),
    )
    .unwrap();
    let clock = Arc::new(ManualClock::at_doc_epoch());
    let topics = Arc::new(Topics::new(
        TopicRules::from_config(&config),
        Arc::new(MemoryTopicStore::default()),
        clock.clone(),
        Vec::new(),
    ));
    let messages = Messages::new(
        topics.clone(),
        clock.clone(),
        MessageRules::from_config(&config),
    );
    Harness {
        topics,
        messages,
        clock,
    }
}

fn harness() -> Harness {
    harness_with(&[])
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

fn json_body(text: &str) -> PublishRequest {
    PublishRequest {
        body: text.as_bytes().to_vec(),
        ..Default::default()
    }
}

fn published_id(published: Published) -> String {
    match published {
        Published::Queue { id } => id,
        Published::Distributor { .. } => panic!("esperava uma fila"),
    }
}

fn publish_ok(h: &Harness, topic: &str, body: &str) -> String {
    published_id(h.messages.publish(topic, json_body(body)).unwrap())
}

async fn consume(h: &Harness, topic: &str, query: ConsumeQuery) -> Vec<Delivered> {
    h.messages.consume(topic, query).await.unwrap()
}

async fn consume_err(h: &Harness, topic: &str, query: ConsumeQuery) -> ErrorCode {
    h.messages.consume(topic, query).await.unwrap_err().code
}

fn publish_err(h: &Harness, topic: &str, request: PublishRequest) -> ErrorCode {
    h.messages.publish(topic, request).unwrap_err().code
}

fn q(max: Option<&str>, visibility: Option<&str>, wait: Option<&str>) -> ConsumeQuery {
    ConsumeQuery {
        max_messages: max.map(Into::into),
        visibility_timeout: visibility.map(Into::into),
        wait_time: wait.map(Into::into),
    }
}

/// UC: UC-MSG-001, UC-MSG-013
#[tokio::test]
async fn publishing_returns_increasing_guid_v7_ids() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    let ids: Vec<String> = (0..50)
        .map(|i| publish_ok(&h, "pedidos", &i.to_string()))
        .collect();
    assert!(ids.iter().all(|i| id::is_guid_v7(i)));
    assert!(ids.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(id::unix_millis_of(&ids[0]), Some(h.clock.now_millis()));
}

/// UC: UC-MSG-010, UC-CNS-018
#[tokio::test]
async fn unknown_topics_are_404() {
    let h = harness();
    assert_eq!(
        publish_err(&h, "nada", json_body("1")),
        ErrorCode::TopicNotFound
    );
    assert_eq!(
        h.messages.publish_limit("nada").unwrap_err().code,
        ErrorCode::TopicNotFound
    );
    assert_eq!(
        consume_err(&h, "nada", ConsumeQuery::default()).await,
        ErrorCode::TopicNotFound
    );
}

/// UC: UC-MSG-004, UC-MSG-005, UC-MSG-006
#[tokio::test]
async fn publishing_validates_content_type_and_body() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    let with = |content_type: Option<&str>, body: &[u8]| PublishRequest {
        body: body.to_vec(),
        content_type: content_type.map(Into::into),
        ..Default::default()
    };
    assert_eq!(
        publish_err(&h, "pedidos", with(Some("nao-e-media-type"), b"x")),
        ErrorCode::ValidationInvalidContentType
    );
    assert_eq!(
        publish_err(&h, "pedidos", with(None, b"{ nao")),
        ErrorCode::ValidationInvalidBody
    );
    assert_eq!(
        publish_err(&h, "pedidos", with(Some("text/plain"), &[0xff])),
        ErrorCode::ValidationInvalidBody
    );
    assert!(
        h.messages
            .publish("pedidos", with(Some("image/png"), &[0xff]))
            .is_ok()
    );
}

/// UC: UC-MSG-008, UC-MSG-009, UC-TOP-031
#[tokio::test]
async fn size_counts_bytes_against_the_topic_limit_or_the_general_one() {
    let h = harness();
    let mut small = queue("pequena");
    small.max_message_bytes = Some(5);
    h.topics.create(small).await.unwrap();
    h.topics.create(queue("padrao")).await.unwrap();

    assert!(h.messages.publish("pequena", json_body("\"abc\"")).is_ok());
    assert_eq!(
        publish_err(&h, "pequena", json_body("\"abcd\"")),
        ErrorCode::MessageTooLarge
    );
    assert_eq!(h.messages.publish_limit("pequena").unwrap(), 5);
    assert_eq!(h.messages.publish_limit("padrao").unwrap(), 5 * 1024 * 1024);
}

/// UC: UC-MSG-011, UC-MSG-012
#[tokio::test]
async fn group_id_is_validated_and_deduplication_is_fifo_only() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    let with = |group: Option<&str>, dedup: Option<&str>| PublishRequest {
        body: b"1".to_vec(),
        group_id: group.map(Into::into),
        deduplication_id: dedup.map(Into::into),
        ..Default::default()
    };
    assert!(
        h.messages
            .publish("pedidos", with(Some("cliente-42"), None))
            .is_ok()
    );
    for bad in ["", "com espaço", &"x".repeat(129)] {
        assert_eq!(
            publish_err(&h, "pedidos", with(Some(bad), None)),
            ErrorCode::ValidationFailed,
            "{bad:?}"
        );
    }
    assert_eq!(
        publish_err(&h, "pedidos", with(None, Some("d1"))),
        ErrorCode::ValidationNotApplicable
    );
}

/// UC: UC-FIF-002
#[tokio::test]
async fn a_fifo_queue_requires_the_group_id() {
    let h = harness();
    let mut fifo = queue("ordem");
    fifo.fifo = Some(true);
    h.topics.create(fifo).await.unwrap();
    assert_eq!(
        publish_err(&h, "ordem", json_body("1")),
        ErrorCode::ValidationGroupIdRequired
    );
    let ok = PublishRequest {
        body: b"1".to_vec(),
        group_id: Some("g".into()),
        deduplication_id: Some("d".into()),
        ..Default::default()
    };
    assert!(h.messages.publish("ordem", ok).is_ok());
}

/// UC: UC-CNS-001, UC-CNS-002
#[tokio::test]
async fn consuming_delivers_the_message_with_its_metadata() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );

    let id = publish_ok(&h, "pedidos", r#"{"a": 1}"#);
    let got = consume(&h, "pedidos", q(None, Some("60"), None)).await;
    assert_eq!(got.len(), 1);
    let m = &got[0];
    assert_eq!(m.id, id);
    assert_eq!(m.delivery_count, 1);
    assert!(id::is_guid_v7(&m.receipt_token));
    assert_eq!(m.content_type, "application/json");
    assert_eq!(m.published_ms, h.clock.now_millis());
    assert_eq!(m.invisible_until_ms, h.clock.now_millis() + 60_000);
}

/// UC: UC-CNS-003, UC-CNS-004
#[tokio::test]
async fn max_messages_defaults_to_10_and_must_be_in_range() {
    let h = harness_with(&[("TABA_DELIVERY_MAX_MESSAGES_MAX", "20")]);
    h.topics.create(queue("pedidos")).await.unwrap();
    for i in 0..30 {
        publish_ok(&h, "pedidos", &i.to_string());
    }
    assert_eq!(
        consume(&h, "pedidos", ConsumeQuery::default()).await.len(),
        10
    );
    assert_eq!(
        consume(&h, "pedidos", q(Some("20"), None, None))
            .await
            .len(),
        20
    );
    assert!(
        consume(&h, "pedidos", q(Some("5"), None, None))
            .await
            .is_empty()
    );

    for bad in ["0", "21", "-1"] {
        assert_eq!(
            consume_err(&h, "pedidos", q(Some(bad), None, None)).await,
            ErrorCode::ValidationOutOfRange,
            "{bad}"
        );
    }
    assert_eq!(
        consume_err(&h, "pedidos", q(Some("dez"), None, None)).await,
        ErrorCode::ValidationFailed
    );
}

/// UC: UC-CNS-004
#[tokio::test]
async fn consuming_returns_what_is_ready_without_waiting_to_fill_the_batch() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    publish_ok(&h, "pedidos", "2");
    assert_eq!(
        consume(&h, "pedidos", q(Some("10"), None, None))
            .await
            .len(),
        2
    );
}

/// UC: UC-CNS-005, UC-CNS-006, UC-CNS-007
#[tokio::test]
async fn a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");

    let first = consume(&h, "pedidos", q(None, Some("30"), None)).await;
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );

    h.clock.advance(Duration::from_secs(29));
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );

    h.clock.advance(Duration::from_secs(1));
    let again = consume(&h, "pedidos", ConsumeQuery::default()).await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, first[0].id);
    assert_eq!(again[0].delivery_count, 2);
    assert_ne!(again[0].receipt_token, first[0].receipt_token);
    assert_eq!(again[0].invisible_until_ms, h.clock.now_millis() + 120_000);
}

/// UC: UC-CNS-005
#[tokio::test]
async fn visibility_timeout_zero_makes_the_message_deliverable_again_at_once() {
    let h = harness_with(&[("TABA_DELIVERY_VISIBILITY_TIMEOUT_MAX", "300s")]);
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    assert_eq!(
        consume(&h, "pedidos", q(None, Some("0"), None)).await.len(),
        1
    );
    assert_eq!(
        consume(&h, "pedidos", q(None, Some("0"), None)).await.len(),
        1
    );
    assert_eq!(
        consume_err(&h, "pedidos", q(None, Some("301"), None)).await,
        ErrorCode::ValidationOutOfRange
    );
    assert_eq!(
        consume_err(&h, "pedidos", q(None, Some("-5"), None)).await,
        ErrorCode::ValidationOutOfRange
    );
}

/// UC: UC-CNS-015
#[tokio::test]
async fn a_message_past_its_ttl_is_not_delivered() {
    let h = harness();
    let mut short = queue("curta");
    short.ttl = Some(60);
    h.topics.create(short).await.unwrap();
    publish_ok(&h, "curta", "1");
    h.clock.advance(Duration::from_secs(60));
    assert!(
        consume(&h, "curta", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-CNS-008, UC-CNS-009, UC-CNS-010, UC-CNS-012, UC-MSG-020
#[tokio::test]
async fn each_message_keeps_its_content_type_and_group_in_a_mixed_batch() {
    let h = harness();
    h.topics.create(queue("arquivos")).await.unwrap();
    let publish = |content_type: &str, body: &[u8], group: Option<&str>| {
        h.messages
            .publish(
                "arquivos",
                PublishRequest {
                    body: body.to_vec(),
                    content_type: Some(content_type.into()),
                    group_id: group.map(Into::into),
                    ..Default::default()
                },
            )
            .unwrap()
    };
    publish("text/plain; charset=utf-8", "Pedido 9001".as_bytes(), None);
    publish("image/png", &[0x89, b'P', b'N', b'G'], Some("g1"));
    publish("application/json", br#"{ "pedido": 9001 }"#, None);

    let got = consume(&h, "arquivos", ConsumeQuery::default()).await;
    let bodies: Vec<String> = got
        .iter()
        .map(|m| serde_json::to_string(&m.body).unwrap())
        .collect();
    assert_eq!(
        bodies,
        ["\"Pedido 9001\"", "\"iVBORw==\"", r#"{ "pedido": 9001 }"#]
    );
    assert_eq!(got[0].content_type, "text/plain; charset=utf-8");
    assert_eq!(got[1].group_id.as_deref(), Some("g1"));
    assert_eq!(got[2].group_id, None);
}

/// UC: UC-CNS-017
#[tokio::test]
async fn a_distributor_cannot_be_consumed() {
    let h = harness();
    h.topics.create(distributor("todos", &["a"])).await.unwrap();
    assert_eq!(
        consume_err(&h, "todos", ConsumeQuery::default()).await,
        ErrorCode::TopicWrongType
    );
}

/// UC: UC-CNS-016
#[tokio::test]
async fn the_dead_letter_is_a_common_queue_for_publish_and_consume() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    let id = publish_ok(&h, "pedidos-dead-letter", "1");
    let got = consume(&h, "pedidos-dead-letter", ConsumeQuery::default()).await;
    assert_eq!(got[0].id, id);
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-DST-001, UC-DST-002, UC-DST-003
#[tokio::test]
async fn a_distributor_copies_the_message_to_every_matching_queue() {
    let h = harness();
    for name in ["pedidos-eu", "pedidos-us", "outra"] {
        h.topics.create(queue(name)).await.unwrap();
    }
    h.topics
        .create(distributor("todos", &["pedidos-"]))
        .await
        .unwrap();
    h.topics
        .create(distributor("nenhum", &["zzz"]))
        .await
        .unwrap();

    let Published::Distributor { deliveries } =
        h.messages.publish("todos", json_body("1")).unwrap()
    else {
        panic!("esperava um distribuidor");
    };
    let targets: Vec<&str> = deliveries.iter().map(|d| d.topic.as_str()).collect();
    assert_eq!(targets, ["pedidos-eu", "pedidos-us"]);
    let ids: Vec<&String> = deliveries
        .iter()
        .map(|d| d.result.as_ref().unwrap())
        .collect();
    assert_ne!(ids[0], ids[1], "cada cópia tem o seu id");
    assert_eq!(
        consume(&h, "pedidos-eu", ConsumeQuery::default())
            .await
            .len(),
        1
    );
    assert_eq!(
        consume(&h, "pedidos-us", ConsumeQuery::default())
            .await
            .len(),
        1
    );
    assert!(
        consume(&h, "outra", ConsumeQuery::default())
            .await
            .is_empty()
    );

    let Published::Distributor { deliveries } =
        h.messages.publish("nenhum", json_body("1")).unwrap()
    else {
        panic!("esperava um distribuidor");
    };
    assert!(deliveries.is_empty(), "zero filas não é erro");
}

/// UC: UC-DST-011
#[tokio::test]
async fn a_distributor_over_max_targets_refuses_the_whole_publication() {
    let h = harness_with(&[("TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS", "1")]);
    for name in ["a1", "a2"] {
        h.topics.create(queue(name)).await.unwrap();
    }
    h.topics.create(distributor("todos", &["a"])).await.unwrap();
    assert_eq!(
        publish_err(&h, "todos", json_body("1")),
        ErrorCode::DistributorTooManyTargets
    );
    assert!(consume(&h, "a1", ConsumeQuery::default()).await.is_empty());
}

/// UC: UC-DST-009
#[tokio::test]
async fn a_distributor_copy_to_a_fifo_queue_without_group_fails_only_that_queue() {
    let h = harness();
    h.topics.create(queue("p-comum")).await.unwrap();
    let mut fifo = queue("p-fifo");
    fifo.fifo = Some(true);
    h.topics.create(fifo).await.unwrap();
    h.topics
        .create(distributor("todos", &["p-"]))
        .await
        .unwrap();

    let Published::Distributor { deliveries } =
        h.messages.publish("todos", json_body("1")).unwrap()
    else {
        panic!("esperava um distribuidor");
    };
    assert!(
        deliveries[1]
            .result
            .as_ref()
            .is_err_and(|e| e.code == ErrorCode::ValidationGroupIdRequired)
    );
    assert!(deliveries[0].result.is_ok());
}

/// UC: UC-TOP-039
#[tokio::test]
async fn deleting_a_queue_drops_its_messages_and_those_of_its_dead_letter() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    publish_ok(&h, "pedidos-dead-letter", "2");

    h.topics.delete("pedidos").await.unwrap();
    h.topics.create(queue("pedidos")).await.unwrap();
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );
    assert!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-CNS-024
#[tokio::test]
async fn wait_time_is_limited_to_0_20() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    for bad in ["21", "-1"] {
        assert_eq!(
            consume_err(&h, "pedidos", q(None, None, Some(bad))).await,
            ErrorCode::ValidationOutOfRange
        );
    }
}

/// UC: UC-CNS-025, UC-CNS-027
#[tokio::test]
async fn long_polling_returns_as_soon_as_a_message_arrives() {
    let h = Arc::new(harness());
    h.topics.create(queue("pedidos")).await.unwrap();
    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume(&h, "pedidos", q(Some("5"), None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiter.is_finished(), "ainda esperando");
    publish_ok(&h, "pedidos", "1");

    let got = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("não esperou o fim do waitTime")
        .unwrap();
    assert_eq!(
        got.len(),
        1,
        "devolve a primeira, sem esperar completar o lote"
    );
}

/// UC: UC-CNS-026
#[tokio::test]
async fn long_polling_ends_with_an_empty_list() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    let started = std::time::Instant::now();
    assert!(
        consume(&h, "pedidos", q(None, None, Some("1")))
            .await
            .is_empty()
    );
    assert!(started.elapsed() >= Duration::from_millis(900));
}

/// UC: UC-CNS-025
#[tokio::test]
async fn long_polling_notices_a_message_whose_invisibility_ends() {
    let h = Arc::new(harness());
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    consume(&h, "pedidos", q(None, Some("1"), None)).await;

    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume(&h, "pedidos", q(None, None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.clock.advance(Duration::from_secs(1));
    let got = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("a mensagem voltou a ser entregável")
        .unwrap();
    assert_eq!(got[0].delivery_count, 2);
}

/// UC: UC-CNS-018
#[tokio::test]
async fn deleting_the_topic_ends_a_long_poll_with_not_found() {
    let h = Arc::new(harness());
    h.topics.create(queue("pedidos")).await.unwrap();
    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume_err(&h, "pedidos", q(None, None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.topics.delete("pedidos").await.unwrap();
    let code = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(code, ErrorCode::TopicNotFound);
}

async fn delivered_one(h: &Harness, topic: &str, visibility: &str) -> Delivered {
    consume(h, topic, q(None, Some(visibility), None))
        .await
        .remove(0)
}

/// UC: UC-ACK-001, UC-ACK-003
#[tokio::test]
async fn ack_removes_the_message_and_a_second_ack_is_not_found() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "30").await;

    h.messages
        .ack("pedidos", &m.id, Some(&m.receipt_token))
        .unwrap();
    assert_eq!(
        h.messages
            .ack("pedidos", &m.id, Some(&m.receipt_token))
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
    h.clock.advance(Duration::from_secs(600));
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-ACK-002, UC-VIS-005
#[tokio::test]
async fn missing_fields_are_reported_together() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    let err = h.messages.ack("pedidos", "x", None).unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationRequired);
    assert_eq!(err.details[0].field, "receiptToken");

    let err = h
        .messages
        .set_visibility("pedidos", "x", None, None)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationRequired);
    let fields: Vec<&str> = err.details.iter().map(|d| d.field.as_str()).collect();
    assert_eq!(fields, ["receiptToken", "visibilityTimeout"]);
}

/// UC: UC-ACK-004, UC-ACK-005
#[tokio::test]
async fn only_the_last_delivery_token_acks_and_it_stays_valid_until_redelivery() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let first = delivered_one(&h, "pedidos", "10").await;

    h.clock.advance(Duration::from_secs(60)); // venceu, mas ninguém a pegou de novo
    let wrong = h
        .messages
        .ack("pedidos", &first.id, Some("outro"))
        .unwrap_err();
    assert_eq!(wrong.code, ErrorCode::MessageStaleReceipt);

    let second = delivered_one(&h, "pedidos", "10").await; // reentregue a outro consumidor
    let stale = h
        .messages
        .ack("pedidos", &first.id, Some(&first.receipt_token))
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::MessageStaleReceipt);
    h.messages
        .ack("pedidos", &second.id, Some(&second.receipt_token))
        .unwrap();
}

/// UC: UC-ACK-005
#[tokio::test]
async fn ack_after_the_deadline_but_before_redelivery_still_works() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "10").await;
    h.clock.advance(Duration::from_secs(11));
    h.messages
        .ack("pedidos", &m.id, Some(&m.receipt_token))
        .unwrap();
}

/// UC: UC-ACK-006, UC-VIS-007
#[tokio::test]
async fn a_message_past_its_ttl_is_not_found_for_ack_and_visibility() {
    let h = harness();
    let mut short = queue("curta");
    short.ttl = Some(60);
    h.topics.create(short).await.unwrap();
    publish_ok(&h, "curta", "1");
    let m = delivered_one(&h, "curta", "600").await;
    h.clock.advance(Duration::from_secs(60));

    let err = h
        .messages
        .set_visibility("curta", &m.id, Some(&m.receipt_token), Some(10))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageNotFound);
    let err = h
        .messages
        .ack("curta", &m.id, Some(&m.receipt_token))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageNotFound);
}

/// UC: UC-ACK-007
#[tokio::test]
async fn ack_checks_the_topic_first() {
    let h = harness();
    h.topics.create(distributor("todos", &["a"])).await.unwrap();
    assert_eq!(
        h.messages.ack("todos", "x", Some("t")).unwrap_err().code,
        ErrorCode::TopicWrongType
    );
    assert_eq!(
        h.messages.ack("nada", "x", Some("t")).unwrap_err().code,
        ErrorCode::TopicNotFound
    );
    assert_eq!(
        h.messages
            .set_visibility("todos", "x", Some("t"), Some(1))
            .unwrap_err()
            .code,
        ErrorCode::TopicWrongType
    );
}

/// UC: UC-VIS-001, UC-VIS-002, UC-VIS-004, UC-VIS-009, UC-VIS-010
#[tokio::test]
async fn visibility_replaces_the_deadline_without_touching_count_or_token() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "30").await;

    h.clock.advance(Duration::from_secs(20));
    let now = h.clock.now_millis();
    let up = h
        .messages
        .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(100))
        .unwrap();
    assert_eq!(
        up.invisible_until_ms,
        now + 100_000,
        "agora + valor, não um acréscimo"
    );
    let again = h
        .messages
        .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(100))
        .unwrap();
    assert_eq!(
        again.invisible_until_ms, up.invisible_until_ms,
        "idempotente"
    );

    h.clock.advance(Duration::from_secs(99)); // passou o prazo antigo, não o novo
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );

    let shorter = h
        .messages
        .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(1))
        .unwrap();
    assert_eq!(
        shorter.invisible_until_ms,
        h.clock.now_millis() + 1000,
        "encurtar também vale"
    );
    h.clock.advance(Duration::from_secs(1));
    let back = delivered_one(&h, "pedidos", "30").await;
    assert_eq!(back.delivery_count, 2, "o PUT não contou entrega");
    assert_ne!(back.receipt_token, m.receipt_token);
}

/// UC: UC-VIS-003
#[tokio::test]
async fn visibility_zero_releases_the_message_at_once_and_wakes_a_long_poll() {
    let h = Arc::new(harness());
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "600").await;
    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume(&h, "pedidos", q(None, None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;

    let up = h
        .messages
        .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(0))
        .unwrap();
    assert_eq!(up.invisible_until_ms, h.clock.now_millis());
    let got = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got[0].id, m.id);
}

/// UC: UC-VIS-005, UC-VIS-006
#[tokio::test]
async fn visibility_rejects_out_of_range_and_stale_tokens() {
    let h = harness_with(&[("TABA_DELIVERY_VISIBILITY_TIMEOUT_MAX", "300s")]);
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "30").await;
    for bad in [-1, 301] {
        let err = h
            .messages
            .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(bad))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationOutOfRange, "{bad}");
    }
    let err = h
        .messages
        .set_visibility("pedidos", &m.id, Some("velho"), Some(10))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageStaleReceipt);
    assert!(
        h.messages
            .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(300))
            .is_ok()
    );
}

fn limited(name: &str, max_deliveries: i64) -> CreateTopic {
    CreateTopic {
        max_deliveries: Some(max_deliveries),
        ..queue(name)
    }
}

/// Entrega a mensagem e deixa passar o prazo, sem confirmar.
async fn fail_once(h: &Harness, topic: &str) -> Delivered {
    let m = delivered_one(h, topic, "10").await;
    h.clock.advance(Duration::from_secs(10));
    m
}

/// UC: UC-RET-008, UC-RET-009
#[tokio::test]
async fn after_max_deliveries_the_message_goes_to_the_dead_letter_with_the_same_id() {
    let h = harness();
    h.topics.create(limited("pedidos", 2)).await.unwrap();
    let id = publish_ok(&h, "pedidos", "1");

    assert_eq!(fail_once(&h, "pedidos").await.delivery_count, 1);
    assert_eq!(fail_once(&h, "pedidos").await.delivery_count, 2);
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty(),
        "a terceira entrega nunca acontece"
    );

    let moved = consume(&h, "pedidos-dead-letter", ConsumeQuery::default()).await;
    assert_eq!(moved.len(), 1);
    let m = &moved[0];
    assert_eq!(m.id, id, "o mesmo id");
    assert_eq!(
        m.delivery_count, 1,
        "voltou a zero e esta é a primeira entrega lá"
    );
    assert_eq!(m.dead_lettered_ms, Some(h.clock.now_millis()));
    assert_eq!(m.dead_letter_reason, Some("max-deliveries"));
    assert_eq!(m.invisible_until_ms, h.clock.now_millis() + 120_000);
}

/// UC: UC-RET-008
#[tokio::test]
async fn an_acked_message_never_reaches_the_dead_letter() {
    let h = harness();
    h.topics.create(limited("pedidos", 1)).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "10").await;
    h.messages
        .ack("pedidos", &m.id, Some(&m.receipt_token))
        .unwrap();
    h.clock.advance(Duration::from_secs(60));
    h.messages.sweep();
    assert!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-008
#[tokio::test]
async fn the_limit_counts_only_when_the_invisibility_ends_without_ack() {
    let h = harness();
    h.topics.create(limited("pedidos", 1)).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    delivered_one(&h, "pedidos", "30").await;
    h.clock.advance(Duration::from_secs(29));
    h.messages.sweep();
    assert!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
    h.clock.advance(Duration::from_secs(1));
    h.messages.sweep();
    assert_eq!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default())
            .await
            .len(),
        1
    );
}

/// UC: UC-RET-008
#[tokio::test]
async fn ack_after_the_last_delivery_expired_finds_the_message_already_moved() {
    let h = harness();
    h.topics.create(limited("pedidos", 1)).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = fail_once(&h, "pedidos").await;
    let err = h
        .messages
        .ack("pedidos", &m.id, Some(&m.receipt_token))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageNotFound);
}

/// UC: UC-VIS-011
#[tokio::test]
async fn visibility_zero_on_an_exhausted_message_sends_it_to_the_dead_letter() {
    let h = harness();
    h.topics.create(limited("pedidos", 1)).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "600").await;

    let up = h
        .messages
        .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(0))
        .unwrap();
    assert_eq!(up.invisible_until_ms, h.clock.now_millis());
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );
    assert_eq!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default()).await[0].id,
        m.id
    );
}

/// UC: UC-VIS-011
#[tokio::test]
async fn visibility_zero_below_the_limit_just_releases_the_message() {
    let h = harness();
    h.topics.create(limited("pedidos", 2)).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    let m = delivered_one(&h, "pedidos", "600").await;
    h.messages
        .set_visibility("pedidos", &m.id, Some(&m.receipt_token), Some(0))
        .unwrap();
    assert_eq!(
        consume(&h, "pedidos", ConsumeQuery::default()).await[0].delivery_count,
        2
    );
}

/// UC: UC-RET-013
#[tokio::test]
async fn the_dead_letter_has_no_dead_letter_and_its_ttl_counts_from_deadlettering() {
    let h = harness();
    let mut topic = limited("pedidos", 1);
    topic.ttl = Some(3600);
    h.topics.create(topic).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    delivered_one(&h, "pedidos", "10").await;
    h.clock.advance(Duration::from_secs(3000)); // quase todo o ttl, contado do id
    h.messages.sweep();

    // Entregue várias vezes na dead-letter sem ack: nunca sai de lá por entregas.
    for _ in 0..3 {
        delivered_one(&h, "pedidos-dead-letter", "10").await;
        h.clock.advance(Duration::from_secs(10));
        h.messages.sweep();
    }
    assert_eq!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default()).await[0].delivery_count,
        4
    );
    // Contado do `id`, o `ttl` já teria vencido (3630 s > 3600 s); contado de
    // `deadLetteredAt`, só vence 3600 s depois dele.
    h.clock.advance(Duration::from_secs(600));
    h.messages.sweep();
    assert_eq!(
        consume(&h, "pedidos-dead-letter", q(None, Some("0"), None))
            .await
            .len(),
        1
    );
    h.clock.advance(Duration::from_secs(3000));
    h.messages.sweep();
    assert!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-001, UC-RET-004
#[tokio::test]
async fn sweeping_drops_expired_messages_even_in_flight_and_not_to_the_dead_letter() {
    let h = harness();
    let mut topic = limited("pedidos", 1);
    topic.ttl = Some(60);
    h.topics.create(topic).await.unwrap();
    publish_ok(&h, "pedidos", "\"pronta\"");
    publish_ok(&h, "pedidos", "\"em processamento\"");
    let in_flight = consume(&h, "pedidos", q(Some("2"), Some("600"), None)).await;
    publish_ok(&h, "pedidos", "\"pronta\"");

    h.clock.advance(Duration::from_secs(60));
    h.messages.sweep();
    let err = h.messages.ack(
        "pedidos",
        &in_flight[0].id,
        Some(&in_flight[0].receipt_token),
    );
    assert_eq!(err.unwrap_err().code, ErrorCode::MessageNotFound);
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );
    assert!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-002, UC-TOP-030
#[tokio::test]
async fn a_new_ttl_applies_to_messages_already_stored() {
    let h = harness();
    h.topics.create(queue("pedidos")).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    h.clock.advance(Duration::from_secs(61));
    h.messages.sweep();
    assert_eq!(
        consume(&h, "pedidos", q(None, Some("0"), None)).await.len(),
        1,
        "o padrão é 14 dias"
    );

    let patch = crate::engine::topics::UpdateTopic {
        ttl: crate::engine::topics::Patch::Value(60),
        ..Default::default()
    };
    h.topics.update("pedidos", patch).await.unwrap();
    h.messages.sweep();
    assert!(
        consume(&h, "pedidos", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-014
#[tokio::test]
async fn the_dead_letter_message_can_be_published_back_to_the_source() {
    let h = harness();
    h.topics.create(limited("pedidos", 1)).await.unwrap();
    publish_ok(&h, "pedidos", "{ \"ruim\": true }");
    fail_once(&h, "pedidos").await;
    let dead = delivered_one(&h, "pedidos-dead-letter", "30").await;

    publish_ok(&h, "pedidos", "{ \"ruim\": false }");
    h.messages
        .ack("pedidos-dead-letter", &dead.id, Some(&dead.receipt_token))
        .unwrap();
    assert_eq!(
        consume(&h, "pedidos", ConsumeQuery::default()).await.len(),
        1
    );
    assert!(
        consume(&h, "pedidos-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-005, UC-RET-008
#[tokio::test]
async fn the_background_sweeper_moves_messages_without_anyone_consuming() {
    let h = harness_with(&[("TABA_RETENTION_SWEEP_INTERVAL", "100ms")]);
    h.topics.create(limited("pedidos", 1)).await.unwrap();
    publish_ok(&h, "pedidos", "1");
    fail_once(&h, "pedidos").await;

    let cancel = tokio_util::sync::CancellationToken::new();
    let sweeper = {
        let messages = h.messages.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { messages.run_sweeper(cancel).await })
    };
    // Quem espera na dead-letter (long polling) é acordado pelo movimento.
    let got = tokio::time::timeout(
        Duration::from_secs(5),
        consume(&h, "pedidos-dead-letter", q(None, None, Some("20"))),
    )
    .await
    .expect("a varredura moveu a mensagem");
    assert_eq!(got.len(), 1);
    cancel.cancel();
    sweeper.await.unwrap();
}
