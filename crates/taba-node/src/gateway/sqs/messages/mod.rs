//! The message actions of SQS (doc 39): send, receive, delete and change the visibility, one by one and in batches.
//!
//! A send is a publication to the queue topic; a receive is a delivery. The `ReceiptHandle` carries the message `id` and the
//! `receiptToken` ([`super::receipt`]), so deleting or hiding a message needs no state in the gateway. The attributes of a
//! message travel in the envelope ([`crate::gateway::envelope`]). The calls always pass the visibility, the wait and the
//! number of messages explicitly, because the defaults of SQS and of Taba differ (30 s against 120 s, one message against
//! ten).
//!
//! On a FIFO queue every message needs a `MessageGroupId` and a deduplication id (`MessageDeduplicationId`, or the SHA-256
//! of the body when the queue has `ContentBasedDeduplication`). Taba takes fewer characters in both than SQS does, so an
//! id with other punctuation goes as a hash and the envelope keeps the original ([`crate::gateway::naming::taba_id`]).
//! Taba delivers one message at a time of each group and has no delay on a FIFO queue.

use axum::http::Method;
use serde_json::{Map, Value, json};

use super::queues::{queue_of, target};
use super::receipt;
use super::{Ctx, number_in, required_text, text, texts, url_encode};
use crate::core::time::parse_ms;
use crate::gateway::aws::errors::AwsError;
use crate::gateway::envelope::{self, Attributes, Origin};
use crate::gateway::naming::{self, Queue};

/// The most entries of a batch.
const MAX_BATCH: usize = 10;
/// The most bytes of the entries of a batch together.
const MAX_BATCH_BYTES: usize = 1_048_576;
/// SQS accepts delays up to 15 minutes.
const MAX_DELAY: i64 = 900;
const MAX_VISIBILITY: i64 = 43_200;

/// The characters an SQS message may hold: `#x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]`.
fn check_contents(body: &str) -> Result<(), AwsError> {
    let allowed = |c: char| {
        matches!(c, '\u{9}' | '\u{A}' | '\u{D}')
            || ('\u{20}'..='\u{D7FF}').contains(&c)
            || ('\u{E000}'..='\u{FFFD}').contains(&c)
            || c >= '\u{10000}'
    };
    if body.chars().all(allowed) {
        Ok(())
    } else {
        Err(AwsError::invalid_message_contents(
            "Invalid characters found. Valid unicode characters are #x9 | #xA | #xD | #x20 to #xD7FF | #xE000 to #xFFFD | #x10000 to #x10FFFF",
        ))
    }
}

/// The bytes a message counts for: the body and, for each attribute, its name, type and value.
fn size_of(body: &str, attributes: &Attributes) -> usize {
    body.len()
        + attributes
            .iter()
            .map(|(name, a)| {
                name.len()
                    + a.data_type.len()
                    + a.string_value.as_ref().map_or(0, String::len)
                    + a.binary_value.as_ref().map_or(0, |b| b.len() * 3 / 4)
            })
            .sum::<usize>()
}

/// How a send treats the queue it goes to.
enum Mode {
    /// A standard queue: the delay of the queue applies when the message does not say one. The group and the
    /// deduplication id of the request are ignored.
    Standard { default_delay: i64 },
    /// A FIFO queue.
    Fifo { content_dedup: bool },
}

impl Mode {
    /// How to send to the queue behind `target`. The dead-letter queue of a FIFO queue is not FIFO in Taba, so it takes
    /// no deduplication id and no group rules.
    fn of(queue: &Queue, target: &super::queues::Target) -> Self {
        if queue.fifo && !target.alias {
            Self::Fifo {
                content_dedup: target.settings.content_dedup,
            }
        } else {
            Self::Standard {
                default_delay: target.settings.delay,
            }
        }
    }
}

/// The group and the deduplication id of a FIFO message as Taba takes them.
struct FifoIds {
    group: String,
    dedup: String,
}

/// One message ready to be published.
struct Prepared {
    packed: envelope::Packed,
    delay: i64,
    fifo: Option<FifoIds>,
    md5_body: String,
    md5_attributes: Option<String>,
}

