//! `Publish` and `PublishBatch` of SNS (doc 39).
//!
//! The distributor does the fan-out: the gateway publishes **one** message to the topic and the node copies it to every
//! queue the subscriptions (prefixes) match. The message is the notification SNS delivers to a queue, as JSON text:
//! `{"Type":"Notification","MessageId":...,"TopicArn":...,"Subject":...,"Message":...,"Timestamp":...,"MessageAttributes":...}`.
//! The `MessageId` is made here, before the publication, because the copies are different Taba messages and the
//! application needs the one id that is in all of them. The signature fields (`Signature`, `SigningCertURL`,
//! `UnsubscribeURL`) are left out: there is no certificate of the AWS to sign with.
//!
//! The publication is refused as a whole when the distributor cannot copy to every queue (the targets are over the limit),
//! and when only some copies fail the call answers an error, so the application repeats it: the queues that already had the
//! copy get a duplicate, which "at least once" allows.

use std::collections::BTreeSet;

use axum::http::Method;
use serde_json::{Map, Value, json};

use super::Ctx;
use super::topics::read_distributor;
use crate::core::time::format_ms;
use crate::gateway::aws::errors::AwsError;
use crate::gateway::aws::params::{text, url_encode};
use crate::gateway::envelope::{self, Attributes};
use crate::gateway::naming;

/// The largest message SNS takes, and the largest sum of a batch.
pub const MAX_MESSAGE_BYTES: usize = 262_144;
const MAX_SUBJECT: usize = 100;
const MAX_BATCH: usize = 10;

fn invalid(what: &str) -> AwsError {
    AwsError::sns_invalid_parameter(format!("Invalid parameter: {what}"))
}

/// The pieces of one message, checked.
struct Notification {
    message: String,
    subject: Option<String>,
    attributes: Attributes,
}

/// What a `Publish` or a batch entry holds, checked against the rules of SNS and the limits of the gateway.
fn notification(entry: &Value) -> Result<Notification, AwsError> {
    // These only mean something on a FIFO topic, which the gateway does not have.
    if text(entry, "MessageGroupId").is_some() || text(entry, "MessageDeduplicationId").is_some() {
        return Err(AwsError::sns_invalid_parameter(
            "Invalid parameter: MessageGroupId and MessageDeduplicationId are only for FIFO topics, which this gateway does not have.",
        ));
    }
    let message = text(entry, "Message").ok_or_else(|| invalid("Empty message"))?;
    let message = match text(entry, "MessageStructure") {
        None => message.to_owned(),
        Some("json") => per_protocol_message(message)?,
        Some(_) => return Err(invalid("MessageStructure")),
    };
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(invalid("Message too long"));
    }
    let subject = match text(entry, "Subject") {
        None => None,
        Some(subject) => {
            if subject.len() > MAX_SUBJECT
                || !subject.chars().all(|c| (' '..='~').contains(&c))
                || subject.starts_with(' ')
            {
                return Err(invalid(
                    "Subject: must be ASCII text that begins with a letter, number, or punctuation mark; must not include line breaks or control characters; and must be less than 100 characters long",
                ));
            }
            Some(subject.to_owned())
        }
    };
    let attributes = envelope::parse_attributes(entry.get("MessageAttributes"))
        .map_err(|error| AwsError::sns_invalid_parameter(error.message))?;
    Ok(Notification {
        message,
        subject,
        attributes,
    })
}

/// `MessageStructure=json`: the message is an object with one text per protocol and a `default`. A queue gets the `sqs`
/// one, or the `default`.
fn per_protocol_message(message: &str) -> Result<String, AwsError> {
    let parsed: Value = serde_json::from_str(message)
        .map_err(|_| invalid("Message Structure - JSON message body failed to parse"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| invalid("Message Structure - JSON message body failed to parse"))?;
    let default = object
        .get("default")
        .ok_or_else(|| invalid("Message Structure - No default entry in JSON message body"))?;
    let chosen = object.get("sqs").unwrap_or(default);
    chosen
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("Message Structure - the entry of the message must be a string"))
}

/// The notification a queue receives, as JSON text.
pub fn envelope_of(
    message_id: &str,
    topic_arn: &str,
    notification_time_ms: u64,
    subject: Option<&str>,
    message: &str,
    attributes: &Attributes,
) -> String {
    let mut object = Map::new();
    object.insert("Type".into(), json!("Notification"));
    object.insert("MessageId".into(), json!(message_id));
    object.insert("TopicArn".into(), json!(topic_arn));
    if let Some(subject) = subject {
        object.insert("Subject".into(), json!(subject));
    }
    object.insert("Message".into(), json!(message));
    object.insert("Timestamp".into(), json!(format_ms(notification_time_ms)));
    if !attributes.is_empty() {
        let mut stored = Map::new();
        for (name, attribute) in attributes {
            let value = if attribute.data_type.starts_with("Binary") {
                attribute.binary_value.clone().unwrap_or_default()
            } else {
                attribute.string_value.clone().unwrap_or_default()
            };
            stored.insert(
                name.clone(),
                json!({"Type": attribute.data_type, "Value": value}),
            );
        }
        object.insert("MessageAttributes".into(), Value::Object(stored));
    }
    Value::Object(object).to_string()
}

