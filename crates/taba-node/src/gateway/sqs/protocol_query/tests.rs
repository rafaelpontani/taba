use serde_json::json;

use super::*;

/// UC: UC-GTW-011
#[test]
fn the_action_and_the_parameters_come_from_the_body_and_the_query() {
    let (action, params) = decode(
        "QueueUrl=http%3A%2F%2Fh%2F0%2Fq",
        "Action=SendMessage&Version=2012-11-05&MessageBody=hi+there&DelaySeconds=5",
    )
    .unwrap();
    assert_eq!(action, "SendMessage");
    assert_eq!(
        params,
        json!({"QueueUrl": "http://h/0/q", "MessageBody": "hi there", "DelaySeconds": "5"})
    );
}

/// UC: UC-GTW-011
#[test]
fn a_request_without_an_action_is_refused() {
    assert_eq!(
        decode("", "MessageBody=x").unwrap_err().json_code,
        "MissingParameter"
    );
}

/// UC: UC-GTW-011
#[test]
fn the_query_lists_take_the_shape_of_the_json_protocol() {
    let (_, params) = decode(
        "",
        "Action=CreateQueue&QueueName=q&Attribute.1.Name=VisibilityTimeout&Attribute.1.Value=45&Tag.1.Key=team&Tag.1.Value=billing",
    )
    .unwrap();
    assert_eq!(params["Attributes"], json!({"VisibilityTimeout": "45"}));
    assert_eq!(params["Tags"], json!({"team": "billing"}));

    let (_, params) = decode(
        "",
        "Action=ReceiveMessage&AttributeName.1=All&MessageAttributeName.1=color&MessageAttributeName.2=size",
    )
    .unwrap();
    assert_eq!(params["AttributeNames"], json!(["All"]));
    assert_eq!(params["MessageAttributeNames"], json!(["color", "size"]));

    let (_, params) = decode("", "Action=UntagQueue&TagKey.1=env&TagKey.2=team").unwrap();
    assert_eq!(params["TagKeys"], json!(["env", "team"]));
}

/// UC: UC-GTW-011
#[test]
fn batch_entries_and_their_message_attributes_take_the_shape_of_the_json_protocol() {
    let (_, params) = decode(
        "",
        "Action=SendMessageBatch&SendMessageBatchRequestEntry.1.Id=a&SendMessageBatchRequestEntry.1.MessageBody=one&SendMessageBatchRequestEntry.1.MessageAttribute.1.Name=n&SendMessageBatchRequestEntry.1.MessageAttribute.1.Value.DataType=Number&SendMessageBatchRequestEntry.1.MessageAttribute.1.Value.StringValue=7&SendMessageBatchRequestEntry.2.Id=b&SendMessageBatchRequestEntry.2.MessageBody=two",
    )
    .unwrap();
    assert_eq!(
        params["Entries"],
        json!([
            {"Id": "a", "MessageBody": "one", "MessageAttributes": {"n": {"DataType": "Number", "StringValue": "7"}}},
            {"Id": "b", "MessageBody": "two"}
        ])
    );
}

/// UC: UC-GTW-011
#[test]
fn a_result_is_written_as_the_xml_of_the_protocol() {
    let xml = encode_ok(
        "ReceiveMessage",
        &json!({"Messages": [{
            "MessageId": "m1",
            "Body": "a < b",
            "Attributes": {"ApproximateReceiveCount": "1"},
            "MessageAttributes": {"color": {"DataType": "String", "StringValue": "blue"}}
        }]}),
        "req-1",
    );
    assert!(xml.starts_with("<?xml version=\"1.0\"?><ReceiveMessageResponse xmlns="));
    assert!(xml.contains("<ReceiveMessageResult><Message>"));
    assert!(xml.contains("<Body>a &lt; b</Body>"));
    assert!(
        xml.contains("<Attribute><Name>ApproximateReceiveCount</Name><Value>1</Value></Attribute>")
    );
    assert!(xml.contains(
        "<MessageAttribute><Name>color</Name><Value><DataType>String</DataType><StringValue>blue</StringValue></Value></MessageAttribute>"
    ));
    assert!(xml.contains("<ResponseMetadata><RequestId>req-1</RequestId></ResponseMetadata>"));
}

/// UC: UC-GTW-011
#[test]
fn lists_are_flat_repeated_elements_and_the_batch_entries_are_named_by_the_action() {
    let urls = encode_ok("ListQueues", &json!({"QueueUrls": ["u1", "u2"]}), "r");
    assert!(urls.contains(
        "<ListQueuesResult><QueueUrl>u1</QueueUrl><QueueUrl>u2</QueueUrl></ListQueuesResult>"
    ));

    let batch = encode_ok(
        "DeleteMessageBatch",
        &json!({"Successful": [{"Id": "a"}], "Failed": [{"Id": "b", "Code": "X", "SenderFault": true}]}),
        "r",
    );
    assert!(
        batch.contains("<DeleteMessageBatchResultEntry><Id>a</Id></DeleteMessageBatchResultEntry>")
    );
    assert!(batch.contains("<BatchResultErrorEntry>"));
    assert!(batch.contains("<SenderFault>true</SenderFault>"));
}

/// UC: UC-GTW-011
#[test]
fn an_action_without_data_answers_only_the_metadata() {
    let xml = encode_ok("DeleteMessage", &json!({}), "r");
    assert!(!xml.contains("DeleteMessageResult"));
    assert!(xml.contains("<DeleteMessageResponse"));
}

/// UC: UC-GTW-011
#[test]
fn an_error_is_written_with_its_query_code() {
    let xml = encode_error(&AwsError::queue_does_not_exist(), "r");
    assert!(xml.contains("<Type>Sender</Type>"));
    assert!(xml.contains("<Code>AWS.SimpleQueueService.NonExistentQueue</Code>"));
    assert!(xml.contains("<RequestId>r</RequestId>"));
}

/// UC: UC-GTW-011
#[test]
fn the_lists_may_be_named_in_the_plural_as_the_java_sdks_do() {
    let (_, params) = decode(
        "",
        "Action=TagQueue&Tags.1.Key=team&Tags.1.Value=billing&Tags.2.Key=env&Tags.2.Value=test",
    )
    .unwrap();
    assert_eq!(params["Tags"], json!({"team": "billing", "env": "test"}));
    let (_, params) = decode(
        "",
        "Action=SetQueueAttributes&Attributes.1.Name=VisibilityTimeout&Attributes.1.Value=60",
    )
    .unwrap();
    assert_eq!(params["Attributes"], json!({"VisibilityTimeout": "60"}));
}
