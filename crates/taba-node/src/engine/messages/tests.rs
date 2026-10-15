use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::config::{Config, MapEnv};
use crate::core::clock::ManualClock;
use crate::core::id;
use crate::engine::topics::{CreateTopic, Kind, TopicRules};
use crate::storage::messages::MessageStore;
use crate::storage::topics::MemoryTopicStore;

struct Harness {
    topics: Arc<Topics>,
    messages: Arc<Messages>,
    clock: Arc<ManualClock>,
    store: Arc<MessageStore>,
    dir: tempfile::TempDir,
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
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(MessageStore::new(dir.path(), config.storage.db_synchronous));
    let messages = Messages::new(
        topics.clone(),
        clock.clone(),
        MessageRules::from_config(&config),
        store.clone(),
        false,
    );
    Harness {
        topics,
        messages,
        clock,
        store,
        dir,
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
        Published::Queue { id, .. } => id,
        Published::Distributor { .. } => panic!("expected a queue"),
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
        ..Default::default()
    }
}

/// UC: UC-MSG-001, UC-MSG-013
#[tokio::test]
async fn publishing_returns_increasing_guid_v7_ids() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let ids: Vec<String> = (0..50)
        .map(|i| publish_ok(&h, "orders", &i.to_string()))
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
        publish_err(&h, "nothing", json_body("1")),
        ErrorCode::TopicNotFound
    );
    assert_eq!(
        h.messages.publish_limit("nothing").unwrap_err().code,
        ErrorCode::TopicNotFound
    );
    assert_eq!(
        consume_err(&h, "nothing", ConsumeQuery::default()).await,
        ErrorCode::TopicNotFound
    );
}

/// UC: UC-MSG-004, UC-MSG-005, UC-MSG-006
#[tokio::test]
async fn publishing_validates_content_type_and_body() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let with = |content_type: Option<&str>, body: &[u8]| PublishRequest {
        body: body.to_vec(),
        content_type: content_type.map(Into::into),
        ..Default::default()
    };
    assert_eq!(
        publish_err(&h, "orders", with(Some("nao-e-media-type"), b"x")),
        ErrorCode::ValidationInvalidContentType
    );
    assert_eq!(
        publish_err(&h, "orders", with(None, b"{ nao")),
        ErrorCode::ValidationInvalidBody
    );
    assert_eq!(
        publish_err(&h, "orders", with(Some("text/plain"), &[0xff])),
        ErrorCode::ValidationInvalidBody
    );
    assert!(
        h.messages
            .publish("orders", with(Some("image/png"), &[0xff]))
            .is_ok()
    );
}

/// UC: UC-MSG-008, UC-MSG-009, UC-TOP-031
#[tokio::test]
async fn size_counts_bytes_against_the_topic_limit_or_the_general_one() {
    let h = harness();
    let mut small = queue("small");
    small.max_message_bytes = Some(5);
    h.topics.create(small).await.unwrap();
    h.topics.create(queue("padrao")).await.unwrap();

    assert!(h.messages.publish("small", json_body("\"abc\"")).is_ok());
    assert_eq!(
        publish_err(&h, "small", json_body("\"abcd\"")),
        ErrorCode::MessageTooLarge
    );
    assert_eq!(h.messages.publish_limit("small").unwrap(), 5);
    assert_eq!(h.messages.publish_limit("padrao").unwrap(), 5 * 1024 * 1024);
}

/// UC: UC-MSG-011, UC-MSG-012
#[tokio::test]
async fn group_id_is_validated_and_deduplication_is_fifo_only() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let with = |group: Option<&str>, dedup: Option<&str>| PublishRequest {
        body: b"1".to_vec(),
        group_id: group.map(Into::into),
        deduplication_id: dedup.map(Into::into),
        ..Default::default()
    };
    assert!(
        h.messages
            .publish("orders", with(Some("customer-42"), None))
            .is_ok()
    );
    for bad in ["", "with space", &"x".repeat(129)] {
        assert_eq!(
            publish_err(&h, "orders", with(Some(bad), None)),
            ErrorCode::ValidationFailed,
            "{bad:?}"
        );
    }
    assert_eq!(
        publish_err(&h, "orders", with(None, Some("d1"))),
        ErrorCode::ValidationNotApplicable
    );
}

/// UC: UC-FIF-002
#[tokio::test]
async fn a_fifo_queue_requires_the_group_id() {
    let h = harness();
    let mut fifo = queue("order");
    fifo.fifo = Some(true);
    h.topics.create(fifo).await.unwrap();
    assert_eq!(
        publish_err(&h, "order", json_body("1")),
        ErrorCode::ValidationGroupIdRequired
    );
    let ok = PublishRequest {
        body: b"1".to_vec(),
        group_id: Some("g".into()),
        deduplication_id: Some("d".into()),
        ..Default::default()
    };
    assert!(h.messages.publish("order", ok).is_ok());
}

/// UC: UC-CNS-001, UC-CNS-002
#[tokio::test]
async fn consuming_delivers_the_message_with_its_metadata() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );

    let id = publish_ok(&h, "orders", r#"{"a": 1}"#);
    let got = consume(&h, "orders", q(None, Some("60"), None)).await;
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
    h.topics.create(queue("orders")).await.unwrap();
    for i in 0..30 {
        publish_ok(&h, "orders", &i.to_string());
    }
    assert_eq!(
        consume(&h, "orders", ConsumeQuery::default()).await.len(),
        10
    );
    assert_eq!(
        consume(&h, "orders", q(Some("20"), None, None)).await.len(),
        20
    );
    assert!(
        consume(&h, "orders", q(Some("5"), None, None))
            .await
            .is_empty()
    );

    for bad in ["0", "21", "-1"] {
        assert_eq!(
            consume_err(&h, "orders", q(Some(bad), None, None)).await,
            ErrorCode::ValidationOutOfRange,
            "{bad}"
        );
    }
    assert_eq!(
        consume_err(&h, "orders", q(Some("dez"), None, None)).await,
        ErrorCode::ValidationFailed
    );
}

/// UC: UC-CNS-004
#[tokio::test]
async fn consuming_returns_what_is_ready_without_waiting_to_fill_the_batch() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    publish_ok(&h, "orders", "2");
    assert_eq!(
        consume(&h, "orders", q(Some("10"), None, None)).await.len(),
        2
    );
}

/// UC: UC-CNS-005, UC-CNS-006, UC-CNS-007
#[tokio::test]
async fn a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");

    let first = consume(&h, "orders", q(None, Some("30"), None)).await;
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );

    h.clock.advance(Duration::from_secs(29));
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );

    h.clock.advance(Duration::from_secs(1));
    let again = consume(&h, "orders", ConsumeQuery::default()).await;
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
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    assert_eq!(
        consume(&h, "orders", q(None, Some("0"), None)).await.len(),
        1
    );
    assert_eq!(
        consume(&h, "orders", q(None, Some("0"), None)).await.len(),
        1
    );
    assert_eq!(
        consume_err(&h, "orders", q(None, Some("301"), None)).await,
        ErrorCode::ValidationOutOfRange
    );
    assert_eq!(
        consume_err(&h, "orders", q(None, Some("-5"), None)).await,
        ErrorCode::ValidationOutOfRange
    );
}

