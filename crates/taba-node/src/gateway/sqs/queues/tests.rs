use serde_json::json;

use super::*;

fn attributes(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// UC: UC-GTW-015
#[test]
fn the_attributes_that_have_a_field_in_taba_use_it_and_the_others_go_to_tags() {
    let changes = parse_changes(&attributes(&[
        ("VisibilityTimeout", "45"),
        ("DelaySeconds", "5"),
        ("ReceiveMessageWaitTimeSeconds", "20"),
        ("MessageRetentionPeriod", "86400"),
        ("MaximumMessageSize", "2048"),
        (
            "RedrivePolicy",
            r#"{"deadLetterTargetArn":"arn:aws:sqs:us-east-1:000000000000:dlq","maxReceiveCount":"5"}"#,
        ),
    ]))
    .unwrap();
    assert_eq!(changes.tags["sqs.vt"], "45");
    assert_eq!(changes.tags["sqs.delay"], "5");
    assert_eq!(changes.tags["sqs.wait"], "20");
    assert_eq!(changes.ttl, Some(86_400));
    assert_eq!(changes.max_bytes, Some(2048));
    assert_eq!(changes.max_deliveries, Some(5));
}

/// UC: UC-GTW-015
#[test]
fn values_out_of_range_and_unknown_names_are_refused() {
    let code = |pairs: &[(&str, &str)]| parse_changes(&attributes(pairs)).unwrap_err().json_code;
    assert_eq!(
        code(&[("VisibilityTimeout", "43201")]),
        "InvalidAttributeValue"
    );
    assert_eq!(code(&[("DelaySeconds", "901")]), "InvalidAttributeValue");
    assert_eq!(code(&[("DelaySeconds", "abc")]), "InvalidAttributeValue");
    assert_eq!(
        code(&[("MessageRetentionPeriod", "59")]),
        "InvalidAttributeValue"
    );
    assert_eq!(
        code(&[("MaximumMessageSize", "1023")]),
        "InvalidAttributeValue"
    );
    assert_eq!(
        code(&[("RedrivePolicy", "not json")]),
        "InvalidAttributeValue"
    );
    assert_eq!(
        code(&[("RedrivePolicy", r#"{"maxReceiveCount":0}"#)]),
        "InvalidAttributeValue"
    );
    assert_eq!(code(&[("Frobnicate", "1")]), "InvalidAttributeName");
}

/// UC: UC-GTW-015
#[test]
fn the_queue_policy_that_sns_needs_is_accepted_and_has_no_effect() {
    let changes = parse_changes(&attributes(&[
        ("Policy", r#"{"Version":"2012-10-17","Statement":[]}"#),
        ("SqsManagedSseEnabled", "true"),
    ]))
    .unwrap();
    assert_eq!(changes, Changes::default());
}

/// UC: UC-GTW-021
#[test]
fn the_fifo_attributes_are_read_and_the_content_based_deduplication_is_a_tag() {
    let changes = parse_changes(&attributes(&[
        ("FifoQueue", "true"),
        ("ContentBasedDeduplication", "true"),
        ("DeduplicationScope", "messageGroup"),
        ("FifoThroughputLimit", "perMessageGroupId"),
    ]))
    .unwrap();
    assert_eq!(changes.fifo, Some(true));
    assert_eq!(changes.content_dedup, Some(true));
    assert_eq!(
        changes.tags.get(TAG_CONTENT_DEDUP).map(String::as_str),
        Some("true")
    );
    for bad in [
        ("FifoQueue", "yes"),
        ("ContentBasedDeduplication", "1"),
        ("DeduplicationScope", "everything"),
        ("FifoThroughputLimit", "fast"),
    ] {
        assert_eq!(
            parse_changes(&attributes(&[bad])).unwrap_err().json_code,
            "InvalidAttributeValue",
            "{bad:?}"
        );
    }
}

/// UC: UC-GTW-021
#[test]
fn the_name_and_the_attributes_must_agree_on_the_kind_of_queue() {
    let fifo = naming::parse("orders.fifo").unwrap();
    let standard = naming::parse("orders").unwrap();
    let with = |pairs: &[(&str, &str)]| parse_changes(&attributes(pairs)).unwrap();
    assert!(check_kind(&fifo, &with(&[("FifoQueue", "true")])).is_ok());
    assert!(check_kind(&standard, &with(&[])).is_ok());
    assert!(check_kind(&standard, &with(&[("FifoQueue", "false")])).is_ok());
    assert_eq!(
        check_kind(&fifo, &with(&[])).unwrap_err().json_code,
        "InvalidParameterValue",
        "a .fifo name needs FifoQueue"
    );
    assert_eq!(
        check_kind(&standard, &with(&[("FifoQueue", "true")]))
            .unwrap_err()
            .json_code,
        "InvalidParameterValue",
        "FifoQueue needs a .fifo name"
    );
    assert_eq!(
        check_kind(&standard, &with(&[("ContentBasedDeduplication", "true")]))
            .unwrap_err()
            .json_code,
        "InvalidAttributeName"
    );
}

/// UC: UC-GTW-021
#[test]
fn a_fifo_queue_cannot_have_a_delay() {
    let fifo = naming::parse("orders.fifo").unwrap();
    let tags = |delay: &str| BTreeMap::from([(TAG_DELAY.to_owned(), delay.to_owned())]);
    assert!(check_fifo_delay(&fifo, &tags("0")).is_ok());
    assert!(check_fifo_delay(&fifo, &BTreeMap::new()).is_ok());
    assert_eq!(
        check_fifo_delay(&fifo, &tags("5")).unwrap_err().json_code,
        "UnsupportedOperation"
    );
    assert!(check_fifo_delay(&naming::parse("orders").unwrap(), &tags("5")).is_ok());
}

/// UC: UC-GTW-023
#[test]
fn the_redrive_policy_names_the_dead_letter_queue_and_the_count() {
    let policy = r#"{"deadLetterTargetArn":"arn:aws:sqs:us-east-1:000000000000:orders-dlq.fifo","maxReceiveCount":"3"}"#;
    let changes = parse_changes(&attributes(&[("RedrivePolicy", policy)])).unwrap();
    assert_eq!(changes.max_deliveries, Some(3));
    assert_eq!(changes.dead_letter.as_deref(), Some("orders-dlq.fifo"));
    let no_target = parse_changes(&attributes(&[(
        "RedrivePolicy",
        r#"{"maxReceiveCount":5}"#,
    )]))
    .unwrap();
    assert_eq!(no_target.dead_letter, None);
    assert_eq!(
        parse_changes(&attributes(&[(
            "RedrivePolicy",
            r#"{"deadLetterTargetArn":"not-an-arn","maxReceiveCount":5}"#
        )]))
        .unwrap_err()
        .json_code,
        "InvalidAttributeValue"
    );
}

/// UC: UC-GTW-023
#[test]
fn a_queue_knows_its_dead_letter_queue_and_the_source_it_is_the_dead_letter_of() {
    let source = Settings::of(&json!({
        "name": "orders", "type": "queue", "fifo": true,
        "tags": {"sqs.cbd": "true", "sqs.dlq": "orders-dlq.fifo"}
    }));
    assert!(source.fifo && source.content_dedup);
    assert_eq!(source.dead_letter.as_deref(), Some("orders-dlq.fifo"));
    assert_eq!(source.dead_letter_of, None);
    assert!(
        source.user_tags().is_empty(),
        "the gateway tags stay hidden"
    );
    let dead = Settings::of(&json!({
        "name": "orders-dlq", "type": "queue", "fifo": true,
        "tags": {"sqs.dlqof": "orders"}
    }));
    assert_eq!(dead.dead_letter_of.as_deref(), Some("orders"));
    assert!(!dead.content_dedup);
    assert!(!Settings::of(&json!({"name": "a", "type": "queue"})).fifo);
}

/// UC: UC-GTW-015
#[test]
fn a_queue_says_its_defaults_and_the_gateway_tags_stay_hidden() {
    let topic = json!({
        "name": "orders", "type": "queue", "createdAt": "2026-11-04T09:00:00.000Z",
        "tags": {"sqs.vt": "45", "sqs.delay": "3", "team": "billing"},
        "ttl": 86400, "maxDeliveries": 7,
        "effective": {"maxMessageBytes": 5_242_880, "ttl": 1_209_600, "maxDeliveries": 10}
    });
    let settings = Settings::of(&topic);
    assert_eq!(settings.visibility, 45);
    assert_eq!(settings.delay, 3);
    assert_eq!(settings.wait, 0);
    assert_eq!(settings.retention, 86_400);
    assert_eq!(
        settings.max_size, MAX_MESSAGE_BYTES,
        "capped at what SQS takes"
    );
    assert_eq!(settings.max_deliveries, Some(7));
    assert!(settings.created_ms > 0);
    assert_eq!(
        settings.user_tags().into_iter().collect::<Vec<_>>(),
        vec![("team".to_owned(), "billing".to_owned())]
    );
}

/// UC: UC-GTW-015
#[test]
fn a_queue_without_settings_has_the_defaults_of_sqs() {
    let settings = Settings::of(&json!({"name": "q", "type": "queue", "tags": {},
        "effective": {"maxMessageBytes": 5_242_880, "ttl": 1_209_600, "maxDeliveries": 10}}));
    assert_eq!(settings.visibility, 30);
    assert_eq!(settings.delay, 0);
    assert_eq!(settings.retention, 1_209_600);
    assert_eq!(settings.max_deliveries, None);
}

/// UC: UC-GTW-015
#[test]
fn tags_the_application_chooses_must_fit_a_taba_tag() {
    let tag = |k: &str, v: &str| check_tags(&[(k.to_owned(), v.to_owned())]);
    assert!(tag("team", "billing").is_ok());
    assert!(tag("env:prod", "").is_ok());
    assert!(tag("has space", "v").is_err());
    assert!(tag("k", "has space").is_err());
    assert!(
        tag("sqs.vt", "1").is_err(),
        "the prefix of the gateway is reserved"
    );
    assert!(tag(&"k".repeat(65), "v").is_err());
    assert!(tag("k", &"v".repeat(129)).is_err());
}

/// UC: UC-GTW-015
#[test]
fn the_queue_comes_from_the_url() {
    let queue = queue_of(&json!({"QueueUrl": "http://gw:9324/000000000000/orders"})).unwrap();
    assert_eq!((queue.topic.as_str(), queue.fifo), ("orders", false));
    let fifo = queue_of(&json!({"QueueUrl": "http://gw:9324/000000000000/orders.fifo"})).unwrap();
    assert_eq!((fifo.topic.as_str(), fifo.fifo), ("orders", true));
    assert_eq!(
        queue_of(&json!({})).unwrap_err().json_code,
        "MissingParameter"
    );
    assert_eq!(
        queue_of(&json!({"QueueUrl": "http://gw:9324/nothing"}))
            .unwrap_err()
            .json_code,
        "QueueDoesNotExist"
    );
}

/// UC: UC-GTW-015
#[test]
fn the_list_token_comes_back_to_where_the_page_stopped() {
    let token = encode_token(3, Some("cursor/with+chars"));
    assert_eq!(
        decode_token(&token).unwrap(),
        (3, Some("cursor/with+chars".to_owned()))
    );
    assert_eq!(decode_token(&encode_token(0, None)).unwrap(), (0, None));
    assert!(decode_token("%%%").is_err());
    assert!(decode_token("bm8gYmFy").is_err());
}
