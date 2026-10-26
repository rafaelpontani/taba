//! What a distributor does with a publication: which queues get a copy, what each copy carries,
//! and what a distributor never holds.

use super::*;
use crate::engine::topics::UpdateTopic;

/// Where each copy of a publication went, in no particular order (there is none to rely on).
fn targets(published: Published) -> Vec<String> {
    let Published::Distributor { deliveries } = published else {
        panic!("expected a distributor");
    };
    let mut names: Vec<String> = deliveries
        .into_iter()
        .map(|d| {
            assert!(d.result.is_ok(), "{}: {:?}", d.topic, d.result);
            d.topic
        })
        .collect();
    names.sort();
    names
}

async fn waiting(h: &Harness, topic: &str) -> usize {
    consume(h, topic, q(Some("10"), Some("600"), None))
        .await
        .len()
}

fn with_group(body: &str, group: &str, dedup: Option<&str>) -> PublishRequest {
    PublishRequest {
        body: body.as_bytes().to_vec(),
        group_id: Some(group.into()),
        deduplication_id: dedup.map(Into::into),
        ..Default::default()
    }
}

/// UC: UC-DST-004, UC-DST-015
#[tokio::test]
async fn a_queue_matching_two_prefixes_of_one_distributor_gets_one_copy() {
    let h = harness();
    h.topics.create(queue("orders-eu")).await.unwrap();
    h.topics
        .create(distributor("all", &["orders-", "orders-e", "ord"]))
        .await
        .unwrap();
    let copies = targets(h.messages.publish("all", json_body("1")).unwrap());
    assert_eq!(copies, ["orders-eu"]);
    assert_eq!(waiting(&h, "orders-eu").await, 1);
}

/// UC: UC-DST-005, UC-DST-006, UC-DST-015
#[tokio::test]
async fn distributors_with_the_same_prefix_each_send_a_copy_and_never_to_a_distributor() {
    let h = harness();
    h.topics.create(queue("a-1")).await.unwrap();
    h.topics.create(distributor("a-d1", &["a-"])).await.unwrap();
    h.topics.create(distributor("d2", &["a-"])).await.unwrap();

    // `a-d1` matches the prefix of `d2` by its name, but only queues are targets.
    assert_eq!(
        targets(h.messages.publish("a-d1", json_body("1")).unwrap()),
        ["a-1"]
    );
    assert_eq!(
        targets(h.messages.publish("d2", json_body("2")).unwrap()),
        ["a-1"]
    );
    assert_eq!(
        waiting(&h, "a-1").await,
        2,
        "one copy from each distributor"
    );
    assert!(h.store.count("a-d1").unwrap() == 0 && h.store.count("d2").unwrap() == 0);
}

/// UC: UC-DST-007, UC-DST-013
#[tokio::test]
async fn the_match_is_made_when_publishing_and_a_new_prefix_counts_from_then_on() {
    let h = harness();
    h.topics.create(distributor("all", &["a-"])).await.unwrap();
    assert!(targets(h.messages.publish("all", json_body("0")).unwrap()).is_empty());

    h.topics.create(queue("a-late")).await.unwrap();
    assert_eq!(
        waiting(&h, "a-late").await,
        0,
        "what came before is not copied"
    );
    assert_eq!(
        targets(h.messages.publish("all", json_body("1")).unwrap()),
        ["a-late"]
    );

    h.topics.create(queue("b-1")).await.unwrap();
    let patch = UpdateTopic {
        prefixes: Some(vec!["b-".into()]),
        ..Default::default()
    };
    h.topics.update("all", patch).await.unwrap();
    assert_eq!(
        targets(h.messages.publish("all", json_body("2")).unwrap()),
        ["b-1"],
        "the new prefixes hold for the next publication"
    );
    assert_eq!(
        waiting(&h, "a-late").await,
        1,
        "and the old ones no longer do"
    );
}

/// UC: UC-DST-010, UC-GRP-014
#[tokio::test]
async fn the_group_and_the_deduplication_key_go_to_every_copy() {
    let h = harness();
    h.topics.create(fifo("f-1")).await.unwrap();
    h.topics.create(fifo("f-2")).await.unwrap();
    h.topics.create(distributor("all", &["f-"])).await.unwrap();

    // With a key, a second body with the same key is a duplicate in each queue.
    h.messages
        .publish("all", with_group("\"x\"", "g", Some("k")))
        .unwrap();
    h.messages
        .publish("all", with_group("\"y\"", "g", Some("k")))
        .unwrap();
    // Without one, the key of each copy is the hash of its body.
    h.messages
        .publish("all", with_group("\"z\"", "g", None))
        .unwrap();
    h.messages
        .publish("all", with_group("\"z\"", "g", None))
        .unwrap();
    for name in ["f-1", "f-2"] {
        let got = fifo_get(&h, name, "10").await;
        assert_eq!(got.len(), 1, "{name}: the group is one at a time");
        assert_eq!(got[0].group_id.as_deref(), Some("g"), "{name}");
        ack(&h, name, &got[0]);
        let next = fifo_get(&h, name, "10").await;
        assert_eq!(next.len(), 1, "{name}: the `z` copy, once");
        ack(&h, name, &next[0]);
        assert!(fifo_get(&h, name, "10").await.is_empty(), "{name}");
    }
}

/// UC: UC-DST-012
#[tokio::test]
async fn a_distributor_checks_the_general_size_and_not_that_of_each_queue() {
    let h = harness_with(&[("TABA_MESSAGE_MAX_BYTES", "20")]);
    let mut small = queue("a-small");
    small.max_message_bytes = Some(5);
    h.topics.create(small).await.unwrap();
    h.topics.create(distributor("all", &["a-"])).await.unwrap();

    let body = "\"0123456789\"";
    assert!(body.len() > 5 && body.len() <= 20);
    assert_eq!(
        publish_err(&h, "a-small", json_body(body)),
        ErrorCode::MessageTooLarge
    );
    assert_eq!(
        targets(h.messages.publish("all", json_body(body)).unwrap()),
        ["a-small"],
        "the copy goes in although the queue would have refused it"
    );
    assert_eq!(
        publish_err(&h, "all", json_body("\"0123456789012345678\"")),
        ErrorCode::MessageTooLarge
    );
}

/// UC: UC-DST-014, UC-DST-016
#[tokio::test]
async fn a_distributor_keeps_no_message_and_has_no_ack() {
    let h = harness();
    h.topics.create(queue("a-1")).await.unwrap();
    h.topics.create(distributor("all", &["a-"])).await.unwrap();
    h.messages.publish("all", json_body("1")).unwrap();

    assert_eq!(h.store.count("all").unwrap(), 0, "nothing on disk");
    assert!(h.messages.states.read().unwrap().get("all").is_none());
    let err = h.messages.ack("all", "any", Some("token")).unwrap_err();
    assert_eq!(err.code, ErrorCode::TopicWrongType);
    assert_eq!(
        consume_err(&h, "all", ConsumeQuery::default()).await,
        ErrorCode::TopicWrongType
    );
}