/// UC: UC-CNS-015
#[tokio::test]
async fn a_message_past_its_ttl_is_not_delivered() {
    let h = harness();
    let mut short = queue("short");
    short.ttl = Some(60);
    h.topics.create(short).await.unwrap();
    publish_ok(&h, "short", "1");
    h.clock.advance(Duration::from_secs(60));
    assert!(
        consume(&h, "short", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-CNS-008, UC-CNS-009, UC-CNS-010, UC-CNS-012, UC-MSG-020
#[tokio::test]
async fn each_message_keeps_its_content_type_and_group_in_a_mixed_batch() {
    let h = harness();
    h.topics.create(queue("files")).await.unwrap();
    let publish = |content_type: &str, body: &[u8], group: Option<&str>| {
        h.messages
            .publish(
                "files",
                PublishRequest {
                    body: body.to_vec(),
                    content_type: Some(content_type.into()),
                    group_id: group.map(Into::into),
                    ..Default::default()
                },
            )
            .unwrap()
    };
    publish("text/plain; charset=utf-8", "Order 9001".as_bytes(), None);
    publish("image/png", &[0x89, b'P', b'N', b'G'], Some("g1"));
    publish("application/json", br#"{ "order": 9001 }"#, None);

    // No ordering guarantee (UC-CNS-013): compares in `id` order.
    let mut got = consume(&h, "files", ConsumeQuery::default()).await;
    got.sort_by(|a, b| a.id.cmp(&b.id));
    let bodies: Vec<String> = got
        .iter()
        .map(|m| serde_json::to_string(&m.body).unwrap())
        .collect();
    assert_eq!(
        bodies,
        ["\"Order 9001\"", "\"iVBORw==\"", r#"{ "order": 9001 }"#]
    );
    assert_eq!(got[0].content_type, "text/plain; charset=utf-8");
    assert_eq!(got[1].group_id.as_deref(), Some("g1"));
    assert_eq!(got[2].group_id, None);
}

/// UC: UC-CNS-017
#[tokio::test]
async fn a_distributor_cannot_be_consumed() {
    let h = harness();
    h.topics.create(distributor("all", &["a"])).await.unwrap();
    assert_eq!(
        consume_err(&h, "all", ConsumeQuery::default()).await,
        ErrorCode::TopicWrongType
    );
}

/// UC: UC-CNS-016
#[tokio::test]
async fn the_dead_letter_is_a_common_queue_for_publish_and_consume() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let id = publish_ok(&h, "orders-dead-letter", "1");
    let got = consume(&h, "orders-dead-letter", ConsumeQuery::default()).await;
    assert_eq!(got[0].id, id);
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-DST-001, UC-DST-002, UC-DST-003
#[tokio::test]
async fn a_distributor_copies_the_message_to_every_matching_queue() {
    let h = harness();
    for name in ["orders-eu", "orders-us", "other"] {
        h.topics.create(queue(name)).await.unwrap();
    }
    h.topics
        .create(distributor("all", &["orders-"]))
        .await
        .unwrap();
    h.topics
        .create(distributor("none", &["zzz"]))
        .await
        .unwrap();

    let Published::Distributor { deliveries } = h.messages.publish("all", json_body("1")).unwrap()
    else {
        panic!("expected a distributor");
    };
    let targets: Vec<&str> = deliveries.iter().map(|d| d.topic.as_str()).collect();
    assert_eq!(targets, ["orders-eu", "orders-us"]);
    let ids: Vec<&String> = deliveries
        .iter()
        .map(|d| d.result.as_ref().unwrap())
        .collect();
    assert_ne!(ids[0], ids[1], "each copy has its own id");
    assert_eq!(
        consume(&h, "orders-eu", ConsumeQuery::default())
            .await
            .len(),
        1
    );
    assert_eq!(
        consume(&h, "orders-us", ConsumeQuery::default())
            .await
            .len(),
        1
    );
    assert!(
        consume(&h, "other", ConsumeQuery::default())
            .await
            .is_empty()
    );

    let Published::Distributor { deliveries } = h.messages.publish("none", json_body("1")).unwrap()
    else {
        panic!("expected a distributor");
    };
    assert!(deliveries.is_empty(), "zero queues is not an error");
}

/// UC: UC-DST-011
#[tokio::test]
async fn a_distributor_over_max_targets_refuses_the_whole_publication() {
    let h = harness_with(&[("TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS", "1")]);
    for name in ["a1", "a2"] {
        h.topics.create(queue(name)).await.unwrap();
    }
    h.topics.create(distributor("all", &["a"])).await.unwrap();
    assert_eq!(
        publish_err(&h, "all", json_body("1")),
        ErrorCode::DistributorTooManyTargets
    );
    assert!(consume(&h, "a1", ConsumeQuery::default()).await.is_empty());
}

/// UC: UC-DST-009
#[tokio::test]
async fn a_distributor_copy_to_a_fifo_queue_without_group_fails_only_that_queue() {
    let h = harness();
    h.topics.create(queue("p-basic")).await.unwrap();
    let mut fifo = queue("p-fifo");
    fifo.fifo = Some(true);
    h.topics.create(fifo).await.unwrap();
    h.topics.create(distributor("all", &["p-"])).await.unwrap();

    let Published::Distributor { deliveries } = h.messages.publish("all", json_body("1")).unwrap()
    else {
        panic!("expected a distributor");
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
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    publish_ok(&h, "orders-dead-letter", "2");

    h.topics.delete("orders").await.unwrap();
    h.topics.create(queue("orders")).await.unwrap();
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
    assert!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-CNS-024
#[tokio::test]
async fn wait_time_is_limited_to_0_20() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    for bad in ["21", "-1"] {
        assert_eq!(
            consume_err(&h, "orders", q(None, None, Some(bad))).await,
            ErrorCode::ValidationOutOfRange
        );
    }
}

/// UC: UC-CNS-025, UC-CNS-027
#[tokio::test]
async fn long_polling_returns_as_soon_as_a_message_arrives() {
    let h = Arc::new(harness());
    h.topics.create(queue("orders")).await.unwrap();
    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume(&h, "orders", q(Some("5"), None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiter.is_finished(), "still waiting");
    publish_ok(&h, "orders", "1");

    let got = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("did not wait for the end of the waitTime")
        .unwrap();
    assert_eq!(
        got.len(),
        1,
        "returns the first, without waiting to complete the batch"
    );
}

/// UC: UC-CNS-026
#[tokio::test]
async fn long_polling_ends_with_an_empty_list() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let started = std::time::Instant::now();
    assert!(
        consume(&h, "orders", q(None, None, Some("1")))
            .await
            .is_empty()
    );
    assert!(started.elapsed() >= Duration::from_millis(900));
}

/// UC: UC-CNS-025
#[tokio::test]
async fn long_polling_notices_a_message_whose_invisibility_ends() {
    let h = Arc::new(harness());
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    consume(&h, "orders", q(None, Some("1"), None)).await;

    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume(&h, "orders", q(None, None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.clock.advance(Duration::from_secs(1));
    let got = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("the message became deliverable again")
        .unwrap();
    assert_eq!(got[0].delivery_count, 2);
}

/// UC: UC-CNS-018
#[tokio::test]
async fn deleting_the_topic_ends_a_long_poll_with_not_found() {
    let h = Arc::new(harness());
    h.topics.create(queue("orders")).await.unwrap();
    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume_err(&h, "orders", q(None, None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.topics.delete("orders").await.unwrap();
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
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "30").await;

    h.messages
        .ack("orders", &m.id, Some(&m.receipt_token))
        .unwrap();
    assert_eq!(
        h.messages
            .ack("orders", &m.id, Some(&m.receipt_token))
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
    h.clock.advance(Duration::from_secs(600));
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-ACK-002, UC-VIS-005
#[tokio::test]
async fn missing_fields_are_reported_together() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let err = h.messages.ack("orders", "x", None).unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationRequired);
    assert_eq!(err.details[0].field, "receiptToken");

    let err = h
        .messages
        .set_visibility("orders", "x", None, None)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationRequired);
    let fields: Vec<&str> = err.details.iter().map(|d| d.field.as_str()).collect();
    assert_eq!(fields, ["receiptToken", "visibilityTimeout"]);
}

/// UC: UC-ACK-004, UC-ACK-005
#[tokio::test]
async fn only_the_last_delivery_token_acks_and_it_stays_valid_until_redelivery() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    let first = delivered_one(&h, "orders", "10").await;

    h.clock.advance(Duration::from_secs(60)); // it expired, but nobody took it again
    let wrong = h
        .messages
        .ack("orders", &first.id, Some("other"))
        .unwrap_err();
    assert_eq!(wrong.code, ErrorCode::MessageStaleReceipt);

    let second = delivered_one(&h, "orders", "10").await; // reentregue a other consumidor
    let stale = h
        .messages
        .ack("orders", &first.id, Some(&first.receipt_token))
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::MessageStaleReceipt);
    h.messages
        .ack("orders", &second.id, Some(&second.receipt_token))
        .unwrap();
}

/// UC: UC-ACK-005
#[tokio::test]
async fn ack_after_the_deadline_but_before_redelivery_still_works() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "10").await;
    h.clock.advance(Duration::from_secs(11));
    h.messages
        .ack("orders", &m.id, Some(&m.receipt_token))
        .unwrap();
}

/// UC: UC-ACK-006, UC-VIS-007
#[tokio::test]
async fn a_message_past_its_ttl_is_not_found_for_ack_and_visibility() {
    let h = harness();
    let mut short = queue("short");
    short.ttl = Some(60);
    h.topics.create(short).await.unwrap();
    publish_ok(&h, "short", "1");
    let m = delivered_one(&h, "short", "600").await;
    h.clock.advance(Duration::from_secs(60));

    let err = h
        .messages
        .set_visibility("short", &m.id, Some(&m.receipt_token), Some(10))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageNotFound);
    let err = h
        .messages
        .ack("short", &m.id, Some(&m.receipt_token))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageNotFound);
}

/// UC: UC-ACK-007
#[tokio::test]
async fn ack_checks_the_topic_first() {
    let h = harness();
    h.topics.create(distributor("all", &["a"])).await.unwrap();
    assert_eq!(
        h.messages.ack("all", "x", Some("t")).unwrap_err().code,
        ErrorCode::TopicWrongType
    );
    assert_eq!(
        h.messages.ack("nothing", "x", Some("t")).unwrap_err().code,
        ErrorCode::TopicNotFound
    );
    assert_eq!(
        h.messages
            .set_visibility("all", "x", Some("t"), Some(1))
            .unwrap_err()
            .code,
        ErrorCode::TopicWrongType
    );
}

/// UC: UC-VIS-001, UC-VIS-002, UC-VIS-004, UC-VIS-009, UC-VIS-010
#[tokio::test]
async fn visibility_replaces_the_deadline_without_touching_count_or_token() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "30").await;

    h.clock.advance(Duration::from_secs(20));
    let now = h.clock.now_millis();
    let up = h
        .messages
        .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(100))
        .unwrap();
    assert_eq!(
        up.invisible_until_ms,
        now + 100_000,
        "now + value, not an increment"
    );
    let again = h
        .messages
        .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(100))
        .unwrap();
    assert_eq!(
        again.invisible_until_ms, up.invisible_until_ms,
        "idempotente"
    );

    h.clock.advance(Duration::from_secs(99)); // the old deadline passed, not the new one
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );

    let shorter = h
        .messages
        .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(1))
        .unwrap();
    assert_eq!(
        shorter.invisible_until_ms,
        h.clock.now_millis() + 1000,
        "shortening also works"
    );
    h.clock.advance(Duration::from_secs(1));
    let back = delivered_one(&h, "orders", "30").await;
    assert_eq!(back.delivery_count, 2, "the PUT did not count a delivery");
    assert_ne!(back.receipt_token, m.receipt_token);
}

/// UC: UC-VIS-003
#[tokio::test]
async fn visibility_zero_releases_the_message_at_once_and_wakes_a_long_poll() {
    let h = Arc::new(harness());
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "600").await;
    let waiter = {
        let h = h.clone();
        tokio::spawn(async move { consume(&h, "orders", q(None, None, Some("20"))).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;

    let up = h
        .messages
        .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(0))
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
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "30").await;
    for bad in [-1, 301] {
        let err = h
            .messages
            .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(bad))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationOutOfRange, "{bad}");
    }
    let err = h
        .messages
        .set_visibility("orders", &m.id, Some("old"), Some(10))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageStaleReceipt);
    assert!(
        h.messages
            .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(300))
            .is_ok()
    );
}

fn limited(name: &str, max_deliveries: i64) -> CreateTopic {
    CreateTopic {
        max_deliveries: Some(max_deliveries),
        ..queue(name)
    }
}

/// Delivers the message and lets the deadline pass, without acknowledging.
async fn fail_once(h: &Harness, topic: &str) -> Delivered {
    let m = delivered_one(h, topic, "10").await;
    h.clock.advance(Duration::from_secs(10));
    m
}

/// UC: UC-RET-008, UC-RET-009
#[tokio::test]
async fn after_max_deliveries_the_message_goes_to_the_dead_letter_with_the_same_id() {
    let h = harness();
    h.topics.create(limited("orders", 2)).await.unwrap();
    let id = publish_ok(&h, "orders", "1");

    assert_eq!(fail_once(&h, "orders").await.delivery_count, 1);
    assert_eq!(fail_once(&h, "orders").await.delivery_count, 2);
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty(),
        "the third delivery never happens"
    );

    let moved = consume(&h, "orders-dead-letter", ConsumeQuery::default()).await;
    assert_eq!(moved.len(), 1);
    let m = &moved[0];
    assert_eq!(m.id, id, "the same id");
    assert_eq!(
        m.delivery_count, 1,
        "back to zero and this is the first delivery there"
    );
    assert_eq!(m.dead_lettered_ms, Some(h.clock.now_millis()));
    assert_eq!(m.dead_letter_reason.as_deref(), Some("max-deliveries"));
    assert_eq!(m.invisible_until_ms, h.clock.now_millis() + 120_000);
}

/// UC: UC-RET-008
#[tokio::test]
async fn an_acked_message_never_reaches_the_dead_letter() {
    let h = harness();
    h.topics.create(limited("orders", 1)).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "10").await;
    h.messages
        .ack("orders", &m.id, Some(&m.receipt_token))
        .unwrap();
    h.clock.advance(Duration::from_secs(60));
    h.messages.sweep();
    assert!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-008
#[tokio::test]
async fn the_limit_counts_only_when_the_invisibility_ends_without_ack() {
    let h = harness();
    h.topics.create(limited("orders", 1)).await.unwrap();
    publish_ok(&h, "orders", "1");
    delivered_one(&h, "orders", "30").await;
    h.clock.advance(Duration::from_secs(29));
    h.messages.sweep();
    assert!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
    h.clock.advance(Duration::from_secs(1));
    h.messages.sweep();
    assert_eq!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .len(),
        1
    );
}

