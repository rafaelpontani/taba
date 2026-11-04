//! The queue actions of SQS (doc 39): create, find, list, delete, purge, attributes and tags.
//!
//! A queue is a Taba queue topic with the same name. The attributes that have a field in Taba use it
//! (`MessageRetentionPeriod` is `ttl`, `MaximumMessageSize` is `maxMessageBytes`, the `maxReceiveCount` of the
//! `RedrivePolicy` is `maxDeliveries`); the ones that do not (`VisibilityTimeout`, `DelaySeconds`,
//! `ReceiveMessageWaitTimeSeconds`) are kept in tags whose key starts with `sqs.`, which the tag actions do not show. A FIFO queue is the Taba FIFO topic of the name without `.fifo`; its
//! `ContentBasedDeduplication` is a tag too. The dead-letter queue an application names in the `RedrivePolicy` is an
//! **alias**: Taba keeps the dead letters of `orders` in `orders-dead-letter`, so the queue the application created for
//! it is marked as the dead-letter queue of `orders` (tag `sqs.dlqof`) and every action on it goes to
//! `orders-dead-letter`; `orders` keeps the name of that queue in the tag `sqs.dlq` to show it in its `RedrivePolicy`.
//! The gateway has no state of its own: every node sees the same queue through the replicated definitions.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::http::Method;
use serde_json::{Map, Value, json};

use super::{Ctx, number_in, required_text, text, text_map, texts, url_encode};
use crate::core::time::parse_ms;
use crate::gateway::aws::errors::AwsError;
use crate::gateway::naming::{self, Queue};

/// The tags of the gateway: their keys start with this and the SQS tag actions never show or change them.
const OWN_TAG: &str = "sqs.";
const TAG_VISIBILITY: &str = "sqs.vt";
const TAG_DELAY: &str = "sqs.delay";
const TAG_WAIT: &str = "sqs.wait";
/// `ContentBasedDeduplication` of a FIFO queue.
const TAG_CONTENT_DEDUP: &str = "sqs.cbd";
/// On a source queue: the name of the queue the application named as its dead-letter queue.
const TAG_DEAD_LETTER: &str = "sqs.dlq";
/// On a dead-letter queue: the topic of the source queue whose dead letters it shows.
const TAG_DEAD_LETTER_OF: &str = "sqs.dlqof";

/// The defaults of SQS for what Taba does not keep as a field.
pub const DEFAULT_VISIBILITY: i64 = 30;
/// The largest message SQS takes.
pub const MAX_MESSAGE_BYTES: i64 = 1_048_576;
const MAX_TAGS: usize = 20;
const TAG_KEY_MAX: usize = 64;
const TAG_VALUE_MAX: usize = 128;

/// What a queue says about how to treat a call that does not say.
#[derive(Debug, Clone)]
pub struct Settings {
    pub visibility: i64,
    pub delay: i64,
    pub wait: i64,
    pub retention: i64,
    pub max_size: i64,
    /// `maxDeliveries` when the queue sets it; it is what the `RedrivePolicy` is made from.
    pub max_deliveries: Option<i64>,
    pub created_ms: u64,
    pub fifo: bool,
    /// `ContentBasedDeduplication` (FIFO only).
    pub content_dedup: bool,
    /// The name of the dead-letter queue named in the `RedrivePolicy`.
    pub dead_letter: Option<String>,
    /// The topic of the source queue, when this queue is the dead-letter queue of another.
    pub dead_letter_of: Option<String>,
    pub tags: BTreeMap<String, String>,
}

