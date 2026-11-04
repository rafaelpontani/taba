//! The JSON protocol of SQS (doc 39): `POST /` with `X-Amz-Target: AmazonSQS.<Action>` and a JSON body. It is what the
//! current AWS SDKs speak. The request is already in the shape the actions read, so decoding is only parsing.
//!
//! An error carries the JSON name in `__type` (`com.amazonaws.sqs#QueueDoesNotExist`) and the name of the Query protocol
//! in `x-amzn-query-error`, as the SDKs that map the old names expect.

use axum::http::HeaderMap;
use serde_json::{Value, json};

use crate::gateway::aws::errors::AwsError;

pub const CONTENT_TYPE: &str = "application/x-amz-json-1.0";
const TARGET_PREFIX: &str = "AmazonSQS.";

/// `true` when the request chose the JSON protocol (it has an `X-Amz-Target`).
pub fn is_json(headers: &HeaderMap) -> bool {
    headers.contains_key("x-amz-target")
}

/// The action named by `X-Amz-Target`.
pub fn action(headers: &HeaderMap) -> Result<String, AwsError> {
    let target = headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    target
        .strip_prefix(TARGET_PREFIX)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| AwsError::invalid_action(target))
}

/// The body as the parameters of the action (an empty body is no parameters).
pub fn decode(body: &[u8]) -> Result<Value, AwsError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(value @ Value::Object(_)) => Ok(value),
        _ => Err(AwsError::invalid_parameter(
            "The request body is not a JSON object.",
        )),
    }
}

/// The body of a success: the result as it is (`{}` when the action has none).
pub fn encode_ok(result: &Value) -> Vec<u8> {
    match result {
        Value::Null => b"{}".to_vec(),
        other => serde_json::to_vec(other).unwrap_or_else(|_| b"{}".to_vec()),
    }
}

/// The body and the extra header (`x-amzn-query-error`) of an error.
pub fn encode_error(error: &AwsError) -> (Vec<u8>, String) {
    let body = json!({
        "__type": format!("com.amazonaws.sqs#{}", error.json_code),
        "message": error.message,
    });
    let fault = if error.sender_fault {
        "Sender"
    } else {
        "Receiver"
    };
    (
        serde_json::to_vec(&body).unwrap_or_default(),
        format!("{};{fault}", error.query_code),
    )
}

#[cfg(test)]
mod tests;