/// UC: UC-RET-008
#[tokio::test]
async fn ack_after_the_last_delivery_expired_finds_the_message_already_moved() {
    let h = harness();
    h.topics.create(limited("orders", 1)).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = fail_once(&h, "orders").await;
    let err = h
        .messages
        .ack("orders", &m.id, Some(&m.receipt_token))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::MessageNotFound);
}

/// UC: UC-VIS-011
#[tokio::test]
async fn visibility_zero_on_an_exhausted_message_sends_it_to_the_dead_letter() {
    let h = harness();
    h.topics.create(limited("orders", 1)).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "600").await;

    let up = h
        .messages
        .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(0))
        .unwrap();
    assert_eq!(up.invisible_until_ms, h.clock.now_millis());
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
    assert_eq!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default()).await[0].id,
        m.id
    );
}

/// UC: UC-VIS-011
#[tokio::test]
async fn visibility_zero_below_the_limit_just_releases_the_message() {
    let h = harness();
    h.topics.create(limited("orders", 2)).await.unwrap();
    publish_ok(&h, "orders", "1");
    let m = delivered_one(&h, "orders", "600").await;
    h.messages
        .set_visibility("orders", &m.id, Some(&m.receipt_token), Some(0))
        .unwrap();
    assert_eq!(
        consume(&h, "orders", ConsumeQuery::default()).await[0].delivery_count,
        2
    );
}

/// UC: UC-RET-013
#[tokio::test]
async fn the_dead_letter_has_no_dead_letter_and_its_ttl_counts_from_deadlettering() {
    let h = harness();
    let mut topic = limited("orders", 1);
    topic.ttl = Some(3600);
    h.topics.create(topic).await.unwrap();
    publish_ok(&h, "orders", "1");
    delivered_one(&h, "orders", "10").await;
    h.clock.advance(Duration::from_secs(3000)); // almost the whole ttl, counted from the id
    h.messages.sweep();

    // Delivered several times in the dead-letter without ack: it never leaves there because of deliveries.
    for _ in 0..3 {
        delivered_one(&h, "orders-dead-letter", "10").await;
        h.clock.advance(Duration::from_secs(10));
        h.messages.sweep();
    }
    assert_eq!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default()).await[0].delivery_count,
        4
    );
    // Counted from the `id`, the `ttl` would already have expired (3630 s > 3600 s); counted from
    // `deadLetteredAt`, it only expires 3600 s after it.
    h.clock.advance(Duration::from_secs(600));
    h.messages.sweep();
    assert_eq!(
        consume(&h, "orders-dead-letter", q(None, Some("0"), None))
            .await
            .len(),
        1
    );
    h.clock.advance(Duration::from_secs(3000));
    h.messages.sweep();
    assert!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-001, UC-RET-004
#[tokio::test]
async fn sweeping_drops_expired_messages_even_in_flight_and_not_to_the_dead_letter() {
    let h = harness();
    let mut topic = limited("orders", 1);
    topic.ttl = Some(60);
    h.topics.create(topic).await.unwrap();
    publish_ok(&h, "orders", "\"ready\"");
    publish_ok(&h, "orders", "\"in processing\"");
    let in_flight = consume(&h, "orders", q(Some("2"), Some("600"), None)).await;
    publish_ok(&h, "orders", "\"ready\"");

    h.clock.advance(Duration::from_secs(60));
    h.messages.sweep();
    let err = h.messages.ack(
        "orders",
        &in_flight[0].id,
        Some(&in_flight[0].receipt_token),
    );
    assert_eq!(err.unwrap_err().code, ErrorCode::MessageNotFound);
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
    assert!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-002, UC-TOP-030
#[tokio::test]
async fn a_new_ttl_applies_to_messages_already_stored() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    h.clock.advance(Duration::from_secs(61));
    h.messages.sweep();
    assert_eq!(
        consume(&h, "orders", q(None, Some("0"), None)).await.len(),
        1,
        "the default is 14 days"
    );

    let patch = crate::engine::topics::UpdateTopic {
        ttl: crate::engine::topics::Patch::Value(60),
        ..Default::default()
    };
    h.topics.update("orders", patch).await.unwrap();
    h.messages.sweep();
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-014
#[tokio::test]
async fn the_dead_letter_message_can_be_published_back_to_the_source() {
    let h = harness();
    h.topics.create(limited("orders", 1)).await.unwrap();
    publish_ok(&h, "orders", "{ \"ruim\": true }");
    fail_once(&h, "orders").await;
    let dead = delivered_one(&h, "orders-dead-letter", "30").await;

    publish_ok(&h, "orders", "{ \"ruim\": false }");
    h.messages
        .ack("orders-dead-letter", &dead.id, Some(&dead.receipt_token))
        .unwrap();
    assert_eq!(
        consume(&h, "orders", ConsumeQuery::default()).await.len(),
        1
    );
    assert!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-RET-005, UC-RET-008
#[tokio::test]
async fn the_background_sweeper_moves_messages_without_anyone_consuming() {
    let h = harness_with(&[("TABA_RETENTION_SWEEP_INTERVAL", "100ms")]);
    h.topics.create(limited("orders", 1)).await.unwrap();
    publish_ok(&h, "orders", "1");
    fail_once(&h, "orders").await;

    let cancel = tokio_util::sync::CancellationToken::new();
    let sweeper = {
        let messages = h.messages.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { messages.run_sweeper(cancel).await })
    };
    // Whoever waits on the dead-letter (long polling) is woken by the move.
    let got = tokio::time::timeout(
        Duration::from_secs(5),
        consume(&h, "orders-dead-letter", q(None, None, Some("20"))),
    )
    .await
    .expect("the sweep moved the message");
    assert_eq!(got.len(), 1);
    cancel.cancel();
    sweeper.await.unwrap();
}

fn reopen(h: &Harness, resume: bool) -> Arc<Messages> {
    reopen_with(h, resume, &[])
}

fn reopen_with(h: &Harness, resume: bool, env: &[(&str, &str)]) -> Arc<Messages> {
    let config = Config::parse(
        None,
        &MapEnv(
            [("TABA_MODE", "development")]
                .into_iter()
                .chain(env.iter().copied())
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        ),
    )
    .unwrap();
    Messages::new(
        h.topics.clone(),
        h.clock.clone(),
        MessageRules::from_config(&config),
        Arc::new(MessageStore::new(
            h.dir.path(),
            config.storage.db_synchronous,
        )),
        resume,
    )
}

/// Memory for a 3-byte body (`"1"` is 3 with the quotes): the next one goes to disk.
fn tiny_memory() -> Harness {
    harness_with(&[("TABA_MEMORY_LIMIT", "3")])
}

fn raw(body: &[u8], content_type: &str) -> PublishRequest {
    PublishRequest {
        body: body.to_vec(),
        content_type: Some(content_type.into()),
        ..Default::default()
    }
}

/// UC: UC-ARM-001, UC-ARM-002
#[tokio::test]
async fn what_does_not_fit_in_memory_overflows_to_disk() {
    let h = tiny_memory();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "\"a\"");
    assert_eq!(h.messages.usage("orders"), (3, 0), "it fit in memory");
    publish_ok(&h, "orders", "\"b\"");
    publish_ok(&h, "orders", "\"c\"");
    assert_eq!(
        h.messages.usage("orders"),
        (3, 2),
        "the excess went to disk"
    );
    assert!(
        h.dir
            .path()
            .join("orders")
            .join(crate::storage::messages::TOPIC_DB_FILE)
            .is_file()
    );
}

/// UC: UC-ARM-003
#[tokio::test]
async fn delivery_merges_memory_and_disk_in_id_order_with_the_exact_bytes() {
    let h = tiny_memory();
    h.topics.create(queue("files")).await.unwrap();
    let all_bytes: Vec<u8> = (0..=255).collect();
    let first = published_id(
        h.messages
            .publish("files", raw(b"abc", "text/plain"))
            .unwrap(),
    );
    let second = published_id(
        h.messages
            .publish("files", raw(&all_bytes, "image/png"))
            .unwrap(),
    );
    let third = published_id(
        h.messages
            .publish("files", raw(b"{ \"x\": 1.50 }", "application/json"))
            .unwrap(),
    );
    assert_eq!(h.messages.usage("files").1, 2);

    // No ordering guarantee (UC-CNS-013): compares in `id` order.
    let mut got = consume(&h, "files", ConsumeQuery::default()).await;
    got.sort_by(|a, b| a.id.cmp(&b.id));
    let ids: Vec<&str> = got.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, [first.as_str(), second.as_str(), third.as_str()]);
    assert_eq!(serde_json::to_string(&got[0].body).unwrap(), "\"abc\"");
    let png = serde_json::to_string(&got[1].body).unwrap();
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(png.trim_matches('"'))
        .unwrap();
    assert_eq!(
        decoded, all_bytes,
        "the body on disk comes out equal to what went in"
    );
    assert_eq!(
        serde_json::to_string(&got[2].body).unwrap(),
        "{ \"x\": 1.50 }"
    );
    assert_eq!(got[1].content_type, "image/png");
}

/// UC: UC-ACK-013, UC-ARM-014
#[tokio::test]
async fn a_message_on_disk_is_delivered_again_and_acked_like_any_other() {
    let h = tiny_memory();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "\"a\"");
    let id = publish_ok(&h, "orders", "\"b\"");
    let first = consume(&h, "orders", q(Some("2"), Some("10"), None)).await;
    let on_disk = first.iter().find(|m| m.id == id).unwrap();
    assert_eq!(on_disk.delivery_count, 1);
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );

    h.clock.advance(Duration::from_secs(10));
    let again = consume(&h, "orders", q(Some("2"), Some("30"), None)).await;
    let again = again.iter().find(|m| m.id == id).unwrap();
    assert_eq!(again.delivery_count, 2);
    assert_ne!(again.receipt_token, on_disk.receipt_token);
    let stale = h
        .messages
        .ack("orders", &id, Some(&on_disk.receipt_token))
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::MessageStaleReceipt);

    h.messages
        .ack("orders", &id, Some(&again.receipt_token))
        .unwrap();
    assert!(
        !h.dir
            .path()
            .join("orders")
            .join(format!("{id}.body"))
            .exists()
    );
    assert_eq!(h.store.get("orders", &id).unwrap(), None);
    assert_eq!(
        h.messages
            .ack("orders", &id, Some(&again.receipt_token))
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
}

/// UC: UC-VIS-001
#[tokio::test]
async fn visibility_works_on_a_message_on_disk() {
    let h = tiny_memory();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "\"a\"");
    let id = publish_ok(&h, "orders", "\"b\"");
    let got = consume(&h, "orders", q(Some("2"), Some("600"), None)).await;
    let m = got.iter().find(|m| m.id == id).unwrap();

    let up = h
        .messages
        .set_visibility("orders", &id, Some(&m.receipt_token), Some(0))
        .unwrap();
    assert_eq!(up.invisible_until_ms, h.clock.now_millis());
    let back = consume(&h, "orders", ConsumeQuery::default()).await;
    assert_eq!(back[0].id, id);
    assert_eq!(back[0].delivery_count, 2);
}

