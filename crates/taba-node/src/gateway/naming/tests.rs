use super::*;

fn standard(topic: &str) -> Queue {
    Queue {
        topic: topic.to_owned(),
        fifo: false,
    }
}

/// UC: UC-GTW-008
#[test]
fn a_valid_sqs_name_is_the_topic_name() {
    let queue = parse("orders-eu_1").expect("valid");
    assert_eq!(queue, standard("orders-eu_1"));
    assert_eq!(queue.name(), "orders-eu_1");
}

/// UC: UC-GTW-008
#[test]
fn names_sqs_refuses_are_refused() {
    for bad in [
        "",
        "has space",
        "dot.name",
        "a/b",
        "ação",
        ".fifo",
        "a.fifo.fifo",
    ] {
        assert_eq!(
            parse(bad).unwrap_err().json_code,
            "InvalidParameterValue",
            "{bad}"
        );
    }
    assert!(parse(&"a".repeat(81)).is_err());
    assert!(parse(&"a".repeat(80)).is_ok());
    assert!(parse(&format!("{}.fifo", "a".repeat(76))).is_err());
    assert!(parse(&format!("{}.fifo", "a".repeat(75))).is_ok());
}

/// UC: UC-GTW-008
#[test]
fn the_dead_letter_names_are_refused_and_a_fifo_name_is_the_topic_without_the_suffix() {
    assert_eq!(
        parse("orders-dead-letter").unwrap_err().json_code,
        "InvalidParameterValue"
    );
    assert_eq!(
        parse("orders-dead-letter.fifo").unwrap_err().json_code,
        "InvalidParameterValue"
    );
    let fifo = parse("orders.fifo").expect("fifo");
    assert_eq!(
        fifo,
        Queue {
            topic: "orders".into(),
            fifo: true
        }
    );
    assert_eq!(fifo.name(), "orders.fifo");
    assert_eq!(fifo.dead_letter_topic(), "orders-dead-letter");
    assert_eq!(Queue::of_topic("orders", true), Some(fifo));
    assert_eq!(Queue::of_topic("orders-dead-letter", false), None);
    assert_eq!(Queue::of_topic(&"a".repeat(78), true), None);
}

/// UC: UC-GTW-008
#[test]
fn the_url_and_the_arn_carry_the_name_and_come_back() {
    let queue = standard("orders");
    let url = queue.url("http://gw:9324/");
    assert_eq!(url, "http://gw:9324/000000000000/orders");
    assert_eq!(name_in_url(&url).expect("name"), "orders");
    assert_eq!(
        queue.arn("eu-west-1"),
        "arn:aws:sqs:eu-west-1:000000000000:orders"
    );
    let fifo = parse("orders.fifo").expect("fifo");
    assert_eq!(
        name_in_url(&fifo.url("http://gw:9324")).expect("name"),
        "orders.fifo"
    );
    assert_eq!(
        name_in_arn(&fifo.arn("eu-west-1")),
        Some("orders.fifo"),
        "the name comes back out of the ARN"
    );
    assert_eq!(name_in_arn("not-an-arn"), None);
    assert_eq!(
        name_in_arn("arn:aws:sns:us-east-1:000000000000:orders"),
        None
    );
}

/// UC: UC-GTW-008
#[test]
fn a_url_without_an_account_and_a_name_is_a_queue_that_does_not_exist() {
    for bad in [
        "http://gw:9324",
        "http://gw:9324/",
        "http://gw:9324/orders",
        "http://gw:9324/000000000000/orders/extra",
        "",
    ] {
        assert_eq!(
            name_in_url(bad).unwrap_err().json_code,
            "QueueDoesNotExist",
            "{bad}"
        );
    }
    assert_eq!(
        name_in_url("/000000000000/orders").expect("path only"),
        "orders"
    );
}

/// UC: UC-GTW-026
#[test]
fn a_topic_name_is_a_distributor_name_and_the_fifo_and_dead_letter_names_are_refused() {
    assert_eq!(parse_topic("orders-eu_1").expect("valid"), "orders-eu_1");
    for bad in [
        "",
        "has space",
        "dot.name",
        "orders.fifo",
        "orders-dead-letter",
    ] {
        assert_eq!(
            parse_topic(bad).unwrap_err().query_code,
            "InvalidParameter",
            "{bad}"
        );
    }
    assert!(parse_topic(&"a".repeat(100)).is_ok());
    assert!(parse_topic(&"a".repeat(101)).is_err());
    assert!(
        parse_topic("orders.fifo")
            .unwrap_err()
            .message
            .contains("FIFO")
    );
}

/// UC: UC-GTW-026
#[test]
fn the_topic_arn_and_the_subscription_arn_carry_their_names_and_come_back() {
    let arn = topic_arn("eu-west-1", "orders");
    assert_eq!(arn, "arn:aws:sns:eu-west-1:000000000000:orders");
    assert_eq!(topic_in_arn(&arn), Some("orders"));
    assert_eq!(
        topic_in_arn("arn:aws:sqs:eu-west-1:000000000000:orders"),
        None
    );
    assert_eq!(topic_in_arn("orders"), None);
    assert_eq!(
        topic_in_arn("arn:aws:sns:eu-west-1:000000000000:orders:sub"),
        None,
        "a subscription ARN is not a topic ARN"
    );

    let sub = subscription_arn("eu-west-1", "orders", "billing.fifo");
    assert_eq!(
        sub,
        "arn:aws:sns:eu-west-1:000000000000:orders:billing.fifo"
    );
    assert_eq!(
        parse_subscription_arn(&sub),
        Some(("orders", "billing.fifo"))
    );
    assert_eq!(parse_subscription_arn(&arn), None);
    assert_eq!(parse_subscription_arn("garbage"), None);
}

/// UC: UC-GTW-020
#[test]
fn a_group_id_taba_accepts_goes_as_it_is_and_another_becomes_a_hash_that_keeps_the_original() {
    assert_eq!(
        taba_id("tenant-42:blue"),
        ("tenant-42:blue".to_owned(), None)
    );
    let (hashed, original) = taba_id("tenant/42!blue");
    assert!(hashed.starts_with("h.") && hashed.len() == 42, "{hashed}");
    assert!(is_taba_id(&hashed));
    assert_eq!(original.as_deref(), Some("tenant/42!blue"));
    assert_eq!(
        taba_id("tenant/42!blue").0,
        hashed,
        "the same text, the same id"
    );
    assert_ne!(taba_id("tenant/43!blue").0, hashed);
}

/// UC: UC-GTW-020
#[test]
fn an_sqs_id_has_up_to_128_letters_digits_and_punctuation() {
    assert!(is_aws_id("a!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"));
    assert!(!is_aws_id(""));
    assert!(!is_aws_id("has space"));
    assert!(!is_aws_id("ação"));
    assert!(!is_aws_id(&"a".repeat(129)));
    assert!(is_aws_id(&"a".repeat(128)));
}