impl Settings {
    pub fn of(topic: &Value) -> Self {
        let tags: BTreeMap<String, String> = topic
            .get("tags")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let from_tag = |key: &str, default: i64| {
            tags.get(key)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(default)
        };
        let effective = |field: &str, default: i64| {
            topic
                .get("effective")
                .and_then(|e| e.get(field))
                .and_then(Value::as_i64)
                .unwrap_or(default)
        };
        Self {
            visibility: from_tag(TAG_VISIBILITY, DEFAULT_VISIBILITY),
            delay: from_tag(TAG_DELAY, 0),
            wait: from_tag(TAG_WAIT, 0),
            retention: topic
                .get("ttl")
                .and_then(Value::as_i64)
                .unwrap_or_else(|| effective("ttl", 1_209_600)),
            max_size: topic
                .get("maxMessageBytes")
                .and_then(Value::as_i64)
                .unwrap_or_else(|| effective("maxMessageBytes", MAX_MESSAGE_BYTES))
                .min(MAX_MESSAGE_BYTES),
            max_deliveries: topic.get("maxDeliveries").and_then(Value::as_i64),
            created_ms: topic
                .get("createdAt")
                .and_then(Value::as_str)
                .and_then(parse_ms)
                .unwrap_or(0),
            fifo: topic.get("fifo").and_then(Value::as_bool).unwrap_or(false),
            content_dedup: tags.get(TAG_CONTENT_DEDUP).is_some_and(|v| v == "true"),
            dead_letter: tags.get(TAG_DEAD_LETTER).cloned(),
            dead_letter_of: tags.get(TAG_DEAD_LETTER_OF).cloned(),
            tags,
        }
    }

    /// The tags the application sees: the gateway's own are hidden.
    pub fn user_tags(&self) -> BTreeMap<String, String> {
        self.tags
            .iter()
            .filter(|(k, _)| !k.starts_with(OWN_TAG))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// The queue of a request: the name in the `QueueUrl`.
pub fn queue_of(params: &Value) -> Result<Queue, AwsError> {
    let url = required_text(params, "QueueUrl")?;
    let name = naming::name_in_url(url)?;
    naming::parse(&name).map_err(|_| AwsError::queue_does_not_exist())
}

/// Reads the topic of a queue; a topic that is not a queue (a distributor, a dead-letter queue) is not one for SQS.
pub async fn read_topic(ctx: &Ctx<'_>, topic: &str) -> Result<Value, AwsError> {
    let value = ctx
        .backend()
        .ok_json(
            ctx.caller,
            Method::GET,
            &format!("/topics/{}", url_encode(topic)),
            None,
        )
        .await?;
    if value.get("type").and_then(Value::as_str) != Some("queue")
        || value.get("deadLetter").and_then(Value::as_bool) == Some(true)
    {
        return Err(AwsError::queue_does_not_exist());
    }
    Ok(value)
}

/// Reads a queue. The name says whether it is FIFO (`.fifo`), and a topic of the other kind is not that queue.
pub async fn read_queue(ctx: &Ctx<'_>, queue: &Queue) -> Result<Value, AwsError> {
    let value = read_topic(ctx, &queue.topic).await?;
    if value.get("fifo").and_then(Value::as_bool).unwrap_or(false) != queue.fifo {
        return Err(AwsError::queue_does_not_exist());
    }
    Ok(value)
}

/// The defaults of a queue for a send or a receive, remembered for a short while.
pub async fn settings(ctx: &Ctx<'_>, queue: &Queue) -> Result<Arc<Settings>, AwsError> {
    if let Some(settings) = ctx
        .state
        .cached_settings(&queue.topic)
        .filter(|s| s.fifo == queue.fifo)
    {
        return Ok(settings);
    }
    let settings = Arc::new(Settings::of(&read_queue(ctx, queue).await?));
    ctx.state.remember_settings(&queue.topic, settings.clone());
    Ok(settings)
}

/// Where the messages of a queue are: its own topic, or the Taba dead-letter queue of the source queue when the
/// application created it as the dead-letter queue of another.
pub struct Target {
    pub topic: String,
    pub settings: Arc<Settings>,
    /// The topic is not the queue's own (it is the dead-letter queue of a source queue).
    pub alias: bool,
}

pub async fn target(ctx: &Ctx<'_>, queue: &Queue) -> Result<Target, AwsError> {
    let settings = settings(ctx, queue).await?;
    Ok(match &settings.dead_letter_of {
        Some(source) => Target {
            topic: format!("{source}-dead-letter"),
            settings,
            alias: true,
        },
        None => Target {
            topic: queue.topic.clone(),
            settings,
            alias: false,
        },
    })
}

/// What a list of attributes asks to change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub tags: BTreeMap<String, String>,
    pub ttl: Option<i64>,
    pub max_bytes: Option<i64>,
    pub max_deliveries: Option<i64>,
    /// `FifoQueue`, when the request says it.
    pub fifo: Option<bool>,
    /// `ContentBasedDeduplication`, when the request says it (it is also in `tags`).
    pub content_dedup: Option<bool>,
    /// The name of the queue the `deadLetterTargetArn` of the `RedrivePolicy` points to.
    pub dead_letter: Option<String>,
}

fn attribute_bool(name: &str, value: &str) -> Result<bool, AwsError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AwsError::invalid_attribute_value(format!(
            "Invalid value for the parameter {name}. Reason: Must be true or false."
        ))),
    }
}

