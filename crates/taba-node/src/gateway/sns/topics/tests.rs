use axum::http::StatusCode;

use super::super::tests::{Fake, TOPIC, ask, element};

fn code(xml: &str) -> &str {
    element(xml, "Code").unwrap_or("-")
}

/// UC: UC-GTW-026
#[tokio::test]
async fn a_topic_is_a_distributor_with_the_same_name_and_creating_it_again_answers_the_same_arn() {
    let fake = Fake::default();
    let (status, xml) = ask(
        &fake,
        "Action=CreateTopic&Name=orders&Tags.member.1.Key=team&Tags.member.1.Value=blue",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert_eq!(element(&xml, "TopicArn"), Some(TOPIC), "{xml}");
    let topic = fake.topic("orders").expect("created");
    assert_eq!(topic["type"], "distributor");
    assert_eq!(topic["tags"]["team"], "blue");

    let (status, again) = ask(&fake, "Action=CreateTopic&Name=orders").await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(element(&again, "TopicArn"), Some(TOPIC));
}

/// UC: UC-GTW-026
#[tokio::test]
async fn a_name_that_is_a_queue_a_fifo_topic_or_a_bad_name_is_refused() {
    let fake = Fake::default();
    fake.queue("billing", false);
    let (status, xml) = ask(&fake, "Action=CreateTopic&Name=billing").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(code(&xml), "InvalidParameter");
    assert!(xml.contains("already used by a queue"), "{xml}");

    for form in [
        "Action=CreateTopic&Name=orders.fifo",
        "Action=CreateTopic&Name=orders.fifo&Attributes.entry.1.key=FifoTopic&Attributes.entry.1.value=true",
        "Action=CreateTopic&Name=orders&Attributes.entry.1.key=FifoTopic&Attributes.entry.1.value=true",
        "Action=CreateTopic&Name=has%20space",
        "Action=CreateTopic&Name=orders-dead-letter",
        "Action=CreateTopic&Name=orders&Attributes.entry.1.key=KmsMasterKeyId&Attributes.entry.1.value=k",
        "Action=CreateTopic&Name=orders&Tags.member.1.Key=a%20b&Tags.member.1.Value=x",
    ] {
        let (status, xml) = ask(&fake, form).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{form}: {xml}");
        assert_eq!(code(&xml), "InvalidParameter", "{form}");
    }
    assert!(fake.topic("orders").is_none(), "nothing was created");
    let (missing, _) = ask(&fake, "Action=CreateTopic").await;
    assert_eq!(missing, StatusCode::BAD_REQUEST);
}

/// UC: UC-GTW-026
#[tokio::test]
async fn the_display_name_and_the_policy_are_accepted() {
    let fake = Fake::default();
    let (status, xml) = ask(
        &fake,
        "Action=CreateTopic&Name=orders&Attributes.entry.1.key=DisplayName&Attributes.entry.1.value=Orders",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    for attribute in ["DisplayName", "Policy"] {
        let (status, xml) = ask(
            &fake,
            &format!("Action=SetTopicAttributes&TopicArn={TOPIC}&AttributeName={attribute}&AttributeValue=x"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{attribute}: {xml}");
    }
    let (status, xml) = ask(
        &fake,
        &format!("Action=SetTopicAttributes&TopicArn={TOPIC}&AttributeName=DeliveryPolicy&AttributeValue=x"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{xml}");
}

/// UC: UC-GTW-026
#[tokio::test]
async fn the_list_has_only_the_topics_and_goes_on_with_the_token_through_pages_full_of_queues() {
    let fake = Fake::default();
    for queue in ["a-q1", "a-q2", "a-q3"] {
        fake.queue(queue, false);
    }
    ask(&fake, "Action=CreateTopic&Name=t1").await;
    ask(&fake, "Action=CreateTopic&Name=t2").await;
    *fake.page_size.lock().unwrap() = 2;

    // The first page of the fake holds two queues, none of SNS: the gateway reads on until it finds topics.
    let (status, first) = ask(&fake, "Action=ListTopics").await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(
        first.contains("arn:aws:sns:us-east-1:000000000000:t1"),
        "{first}"
    );
    assert!(!first.contains("a-q"), "{first}");
    let token = element(&first, "NextToken").expect("more pages").to_owned();

    let (_, second) = ask(&fake, &format!("Action=ListTopics&NextToken={token}")).await;
    assert!(second.contains(":t2<"), "{second}");
    assert!(element(&second, "NextToken").is_none(), "{second}");
}

/// UC: UC-GTW-026
#[tokio::test]
async fn the_attributes_of_a_topic_count_its_subscriptions() {
    let fake = Fake::default();
    ask(&fake, "Action=CreateTopic&Name=orders").await;
    let (status, xml) = ask(
        &fake,
        &format!("Action=GetTopicAttributes&TopicArn={TOPIC}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert!(
        xml.contains(&format!("<key>TopicArn</key><value>{TOPIC}</value>")),
        "{xml}"
    );
    assert!(
        xml.contains("<key>SubscriptionsConfirmed</key><value>0</value>"),
        "{xml}"
    );
    assert!(
        xml.contains("<key>Owner</key><value>000000000000</value>"),
        "{xml}"
    );

    let (status, missing) = ask(
        &fake,
        "Action=GetTopicAttributes&TopicArn=arn:aws:sns:us-east-1:000000000000:nothing",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(code(&missing), "NotFound");
    let (bad, _) = ask(&fake, "Action=GetTopicAttributes&TopicArn=not-an-arn").await;
    assert_eq!(bad, StatusCode::BAD_REQUEST);
}

/// UC: UC-GTW-026
#[tokio::test]
async fn deleting_a_topic_removes_it_and_deleting_what_is_not_a_topic_changes_nothing() {
    let fake = Fake::default();
    fake.queue("billing", false);
    ask(&fake, "Action=CreateTopic&Name=orders").await;

    let (status, xml) = ask(&fake, &format!("Action=DeleteTopic&TopicArn={TOPIC}")).await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert!(fake.topic("orders").is_none());

    let (status, _) = ask(&fake, &format!("Action=DeleteTopic&TopicArn={TOPIC}")).await;
    assert_eq!(status, StatusCode::OK, "deleting twice is not an error");

    let (status, _) = ask(
        &fake,
        "Action=DeleteTopic&TopicArn=arn:aws:sns:us-east-1:000000000000:billing",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        fake.topic("billing").is_some(),
        "a queue is not deleted through SNS"
    );
}

/// UC: UC-GTW-026
#[tokio::test]
async fn the_tags_of_a_topic_are_added_replaced_removed_and_listed() {
    let fake = Fake::default();
    ask(
        &fake,
        "Action=CreateTopic&Name=orders&Tags.member.1.Key=a&Tags.member.1.Value=1",
    )
    .await;
    let (status, xml) = ask(
        &fake,
        &format!(
            "Action=TagResource&ResourceArn={TOPIC}&Tags.member.1.Key=a&Tags.member.1.Value=2&Tags.member.2.Key=b&Tags.member.2.Value=3"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    let (_, listed) = ask(
        &fake,
        &format!("Action=ListTagsForResource&ResourceArn={TOPIC}"),
    )
    .await;
    assert!(
        listed.contains("<member><Key>a</Key><Value>2</Value></member>"),
        "{listed}"
    );
    assert!(
        listed.contains("<member><Key>b</Key><Value>3</Value></member>"),
        "{listed}"
    );

    ask(
        &fake,
        &format!("Action=UntagResource&ResourceArn={TOPIC}&TagKeys.member.1=a"),
    )
    .await;
    let (_, after) = ask(
        &fake,
        &format!("Action=ListTagsForResource&ResourceArn={TOPIC}"),
    )
    .await;
    assert!(!after.contains("<Key>a</Key>"), "{after}");
    assert!(after.contains("<Key>b</Key>"), "{after}");

    let (status, xml) = ask(
        &fake,
        "Action=ListTagsForResource&ResourceArn=arn:aws:sns:us-east-1:000000000000:nothing",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{xml}");
}