/// The group and the deduplication id of a message for a FIFO queue, and the original ones when Taba cannot hold them.
fn fifo_ids(entry: &Value, body: &str, content_dedup: bool) -> Result<(FifoIds, Origin), AwsError> {
    let group = text(entry, "MessageGroupId")
        .ok_or_else(|| AwsError::missing_parameter("MessageGroupId"))?;
    if !naming::is_aws_id(group) {
        return Err(AwsError::invalid_parameter(
            "Invalid value for the parameter MessageGroupId. Reason: use up to 128 letters, digits and punctuation.",
        ));
    }
    let dedup = match text(entry, "MessageDeduplicationId") {
        Some(id) if naming::is_aws_id(id) => id.to_owned(),
        Some(_) => {
            return Err(AwsError::invalid_parameter(
                "Invalid value for the parameter MessageDeduplicationId. Reason: use up to 128 letters, digits and punctuation.",
            ));
        }
        None if content_dedup => envelope::sha256_hex(body.as_bytes()),
        None => {
            return Err(AwsError::invalid_parameter(
                "The queue should either have ContentBasedDeduplication enabled or MessageDeduplicationId provided explicitly.",
            ));
        }
    };
    let (group, original_group) = naming::taba_id(group);
    let (dedup, original_dedup) = naming::taba_id(&dedup);
    Ok((
        FifoIds { group, dedup },
        Origin {
            group: original_group,
            dedup: original_dedup,
        },
    ))
}

#[cfg(test)]
fn prepare(entry: &Value, default_delay: i64, max_size: i64) -> Result<Prepared, AwsError> {
    prepare_in(entry, &Mode::Standard { default_delay }, max_size)
}

fn prepare_in(entry: &Value, mode: &Mode, max_size: i64) -> Result<Prepared, AwsError> {
    let body =
        text(entry, "MessageBody").ok_or_else(|| AwsError::missing_parameter("MessageBody"))?;
    check_contents(body)?;
    let attributes = envelope::parse_attributes(entry.get("MessageAttributes"))?;
    if size_of(body, &attributes) as i64 > max_size {
        return Err(AwsError::invalid_parameter(format!(
            "One or more parameters are invalid. Reason: Message must be shorter than {max_size} bytes."
        )));
    }
    let (delay, fifo, origin) = match mode {
        Mode::Standard { default_delay } => (
            number_in(entry, "DelaySeconds", 0, MAX_DELAY)?.unwrap_or(*default_delay),
            None,
            Origin::default(),
        ),
        Mode::Fifo { content_dedup } => {
            if number_in(entry, "DelaySeconds", 0, MAX_DELAY)?.is_some() {
                return Err(AwsError::invalid_parameter(
                    "The request include parameter that is not valid for this queue type: DelaySeconds.",
                ));
            }
            let (ids, origin) = fifo_ids(entry, body, *content_dedup)?;
            (0, Some(ids), origin)
        }
    };
    Ok(Prepared {
        md5_body: envelope::md5_of_body(body),
        md5_attributes: envelope::md5_of_attributes(&attributes),
        packed: envelope::pack_with(body, &attributes, &origin),
        delay,
        fifo,
    })
}

/// The `SequenceNumber` of a FIFO message: its `id` (a UUID that grows inside each group) as a decimal number.
fn sequence_number(id: &str) -> Option<String> {
    let digits: String = id.chars().filter(|c| *c != '-').collect();
    u128::from_str_radix(&digits, 16)
        .ok()
        .map(|n| n.to_string())
}

fn sent(id: &str, prepared: &Prepared) -> Map<String, Value> {
    let mut result = Map::new();
    result.insert("MessageId".into(), json!(id));
    result.insert("MD5OfMessageBody".into(), json!(prepared.md5_body));
    if let Some(md5) = &prepared.md5_attributes {
        result.insert("MD5OfMessageAttributes".into(), json!(md5));
    }
    if prepared.fifo.is_some()
        && let Some(sequence) = sequence_number(id)
    {
        result.insert("SequenceNumber".into(), json!(sequence));
    }
    result
}

/// `SendMessage`
pub async fn send_message(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let target = target(ctx, &queue).await?;
    let prepared = prepare_in(params, &Mode::of(&queue, &target), target.settings.max_size)?;
    let mut path = format!("/topics/{}/messages", url_encode(&target.topic));
    let mut query: Vec<String> = Vec::new();
    if prepared.delay > 0 {
        query.push(format!("delaySeconds={}", prepared.delay));
    }
    if let Some(ids) = &prepared.fifo {
        query.push(format!("groupId={}", url_encode(&ids.group)));
        query.push(format!("deduplicationId={}", url_encode(&ids.dedup)));
    }
    if !query.is_empty() {
        path.push('?');
        path.push_str(&query.join("&"));
    }
    let reply = ctx
        .backend()
        .call(
            ctx.caller,
            Method::POST,
            &path,
            Some(prepared.packed.content_type),
            prepared.packed.bytes.clone(),
        )
        .await?;
    if !reply.is_success() {
        return Err(reply.aws_error());
    }
    let id = reply
        .json()
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(Value::Object(sent(&id, &prepared)))
}