fn attribute_number(name: &str, value: &str, low: i64, high: i64) -> Result<i64, AwsError> {
    let n = value.trim().parse::<i64>().map_err(|_| {
        AwsError::invalid_attribute_value(format!("Invalid value for the parameter {name}."))
    })?;
    if (low..=high).contains(&n) {
        Ok(n)
    } else {
        Err(AwsError::invalid_attribute_value(format!(
            "Invalid value for the parameter {name}. Reason: Must be between {low} and {high}, if provided."
        )))
    }
}

/// Reads the `Attributes` of `CreateQueue` and `SetQueueAttributes`.
pub fn parse_changes(attributes: &[(String, String)]) -> Result<Changes, AwsError> {
    let mut changes = Changes::default();
    for (name, value) in attributes {
        match name.as_str() {
            "VisibilityTimeout" => {
                let n = attribute_number(name, value, 0, 43_200)?;
                changes.tags.insert(TAG_VISIBILITY.into(), n.to_string());
            }
            "DelaySeconds" => {
                let n = attribute_number(name, value, 0, 900)?;
                changes.tags.insert(TAG_DELAY.into(), n.to_string());
            }
            "ReceiveMessageWaitTimeSeconds" => {
                let n = attribute_number(name, value, 0, 20)?;
                changes.tags.insert(TAG_WAIT.into(), n.to_string());
            }
            "MessageRetentionPeriod" => {
                changes.ttl = Some(attribute_number(name, value, 60, 1_209_600)?);
            }
            "MaximumMessageSize" => {
                changes.max_bytes = Some(attribute_number(name, value, 1024, MAX_MESSAGE_BYTES)?);
            }
            "RedrivePolicy" => {
                let policy: Value = serde_json::from_str(value).map_err(|_| {
                    AwsError::invalid_attribute_value("The RedrivePolicy is not valid JSON.")
                })?;
                let count = policy
                    .get("maxReceiveCount")
                    .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                    .ok_or_else(|| {
                        AwsError::invalid_attribute_value(
                            "The RedrivePolicy must have a maxReceiveCount.",
                        )
                    })?;
                if !(1..=1000).contains(&count) {
                    return Err(AwsError::invalid_attribute_value(
                        "The maxReceiveCount must be between 1 and 1000.",
                    ));
                }
                changes.max_deliveries = Some(count);
                if let Some(arn) = policy.get("deadLetterTargetArn").and_then(Value::as_str) {
                    let name = naming::name_in_arn(arn).ok_or_else(|| {
                        AwsError::invalid_attribute_value(
                            "The deadLetterTargetArn of the RedrivePolicy is not a queue ARN.",
                        )
                    })?;
                    changes.dead_letter = Some(name.to_owned());
                }
            }
            // Accepted and ignored: Taba applies no queue policy and encrypts at the node, not per queue. An application that
            // subscribes a queue to SNS always writes the `Policy`, so refusing it would stop the migration.
            "Policy"
            | "SqsManagedSseEnabled"
            | "KmsMasterKeyId"
            | "KmsDataKeyReusePeriodSeconds" => {}
            "FifoQueue" => changes.fifo = Some(attribute_bool(name, value)?),
            "ContentBasedDeduplication" => {
                let on = attribute_bool(name, value)?;
                changes.content_dedup = Some(on);
                changes
                    .tags
                    .insert(TAG_CONTENT_DEDUP.into(), on.to_string());
            }
            // Accepted and ignored: the deduplication of Taba is always by group, and a group is one message at a time.
            "DeduplicationScope" => {
                if !matches!(value.as_str(), "messageGroup" | "queue") {
                    return Err(AwsError::invalid_attribute_value(format!(
                        "Invalid value for the parameter {name}."
                    )));
                }
            }
            "FifoThroughputLimit" => {
                if !matches!(value.as_str(), "perQueue" | "perMessageGroupId") {
                    return Err(AwsError::invalid_attribute_value(format!(
                        "Invalid value for the parameter {name}."
                    )));
                }
            }
            other => {
                return Err(AwsError::invalid_attribute_name(format!(
                    "Unknown Attribute {other}."
                )));
            }
        }
    }
    Ok(changes)
}

