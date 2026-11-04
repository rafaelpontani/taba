use serde_json::json;

use super::*;

/// UC: UC-GTW-026
#[test]
fn the_member_and_entry_levels_of_the_form_disappear() {
    let (action, params) = decode(
        "",
        "Action=CreateTopic&Version=2010-03-31&Name=orders\
         &Attributes.entry.1.key=DisplayName&Attributes.entry.1.value=Orders\
         &Tags.member.1.Key=team&Tags.member.1.Value=blue\
         &Tags.member.2.Key=env&Tags.member.2.Value=prod",
    )
    .expect("valid form");
    assert_eq!(action, "CreateTopic");
    assert_eq!(
        params,
        json!({
            "Name": "orders",
            "Attributes": {"DisplayName": "Orders"},
            "Tags": {"team": "blue", "env": "prod"},
        })
    );
}

/// UC: UC-GTW-028
#[test]
fn the_message_attributes_and_the_batch_entries_have_the_shape_the_actions_read() {
    let (action, params) = decode(
        "",
        "Action=PublishBatch&TopicArn=arn%3Aaws%3Asns%3Aus-east-1%3A000000000000%3At\
         &PublishBatchRequestEntries.member.1.Id=a&PublishBatchRequestEntries.member.1.Message=one\
         &PublishBatchRequestEntries.member.2.Id=b&PublishBatchRequestEntries.member.2.Message=two\
         &PublishBatchRequestEntries.member.2.MessageAttributes.entry.1.Name=color\
         &PublishBatchRequestEntries.member.2.MessageAttributes.entry.1.Value.DataType=String\
         &PublishBatchRequestEntries.member.2.MessageAttributes.entry.1.Value.StringValue=blue",
    )
    .expect("valid form");
    assert_eq!(action, "PublishBatch");
    assert_eq!(
        params["PublishBatchRequestEntries"],
        json!([
            {"Id": "a", "Message": "one"},
            {"Id": "b", "Message": "two",
             "MessageAttributes": {"color": {"DataType": "String", "StringValue": "blue"}}},
        ])
    );
}

/// UC: UC-GTW-026
#[test]
fn a_list_of_texts_and_the_query_string_of_a_get_are_read() {
    let (_, params) = decode(
        "Action=UntagResource&ResourceArn=arn",
        "TagKeys.member.1=a&TagKeys.member.2=b",
    )
    .expect("valid form");
    assert_eq!(params["TagKeys"], json!(["a", "b"]));
    assert_eq!(params["ResourceArn"], "arn");
}

/// UC: UC-GTW-026
#[test]
fn a_request_without_an_action_or_with_a_broken_form_is_refused() {
    assert_eq!(
        decode("", "Name=orders").unwrap_err().query_code,
        "MissingParameter"
    );
    assert_eq!(
        decode("", "Action=ListTopics&A=%FF")
            .unwrap_err()
            .query_code,
        "InvalidParameter"
    );
}

/// UC: UC-GTW-026
#[test]
fn a_result_is_written_with_members_and_entries() {
    let xml = encode_ok(
        "ListTopics",
        &json!({"Topics": [{"TopicArn": "arn:1"}, {"TopicArn": "arn:2"}], "NextToken": "t"}),
        "req-1",
    );
    assert!(xml.starts_with("<?xml version=\"1.0\"?><ListTopicsResponse xmlns=\"http://sns.amazonaws.com/doc/2010-03-31/\">"));
    assert!(xml.contains(
        "<ListTopicsResult><NextToken>t</NextToken><Topics><member><TopicArn>arn:1</TopicArn></member><member><TopicArn>arn:2</TopicArn></member></Topics></ListTopicsResult>"
    ), "{xml}");
    assert!(xml.contains("<ResponseMetadata><RequestId>req-1</RequestId></ResponseMetadata>"));

    let attributes = encode_ok(
        "GetTopicAttributes",
        &json!({"Attributes": {"TopicArn": "arn:1", "Owner": "000000000000"}}),
        "r",
    );
    assert!(attributes.contains(
        "<Attributes><entry><key>Owner</key><value>000000000000</value></entry><entry><key>TopicArn</key><value>arn:1</value></entry></Attributes>"
    ), "{attributes}");

    let empty = encode_ok("ListTopics", &json!({"Topics": []}), "r");
    assert!(empty.contains("<Topics></Topics>"), "{empty}");
}

/// UC: UC-GTW-026
#[test]
fn an_action_without_data_answers_only_the_metadata_and_text_is_escaped() {
    let xml = encode_ok("DeleteTopic", &json!({}), "req-2");
    assert!(!xml.contains("Result"), "{xml}");
    assert!(xml.contains("<DeleteTopicResponse"), "{xml}");
    let escaped = encode_ok("Publish", &json!({"MessageId": "a<b&c"}), "r");
    assert!(
        escaped.contains("<MessageId>a&lt;b&amp;c</MessageId>"),
        "{escaped}"
    );
}

/// UC: UC-GTW-026
#[test]
fn an_error_names_the_sns_code_and_who_is_to_blame() {
    let xml = encode_error(&AwsError::sns_not_found("Topic does not exist"), "req-3");
    assert!(xml.contains("<Error><Type>Sender</Type><Code>NotFound</Code><Message>Topic does not exist</Message></Error><RequestId>req-3</RequestId>"), "{xml}");
    let receiver = encode_error(&AwsError::internal("boom"), "r");
    assert!(receiver.contains("<Type>Receiver</Type>"), "{receiver}");
}