/// UC: UC-RET-001, UC-RET-008
#[tokio::test]
async fn ttl_and_the_dead_letter_work_on_messages_on_disk() {
    let h = tiny_memory();
    let mut topic = limited("orders", 1);
    topic.ttl = Some(600);
    h.topics.create(topic).await.unwrap();
    publish_ok(&h, "orders", "\"a\""); // takes up the memory
    let moving = publish_ok(&h, "orders", "\"b\"");
    let doomed = publish_ok(&h, "orders", "\"c\"");
    assert_eq!(h.messages.usage("orders"), (3, 2));

    let got = consume(&h, "orders", q(Some("3"), Some("10"), None)).await;
    assert_eq!(got.len(), 3);
    for m in &got {
        if m.id != doomed {
            h.messages.ack("orders", &m.id, Some(&m.receipt_token)).ok();
        }
    }
    // `moving` was confirmed above; republishes one that will really fail.
    let _ = moving;
    publish_ok(&h, "orders", "\"e\""); // takes up the memory again
    let failing = publish_ok(&h, "orders", "\"d\"");
    h.clock.advance(Duration::from_secs(20));
    let d = consume(&h, "orders", q(Some("5"), Some("10"), None)).await;
    assert!(d.iter().any(|m| m.id == failing));
    h.clock.advance(Duration::from_secs(10));
    h.messages.sweep();

    let dead = consume(&h, "orders-dead-letter", ConsumeQuery::default()).await;
    assert!(
        dead.iter().any(|m| m.id == failing),
        "same id in the dead-letter"
    );
    assert!(
        dead.iter()
            .all(|m| m.dead_letter_reason.as_deref() == Some("max-deliveries"))
    );
    assert_eq!(h.store.get("orders", &failing).unwrap(), None);
    assert!(
        h.dir
            .path()
            .join("orders-dead-letter")
            .join(format!("{failing}.body"))
            .exists()
    );

    // `ttl`: what expired disappears from disk, including the body.
    h.clock.advance(Duration::from_secs(3600));
    h.messages.sweep();
    assert_eq!(h.messages.usage("orders"), (0, 0));
    assert_eq!(h.messages.usage("orders-dead-letter"), (0, 0));
    assert!(
        consume(&h, "orders-dead-letter", ConsumeQuery::default())
            .await
            .is_empty()
    );
}

/// UC: UC-DRN-010, UC-DRN-022, UC-DRN-024, UC-ARM-024
#[tokio::test]
async fn drain_saves_everything_in_memory_with_its_metadata_and_a_restart_resumes_it() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let mut grouped = raw(b"\"x\"", "application/json");
    grouped.group_id = Some("g1".into());
    let in_flight = published_id(h.messages.publish("orders", grouped).unwrap());
    let ready = publish_ok(&h, "orders", "\"y\"");
    let m = consume(&h, "orders", q(Some("1"), Some("600"), None))
        .await
        .remove(0);
    assert_eq!(m.id, in_flight);
    assert_eq!(
        h.messages.usage("orders").1,
        0,
        "with no limit, nothing went to disk"
    );

    let report = h.messages.drain();
    assert_eq!(
        report,
        DrainReport {
            moved: 2,
            failed: 0,
            remaining: 0
        }
    );
    assert_eq!(h.messages.usage("orders"), (0, 2));
    let late = publish_ok(&h, "orders", "\"z\"");
    assert_eq!(
        h.messages.usage("orders"),
        (0, 3),
        "in the drain everything new goes straight to disk"
    );

    // The next start resumes from disk: the same in-processing message, with the same
    // token, and the ready ones.
    let restarted = reopen(&h, true);
    restarted
        .ack("orders", &in_flight, Some(&m.receipt_token))
        .expect("the receiptToken survived the save");
    let got = restarted
        .consume("orders", ConsumeQuery::default())
        .await
        .unwrap();
    let ids: Vec<&str> = got.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, [ready.as_str(), late.as_str()]);
}

/// UC: UC-DRN-010, UC-DRN-022
#[tokio::test]
async fn drain_keeps_the_group_and_the_delivery_count() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let mut grouped = raw(b"\"x\"", "text/plain");
    grouped.group_id = Some("g1".into());
    h.messages.publish("orders", grouped).unwrap();
    delivered_one(&h, "orders", "10").await;
    h.messages.drain();
    h.clock.advance(Duration::from_secs(10));

    let restarted = reopen(&h, true);
    let got = restarted
        .consume("orders", ConsumeQuery::default())
        .await
        .unwrap();
    assert_eq!(got[0].delivery_count, 2);
    assert_eq!(got[0].group_id.as_deref(), Some("g1"));
    assert_eq!(got[0].content_type, "text/plain");
}

/// UC: UC-DRN-027
#[tokio::test]
async fn without_resume_the_disk_is_cleaned_and_orphan_folders_never_come_back() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    h.topics.create(queue("other")).await.unwrap();
    // The "previous run": a node with little memory, which leaves messages on disk.
    let first = reopen_with(&h, true, &[("TABA_MEMORY_LIMIT", "3")]);
    for topic in ["orders", "other"] {
        for body in ["\"a\"", "\"b\""] {
            first.publish(topic, json_body(body)).unwrap();
        }
    }
    assert_eq!(first.usage("orders").1, 1);
    assert_eq!(
        first.usage("other").1,
        2,
        "the memory limit is the node's: the memory was already full"
    );
    drop(first); // the previous process ended

    let restarted = reopen(&h, true);
    assert_eq!(restarted.usage("orders").1, 1, "retomou");
    h.topics.delete("other").await.unwrap(); // the folder goes with the topic
    assert!(!h.dir.path().join("other").exists());
    drop(restarted);

    let wiped = reopen(&h, false);
    assert_eq!(wiped.usage("orders").1, 0, "without resume it starts empty");
    assert!(consume_with(&wiped, "orders").await.is_empty());
}

async fn consume_with(messages: &Arc<Messages>, topic: &str) -> Vec<Delivered> {
    messages
        .consume(topic, ConsumeQuery::default())
        .await
        .unwrap()
}

/// UC: UC-TOP-039
#[tokio::test]
async fn removing_a_topic_removes_its_folder_with_database_and_bodies() {
    let h = tiny_memory();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "\"a\"");
    publish_ok(&h, "orders", "\"b\"");
    assert!(h.dir.path().join("orders").is_dir());

    h.topics.delete("orders").await.unwrap();
    assert!(!h.dir.path().join("orders").exists());
    h.topics.create(queue("orders")).await.unwrap();
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
    assert_eq!(h.messages.usage("orders"), (0, 0));
}

/// UC: UC-DEF-024
#[tokio::test]
async fn topics_that_differ_only_in_case_keep_separate_folders() {
    let h = tiny_memory();
    h.topics.create(queue("orders")).await.unwrap();
    h.topics.create(queue("Orders")).await.unwrap();
    for topic in ["orders", "Orders"] {
        publish_ok(&h, topic, "\"a\"");
        publish_ok(&h, topic, "\"b\"");
    }
    assert!(h.dir.path().join("orders").is_dir());
    assert!(h.dir.path().join("~orders").is_dir());
    h.topics.delete("Orders").await.unwrap();
    assert_eq!(
        h.messages.usage("orders").1,
        1,
        "the other one was not touched"
    );
}

fn auto(h: &Harness, name: &str, f: impl FnOnce(&mut ConsumeQuery)) -> ConsumeQuery {
    let _ = (h, name);
    let mut query = ConsumeQuery {
        auto_create: Some("true".into()),
        can_create: true,
        ..Default::default()
    };
    f(&mut query);
    query
}

fn volatile_queue(name: &str, idle_secs: i64) -> CreateTopic {
    CreateTopic {
        volatile: Some(true),
        idle_timeout: Some(idle_secs),
        ..queue(name)
    }
}

/// UC: UC-CNS-031
#[tokio::test]
async fn auto_create_makes_a_volatile_queue_with_the_default_idle_timeout() {
    let h = harness();
    let got = h
        .messages
        .consume("new", auto(&h, "new", |_| {}))
        .await
        .unwrap();
    assert!(got.is_empty());
    let view = h.topics.get("new").unwrap();
    let queue = view.definition.queue().unwrap();
    assert!(queue.volatile);
    assert_eq!(queue.idle_timeout, Some(600));
    assert!(!queue.fifo);
}

/// UC: UC-CNS-032
#[tokio::test]
async fn auto_create_takes_the_creation_attributes_from_the_query() {
    let h = harness();
    h.messages
        .consume(
            "persistente",
            auto(&h, "persistente", |q| {
                q.volatile = Some("false".into());
                q.fifo = Some("true".into());
                q.tags.insert("time".into(), "financeiro".into());
            }),
        )
        .await
        .unwrap();
    let view = h.topics.get("persistente").unwrap();
    let queue = view.definition.queue().unwrap();
    assert!(!queue.volatile && queue.fifo);
    assert_eq!(view.definition.tags["time"], "financeiro");

    h.messages
        .consume(
            "short",
            auto(&h, "short", |q| q.idle_timeout = Some("120".into())),
        )
        .await
        .unwrap();
    let view = h.topics.get("short").unwrap();
    assert_eq!(view.definition.queue().unwrap().idle_timeout, Some(120));

    // `idleTimeout` on a queue that is not volatile does not apply; out of range is `400`.
    for (volatile, idle, code) in [
        ("false", "120", ErrorCode::ValidationNotApplicable),
        ("true", "5", ErrorCode::ValidationOutOfRange),
        ("true", "lots", ErrorCode::ValidationFailed),
    ] {
        let query = auto(&h, "x", |q| {
            q.volatile = Some(volatile.into());
            q.idle_timeout = Some(idle.into());
        });
        assert_eq!(consume_err(&h, "x", query).await, code, "{volatile} {idle}");
    }
    assert!(h.topics.get("x").is_err(), "none of them created the queue");
}

/// UC: UC-CNS-033
#[tokio::test]
async fn auto_create_on_an_existing_queue_ignores_the_creation_parameters() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    publish_ok(&h, "orders", "1");
    let query = auto(&h, "orders", |q| {
        q.volatile = Some("true".into());
        q.idle_timeout = Some("60".into());
        q.fifo = Some("true".into());
        q.tags.insert("a".into(), "b".into());
    });
    let got = h.messages.consume("orders", query).await.unwrap();
    assert_eq!(got.len(), 1, "it is a normal consume");
    let view = h.topics.get("orders").unwrap();
    assert!(
        !view.definition.queue().unwrap().volatile,
        "the queue did not change"
    );
    assert!(view.definition.tags.is_empty());
}