/// Checks tags the application chose against what a Taba tag can hold.
fn check_tags(tags: &[(String, String)]) -> Result<(), AwsError> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':');
    for (key, value) in tags {
        if key.is_empty() || key.len() > TAG_KEY_MAX || !key.chars().all(allowed) {
            return Err(AwsError::invalid_parameter(format!(
                "The tag key `{key}` is not valid: 1 to {TAG_KEY_MAX} letters, digits, `_`, `-`, `.` or `:`."
            )));
        }
        if key.starts_with(OWN_TAG) {
            return Err(AwsError::invalid_parameter(format!(
                "The tag key `{key}` uses the prefix `{OWN_TAG}`, which the gateway keeps for itself."
            )));
        }
        if value.len() > TAG_VALUE_MAX || !value.chars().all(allowed) {
            return Err(AwsError::invalid_parameter(format!(
                "The value of the tag `{key}` is not valid: up to {TAG_VALUE_MAX} letters, digits, `_`, `-`, `.` or `:`."
            )));
        }
    }
    Ok(())
}

/// A FIFO queue cannot delay its messages: Taba refuses a delay on a FIFO queue, because a message waiting at the head
/// would hold its whole group.
fn check_fifo_delay(queue: &Queue, tags: &BTreeMap<String, String>) -> Result<(), AwsError> {
    let delay = tags
        .get(TAG_DELAY)
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    if queue.fifo && delay != 0 {
        return Err(AwsError::unsupported(
            "A FIFO queue cannot delay its messages in this gateway: DelaySeconds must be 0.",
        ));
    }
    Ok(())
}

/// What the attributes say about the kind of queue, against the name: `.fifo` goes with `FifoQueue=true`, and
/// `ContentBasedDeduplication` only exists on a FIFO queue.
fn check_kind(queue: &Queue, changes: &Changes) -> Result<(), AwsError> {
    match (queue.fifo, changes.fifo) {
        (true, Some(true)) | (false, None | Some(false)) => {}
        (true, _) => {
            return Err(AwsError::invalid_parameter(
                "The name of a FIFO queue ends with .fifo and the attribute FifoQueue must be true.",
            ));
        }
        (false, Some(true)) => {
            return Err(AwsError::invalid_parameter(
                "The name of a FIFO queue must end with .fifo.",
            ));
        }
    }
    if !queue.fifo && changes.content_dedup.is_some() {
        return Err(AwsError::invalid_attribute_name(
            "Unknown Attribute ContentBasedDeduplication.",
        ));
    }
    Ok(())
}

/// Checks the queue the `RedrivePolicy` names as the dead-letter queue and returns it. It must exist, be of the same kind
/// and not be the dead-letter queue of another source: Taba has one dead-letter queue per source queue.
async fn check_dead_letter(ctx: &Ctx<'_>, source: &Queue, name: &str) -> Result<Queue, AwsError> {
    let dead = naming::parse(name).map_err(|_| {
        AwsError::invalid_attribute_value(
            "The dead-letter target of the RedrivePolicy is not a valid queue.",
        )
    })?;
    if dead.topic == source.topic {
        return Err(AwsError::invalid_attribute_value(
            "A queue cannot be its own dead-letter queue.",
        ));
    }
    if dead.fifo != source.fifo {
        return Err(AwsError::invalid_attribute_value(
            "The dead-letter queue of a FIFO queue must be FIFO, and of a standard queue, standard.",
        ));
    }
    let value = read_queue(ctx, &dead).await.map_err(|error| {
        if error.json_code == "QueueDoesNotExist" {
            AwsError::invalid_attribute_value(
                "The dead-letter queue of the RedrivePolicy does not exist.",
            )
        } else {
            error
        }
    })?;
    if let Some(other) = Settings::of(&value).dead_letter_of
        && other != source.topic
    {
        return Err(AwsError::unsupported(
            "The queue is already the dead-letter queue of another one: Taba keeps one dead-letter queue per source queue.",
        ));
    }
    Ok(dead)
}

