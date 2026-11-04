use axum::http::StatusCode;

use super::super::tests::{Fake, TOPIC, ask, element};

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:billing";
const SUB: &str = "arn:aws:sns:us-east-1:000000000000:orders:billing";

fn code(xml: &str) -> &str {
    element(xml, "Code").unwrap_or("-")
}

async fn topic_and_queue() -> Fake {
    let fake = Fake::default();
    fake.queue("billing", false);
    ask(&fake, "Action=CreateTopic&Name=orders").await;
    fake
}

fn subscribe(attributes: &str) -> String {
    format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint={QUEUE}{attributes}")
}

/// UC: UC-GTW-027
#[tokio::test]
async fn subscribing_a_queue_adds_its_name_as_a_prefix_and_the_arn_says_topic_and_queue() {
    let fake = topic_and_queue().await;
    let (status, xml) = ask(&fake, &subscribe("")).await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert_eq!(element(&xml, "SubscriptionArn"), Some(SUB), "{xml}");
    assert_eq!(
        fake.topic("orders").unwrap()["prefixes"],
        serde_json::json!(["billing"])
    );

    // Subscribing again is the same subscription.
    let (status, again) = ask(&fake, &subscribe("")).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(element(&again, "SubscriptionArn"), Some(SUB));
    assert_eq!(
        fake.topic("orders").unwrap()["prefixes"],
        serde_json::json!(["billing"])
    );
}

/// UC: UC-GTW-027
#[tokio::test]
async fn a_fifo_queue_is_subscribed_by_its_topic_name_and_listed_with_its_fifo_name() {
    let fake = Fake::default();
    fake.queue("jobs", true);
    ask(&fake, "Action=CreateTopic&Name=orders").await;
    let (status, xml) = ask(
        &fake,
        &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:jobs.fifo"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert_eq!(
        element(&xml, "SubscriptionArn"),
        Some("arn:aws:sns:us-east-1:000000000000:orders:jobs.fifo")
    );
    assert_eq!(
        fake.topic("orders").unwrap()["prefixes"],
        serde_json::json!(["jobs"])
    );
    let (_, listed) = ask(
        &fake,
        &format!("Action=ListSubscriptionsByTopic&TopicArn={TOPIC}"),
    )
    .await;
    assert_eq!(
        element(&listed, "Endpoint"),
        Some("arn:aws:sqs:us-east-1:000000000000:jobs.fifo"),
        "{listed}"
    );

    // The name without `.fifo` is another queue, which does not exist.
    let (status, xml) = ask(
        &fake,
        &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:jobs"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{xml}");
}