/// UC: UC-CNS-034, UC-CNS-035
#[tokio::test]
async fn auto_create_never_makes_a_distributor_and_refuses_bad_names() {
    let h = harness();
    h.topics.create(distributor("dist", &["a-"])).await.unwrap();
    assert_eq!(
        consume_err(&h, "dist", auto(&h, "dist", |_| {})).await,
        ErrorCode::TopicWrongType
    );
    assert_eq!(
        consume_err(&h, "x-dead-letter", auto(&h, "x", |_| {})).await,
        ErrorCode::ValidationReservedName
    );
    assert_eq!(
        consume_err(&h, "with space", auto(&h, "x", |_| {})).await,
        ErrorCode::ValidationInvalidName
    );
}

/// UC: UC-CNS-031
#[tokio::test]
async fn without_auto_create_a_missing_topic_is_404_and_a_bad_flag_is_400() {
    let h = harness();
    let plain = ConsumeQuery {
        can_create: true,
        ..Default::default()
    };
    assert_eq!(
        consume_err(&h, "nothing", plain).await,
        ErrorCode::TopicNotFound
    );
    let off = auto(&h, "nothing", |q| q.auto_create = Some("false".into()));
    assert_eq!(
        consume_err(&h, "nothing", off).await,
        ErrorCode::TopicNotFound
    );
    let bad = auto(&h, "nothing", |q| q.auto_create = Some("sim".into()));
    assert_eq!(
        consume_err(&h, "nothing", bad).await,
        ErrorCode::ValidationFailed
    );
}

/// UC: UC-CNS-040
#[tokio::test]
async fn creating_by_auto_create_needs_the_create_permission_but_consuming_does_not() {
    let h = harness();
    let no_create = auto(&h, "new", |q| q.can_create = false);
    assert_eq!(
        consume_err(&h, "new", no_create).await,
        ErrorCode::AuthForbidden
    );
    assert!(h.topics.get("new").is_err());

    h.topics.create(queue("exists")).await.unwrap();
    let query = auto(&h, "exists", |q| q.can_create = false);
    assert!(
        h.messages
            .consume("exists", query)
            .await
            .unwrap()
            .is_empty()
    );
}

/// UC: UC-CNS-036
#[tokio::test]
async fn auto_create_stops_at_the_volatile_ceiling_but_post_does_not() {
    let h = harness_with(&[("TABA_TOPICS_VOLATILE_MAX_COUNT", "2")]);
    for name in ["a", "b"] {
        h.messages
            .consume(name, auto(&h, name, |_| {}))
            .await
            .unwrap();
    }
    assert_eq!(
        consume_err(&h, "c", auto(&h, "c", |_| {})).await,
        ErrorCode::TopicVolatileLimit
    );
    assert!(h.topics.get("c").is_err());
    // A queue that is not volatile does not enter the count, and an existing one remains consumable.
    let persistent = auto(&h, "p", |q| q.volatile = Some("false".into()));
    h.messages.consume("p", persistent).await.unwrap();
    h.messages
        .consume("a", auto(&h, "a", |_| {}))
        .await
        .unwrap();
    // The ceiling applies to `autoCreate`; normal creation is not blocked.
    h.topics.create(volatile_queue("d", 60)).await.unwrap();
    assert_eq!(h.topics.volatile_queues().len(), 3);

    // Without the ceiling there is no limit.
    let free = harness();
    for n in 0..20 {
        let name = format!("q{n}");
        free.messages
            .consume(&name, auto(&free, &name, |_| {}))
            .await
            .unwrap();
    }
}

/// UC: UC-CNS-038
#[tokio::test]
async fn consumers_racing_to_auto_create_the_same_name_all_succeed() {
    let h = harness();
    let results = futures_join(
        (0..8)
            .map(|_| {
                let messages = h.messages.clone();
                let query = auto(&h, "corrida", |_| {});
                tokio::spawn(async move { messages.consume("corrida", query).await })
            })
            .collect(),
    )
    .await;
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    assert_eq!(h.topics.volatile_queues().len(), 1);
}

async fn futures_join<T: Send + 'static>(handles: Vec<tokio::task::JoinHandle<T>>) -> Vec<T> {
    let mut out = Vec::new();
    for handle in handles {
        out.push(handle.await.unwrap());
    }
    out
}

/// UC: UC-VOL-001, UC-VOL-002
#[tokio::test]
async fn an_idle_volatile_queue_is_deleted_with_its_messages_and_dead_letter() {
    let h = harness();
    h.topics
        .create(volatile_queue("ephemeral", 60))
        .await
        .unwrap();
    publish_ok(&h, "ephemeral", "1");
    publish_ok(&h, "ephemeral", "2");
    delivered_one(&h, "ephemeral", "5").await; // the clock already counted this `GET`
    h.clock.advance(Duration::from_secs(10));

    h.clock.advance(Duration::from_secs(49));
    assert!(h.messages.expire_idle().await.is_empty(), "faltam 1 s");
    h.clock.advance(Duration::from_secs(1));
    assert_eq!(h.messages.expire_idle().await, ["ephemeral"]);
    assert!(h.topics.get("ephemeral").is_err());
    assert!(h.topics.get("ephemeral-dead-letter").is_err());
    assert_eq!(
        h.messages.usage("ephemeral"),
        (0, 0),
        "the messages went with it"
    );
    assert!(h.messages.expire_idle().await.is_empty());
}

/// UC: UC-VOL-002
#[tokio::test]
async fn the_clock_runs_from_the_creation_and_is_reset_by_each_get() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    h.clock.advance(Duration::from_secs(59));
    consume(&h, "queue", q(None, None, None)).await; // reinicia o prazo
    h.clock.advance(Duration::from_secs(59));
    assert!(h.messages.expire_idle().await.is_empty());
    h.clock.advance(Duration::from_secs(1));
    assert_eq!(h.messages.expire_idle().await, ["queue"]);
}

/// UC: UC-VOL-003
#[tokio::test]
async fn any_get_counts_with_or_without_messages_and_a_long_poll_counts_from_its_start() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    h.clock.advance(Duration::from_secs(30));
    let messages = h.messages.clone();
    let poll =
        tokio::spawn(async move { messages.consume("queue", q(None, None, Some("2"))).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The `GET` is still waiting, and the instant was already recorded at the start.
    h.clock.advance(Duration::from_secs(40));
    assert!(
        h.messages.expire_idle().await.is_empty(),
        "30 s + 60 s has not arrived: the in-progress GET already counted"
    );
    poll.await.unwrap().unwrap();
}

/// UC: UC-VOL-004
#[tokio::test]
async fn publishing_does_not_count_as_a_consumer() {
    let h = harness();
    h.topics
        .create(volatile_queue("so-produtor", 60))
        .await
        .unwrap();
    h.clock.advance(Duration::from_secs(40));
    publish_ok(&h, "so-produtor", "1");
    h.clock.advance(Duration::from_secs(20));
    assert_eq!(h.messages.expire_idle().await, ["so-produtor"]);
}

/// UC: UC-VOL-005, UC-RET-017
#[tokio::test]
async fn a_get_on_the_dead_letter_does_not_count_as_a_consumer() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    h.clock.advance(Duration::from_secs(40));
    consume(&h, "queue-dead-letter", q(None, None, None)).await;
    h.clock.advance(Duration::from_secs(20));
    assert_eq!(h.messages.expire_idle().await, ["queue"]);
}

/// UC: UC-VOL-021
#[tokio::test]
async fn patching_the_idle_timeout_applies_to_the_next_check() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    h.clock.advance(Duration::from_secs(40));
    h.topics
        .update(
            "queue",
            crate::engine::topics::UpdateTopic {
                idle_timeout: Some(300),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    h.clock.advance(Duration::from_secs(100));
    assert!(h.messages.expire_idle().await.is_empty());
    h.clock.advance(Duration::from_secs(160));
    assert_eq!(h.messages.expire_idle().await, ["queue"]);
}

/// UC: UC-VOL-001
#[tokio::test]
async fn persistent_queues_and_distributors_are_never_deleted_for_idleness() {
    let h = harness();
    h.topics.create(queue("persistente")).await.unwrap();
    h.topics.create(distributor("dist", &["a-"])).await.unwrap();
    h.clock.advance(Duration::from_secs(10 * 86_400));
    assert!(h.messages.expire_idle().await.is_empty());
    assert!(h.topics.get("persistente").is_ok() && h.topics.get("dist").is_ok());
}

/// UC: UC-VOL-022
#[tokio::test]
async fn a_fifo_queue_can_be_volatile() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            fifo: Some(true),
            ..volatile_queue("fifo", 60)
        })
        .await
        .unwrap();
    h.clock.advance(Duration::from_secs(60));
    assert_eq!(h.messages.expire_idle().await, ["fifo"]);
}

/// UC: UC-DRN-023
#[tokio::test]
async fn a_restart_does_not_count_the_time_the_node_was_stopped() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    h.clock.advance(Duration::from_secs(3600)); // the node stayed stopped for an hour
    let restarted = reopen(&h, true);
    assert!(
        restarted.expire_idle().await.is_empty(),
        "the deadline counts from the start, not from the creation"
    );
    h.clock.advance(Duration::from_secs(59));
    assert!(restarted.expire_idle().await.is_empty());
    h.clock.advance(Duration::from_secs(1));
    assert_eq!(restarted.expire_idle().await, ["queue"]);
}

/// UC: UC-VOL-014
#[tokio::test]
async fn an_ack_after_the_queue_was_deleted_is_not_found() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    publish_ok(&h, "queue", "1");
    let m = delivered_one(&h, "queue", "600").await;
    h.clock.advance(Duration::from_secs(60));
    assert_eq!(h.messages.expire_idle().await, ["queue"]);
    let error = h
        .messages
        .ack("queue", &m.id, Some(&m.receipt_token))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TopicNotFound);
}

/// UC: UC-VOL-012
#[tokio::test]
async fn a_recreated_queue_is_not_deleted_by_the_old_deadline() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    let old = h.topics.volatile_queues().remove(0);
    h.topics.delete("queue").await.unwrap();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    let error = h
        .topics
        .delete_expected("queue", &old.id)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TopicNotFound);
    assert!(h.topics.get("queue").is_ok(), "the new queue stayed");
}

/// UC: UC-VOL-002
#[tokio::test]
async fn a_recreated_queue_does_not_inherit_the_last_get_of_the_old_one() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    h.clock.advance(Duration::from_secs(50));
    consume(&h, "queue", q(None, None, None)).await;
    h.topics.delete("queue").await.unwrap();
    h.clock.advance(Duration::from_secs(5));
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    h.clock.advance(Duration::from_secs(60));
    assert_eq!(
        h.messages.expire_idle().await,
        ["queue"],
        "count from the new creation"
    );
}

fn publish_grouped(h: &Harness, topic: &str, group: Option<&str>, body: &str) -> String {
    let mut request = json_body(body);
    request.group_id = group.map(Into::into);
    published_id(h.messages.publish(topic, request).unwrap())
}

fn groups_of(messages: &[Delivered]) -> Vec<String> {
    messages
        .iter()
        .map(|m| m.group_id.clone().unwrap_or_default())
        .collect()
}