/// Sets or removes one tag of the gateway on a queue (the tags are written back whole).
async fn set_own_tag(
    ctx: &Ctx<'_>,
    queue: &Queue,
    key: &str,
    value: Option<&str>,
) -> Result<(), AwsError> {
    let mut tags = Settings::of(&read_queue(ctx, queue).await?).tags;
    match value {
        Some(value) => tags.insert(key.to_owned(), value.to_owned()),
        None => tags.remove(key),
    };
    patch(ctx, &queue.topic, &tags, &Changes::default()).await
}

/// Marks the queue the application named as the dead-letter queue of `source`, so that its messages are the dead letters.
async fn link_dead_letter(ctx: &Ctx<'_>, dead: &Queue, source: &Queue) -> Result<(), AwsError> {
    set_own_tag(ctx, dead, TAG_DEAD_LETTER_OF, Some(&source.topic)).await
}

/// Takes the mark back, when it is still the one of `source`. The queue may be gone already, so a failure is not an error.
async fn unlink_dead_letter(ctx: &Ctx<'_>, name: &str, source: &Queue) {
    let Ok(dead) = naming::parse(name) else {
        return;
    };
    let Ok(value) = read_queue(ctx, &dead).await else {
        return;
    };
    if Settings::of(&value).dead_letter_of.as_deref() == Some(source.topic.as_str()) {
        let _ = set_own_tag(ctx, &dead, TAG_DEAD_LETTER_OF, None).await;
    }
}

/// `CreateQueue`
pub async fn create_queue(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = naming::parse(required_text(params, "QueueName")?)?;
    let topic = queue.topic.clone();
    let changes = parse_changes(&text_map(params, "Attributes"))?;
    check_kind(&queue, &changes)?;
    check_fifo_delay(&queue, &changes.tags)?;
    let dead = match &changes.dead_letter {
        Some(name) => Some(check_dead_letter(ctx, &queue, name).await?),
        None => None,
    };
    let user_tags = text_map(params, "Tags");
    check_tags(&user_tags)?;
    let mut tags = changes.tags.clone();
    if let Some(dead) = &dead {
        tags.insert(TAG_DEAD_LETTER.into(), dead.name());
    }
    tags.extend(user_tags);
    if tags.len() > MAX_TAGS {
        return Err(AwsError::invalid_parameter(
            "A queue can have up to 20 tags.",
        ));
    }
    let mut body = Map::new();
    body.insert("name".into(), json!(topic));
    body.insert("type".into(), json!("queue"));
    if queue.fifo {
        body.insert("fifo".into(), json!(true));
    }
    body.insert("tags".into(), json!(tags));
    if let Some(ttl) = changes.ttl {
        body.insert("ttl".into(), json!(ttl));
    }
    if let Some(bytes) = changes.max_bytes {
        body.insert("maxMessageBytes".into(), json!(bytes));
    }
    if let Some(deliveries) = changes.max_deliveries {
        body.insert("maxDeliveries".into(), json!(deliveries));
    }
    let reply = ctx
        .backend()
        .json(
            ctx.caller,
            Method::POST,
            "/topics",
            Some(&Value::Object(body)),
        )
        .await?;
    if reply.is_success() {
        if let Some(dead) = &dead {
            link_dead_letter(ctx, dead, &queue).await?;
        }
        return Ok(json!({"QueueUrl": queue.url(&ctx.base_url)}));
    }
    if reply.error_code().as_deref() != Some("topic.already-exists") {
        return Err(reply.aws_error());
    }
    // The queue exists: SQS answers with its URL if the attributes asked for are the same ones, and with an error if not.
    let differs = |name: &str| {
        AwsError::queue_name_exists(format!(
            "A queue already exists with the same name and a different value for attribute {name}."
        ))
    };
    let existing = Settings::of(&read_topic(ctx, &topic).await?);
    if existing.fifo != queue.fifo {
        return Err(differs("FifoQueue"));
    }
    for (key, value) in &changes.tags {
        if existing.tags.get(key) != Some(value) {
            let name = match key.as_str() {
                TAG_VISIBILITY => "VisibilityTimeout",
                TAG_DELAY => "DelaySeconds",
                TAG_CONTENT_DEDUP => "ContentBasedDeduplication",
                _ => "ReceiveMessageWaitTimeSeconds",
            };
            return Err(differs(name));
        }
    }
    if dead.as_ref().map(Queue::name) != existing.dead_letter {
        return Err(differs("RedrivePolicy"));
    }
    if changes.ttl.is_some_and(|ttl| ttl != existing.retention) {
        return Err(differs("MessageRetentionPeriod"));
    }
    if changes.max_bytes.is_some_and(|b| b != existing.max_size) {
        return Err(differs("MaximumMessageSize"));
    }
    if changes
        .max_deliveries
        .is_some_and(|d| existing.max_deliveries != Some(d))
    {
        return Err(differs("RedrivePolicy"));
    }
    Ok(json!({"QueueUrl": queue.url(&ctx.base_url)}))
}