/// The entries of a batch request: how many, and that each has a valid, distinct `Id`.
fn entries(params: &Value) -> Result<Vec<(String, &Value)>, AwsError> {
    let list = match params.get("Entries") {
        Some(Value::Array(list)) => list,
        _ => return Err(AwsError::empty_batch()),
    };
    if list.is_empty() {
        return Err(AwsError::empty_batch());
    }
    if list.len() > MAX_BATCH {
        return Err(AwsError::too_many_entries());
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        let id = text(entry, "Id").ok_or_else(|| AwsError::missing_parameter("Id"))?;
        if id.len() > 80
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(AwsError::invalid_entry_id());
        }
        if !seen.insert(id.to_owned()) {
            return Err(AwsError::entry_ids_not_distinct());
        }
        out.push((id.to_owned(), entry));
    }
    Ok(out)
}

fn failed_entry(id: &str, error: &AwsError) -> Value {
    json!({
        "Id": id,
        "SenderFault": error.sender_fault,
        "Code": error.json_code,
        "Message": error.message,
    })
}

fn batch_result(successful: Vec<Value>, failed: Vec<Value>) -> Value {
    let mut result = Map::new();
    result.insert("Successful".into(), Value::Array(successful));
    if !failed.is_empty() {
        result.insert("Failed".into(), Value::Array(failed));
    }
    Value::Object(result)
}

/// `SendMessageBatch`
pub async fn send_message_batch(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let entries = entries(params)?;
    let target = target(ctx, &queue).await?;
    let mode = Mode::of(&queue, &target);
    let total: usize = entries
        .iter()
        .map(|(_, e)| text(e, "MessageBody").map_or(0, str::len))
        .sum();
    if total > MAX_BATCH_BYTES {
        return Err(AwsError::batch_too_long());
    }
    let mut failed = Vec::new();
    let mut prepared: Vec<(String, Prepared)> = Vec::new();
    for (id, entry) in &entries {
        match prepare_in(entry, &mode, target.settings.max_size) {
            Ok(p) => prepared.push((id.clone(), p)),
            Err(error) => failed.push(failed_entry(id, &error)),
        }
    }
    let mut successful = Vec::new();
    if !prepared.is_empty() {
        let items: Vec<Value> = prepared
            .iter()
            .map(|(_, p)| {
                let mut item = Map::new();
                item.insert("body".into(), p.packed.value.clone());
                item.insert("contentType".into(), json!(p.packed.content_type));
                if p.delay > 0 {
                    item.insert("delaySeconds".into(), json!(p.delay));
                }
                if let Some(ids) = &p.fifo {
                    item.insert("groupId".into(), json!(ids.group));
                    item.insert("deduplicationId".into(), json!(ids.dedup));
                }
                Value::Object(item)
            })
            .collect();
        let reply = ctx
            .backend()
            .ok_json(
                ctx.caller,
                Method::POST,
                &format!("/topics/{}/messages/batch", url_encode(&target.topic)),
                Some(&json!({"messages": items})),
            )
            .await?;
        let results = reply
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (index, (id, p)) in prepared.iter().enumerate() {
            let result = results.get(index).cloned().unwrap_or(Value::Null);
            match item_error(&result) {
                None => {
                    let message_id = result.get("id").and_then(Value::as_str).unwrap_or_default();
                    let mut entry = sent(message_id, p);
                    entry.insert("Id".into(), json!(id));
                    successful.push(Value::Object(entry));
                }
                Some(error) => failed.push(failed_entry(id, &error)),
            }
        }
    }
    Ok(batch_result(successful, failed))
}

/// The AWS error of an item of a batch answer of the client API; `None` when the item succeeded.
fn item_error(result: &Value) -> Option<AwsError> {
    let status = result.get("status").and_then(Value::as_u64).unwrap_or(0);
    if (200..300).contains(&status) && result.get("error").is_none_or(Value::is_null) {
        return None;
    }
    let error = result.get("error");
    let code = error
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let message = error
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The message could not be processed.");
    Some(AwsError::from_taba(
        u16::try_from(status).unwrap_or(500),
        code,
        message,
        None,
    ))
}

/// The attributes the application asked for (`All`, a name, or `prefix.*`), with the `.*` that means all.
fn wanted(names: &[String], name: &str) -> bool {
    names.iter().any(|n| {
        n == "All"
            || n == ".*"
            || n == name
            || n.strip_suffix(".*")
                .is_some_and(|prefix| name.starts_with(prefix))
    })
}