/// UC: UC-GRP-010, UC-GRP-003
#[tokio::test]
async fn a_small_group_is_delivered_before_the_big_one_is_exhausted() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    for n in 0..100 {
        publish_grouped(&h, "orders", Some("noisy"), &n.to_string());
    }
    for n in 0..3 {
        publish_grouped(&h, "orders", Some("small"), &n.to_string());
    }
    let first = consume(&h, "orders", q(Some("3"), Some("600"), None)).await;
    assert_eq!(
        groups_of(&first),
        ["small", "small", "small"],
        "the 3 of the small group come out before any of the big one, even if published later"
    );
    // On a plain queue the group does not serialize: several of the same group go together, in flight.
    let more = consume(&h, "orders", q(Some("5"), Some("600"), None)).await;
    assert_eq!(more.len(), 5);
}

/// UC: UC-GRP-011
#[tokio::test]
async fn every_nth_delivery_takes_the_oldest_message_so_the_big_group_never_starves() {
    let h = harness_with(&[("TABA_DELIVERY_FAIR_NORMAL_EVERY", "5")]);
    h.topics.create(queue("orders")).await.unwrap();
    let oldest = publish_grouped(&h, "orders", Some("big"), "0");
    for n in 1..50 {
        publish_grouped(&h, "orders", Some("big"), &n.to_string());
    }
    for n in 0..20 {
        publish_grouped(&h, "orders", Some("small"), &n.to_string());
    }
    let got = consume(&h, "orders", q(Some("5"), Some("600"), None)).await;
    assert!(
        got.iter().any(|m| m.id == oldest),
        "the 5th delivery takes the oldest of all, from the big group"
    );
    assert!(groups_of(&got).iter().filter(|g| *g == "small").count() >= 3);

    // With `normal-every` zero the guarantee goes away: only the small one comes out while it has messages.
    let off = harness_with(&[("TABA_DELIVERY_FAIR_NORMAL_EVERY", "0")]);
    off.topics.create(queue("orders")).await.unwrap();
    let old = publish_grouped(&off, "orders", Some("big"), "0");
    for n in 1..50 {
        publish_grouped(&off, "orders", Some("big"), &n.to_string());
    }
    for n in 0..4 {
        publish_grouped(&off, "orders", Some("small"), &n.to_string());
    }
    let got = consume(&off, "orders", q(Some("4"), Some("600"), None)).await;
    assert!(groups_of(&got).iter().all(|g| g == "small"));
    assert!(got.iter().all(|m| m.id != old));
}

/// UC: UC-GRP-013, UC-DRN-023
#[tokio::test]
async fn the_index_is_rebuilt_from_the_disk_after_a_restart() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    for n in 0..20 {
        publish_grouped(&h, "orders", Some("big"), &n.to_string());
    }
    for n in 0..2 {
        publish_grouped(&h, "orders", Some("small"), &n.to_string());
    }
    let in_flight = consume(&h, "orders", q(Some("1"), Some("600"), None)).await;
    assert_eq!(groups_of(&in_flight), ["small"]);
    h.messages.drain();

    let restarted = reopen(&h, true);
    // What was in processing stays invisible; the small group still has its turn.
    let got = restarted
        .consume("orders", q(Some("2"), Some("600"), None))
        .await
        .unwrap();
    assert_eq!(groups_of(&got), ["small", "big"]);
    assert_ne!(got[0].id, in_flight[0].id);
    let again = restarted
        .consume("orders", q(Some("100"), Some("600"), None))
        .await
        .unwrap();
    assert_eq!(again.len(), 19, "20 + 2 minus the 3 already in flight");
}

/// UC: UC-RET-006
#[tokio::test]
async fn an_exhausted_message_that_waited_out_the_downtime_goes_to_the_dead_letter() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            max_deliveries: Some(1),
            ..queue("orders")
        })
        .await
        .unwrap();
    let id = publish_ok(&h, "orders", "1");
    delivered_one(&h, "orders", "10").await;
    h.messages.drain();
    h.clock.advance(Duration::from_secs(60)); // the deadline expired with the node stopped

    let restarted = reopen(&h, true);
    assert!(
        restarted
            .consume("orders", ConsumeQuery::default())
            .await
            .unwrap()
            .is_empty()
    );
    let dead = restarted
        .consume("orders-dead-letter", ConsumeQuery::default())
        .await
        .unwrap();
    assert_eq!(dead[0].id, id);
}

/// UC: UC-RET-006
#[tokio::test]
async fn lowering_max_deliveries_sends_the_ready_messages_over_the_new_limit_to_the_dead_letter() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            max_deliveries: Some(5),
            ..queue("orders")
        })
        .await
        .unwrap();
    let id = publish_ok(&h, "orders", "1");
    for _ in 0..2 {
        delivered_one(&h, "orders", "0").await; // comes back right away, with 1 and then 2 deliveries
    }
    h.topics
        .update(
            "orders",
            crate::engine::topics::UpdateTopic {
                max_deliveries: crate::engine::topics::Patch::Value(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        consume(&h, "orders", ConsumeQuery::default())
            .await
            .is_empty()
    );
    let dead = consume(&h, "orders-dead-letter", ConsumeQuery::default()).await;
    assert_eq!(dead[0].id, id);
}

/// UC: UC-ARM-013
#[tokio::test]
async fn a_slow_topic_never_blocks_the_other_topics() {
    let h = harness();
    h.topics.create(queue("slow")).await.unwrap();
    h.topics.create(queue("rapido")).await.unwrap();
    publish_ok(&h, "slow", "1");

    // Holds the slow topic's lock, like a long write to disk.
    let slow = h.messages.state_of("slow");
    let held = lock(&slow);
    let messages = h.messages.clone();
    let worker = std::thread::spawn(move || {
        let id = published_id(messages.publish("rapido", json_body("2")).unwrap());
        let got = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(messages.consume("rapido", ConsumeQuery::default()))
            .unwrap();
        assert_eq!(got[0].id, id);
        messages
            .ack("rapido", &got[0].id, Some(&got[0].receipt_token))
            .unwrap();
    });
    // The other topic moves along in full with the slow one locked.
    worker
        .join()
        .expect("the fast topic did not wait for the slow one");
    drop(held);
    assert_eq!(consume(&h, "slow", ConsumeQuery::default()).await.len(), 1);
}

/// UC: UC-ARM-006, UC-ARM-004
#[tokio::test]
async fn a_new_topic_keeps_its_reserve_by_pushing_the_excess_of_a_low_rate_topic_to_disk() {
    let h = harness_with(&[
        ("TABA_MEMORY_LIMIT", "4"),
        ("TABA_MEMORY_TOPIC_RESERVE", "1"),
    ]);
    h.topics.create(queue("old")).await.unwrap();
    h.topics.create(queue("new")).await.unwrap();
    for n in 0..4 {
        publish_ok(&h, "old", &n.to_string());
    }
    assert_eq!(h.messages.usage("old"), (4, 0), "memory is full");

    // The new topic has nothing in memory yet: its reserve is worth more than the old one's excess.
    publish_ok(&h, "new", "9");
    assert_eq!(
        h.messages.usage("new"),
        (1, 0),
        "the first message did not go to disk"
    );
    assert_eq!(
        h.messages.usage("old"),
        (3, 1),
        "the old one yielded one to disk"
    );
}

/// UC: UC-ARM-004, UC-ARM-005
#[tokio::test]
async fn above_its_reserve_a_topic_only_takes_memory_from_topics_that_are_read_less() {
    let h = harness_with(&[
        ("TABA_MEMORY_LIMIT", "4"),
        ("TABA_MEMORY_TOPIC_RESERVE", "1"),
    ]);
    h.topics.create(queue("a")).await.unwrap();
    h.topics.create(queue("b")).await.unwrap();
    for n in 0..4 {
        publish_ok(&h, "b", &n.to_string());
    }
    publish_ok(&h, "a", "1"); // within the reserve: takes one from b
    assert_eq!(h.messages.usage("a"), (1, 0));

    // It passed the reserve and nobody reads less than a: the new one goes to disk.
    publish_ok(&h, "a", "2");
    assert_eq!(h.messages.usage("a"), (1, 1));
    assert_eq!(h.messages.usage("b"), (3, 1));

    // a starts being read: its rate rises above b's, and it gains memory from b.
    let got = consume(&h, "a", q(Some("1"), Some("600"), None)).await;
    assert_eq!(got.len(), 1);
    let (_, rate_a) = h.messages.rates("a");
    let (_, rate_b) = h.messages.rates("b");
    assert!(rate_a > rate_b, "{rate_a} > {rate_b}");
    publish_ok(&h, "a", "3");
    assert_eq!(h.messages.usage("a").0, 2, "a gained memory");
    assert_eq!(h.messages.usage("b"), (2, 2), "b yielded one more to disk");
}

/// UC: UC-ARM-004
#[tokio::test]
async fn a_topic_that_is_read_more_is_never_demoted_by_one_that_is_read_less() {
    let h = harness_with(&[
        ("TABA_MEMORY_LIMIT", "3"),
        ("TABA_MEMORY_TOPIC_RESERVE", "1"),
    ]);
    h.topics.create(queue("hot")).await.unwrap();
    h.topics.create(queue("cold")).await.unwrap();
    for n in 0..3 {
        publish_ok(&h, "hot", &n.to_string());
    }
    consume(&h, "hot", q(Some("1"), Some("0"), None)).await; // gives the hot one a read rate
    publish_ok(&h, "cold", "1"); // within the reserve: the hot one yields one
    assert_eq!(h.messages.usage("cold"), (1, 0));
    publish_ok(&h, "cold", "2"); // passed the reserve and reads less: goes to disk
    assert_eq!(h.messages.usage("cold"), (1, 1));
    assert_eq!(
        h.messages.usage("hot"),
        (2, 1),
        "the hot one did not yield anything more"
    );
}

/// UC: UC-ARM-007
#[tokio::test]
async fn demotion_takes_the_middle_of_the_ready_list_and_keeps_the_ends() {
    let h = harness_with(&[
        ("TABA_MEMORY_LIMIT", "5"),
        ("TABA_MEMORY_TOPIC_RESERVE", "1"),
    ]);
    h.topics.create(queue("old")).await.unwrap();
    h.topics.create(queue("new")).await.unwrap();
    let ids: Vec<String> = (0..5)
        .map(|n| publish_ok(&h, "old", &n.to_string()))
        .collect();
    publish_ok(&h, "new", "1");
    // The oldest and the newest (the accordion's ends) stay in memory; the middle went to disk.
    assert_eq!(
        h.store.get("old", &ids[2]).unwrap().map(|m| m.id),
        Some(ids[2].clone())
    );
    for end in [&ids[0], &ids[4]] {
        assert_eq!(
            h.store.get("old", end).unwrap(),
            None,
            "the ends stay in memory"
        );
    }
}

/// UC: UC-ARM-007
#[tokio::test]
async fn messages_in_processing_are_demoted_before_the_ready_ones() {
    let h = harness_with(&[
        ("TABA_MEMORY_LIMIT", "4"),
        ("TABA_MEMORY_TOPIC_RESERVE", "1"),
    ]);
    h.topics.create(queue("old")).await.unwrap();
    h.topics.create(queue("new")).await.unwrap();
    for n in 0..4 {
        publish_ok(&h, "old", &n.to_string());
    }
    let in_flight = consume(&h, "old", q(Some("1"), Some("600"), None)).await;
    publish_ok(&h, "new", "1");
    assert!(
        h.store.get("old", &in_flight[0].id).unwrap().is_some(),
        "the one that was in processing was the one demoted"
    );
}

/// UC: UC-ARM-005
#[tokio::test]
async fn publish_and_consume_rates_are_tracked_per_topic() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    for n in 0..30 {
        publish_ok(&h, "orders", &n.to_string());
    }
    consume(&h, "orders", q(Some("10"), Some("600"), None)).await;
    let (published, consumed) = h.messages.rates("orders");
    assert!((published - 30.0 / 300.0).abs() < 1e-9, "{published}");
    assert!((consumed - 10.0 / 300.0).abs() < 1e-9, "{consumed}");
    h.clock.advance(Duration::from_secs(301));
    assert_eq!(h.messages.rates("orders"), (0.0, 0.0), "left the window");
}