/// `GetQueueUrl`
pub async fn get_queue_url(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = naming::parse(required_text(params, "QueueName")?)
        .map_err(|_| AwsError::queue_does_not_exist())?;
    read_queue(ctx, &queue).await?;
    Ok(json!({"QueueUrl": queue.url(&ctx.base_url)}))
}

/// `DeleteQueue`. A queue that was the dead-letter queue named by another stops being one; a queue that named one frees it.
pub async fn delete_queue(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let before = read_queue(ctx, &queue).await.ok().map(|v| Settings::of(&v));
    ctx.backend()
        .ok_json(
            ctx.caller,
            Method::DELETE,
            &format!("/topics/{}", url_encode(&queue.topic)),
            None,
        )
        .await?;
    ctx.state.forget_settings(&queue.topic);
    if let Some(name) = before.and_then(|s| s.dead_letter) {
        unlink_dead_letter(ctx, &name, &queue).await;
    }
    Ok(json!({}))
}

/// `PurgeQueue`
pub async fn purge_queue(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let target = target(ctx, &queue).await?;
    ctx.backend()
        .ok_json(
            ctx.caller,
            Method::DELETE,
            &format!("/topics/{}/messages", url_encode(&target.topic)),
            None,
        )
        .await?;
    Ok(json!({}))
}

/// `ListQueues`: the queues of the topics the token reaches, by name prefix. The dead-letter queues and the distributors
/// are not SQS queues and stay out. The `NextToken` says where the page stopped: how many matches of the Taba page were
/// already given and the cursor of that page.
pub async fn list_queues(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let prefix = text(params, "QueueNamePrefix").unwrap_or("");
    let wanted = number_in(params, "MaxResults", 1, 1000)?.unwrap_or(1000) as usize;
    let (mut skip, mut cursor) = match text(params, "NextToken") {
        Some(token) => decode_token(token)?,
        None => (0, None),
    };
    let mut urls: Vec<String> = Vec::new();
    let mut next: Option<String> = None;
    loop {
        let mut path = String::from("/topics?limit=1000");
        // The name of a FIFO queue ends in `.fifo`, which its topic does not have: ask the server only for the part of
        // the prefix before the first dot, and filter the rest here.
        let server_prefix = prefix.split('.').next().unwrap_or("");
        if !server_prefix.is_empty() {
            path.push_str("&name=");
            path.push_str(&url_encode(server_prefix));
        }
        if let Some(c) = &cursor {
            path.push_str("&cursor=");
            path.push_str(&url_encode(c));
        }
        let page = ctx
            .backend()
            .ok_json(ctx.caller, Method::GET, &path, None)
            .await?;
        let found: Vec<String> = page
            .get("items")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|item| {
                item.get("type").and_then(Value::as_str) == Some("queue")
                    && item.get("deadLetter").and_then(Value::as_bool) != Some(true)
            })
            .filter_map(|item| {
                let topic = item.get("name").and_then(Value::as_str)?;
                let fifo = item.get("fifo").and_then(Value::as_bool).unwrap_or(false);
                Queue::of_topic(topic, fifo)
            })
            .filter(|queue| queue.name().starts_with(prefix))
            .map(|queue| queue.url(&ctx.base_url))
            .collect();
        let following = page
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut taken = skip;
        for url in found.iter().skip(skip) {
            if urls.len() == wanted {
                break;
            }
            urls.push(url.clone());
            taken += 1;
        }
        if taken < found.len() {
            next = Some(encode_token(taken, cursor.as_deref()));
            break;
        }
        match following {
            Some(c) if urls.len() == wanted => {
                next = Some(encode_token(0, Some(&c)));
                break;
            }
            Some(c) => {
                cursor = Some(c);
                skip = 0;
            }
            None => break,
        }
    }
    let mut result = Map::new();
    if !urls.is_empty() {
        result.insert("QueueUrls".into(), json!(urls));
    }
    if let Some(token) = next {
        result.insert("NextToken".into(), json!(token));
    }
    Ok(Value::Object(result))
}