/// Publishes one notification to the distributor and returns its `MessageId`.
async fn deliver(
    ctx: &Ctx<'_>,
    topic: &str,
    arn: &str,
    notification: &Notification,
) -> Result<String, AwsError> {
    let now = ctx.state.clock.now_millis();
    let id = crate::core::id::new_at(now);
    let body = envelope_of(
        &id,
        arn,
        now,
        notification.subject.as_deref(),
        &notification.message,
        &notification.attributes,
    );
    let reply = ctx
        .backend()
        .call(
            ctx.caller,
            Method::POST,
            &format!("/topics/{}/messages", url_encode(topic)),
            Some(envelope::PLAIN_CONTENT_TYPE),
            body.into_bytes(),
        )
        .await?;
    if !reply.is_success() {
        return Err(reply.sns_error());
    }
    let answer = reply.json();
    let failed: Vec<String> = answer
        .get("deliveries")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|d| d.get("error").is_some())
                .filter_map(|d| d.get("topic").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if !failed.is_empty() {
        return Err(AwsError::internal(format!(
            "The message could not be copied to the queues {}; publish it again.",
            failed.join(", ")
        )));
    }
    Ok(id)
}

/// The topic of a publication. A phone number or a platform endpoint is not a destination the gateway has.
fn destination(params: &Value) -> Result<(String, String), AwsError> {
    if text(params, "PhoneNumber").is_some() {
        return Err(invalid(
            "PhoneNumber: SMS is not supported by this gateway.",
        ));
    }
    let (parameter, arn) = match (text(params, "TopicArn"), text(params, "TargetArn")) {
        (Some(arn), _) => ("TopicArn", arn),
        (None, Some(arn)) => ("TargetArn", arn),
        (None, None) => {
            return Err(invalid(
                "TopicArn or TargetArn Reason: no value for required parameter",
            ));
        }
    };
    let name = naming::topic_in_arn(arn).ok_or_else(|| invalid(parameter))?;
    Ok((name.to_owned(), arn.to_owned()))
}

/// `Publish`
pub async fn publish(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (topic, arn) = destination(params)?;
    let notification = notification(params)?;
    // A name that is a queue is not a topic: publishing to it would put the notification in the queue itself.
    read_distributor(ctx, &topic).await?;
    let id = deliver(ctx, &topic, &arn, &notification).await?;
    Ok(json!({"MessageId": id}))
}

/// `PublishBatch`: up to 10 entries, each published on its own; the answer says which ones failed.
pub async fn publish_batch(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (topic, arn) = destination(params)?;
    let entries = match params.get("PublishBatchRequestEntries") {
        Some(Value::Array(list)) => list.as_slice(),
        _ => &[],
    };
    if entries.is_empty() {
        return Err(AwsError::sns_empty_batch());
    }
    if entries.len() > MAX_BATCH {
        return Err(AwsError::sns_too_many_entries());
    }
    let mut ids = BTreeSet::new();
    let mut total = 0usize;
    for entry in entries {
        let id = text(entry, "Id").ok_or_else(AwsError::sns_invalid_entry_id)?;
        if id.len() > 80
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(AwsError::sns_invalid_entry_id());
        }
        if !ids.insert(id.to_owned()) {
            return Err(AwsError::sns_entry_ids_not_distinct());
        }
        total += text(entry, "Message").map_or(0, str::len);
    }
    if total > MAX_MESSAGE_BYTES {
        return Err(AwsError::sns_batch_too_long());
    }
    // The topic must exist before anything is published, so a batch to a missing topic fails as a whole.
    read_distributor(ctx, &topic).await?;
    let mut successful = Vec::new();
    let mut failed = Vec::new();
    for entry in entries {
        let id = text(entry, "Id").unwrap_or_default();
        let outcome = match notification(entry) {
            Ok(notification) => deliver(ctx, &topic, &arn, &notification).await,
            Err(error) => Err(error),
        };
        match outcome {
            Ok(message_id) => successful.push(json!({"Id": id, "MessageId": message_id})),
            Err(error) => failed.push(json!({
                "Id": id,
                "Code": error.query_code,
                "Message": error.message,
                "SenderFault": error.sender_fault,
            })),
        }
    }
    Ok(json!({"Successful": successful, "Failed": failed}))
}

#[cfg(test)]
mod tests;