/// UC: UC-CNS-029
#[tokio::test]
async fn waiting_gets_are_served_in_the_order_they_arrived() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let spawn_wait = |h: &Harness| {
        let messages = h.messages.clone();
        tokio::spawn(async move {
            messages
                .consume("orders", q(Some("1"), Some("600"), Some("5")))
                .await
        })
    };
    let first = spawn_wait(&h);
    tokio::time::sleep(Duration::from_millis(60)).await;
    let second = spawn_wait(&h);
    tokio::time::sleep(Duration::from_millis(60)).await;
    let third = spawn_wait(&h);
    tokio::time::sleep(Duration::from_millis(60)).await;

    let a = publish_ok(&h, "orders", "1");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        first.is_finished(),
        "the first in the listener queue takes the message"
    );
    assert!(!second.is_finished() && !third.is_finished());
    assert_eq!(first.await.unwrap().unwrap()[0].id, a);

    let b = publish_ok(&h, "orders", "2");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(second.is_finished() && !third.is_finished());
    assert_eq!(second.await.unwrap().unwrap()[0].id, b);

    let c = publish_ok(&h, "orders", "3");
    assert_eq!(third.await.unwrap().unwrap()[0].id, c);
}

/// UC: UC-CNS-029
#[tokio::test]
async fn one_publish_with_several_messages_wakes_the_next_waiting_get_too() {
    let h = harness();
    h.topics.create(queue("orders")).await.unwrap();
    let spawn_wait = |h: &Harness| {
        let messages = h.messages.clone();
        tokio::spawn(async move {
            messages
                .consume("orders", q(Some("1"), Some("600"), Some("5")))
                .await
        })
    };
    let first = spawn_wait(&h);
    tokio::time::sleep(Duration::from_millis(60)).await;
    let second = spawn_wait(&h);
    tokio::time::sleep(Duration::from_millis(60)).await;
    publish_ok(&h, "orders", "1");
    publish_ok(&h, "orders", "2");
    let got_first = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let got_second = tokio::time::timeout(Duration::from_secs(2), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(got_first.len() + got_second.len(), 2);
    assert_ne!(got_first[0].id, got_second[0].id);
}

fn fifo(name: &str) -> CreateTopic {
    CreateTopic {
        fifo: Some(true),
        ..queue(name)
    }
}

fn fifo_publish(
    h: &Harness,
    topic: &str,
    group: &str,
    dedup: Option<&str>,
    body: &str,
) -> (String, Option<bool>) {
    let request = PublishRequest {
        body: body.as_bytes().to_vec(),
        group_id: Some(group.into()),
        deduplication_id: dedup.map(Into::into),
        ..Default::default()
    };
    match h.messages.publish(topic, request).unwrap() {
        Published::Queue { id, duplicate } => (id, duplicate),
        Published::Distributor { .. } => panic!("expected a queue"),
    }
}

async fn fifo_get(h: &Harness, topic: &str, max: &str) -> Vec<Delivered> {
    consume(h, topic, q(Some(max), Some("600"), None)).await
}

fn ack(h: &Harness, topic: &str, m: &Delivered) {
    h.messages
        .ack(topic, &m.id, Some(&m.receipt_token))
        .unwrap();
}

/// UC: UC-FIF-002, UC-FIF-003
#[tokio::test]
async fn a_fifo_publish_needs_a_group_and_says_whether_it_was_a_duplicate() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    assert_eq!(
        publish_err(&h, "f", json_body("1")),
        ErrorCode::ValidationGroupIdRequired
    );
    let (_, duplicate) = fifo_publish(&h, "f", "g1", None, "1");
    assert_eq!(duplicate, Some(false));
    // Outside FIFO there is no `duplicate`.
    h.topics.create(queue("plain")).await.unwrap();
    assert!(matches!(
        h.messages.publish("plain", json_body("1")).unwrap(),
        Published::Queue {
            duplicate: None,
            ..
        }
    ));
}

/// UC: UC-FIF-004, UC-FIF-006
#[tokio::test]
async fn a_repeated_publish_in_the_window_returns_the_original_and_creates_nothing() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (original, _) = fifo_publish(&h, "f", "g1", Some("order-9001"), "1");
    let (again, duplicate) = fifo_publish(&h, "f", "g1", Some("order-9001"), "2");
    assert_eq!((again.as_str(), duplicate), (original.as_str(), Some(true)));

    let got = fifo_get(&h, "f", "10").await;
    assert_eq!(got.len(), 1, "no new message");
    assert_eq!(got[0].id, original);
    assert_eq!(got[0].deduplication_id.as_deref(), Some("order-9001"));
}

/// UC: UC-FIF-005
#[tokio::test]
async fn without_a_deduplication_id_the_sha256_of_the_body_is_the_key() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (first, _) = fifo_publish(&h, "f", "g1", None, "{ \"a\": 1 }");
    // The same body is a duplicate, even with another `Content-Type`.
    let request = PublishRequest {
        body: b"{ \"a\": 1 }".to_vec(),
        content_type: Some("text/plain".into()),
        group_id: Some("g1".into()),
        ..Default::default()
    };
    let Published::Queue { id, duplicate } = h.messages.publish("f", request).unwrap() else {
        panic!("expected a queue");
    };
    assert_eq!((id, duplicate), (first.clone(), Some(true)));
    // Another body, another message.
    let (second, duplicate) = fifo_publish(&h, "f", "g1", None, "{ \"a\": 2 }");
    assert_eq!(duplicate, Some(false));
    assert_ne!(second, first);

    let got = fifo_get(&h, "f", "10").await;
    let hash = got[0].deduplication_id.clone().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    // The SHA-256 of `{ "a": 1 }`, computed outside the code.
    assert_eq!(hash, body_hash(b"{ \"a\": 1 }"));
    assert_eq!(
        body_hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// UC: UC-FIF-007, UC-FIF-008
#[tokio::test]
async fn the_window_ends_and_the_scope_is_the_group() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (original, _) = fifo_publish(&h, "f", "g1", Some("k"), "1");
    let (other_group, duplicate) = fifo_publish(&h, "f", "g2", Some("k"), "1");
    assert_eq!(duplicate, Some(false), "another group is not a duplicate");
    assert_ne!(other_group, original);

    h.clock.advance(Duration::from_secs(299));
    assert_eq!(fifo_publish(&h, "f", "g1", Some("k"), "1").1, Some(true));
    h.clock.advance(Duration::from_secs(2));
    let (renewed, duplicate) = fifo_publish(&h, "f", "g1", Some("k"), "1");
    assert_eq!(
        duplicate,
        Some(false),
        "the window counts from the original's id"
    );
    assert_ne!(renewed, original);
}

/// UC: UC-FIF-011, UC-FIF-036, UC-FIF-040
#[tokio::test]
async fn an_ack_leaves_a_short_row_that_keeps_deduplicating_and_is_invisible_to_everything_else() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (id, _) = fifo_publish(&h, "f", "g1", Some("k"), "1");
    let m = fifo_get(&h, "f", "1").await.remove(0);
    ack(&h, "f", &m);

    // The short row exists, with no body or delivery metadata.
    let row = h.store.get("f", &id).unwrap().expect("the row stayed");
    assert_eq!(row.deduplication_id.as_deref(), Some("k"));
    assert_eq!(row.group_id.as_deref(), Some("g1"));
    assert_eq!((row.delivery_count, row.receipt_token), (0, None));
    assert!(h.store.body("f", &id).is_err(), "o corpo saiu");
    assert_eq!(
        h.messages.usage("f"),
        (0, 0),
        "the short row does not count"
    );

    // Deduplication keeps applying after the ack.
    let (again, duplicate) = fifo_publish(&h, "f", "g1", Some("k"), "1");
    assert_eq!((again.as_str(), duplicate), (id.as_str(), Some(true)));

    // And the rest of the queue does not see it.
    assert!(fifo_get(&h, "f", "10").await.is_empty());
    assert_eq!(
        h.messages
            .ack("f", &id, Some(&m.receipt_token))
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
    assert_eq!(
        h.messages
            .set_visibility("f", &id, Some(&m.receipt_token), Some(30))
            .unwrap_err()
            .code,
        ErrorCode::MessageNotFound
    );
}

/// UC: UC-FIF-037
#[tokio::test]
async fn an_ack_after_the_window_deletes_the_row_at_once() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (id, _) = fifo_publish(&h, "f", "g1", Some("k"), "1");
    let m = fifo_get(&h, "f", "1").await.remove(0);
    h.clock.advance(Duration::from_secs(301));
    ack(&h, "f", &m);
    assert!(h.store.get("f", &id).unwrap().is_none());
}

/// UC: UC-FIF-039
#[tokio::test]
async fn the_sweep_deletes_the_rows_whose_delete_at_passed() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (id, _) = fifo_publish(&h, "f", "g1", Some("k"), "1");
    let m = fifo_get(&h, "f", "1").await.remove(0);
    ack(&h, "f", &m);
    h.messages.sweep();
    assert!(
        h.store.get("f", &id).unwrap().is_some(),
        "still inside the window"
    );

    h.clock.advance(Duration::from_secs(300));
    h.messages.sweep();
    assert!(h.store.get("f", &id).unwrap().is_none());
    let (_, duplicate) = fifo_publish(&h, "f", "g1", Some("k"), "1");
    assert_eq!(duplicate, Some(false));
}

/// UC: UC-FIF-012, UC-FIF-018
#[tokio::test]
async fn the_next_message_of_a_group_waits_for_the_ack_of_the_previous_one() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (a, _) = fifo_publish(&h, "f", "g1", Some("a"), "1");
    let (b, _) = fifo_publish(&h, "f", "g1", Some("b"), "2");
    let first = fifo_get(&h, "f", "10").await;
    assert_eq!(
        first.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [a.as_str()]
    );
    assert!(
        fifo_get(&h, "f", "10").await.is_empty(),
        "the group is busy"
    );
    ack(&h, "f", &first[0]);
    let second = fifo_get(&h, "f", "10").await;
    assert_eq!(second[0].id, b);
}