fn encode_token(skip: usize, cursor: Option<&str>) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(format!("{skip}|{}", cursor.unwrap_or("")))
}

fn decode_token(token: &str) -> Result<(usize, Option<String>), AwsError> {
    use base64::Engine as _;
    let invalid = || AwsError::invalid_parameter("The NextToken is not valid.");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| invalid())?;
    let text = String::from_utf8(bytes).map_err(|_| invalid())?;
    let (skip, cursor) = text.split_once('|').ok_or_else(invalid)?;
    Ok((
        skip.parse().map_err(|_| invalid())?,
        Some(cursor.to_owned()).filter(|c| !c.is_empty()),
    ))
}

// The attributes of a queue by name, with the counts read only when asked for.
pub async fn get_queue_attributes(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let names = texts(params, "AttributeNames");
    let all = names.iter().any(|n| n == "All");
    let wants = |name: &str| all || names.iter().any(|n| n == name);
    for name in &names {
        if name != "All" && !KNOWN_ATTRIBUTES.contains(&name.as_str()) {
            return Err(AwsError::invalid_attribute_name(format!(
                "Unknown Attribute {name}."
            )));
        }
    }
    let settings = Settings::of(&read_queue(ctx, &queue).await?);
    let mut attributes = Map::new();
    let mut put = |name: &str, value: String| {
        if wants(name) {
            attributes.insert(name.to_owned(), Value::String(value));
        }
    };
    put("QueueArn", queue.arn(&ctx.region));
    put("VisibilityTimeout", settings.visibility.to_string());
    put("DelaySeconds", settings.delay.to_string());
    put("ReceiveMessageWaitTimeSeconds", settings.wait.to_string());
    put("MessageRetentionPeriod", settings.retention.to_string());
    put("MaximumMessageSize", settings.max_size.to_string());
    put("CreatedTimestamp", (settings.created_ms / 1000).to_string());
    put(
        "LastModifiedTimestamp",
        (settings.created_ms / 1000).to_string(),
    );
    if queue.fifo {
        put("FifoQueue", "true".to_owned());
        put(
            "ContentBasedDeduplication",
            settings.content_dedup.to_string(),
        );
    }
    if let Some(count) = settings.max_deliveries {
        // The queue the application named as the dead-letter queue, or the one Taba keeps for this queue.
        let target = match &settings.dead_letter {
            Some(name) => naming::queue_arn(&ctx.region, name),
            None => naming::queue_arn(&ctx.region, &queue.dead_letter_topic()),
        };
        put(
            "RedrivePolicy",
            json!({"deadLetterTargetArn": target, "maxReceiveCount": count}).to_string(),
        );
    }
    let wants_counts = all
        || names.iter().any(|n| {
            matches!(
                n.as_str(),
                "ApproximateNumberOfMessages"
                    | "ApproximateNumberOfMessagesNotVisible"
                    | "ApproximateNumberOfMessagesDelayed"
            )
        });
    if wants_counts {
        let target = target(ctx, &queue).await?;
        let stats = ctx
            .backend()
            .ok_json(
                ctx.caller,
                Method::GET,
                &format!("/stats/topics/{}", url_encode(&target.topic)),
                None,
            )
            .await?;
        let depth = stats.get("depth").and_then(Value::as_i64).unwrap_or(0);
        let invisible = stats
            .get("invisibleMessages")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .min(depth);
        // The delayed messages are counted as in processing while PRB-093 is open, so `Delayed` stays 0.
        put(
            "ApproximateNumberOfMessages",
            (depth - invisible).to_string(),
        );
        put(
            "ApproximateNumberOfMessagesNotVisible",
            invisible.to_string(),
        );
        put("ApproximateNumberOfMessagesDelayed", "0".to_owned());
    }
    Ok(json!({"Attributes": attributes}))
}