/// UC: UC-GTW-027
#[tokio::test]
async fn what_the_distributor_cannot_do_is_refused_with_a_clear_message() {
    let fake = topic_and_queue().await;
    let refused = |form: String, needle: &'static str| {
        let fake = fake.clone();
        async move {
            let (status, xml) = ask(&fake, &form).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{form}: {xml}");
            assert_eq!(code(&xml), "InvalidParameter", "{form}");
            assert!(xml.contains(needle), "{form}: {xml}");
        }
    };
    refused(
        subscribe("&Attributes.entry.1.key=RawMessageDelivery&Attributes.entry.1.value=true"),
        "RawMessageDelivery=true is not supported",
    )
    .await;
    refused(
        subscribe("&Attributes.entry.1.key=FilterPolicy&Attributes.entry.1.value=%7B%7D"),
        "FilterPolicy is not supported",
    )
    .await;
    refused(
        subscribe("&Attributes.entry.1.key=RedrivePolicy&Attributes.entry.1.value=%7B%7D"),
        "RedrivePolicy",
    )
    .await;
    for protocol in ["https", "email", "lambda", "sms"] {
        refused(
            format!("Action=Subscribe&TopicArn={TOPIC}&Protocol={protocol}&Endpoint=x"),
            "copies the messages of a topic to queues only",
        )
        .await;
    }
    refused(
        format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=not-an-arn"),
        "Endpoint",
    )
    .await;
    refused(
        format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:ghost"),
        "does not exist",
    )
    .await;
    assert_eq!(
        fake.topic("orders").unwrap()["prefixes"],
        serde_json::json!([]),
        "a refused subscription changes nothing"
    );
    // `RawMessageDelivery=false` is how the delivery always is.
    let (status, xml) = ask(
        &fake,
        &subscribe("&Attributes.entry.1.key=RawMessageDelivery&Attributes.entry.1.value=false"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
}

/// UC: UC-GTW-027
#[tokio::test]
async fn a_topic_that_does_not_exist_or_a_dead_letter_queue_cannot_be_subscribed() {
    let fake = topic_and_queue().await;
    fake.queue("billing-dead-letter", false);
    let (status, xml) = ask(
        &fake,
        &format!("Action=Subscribe&TopicArn=arn:aws:sns:us-east-1:000000000000:ghost&Protocol=sqs&Endpoint={QUEUE}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{xml}");
    assert_eq!(code(&xml), "NotFound");
    let (status, xml) = ask(
        &fake,
        &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:billing-dead-letter"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{xml}");
}

/// UC: UC-GTW-027
#[tokio::test]
async fn the_twenty_first_subscription_is_refused_and_the_first_twenty_stay() {
    let fake = Fake::default();
    ask(&fake, "Action=CreateTopic&Name=orders").await;
    for n in 0..21 {
        fake.queue(&format!("q{n:02}"), false);
    }
    for n in 0..20 {
        let (status, xml) = ask(
            &fake,
            &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:q{n:02}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{n}: {xml}");
    }
    let (status, xml) = ask(
        &fake,
        &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:q20"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{xml}");
    assert_eq!(code(&xml), "SubscriptionLimitExceeded");
    assert_eq!(
        fake.topic("orders").unwrap()["prefixes"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
}

/// UC: UC-GTW-027
#[tokio::test]
async fn unsubscribing_removes_the_prefix_and_doing_it_twice_is_not_an_error() {
    let fake = topic_and_queue().await;
    fake.queue("shipping", false);
    ask(&fake, &subscribe("")).await;
    ask(
        &fake,
        &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:shipping"),
    )
    .await;

    let (status, xml) = ask(&fake, &format!("Action=Unsubscribe&SubscriptionArn={SUB}")).await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert_eq!(
        fake.topic("orders").unwrap()["prefixes"],
        serde_json::json!(["shipping"])
    );
    let (status, _) = ask(&fake, &format!("Action=Unsubscribe&SubscriptionArn={SUB}")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, xml) = ask(&fake, "Action=Unsubscribe&SubscriptionArn=garbage").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{xml}");
    let (status, xml) = ask(
        &fake,
        "Action=Unsubscribe&SubscriptionArn=arn:aws:sns:us-east-1:000000000000:ghost:billing",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{xml}");
}

/// UC: UC-GTW-027
#[tokio::test]
async fn the_subscriptions_of_a_topic_and_of_all_the_topics_are_listed() {
    let fake = topic_and_queue().await;
    fake.queue("shipping", false);
    ask(&fake, "Action=CreateTopic&Name=alerts").await;
    ask(&fake, &subscribe("")).await;
    ask(
        &fake,
        "Action=Subscribe&TopicArn=arn:aws:sns:us-east-1:000000000000:alerts&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:shipping",
    )
    .await;

    let (_, by_topic) = ask(
        &fake,
        &format!("Action=ListSubscriptionsByTopic&TopicArn={TOPIC}"),
    )
    .await;
    assert!(
        by_topic.contains(&format!("<SubscriptionArn>{SUB}</SubscriptionArn>")),
        "{by_topic}"
    );
    assert!(by_topic.contains("<Protocol>sqs</Protocol>"), "{by_topic}");
    assert!(
        by_topic.contains(&format!("<Endpoint>{QUEUE}</Endpoint>")),
        "{by_topic}"
    );
    assert!(
        by_topic.contains(&format!("<TopicArn>{TOPIC}</TopicArn>")),
        "{by_topic}"
    );
    assert!(!by_topic.contains("shipping"), "{by_topic}");

    let (_, all) = ask(&fake, "Action=ListSubscriptions").await;
    assert!(all.contains(":orders:billing<"), "{all}");
    assert!(all.contains(":alerts:shipping<"), "{all}");
}

/// UC: UC-GTW-027
#[tokio::test]
async fn the_attributes_of_a_subscription_say_it_is_confirmed_and_never_raw() {
    let fake = topic_and_queue().await;
    ask(&fake, &subscribe("")).await;
    let (status, xml) = ask(
        &fake,
        &format!("Action=GetSubscriptionAttributes&SubscriptionArn={SUB}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    for (key, value) in [
        ("SubscriptionArn", SUB),
        ("TopicArn", TOPIC),
        ("Protocol", "sqs"),
        ("Endpoint", QUEUE),
        ("RawMessageDelivery", "false"),
        ("PendingConfirmation", "false"),
    ] {
        assert!(
            xml.contains(&format!("<key>{key}</key><value>{value}</value>")),
            "{key}: {xml}"
        );
    }

    let set = |name: &str, value: &str| {
        format!(
            "Action=SetSubscriptionAttributes&SubscriptionArn={SUB}&AttributeName={name}&AttributeValue={value}"
        )
    };
    let (status, _) = ask(&fake, &set("RawMessageDelivery", "false")).await;
    assert_eq!(status, StatusCode::OK);
    for (name, value) in [
        ("RawMessageDelivery", "true"),
        ("FilterPolicy", "%7B%7D"),
        ("DeliveryPolicy", "x"),
    ] {
        let (status, xml) = ask(&fake, &set(name, value)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {xml}");
    }

    ask(&fake, &format!("Action=Unsubscribe&SubscriptionArn={SUB}")).await;
    let (status, xml) = ask(
        &fake,
        &format!("Action=GetSubscriptionAttributes&SubscriptionArn={SUB}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{xml}");
}