/// `ReceiveMessage`
pub async fn receive_message(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let max = number_in(params, "MaxNumberOfMessages", 1, 10)?.unwrap_or(1);
    let visibility = number_in(params, "VisibilityTimeout", 0, MAX_VISIBILITY)?;
    let wait = number_in(params, "WaitTimeSeconds", 0, 20)?;
    let target = target(ctx, &queue).await?;
    let visibility = visibility.unwrap_or(target.settings.visibility);
    let wait = wait.unwrap_or(target.settings.wait);
    let reply = ctx
        .backend()
        .ok_json(
            ctx.caller,
            Method::GET,
            &format!(
                "/topics/{}/messages?maxMessages={max}&visibilityTimeout={visibility}&waitTime={wait}",
                url_encode(&target.topic)
            ),
            None,
        )
        .await?;
    let system = {
        let mut names = texts(params, "AttributeNames");
        names.extend(texts(params, "MessageSystemAttributeNames"));
        names
    };
    let attribute_names = texts(params, "MessageAttributeNames");
    let received = reply
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut messages = Vec::with_capacity(received.len());
    for message in received {
        let id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let token = message
            .get("receiptToken")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let opened = envelope::unpack(
            message.get("body").unwrap_or(&Value::Null),
            message.get("bodyEncoding").and_then(Value::as_str),
        );
        let mut out = Map::new();
        out.insert("MessageId".into(), json!(id));
        out.insert("ReceiptHandle".into(), json!(receipt::encode(id, token)));
        out.insert(
            "MD5OfBody".into(),
            json!(envelope::md5_of_body(&opened.body)),
        );
        out.insert("Body".into(), json!(opened.body));

        let mut attributes = Map::new();
        let mut put = |name: &str, value: Option<String>| {
            if let Some(value) = value.filter(|_| wanted(&system, name)) {
                attributes.insert(name.to_owned(), Value::String(value));
            }
        };
        put(
            "ApproximateReceiveCount",
            message
                .get("deliveryCount")
                .and_then(Value::as_i64)
                .map(|n| n.to_string()),
        );
        put(
            "SentTimestamp",
            message
                .get("publishedAt")
                .and_then(Value::as_str)
                .and_then(parse_ms)
                .map(|ms| ms.to_string()),
        );
        put("SenderId", Some(crate::gateway::naming::ACCOUNT.to_owned()));
        // The ids the application wrote, when Taba holds a hash of them.
        put(
            "MessageGroupId",
            opened.origin.group.clone().or_else(|| {
                message
                    .get("groupId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
        );
        put(
            "MessageDeduplicationId",
            opened.origin.dedup.clone().or_else(|| {
                message
                    .get("deduplicationId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
        );
        put(
            "SequenceNumber",
            queue.fifo.then_some(id).and_then(sequence_number),
        );
        if !attributes.is_empty() {
            out.insert("Attributes".into(), Value::Object(attributes));
        }

        let returned: Attributes = opened
            .attributes
            .into_iter()
            .filter(|(name, _)| wanted(&attribute_names, name))
            .collect();
        if !returned.is_empty() {
            let mut shown = Map::new();
            for (name, a) in &returned {
                let mut entry = Map::new();
                entry.insert("DataType".into(), json!(a.data_type));
                if let Some(v) = &a.string_value {
                    entry.insert("StringValue".into(), json!(v));
                }
                if let Some(v) = &a.binary_value {
                    entry.insert("BinaryValue".into(), json!(v));
                }
                shown.insert(name.clone(), Value::Object(entry));
            }
            out.insert("MessageAttributes".into(), Value::Object(shown));
            if let Some(md5) = envelope::md5_of_attributes(&returned) {
                out.insert("MD5OfMessageAttributes".into(), json!(md5));
            }
        }
        messages.push(Value::Object(out));
    }
    if messages.is_empty() {
        Ok(json!({}))
    } else {
        Ok(json!({"Messages": messages}))
    }
}

/// An outcome of the client API that SQS reports as success: deleting what is already gone is not an error.
fn gone(code: &str) -> bool {
    matches!(code, "message.not-found" | "message.stale-receipt")
}

/// `DeleteMessage`
pub async fn delete_message(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let topic = target(ctx, &queue_of(params)?).await?.topic;
    let (id, token) = receipt::decode(required_text(params, "ReceiptHandle")?)?;
    let reply = ctx
        .backend()
        .json(
            ctx.caller,
            Method::DELETE,
            &format!(
                "/topics/{}/messages/{}?receiptToken={}",
                url_encode(&topic),
                url_encode(&id),
                url_encode(&token)
            ),
            None,
        )
        .await?;
    if reply.is_success() || reply.error_code().is_some_and(|c| gone(&c)) {
        Ok(json!({}))
    } else {
        Err(reply.aws_error())
    }
}

/// `DeleteMessageBatch`
pub async fn delete_message_batch(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let topic = target(ctx, &queue_of(params)?).await?.topic;
    let entries = entries(params)?;
    let mut failed = Vec::new();
    let mut items = Vec::new();
    let mut ids = Vec::new();
    for (id, entry) in &entries {
        match text(entry, "ReceiptHandle")
            .ok_or_else(|| AwsError::missing_parameter("ReceiptHandle"))
            .and_then(receipt::decode)
        {
            Ok((message, token)) => {
                items.push(json!({"id": message, "receiptToken": token}));
                ids.push(id.clone());
            }
            Err(error) => failed.push(failed_entry(id, &error)),
        }
    }
    let mut successful = Vec::new();
    if !items.is_empty() {
        let reply = ctx
            .backend()
            .ok_json(
                ctx.caller,
                Method::POST,
                &format!("/topics/{}/messages/ack", url_encode(&topic)),
                Some(&json!({"messages": items})),
            )
            .await?;
        let results = reply
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (index, id) in ids.iter().enumerate() {
            let result = results.get(index).cloned().unwrap_or(Value::Null);
            let already_gone = result
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .is_some_and(gone);
            match item_error(&result) {
                Some(error) if !already_gone => failed.push(failed_entry(id, &error)),
                _ => successful.push(json!({"Id": id})),
            }
        }
    }
    Ok(batch_result(successful, failed))
}

/// `ChangeMessageVisibility`
pub async fn change_message_visibility(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let topic = target(ctx, &queue_of(params)?).await?.topic;
    let (id, token) = receipt::decode(required_text(params, "ReceiptHandle")?)?;
    let seconds = number_in(params, "VisibilityTimeout", 0, MAX_VISIBILITY)?
        .ok_or_else(|| AwsError::missing_parameter("VisibilityTimeout"))?;
    ctx.backend()
        .ok_json(
            ctx.caller,
            Method::PUT,
            &format!(
                "/topics/{}/messages/{}/visibility?receiptToken={}",
                url_encode(&topic),
                url_encode(&id),
                url_encode(&token)
            ),
            Some(&json!({"visibilityTimeout": seconds})),
        )
        .await?;
    Ok(json!({}))
}

/// `ChangeMessageVisibilityBatch`
pub async fn change_message_visibility_batch(
    ctx: &Ctx<'_>,
    params: &Value,
) -> Result<Value, AwsError> {
    let topic = target(ctx, &queue_of(params)?).await?.topic;
    let entries = entries(params)?;
    let mut failed = Vec::new();
    let mut items = Vec::new();
    let mut ids = Vec::new();
    for (id, entry) in &entries {
        let prepared = text(entry, "ReceiptHandle")
            .ok_or_else(|| AwsError::missing_parameter("ReceiptHandle"))
            .and_then(receipt::decode)
            .and_then(|handle| {
                let seconds = number_in(entry, "VisibilityTimeout", 0, MAX_VISIBILITY)?
                    .ok_or_else(|| AwsError::missing_parameter("VisibilityTimeout"))?;
                Ok((handle, seconds))
            });
        match prepared {
            Ok(((message, token), seconds)) => {
                items.push(
                    json!({"id": message, "receiptToken": token, "visibilityTimeout": seconds}),
                );
                ids.push(id.clone());
            }
            Err(error) => failed.push(failed_entry(id, &error)),
        }
    }
    let mut successful = Vec::new();
    if !items.is_empty() {
        let reply = ctx
            .backend()
            .ok_json(
                ctx.caller,
                Method::POST,
                &format!("/topics/{}/messages/visibility", url_encode(&topic)),
                Some(&json!({"messages": items})),
            )
            .await?;
        let results = reply
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (index, id) in ids.iter().enumerate() {
            let result = results.get(index).cloned().unwrap_or(Value::Null);
            match item_error(&result) {
                None => successful.push(json!({"Id": id})),
                Some(error) => failed.push(failed_entry(id, &error)),
            }
        }
    }
    Ok(batch_result(successful, failed))
}

#[cfg(test)]
mod tests;