/// The attributes `GetQueueAttributes` answers to.
const KNOWN_ATTRIBUTES: &[&str] = &[
    "QueueArn",
    "VisibilityTimeout",
    "DelaySeconds",
    "ReceiveMessageWaitTimeSeconds",
    "MessageRetentionPeriod",
    "MaximumMessageSize",
    "CreatedTimestamp",
    "LastModifiedTimestamp",
    "RedrivePolicy",
    "ApproximateNumberOfMessages",
    "ApproximateNumberOfMessagesNotVisible",
    "ApproximateNumberOfMessagesDelayed",
    "Policy",
    "SqsManagedSseEnabled",
    "KmsMasterKeyId",
    "KmsDataKeyReusePeriodSeconds",
    "RedriveAllowPolicy",
    "FifoQueue",
    "ContentBasedDeduplication",
    "DeduplicationScope",
    "FifoThroughputLimit",
];

/// Writes the tags of a queue back (the whole map is replaced) with the other fields that changed.
async fn patch(
    ctx: &Ctx<'_>,
    topic: &str,
    tags: &BTreeMap<String, String>,
    changes: &Changes,
) -> Result<(), AwsError> {
    if tags.len() > MAX_TAGS {
        return Err(AwsError::invalid_parameter(
            "A queue can have up to 20 tags.",
        ));
    }
    let mut body = Map::new();
    body.insert("tags".into(), json!(tags));
    if let Some(ttl) = changes.ttl {
        body.insert("ttl".into(), json!(ttl));
    }
    if let Some(bytes) = changes.max_bytes {
        body.insert("maxMessageBytes".into(), json!(bytes));
    }
    if let Some(deliveries) = changes.max_deliveries {
        body.insert("maxDeliveries".into(), json!(deliveries));
    }
    ctx.backend()
        .ok_json(
            ctx.caller,
            Method::PATCH,
            &format!("/topics/{}", url_encode(topic)),
            Some(&Value::Object(body)),
        )
        .await?;
    ctx.state.forget_settings(topic);
    Ok(())
}

/// `SetQueueAttributes`
pub async fn set_queue_attributes(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let changes = parse_changes(&text_map(params, "Attributes"))?;
    if changes.fifo.is_some() {
        return Err(AwsError::invalid_attribute_name(
            "The attribute FifoQueue can only be set when the queue is created.",
        ));
    }
    if !queue.fifo && changes.content_dedup.is_some() {
        return Err(AwsError::invalid_attribute_name(
            "Unknown Attribute ContentBasedDeduplication.",
        ));
    }
    let current = Settings::of(&read_queue(ctx, &queue).await?);
    let mut tags = current.tags.clone();
    tags.extend(changes.tags.clone());
    check_fifo_delay(&queue, &tags)?;
    let dead = match &changes.dead_letter {
        Some(name) => Some(check_dead_letter(ctx, &queue, name).await?),
        None => None,
    };
    if let Some(dead) = &dead {
        tags.insert(TAG_DEAD_LETTER.into(), dead.name());
    }
    patch(ctx, &queue.topic, &tags, &changes).await?;
    if let Some(dead) = &dead {
        link_dead_letter(ctx, dead, &queue).await?;
        if let Some(previous) = current.dead_letter.filter(|p| *p != dead.name()) {
            unlink_dead_letter(ctx, &previous, &queue).await;
        }
    }
    Ok(json!({}))
}

/// `TagQueue`
pub async fn tag_queue(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let new = text_map(params, "Tags");
    if new.is_empty() {
        return Err(AwsError::missing_parameter("Tags"));
    }
    check_tags(&new)?;
    let mut tags = Settings::of(&read_queue(ctx, &queue).await?).tags;
    tags.extend(new);
    patch(ctx, &queue.topic, &tags, &Changes::default()).await?;
    Ok(json!({}))
}

/// `UntagQueue`
pub async fn untag_queue(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let keys = texts(params, "TagKeys");
    if keys.is_empty() {
        return Err(AwsError::missing_parameter("TagKeys"));
    }
    let mut tags = Settings::of(&read_queue(ctx, &queue).await?).tags;
    for key in keys.iter().filter(|k| !k.starts_with(OWN_TAG)) {
        tags.remove(key);
    }
    patch(ctx, &queue.topic, &tags, &Changes::default()).await?;
    Ok(json!({}))
}

/// `ListQueueTags`
pub async fn list_queue_tags(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let queue = queue_of(params)?;
    let tags = Settings::of(&read_queue(ctx, &queue).await?).user_tags();
    Ok(json!({"Tags": tags}))
}

#[cfg(test)]
mod tests;
