use axum::http::HeaderValue;
use serde_json::json;

use super::*;

fn with_target(target: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-amz-target", HeaderValue::from_str(target).unwrap());
    headers
}

/// UC: UC-GTW-010
#[test]
fn the_target_header_chooses_the_protocol_and_names_the_action() {
    assert!(!is_json(&HeaderMap::new()));
    let headers = with_target("AmazonSQS.SendMessage");
    assert!(is_json(&headers));
    assert_eq!(action(&headers).unwrap(), "SendMessage");
}

/// UC: UC-GTW-010
#[test]
fn a_target_that_is_not_sqs_is_an_invalid_action() {
    for target in ["Other.SendMessage", "AmazonSQS.", ""] {
        assert_eq!(
            action(&with_target(target)).unwrap_err().json_code,
            "InvalidAction",
            "{target}"
        );
    }
}

/// UC: UC-GTW-010
#[test]
fn the_body_is_an_object_and_an_empty_body_is_no_parameters() {
    assert_eq!(decode(b"").unwrap(), json!({}));
    assert_eq!(decode(b"  \n").unwrap(), json!({}));
    assert_eq!(
        decode(br#"{"QueueUrl":"http://h/0/q"}"#).unwrap(),
        json!({"QueueUrl": "http://h/0/q"})
    );
    assert!(decode(b"[1]").is_err());
    assert!(decode(b"not json").is_err());
}

/// UC: UC-GTW-010
#[test]
fn an_error_carries_both_names_of_the_code() {
    let (body, query_error) = encode_error(&AwsError::queue_does_not_exist());
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["__type"], "com.amazonaws.sqs#QueueDoesNotExist");
    assert!(body["message"].as_str().unwrap().contains("does not exist"));
    assert_eq!(
        query_error,
        "AWS.SimpleQueueService.NonExistentQueue;Sender"
    );
    let (_, receiver) = encode_error(&AwsError::internal("boom"));
    assert!(receiver.ends_with(";Receiver"));
}

/// UC: UC-GTW-010
#[test]
fn a_result_without_content_is_an_empty_object() {
    assert_eq!(encode_ok(&Value::Null), b"{}");
    assert_eq!(encode_ok(&json!({"QueueUrl": "u"})), br#"{"QueueUrl":"u"}"#);
}