/// UC: UC-FIF-013, UC-FIF-014
#[tokio::test]
async fn different_groups_go_in_parallel_and_a_batch_never_has_two_of_the_same_group() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    for n in 0..3 {
        fifo_publish(&h, "f", "g1", Some(&format!("a{n}")), "1");
        fifo_publish(&h, "f", "g2", Some(&format!("b{n}")), "1");
        fifo_publish(&h, "f", "g3", Some(&format!("c{n}")), "1");
    }
    let batch = fifo_get(&h, "f", "10").await;
    assert_eq!(batch.len(), 3);
    let mut groups = groups_of(&batch);
    groups.sort();
    assert_eq!(groups, ["g1", "g2", "g3"]);
}

/// UC: UC-FIF-015, UC-FIF-019, UC-FIF-020
#[tokio::test]
async fn inside_a_group_the_order_is_the_id_and_among_free_groups_the_oldest_head_wins() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (a1, _) = fifo_publish(&h, "f", "a", Some("1"), "1");
    let (b1, _) = fifo_publish(&h, "f", "b", Some("1"), "1");
    let (a2, _) = fifo_publish(&h, "f", "a", Some("2"), "1");
    let (b2, _) = fifo_publish(&h, "f", "b", Some("2"), "1");

    // The heads are a1 and b1; the oldest comes out first, with no band or accordion.
    let one = fifo_get(&h, "f", "1").await;
    assert_eq!(one[0].id, a1);
    let two = fifo_get(&h, "f", "1").await;
    assert_eq!(two[0].id, b1);
    ack(&h, "f", &one[0]);
    ack(&h, "f", &two[0]);
    let order: Vec<String> = fifo_get(&h, "f", "10")
        .await
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(
        order,
        [a2, b2],
        "the next of each group, by the oldest head"
    );
}

/// UC: UC-FIF-016, UC-FIF-017
#[tokio::test]
async fn an_expired_or_released_head_comes_back_first_and_the_order_holds() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (a, _) = fifo_publish(&h, "f", "g", Some("a"), "1");
    let (b, _) = fifo_publish(&h, "f", "g", Some("b"), "2");
    let first = h
        .messages
        .consume("f", q(Some("1"), Some("10"), None))
        .await
        .unwrap();
    assert_eq!(first[0].id, a);

    h.clock.advance(Duration::from_secs(10));
    let again = h
        .messages
        .consume("f", q(Some("1"), Some("10"), None))
        .await
        .unwrap();
    assert_eq!(
        (again[0].id.as_str(), again[0].delivery_count),
        (a.as_str(), 2)
    );

    // `PUT visibility` with 0 on the head releases the group like an expiry.
    h.messages
        .set_visibility("f", &a, Some(&again[0].receipt_token), Some(0))
        .unwrap();
    let third = h
        .messages
        .consume("f", q(Some("1"), Some("600"), None))
        .await
        .unwrap();
    assert_eq!(
        (third[0].id.as_str(), third[0].delivery_count),
        (a.as_str(), 3)
    );
    ack(&h, "f", &third[0]);
    assert_eq!(fifo_get(&h, "f", "1").await[0].id, b);
}

/// UC: UC-FIF-021
#[tokio::test]
async fn the_head_over_max_deliveries_goes_to_the_dead_letter_and_the_group_goes_on() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            max_deliveries: Some(1),
            ..fifo("f")
        })
        .await
        .unwrap();
    let (a, _) = fifo_publish(&h, "f", "g", Some("a"), "1");
    let (b, _) = fifo_publish(&h, "f", "g", Some("b"), "2");
    let (c, _) = fifo_publish(&h, "f", "g", Some("c"), "3");
    let first = h
        .messages
        .consume("f", q(Some("1"), Some("5"), None))
        .await
        .unwrap();
    assert_eq!(first[0].id, a);
    h.clock.advance(Duration::from_secs(5));

    // The head used up the single delivery: it goes to the dead-letter and the group goes on with the next.
    let next = h
        .messages
        .consume("f", q(Some("1"), Some("600"), None))
        .await
        .unwrap();
    assert_eq!(next[0].id, b);
    ack(&h, "f", &next[0]);
    assert_eq!(
        fifo_get(&h, "f", "1").await[0].id,
        c,
        "the order of the rest is kept"
    );

    // The dead-letter is not FIFO: it accepts a publication without a group and delivers without serializing.
    let dead = consume(&h, "f-dead-letter", q(Some("10"), Some("600"), None)).await;
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].id, a);
    assert_eq!(dead[0].deduplication_id, None);
    h.messages.publish("f-dead-letter", json_body("9")).unwrap();
}

/// UC: UC-FIF-021
#[tokio::test]
async fn a_message_that_went_to_the_dead_letter_still_deduplicates_in_the_window() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            max_deliveries: Some(1),
            ..fifo("f")
        })
        .await
        .unwrap();
    let (a, _) = fifo_publish(&h, "f", "g", Some("a"), "1");
    h.messages
        .consume("f", q(Some("1"), Some("5"), None))
        .await
        .unwrap();
    h.clock.advance(Duration::from_secs(5));
    h.messages.sweep();
    assert!(
        h.store.get("f-dead-letter", &a).unwrap().is_some()
            || h.messages.usage("f-dead-letter").0 > 0
    );
    let (again, duplicate) = fifo_publish(&h, "f", "g", Some("a"), "1");
    assert_eq!((again.as_str(), duplicate), (a.as_str(), Some(true)));
}

/// UC: UC-FIF-022
#[tokio::test]
async fn a_head_past_its_ttl_is_dropped_and_the_group_is_freed() {
    let h = harness();
    h.topics
        .create(CreateTopic {
            ttl: Some(60),
            ..fifo("f")
        })
        .await
        .unwrap();
    let (a, _) = fifo_publish(&h, "f", "g", Some("a"), "1");
    let first = h
        .messages
        .consume("f", q(Some("1"), Some("600"), None))
        .await
        .unwrap();
    assert_eq!(first[0].id, a);
    h.clock.advance(Duration::from_secs(30));
    let (b, _) = fifo_publish(&h, "f", "g", Some("b"), "2");
    assert!(
        fifo_get(&h, "f", "1").await.is_empty(),
        "the head stays in flight"
    );
    h.clock.advance(Duration::from_secs(31));
    assert_eq!(
        fifo_get(&h, "f", "1").await[0].id,
        b,
        "the head expired and the group went on"
    );
}

/// UC: UC-FIF-046
#[tokio::test]
async fn groups_busy_state_and_deduplication_survive_a_restart() {
    let h = harness();
    h.topics.create(fifo("f")).await.unwrap();
    let (a, _) = fifo_publish(&h, "f", "g1", Some("a"), "1");
    fifo_publish(&h, "f", "g1", Some("b"), "2");
    let (c, _) = fifo_publish(&h, "f", "g2", Some("c"), "3");
    let in_flight = fifo_get(&h, "f", "1").await;
    assert_eq!(in_flight[0].id, a);
    let done = fifo_get(&h, "f", "1").await;
    assert_eq!(done[0].id, c);
    ack(&h, "f", &done[0]);
    h.messages.drain();

    let restarted = reopen(&h, true);
    // Group g1 stays busy with the in-flight message (the same `receiptToken` still applies).
    assert!(
        restarted
            .consume("f", q(Some("10"), Some("600"), None))
            .await
            .unwrap()
            .is_empty()
    );
    // And the deduplication of what had already been acknowledged survives the resume.
    let request = PublishRequest {
        body: b"3".to_vec(),
        group_id: Some("g2".into()),
        deduplication_id: Some("c".into()),
        ..Default::default()
    };
    let Published::Queue { id, duplicate } = restarted.publish("f", request).unwrap() else {
        panic!("expected a queue");
    };
    assert_eq!((id.as_str(), duplicate), (c.as_str(), Some(true)));
    restarted
        .ack("f", &a, Some(&in_flight[0].receipt_token))
        .unwrap();
    let next = restarted
        .consume("f", q(Some("10"), Some("600"), None))
        .await
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].deduplication_id.as_deref(), Some("b"));
}

/// UC: UC-FIF-043
#[tokio::test]
async fn a_distributor_copy_without_a_group_fails_only_that_fifo_queue() {
    let h = harness();
    h.topics.create(fifo("order-a")).await.unwrap();
    h.topics.create(queue("order-b")).await.unwrap();
    h.topics
        .create(distributor("dist", &["order-"]))
        .await
        .unwrap();
    let Published::Distributor { deliveries } = h.messages.publish("dist", json_body("1")).unwrap()
    else {
        panic!("expected a distributor");
    };
    let by_topic: std::collections::HashMap<_, _> = deliveries
        .into_iter()
        .map(|d| (d.topic, d.result))
        .collect();
    assert_eq!(
        by_topic["order-a"].as_ref().unwrap_err().code,
        ErrorCode::ValidationGroupIdRequired
    );
    assert!(by_topic["order-b"].is_ok());

    // With the group, the copy goes to all, and each FIFO deduplicates its own.
    let request = PublishRequest {
        body: b"2".to_vec(),
        group_id: Some("g".into()),
        deduplication_id: Some("k".into()),
        ..Default::default()
    };
    let Published::Distributor { deliveries } =
        h.messages.publish("dist", request.clone()).unwrap()
    else {
        panic!("expected a distributor");
    };
    assert!(deliveries.iter().all(|d| d.result.is_ok()));
    h.messages.publish("dist", request).unwrap();
    assert_eq!(
        fifo_get(&h, "order-a", "10").await.len(),
        1,
        "the second was a duplicate"
    );
}

mod stats_events;

mod backup;
mod blocks;
mod cluster;

/// UC: UC-VOL-013, UC-VOL-020
#[tokio::test]
async fn a_get_after_the_final_deletion_is_not_found_or_recreates_an_empty_queue_with_a_new_id() {
    let h = harness();
    h.topics.create(volatile_queue("queue", 60)).await.unwrap();
    publish_ok(&h, "queue", "1");
    let old = h.topics.volatile_queues().remove(0);

    // The manager's deletion (with the `id` it saw) applies once; the second attempt, from whoever
    // took over the queue in the middle of the manager switch, is rejected without effect (UC-VOL-020).
    h.topics.delete_expected("queue", &old.id).await.unwrap();
    let error = h
        .topics
        .delete_expected("queue", &old.id)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TopicNotFound);

    // A `GET` that arrives after the deletion receives `404`...
    let error = h
        .messages
        .consume("queue", q(None, None, Some("0")))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TopicNotFound);

    // ...or, with `autoCreate`, recreates the queue with a new `id` and without the old messages.
    let got = h
        .messages
        .consume("queue", auto(&h, "queue", |_| {}))
        .await
        .unwrap();
    assert!(got.is_empty(), "the old messages are gone");
    let recreated = h.topics.volatile_queues().remove(0);
    assert_ne!(recreated.id, old.id);
}
