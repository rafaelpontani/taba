use axum::http::StatusCode;
use serde_json::{Value, json};

use super::super::tests::{Fake, TOPIC, ask, element};
use super::*;

fn code(xml: &str) -> &str {
    element(xml, "Code").unwrap_or("-")
}

async fn with_subscribers() -> Fake {
    let fake = Fake::default();
    fake.queue("billing", false);
    fake.queue("shipping", false);
    ask(&fake, "Action=CreateTopic&Name=orders").await;
    for queue in ["billing", "shipping"] {
        ask(
            &fake,
            &format!("Action=Subscribe&TopicArn={TOPIC}&Protocol=sqs&Endpoint=arn:aws:sqs:us-east-1:000000000000:{queue}"),
        )
        .await;
    }
    fake
}

fn published_json(fake: &Fake, at: usize) -> Value {
    serde_json::from_str(&fake.published()[at].body).expect("the notification is JSON")
}

/// UC: UC-GTW-028
#[tokio::test]
async fn a_publication_is_one_notification_to_the_distributor_with_the_id_the_application_gets() {
    let fake = with_subscribers().await;
    let (status, xml) = ask(
        &fake,
        &format!("Action=Publish&TopicArn={TOPIC}&Message=hello%20sns"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    let id = element(&xml, "MessageId").expect("a message id").to_owned();

    let published = fake.published();
    assert_eq!(
        published.len(),
        1,
        "one publication; the node does the copies"
    );
    assert_eq!(published[0].topic, "orders");
    assert_eq!(published[0].content_type, "text/plain; charset=utf-8");
    let notification = published_json(&fake, 0);
    assert_eq!(notification["Type"], "Notification");
    assert_eq!(notification["MessageId"], id.as_str());
    assert_eq!(notification["TopicArn"], TOPIC);
    assert_eq!(notification["Message"], "hello sns");
    assert!(notification.get("Subject").is_none());
    assert!(notification.get("MessageAttributes").is_none());
    assert!(notification["Timestamp"].as_str().unwrap().ends_with('Z'));
}

/// UC: UC-GTW-028
#[tokio::test]
async fn the_subject_and_the_message_attributes_go_inside_the_notification() {
    let fake = with_subscribers().await;
    let (status, xml) = ask(
        &fake,
        &format!(
            "Action=Publish&TopicArn={TOPIC}&Message=m&Subject=greeting\
             &MessageAttributes.entry.1.Name=color&MessageAttributes.entry.1.Value.DataType=String&MessageAttributes.entry.1.Value.StringValue=blue\
             &MessageAttributes.entry.2.Name=blob&MessageAttributes.entry.2.Value.DataType=Binary&MessageAttributes.entry.2.Value.BinaryValue=AAEC"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    let notification = published_json(&fake, 0);
    assert_eq!(notification["Subject"], "greeting");
    assert_eq!(
        notification["MessageAttributes"]["color"],
        json!({"Type": "String", "Value": "blue"})
    );
    assert_eq!(
        notification["MessageAttributes"]["blob"],
        json!({"Type": "Binary", "Value": "AAEC"})
    );
}

/// UC: UC-GTW-028
#[tokio::test]
async fn text_that_needs_escaping_survives_the_notification() {
    let fake = with_subscribers().await;
    let message = "café \"quoted\" \\slash\\\nline2";
    let encoded = message
        .bytes()
        .map(|b| format!("%{b:02X}"))
        .collect::<String>();
    ask(
        &fake,
        &format!("Action=Publish&TopicArn={TOPIC}&Message={encoded}"),
    )
    .await;
    assert_eq!(published_json(&fake, 0)["Message"], message);
}

/// UC: UC-GTW-028
#[tokio::test]
async fn a_message_for_each_protocol_gives_the_queue_the_sqs_text_or_the_default() {
    let fake = with_subscribers().await;
    let structure = |value: &Value| {
        let text = value.to_string();
        let encoded = text
            .bytes()
            .map(|b| format!("%{b:02X}"))
            .collect::<String>();
        format!("Action=Publish&TopicArn={TOPIC}&MessageStructure=json&Message={encoded}")
    };
    ask(
        &fake,
        &structure(&json!({"default": "d", "sqs": "for the queue"})),
    )
    .await;
    ask(&fake, &structure(&json!({"default": "only default"}))).await;
    assert_eq!(published_json(&fake, 0)["Message"], "for the queue");
    assert_eq!(published_json(&fake, 1)["Message"], "only default");

    for bad in [json!({"sqs": "no default"}), json!({"default": 1})] {
        let (status, xml) = ask(&fake, &structure(&bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{xml}");
        assert_eq!(code(&xml), "InvalidParameter");
    }
    let (status, _) = ask(
        &fake,
        &format!("Action=Publish&TopicArn={TOPIC}&MessageStructure=json&Message=not-json"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// UC: UC-GTW-028
#[tokio::test]
async fn what_a_topic_of_this_gateway_cannot_take_is_refused_before_anything_is_published() {
    let fake = with_subscribers().await;
    let long = "x".repeat(MAX_MESSAGE_BYTES + 1);
    let cases = [
        format!("Action=Publish&TopicArn={TOPIC}"),
        format!("Action=Publish&TopicArn={TOPIC}&Message={long}"),
        format!("Action=Publish&TopicArn={TOPIC}&Message=m&MessageGroupId=g"),
        format!("Action=Publish&TopicArn={TOPIC}&Message=m&MessageDeduplicationId=d"),
        format!("Action=Publish&TopicArn={TOPIC}&Message=m&Subject=line%0Abreak"),
        format!(
            "Action=Publish&TopicArn={TOPIC}&Message=m&Subject={}",
            "s".repeat(101)
        ),
        format!("Action=Publish&TopicArn={TOPIC}&Message=m&MessageStructure=xml"),
        "Action=Publish&PhoneNumber=%2B15555550100&Message=m".to_owned(),
        "Action=Publish&Message=m".to_owned(),
        "Action=Publish&TopicArn=not-an-arn&Message=m".to_owned(),
        format!(
            "Action=Publish&TopicArn={TOPIC}&Message=m&MessageAttributes.entry.1.Name=a&MessageAttributes.entry.1.Value.DataType=Boolean&MessageAttributes.entry.1.Value.StringValue=x"
        ),
    ];
    for form in cases {
        let (status, xml) = ask(&fake, &form).await;
        let shown = &form[..form.len().min(120)];
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{shown}: {}",
            &xml[..xml.len().min(200)]
        );
        assert_eq!(code(&xml), "InvalidParameter", "{shown}");
    }
    assert!(fake.published().is_empty());
}

/// UC: UC-GTW-028
#[tokio::test]
async fn publishing_to_a_topic_that_does_not_exist_or_to_a_queue_is_not_found_and_publishes_nothing()
 {
    let fake = with_subscribers().await;
    for arn in [
        "arn:aws:sns:us-east-1:000000000000:ghost",
        "arn:aws:sns:us-east-1:000000000000:billing",
    ] {
        let (status, xml) = ask(&fake, &format!("Action=Publish&TopicArn={arn}&Message=m")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{arn}: {xml}");
        assert_eq!(code(&xml), "NotFound");
    }
    assert!(fake.published().is_empty(), "the queue got nothing");
}

/// UC: UC-GTW-028
#[tokio::test]
async fn a_topic_with_no_subscription_accepts_the_publication() {
    let fake = Fake::default();
    ask(&fake, "Action=CreateTopic&Name=orders").await;
    let (status, xml) = ask(&fake, &format!("Action=Publish&TopicArn={TOPIC}&Message=m")).await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert!(element(&xml, "MessageId").is_some());
}

/// UC: UC-GTW-028
#[tokio::test]
async fn when_a_copy_fails_the_call_fails_so_the_application_publishes_again() {
    let fake = with_subscribers().await;
    *fake.failing_copy.lock().unwrap() = Some("shipping".into());
    let (status, xml) = ask(&fake, &format!("Action=Publish&TopicArn={TOPIC}&Message=m")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{xml}");
    assert_eq!(code(&xml), "InternalError");
    assert_eq!(
        element(&xml, "Type"),
        Some("Receiver"),
        "the SDK retries it"
    );
    assert!(xml.contains("shipping"), "{xml}");
    assert!(!xml.contains("billing"), "{xml}");
}

fn entry_form(entries: &[(&str, &str)]) -> String {
    let mut form = format!("Action=PublishBatch&TopicArn={TOPIC}");
    for (n, (id, message)) in entries.iter().enumerate() {
        let at = n + 1;
        form.push_str(&format!(
            "&PublishBatchRequestEntries.member.{at}.Id={id}&PublishBatchRequestEntries.member.{at}.Message={message}"
        ));
    }
    form
}

/// UC: UC-GTW-028
#[tokio::test]
async fn a_batch_publishes_each_entry_and_answers_an_id_for_each() {
    let fake = with_subscribers().await;
    let (status, xml) = ask(
        &fake,
        &entry_form(&[("a", "one"), ("b", "two"), ("c", "three")]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert_eq!(xml.matches("<member><Id>").count(), 3, "{xml}");
    assert!(xml.contains("<Failed></Failed>"), "{xml}");
    let messages: Vec<String> = (0..3)
        .map(|n| {
            published_json(&fake, n)["Message"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(messages, vec!["one", "two", "three"]);
    // The id in the answer is the id inside the notification.
    let first = published_json(&fake, 0)["MessageId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        xml.contains(&format!("<Id>a</Id><MessageId>{first}</MessageId>")),
        "{xml}"
    );
}

/// UC: UC-GTW-028
#[tokio::test]
async fn an_entry_that_is_wrong_fails_alone_and_the_others_go() {
    let fake = with_subscribers().await;
    let mut form = entry_form(&[("a", "one"), ("b", "two")]);
    form.push_str("&PublishBatchRequestEntries.member.2.MessageGroupId=g");
    let (status, xml) = ask(&fake, &form).await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert!(xml.contains("<Successful><member><Id>a</Id>"), "{xml}");
    assert!(
        xml.contains("<Failed><member><Code>InvalidParameter</Code><Id>b</Id>"),
        "{xml}"
    );
    assert!(xml.contains("<SenderFault>true</SenderFault>"), "{xml}");
    assert_eq!(fake.published().len(), 1);
}

/// UC: UC-GTW-028
#[tokio::test]
async fn a_batch_with_a_bad_shape_or_a_missing_topic_fails_as_a_whole() {
    let fake = with_subscribers().await;
    let eleven: Vec<(String, &str)> = (0..11).map(|n| (format!("e{n}"), "m")).collect();
    let eleven_refs: Vec<(&str, &str)> = eleven.iter().map(|(a, b)| (a.as_str(), *b)).collect();
    let big = "x".repeat(MAX_MESSAGE_BYTES / 2 + 1);
    let cases: Vec<(String, &str)> = vec![
        (
            format!("Action=PublishBatch&TopicArn={TOPIC}"),
            "EmptyBatchRequest",
        ),
        (entry_form(&eleven_refs), "TooManyEntriesInBatchRequest"),
        (
            entry_form(&[("a", "x"), ("a", "y")]),
            "BatchEntryIdsNotDistinct",
        ),
        (entry_form(&[("has.dot", "x")]), "InvalidBatchEntryId"),
        (
            entry_form(&[("a", &big), ("b", &big)]),
            "BatchRequestTooLong",
        ),
    ];
    for (form, expected) in cases {
        let (status, xml) = ask(&fake, &form).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{expected}: {}",
            &xml[..xml.len().min(200)]
        );
        assert_eq!(code(&xml), expected);
    }
    let missing = entry_form(&[("a", "x")]).replace(":orders", ":ghost");
    let (status, xml) = ask(&fake, &missing).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{xml}");
    assert!(fake.published().is_empty());
}

/// UC: UC-GTW-028
#[test]
fn the_notification_has_the_fields_of_sns_and_none_of_the_signature() {
    let text = envelope_of(
        "id-1",
        TOPIC,
        1_700_000_000_123,
        Some("subject"),
        "body",
        &Attributes::new(),
    );
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value,
        json!({
            "Type": "Notification",
            "MessageId": "id-1",
            "TopicArn": TOPIC,
            "Subject": "subject",
            "Message": "body",
            "Timestamp": "2023-11-14T22:13:20.123Z",
        })
    );
    assert!(value.get("Signature").is_none() && value.get("UnsubscribeURL").is_none());
}
