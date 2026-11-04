//! The subscription actions of SNS (doc 39).
//!
//! A subscription is a **prefix** in the list of the distributor: the node copies each publication to the queues whose
//! name starts with one of its prefixes. So the subscription of the queue `billing` is the prefix `billing`, it is born when
//! `Subscribe` adds it and gone when `Unsubscribe` removes it, and it counts only for the publications after that. The
//! gateway keeps no state: the ARN of a subscription is the ARN of the topic and the name of the queue, and everything else
//! is read from the distributor.
//!
//! What a distributor cannot do is refused, never accepted and ignored: a destination that is not a queue, the raw delivery,
//! the filter policy, the delivery policy and the redrive policy of the subscription.

use std::collections::HashMap;

use axum::http::Method;
use serde_json::{Map, Value, json};

use super::Ctx;
use super::topics::{distributor_page, prefixes_of, read_distributor, topic_of};
use crate::gateway::aws::errors::AwsError;
use crate::gateway::aws::params::{required_text, text, text_map, url_encode};
use crate::gateway::naming::{self, Queue};

/// The most prefixes a distributor holds.
pub const MAX_SUBSCRIPTIONS: usize = 20;

fn refused(what: impl Into<String>) -> AwsError {
    AwsError::sns_invalid_parameter(format!("Invalid parameter: {}", what.into()))
}

/// The protocols SNS has that the distributor cannot serve, for a clear message.
const OTHER_PROTOCOLS: &[&str] = &[
    "http",
    "https",
    "email",
    "email-json",
    "sms",
    "lambda",
    "application",
    "firehose",
];

fn check_protocol(protocol: &str) -> Result<(), AwsError> {
    if protocol == "sqs" {
        return Ok(());
    }
    if OTHER_PROTOCOLS.contains(&protocol) {
        return Err(refused(format!(
            "Protocol {protocol} is not supported by this gateway: Taba copies the messages of a topic to queues only."
        )));
    }
    Err(refused(format!("Protocol {protocol}")))
}

/// The attributes of a subscription the gateway can honor: only `RawMessageDelivery=false`, which is how it always delivers.
fn check_attribute(name: &str, value: &str) -> Result<(), AwsError> {
    match name {
        "RawMessageDelivery" if value.eq_ignore_ascii_case("false") => Ok(()),
        "RawMessageDelivery" if value.eq_ignore_ascii_case("true") => Err(refused(
            "RawMessageDelivery=true is not supported by this gateway: the queue always receives the SNS notification, with the message in its Message field.",
        )),
        "RawMessageDelivery" => Err(refused("RawMessageDelivery must be true or false")),
        "FilterPolicy" | "FilterPolicyScope" => Err(refused(
            "FilterPolicy is not supported by this gateway: the distributor of Taba does not filter, and accepting the policy would deliver everything.",
        )),
        other => Err(refused(format!(
            "the subscription attribute {other} is not supported by this gateway."
        ))),
    }
}

/// The queue an `Endpoint` ARN names.
fn endpoint_queue(arn: &str) -> Result<Queue, AwsError> {
    let name = naming::name_in_arn(arn).ok_or_else(|| {
        refused("Endpoint: the endpoint of an SQS subscription is the ARN of a queue")
    })?;
    naming::parse(name).map_err(|error| refused(format!("Endpoint: {}", error.message)))
}

/// Whether the topic with the name of a prefix is a FIFO queue; a prefix with no queue of that name is a standard one.
async fn prefix_is_fifo(ctx: &Ctx<'_>, prefix: &str, seen: &mut HashMap<String, bool>) -> bool {
    if let Some(fifo) = seen.get(prefix) {
        return *fifo;
    }
    let fifo = match ctx
        .backend()
        .json(
            ctx.caller,
            Method::GET,
            &format!("/topics/{}", url_encode(prefix)),
            None,
        )
        .await
    {
        Ok(reply) if reply.is_success() => {
            reply.json().get("fifo").and_then(Value::as_bool) == Some(true)
        }
        _ => false,
    };
    seen.insert(prefix.to_owned(), fifo);
    fifo
}

/// The subscriptions of a distributor, as the answer lists them.
async fn subscriptions_of(
    ctx: &Ctx<'_>,
    topic: &str,
    prefixes: &[String],
    seen: &mut HashMap<String, bool>,
) -> Vec<Value> {
    let mut out = Vec::with_capacity(prefixes.len());
    for prefix in prefixes {
        let queue = Queue {
            fifo: prefix_is_fifo(ctx, prefix, seen).await,
            topic: prefix.clone(),
        };
        out.push(json!({
            "SubscriptionArn": naming::subscription_arn(&ctx.region, topic, &queue.name()),
            "Owner": naming::ACCOUNT,
            "Protocol": "sqs",
            "Endpoint": queue.arn(&ctx.region),
            "TopicArn": naming::topic_arn(&ctx.region, topic),
        }));
    }
    out
}

async fn set_prefixes(ctx: &Ctx<'_>, topic: &str, prefixes: &[String]) -> Result<(), AwsError> {
    ctx.backend()
        .ok_json_sns(
            ctx.caller,
            Method::PATCH,
            &format!("/topics/{}", url_encode(topic)),
            Some(&json!({"prefixes": prefixes})),
        )
        .await?;
    Ok(())
}

/// `Subscribe`. It is idempotent, as in SNS: subscribing a queue that already is one answers with the same ARN.
pub async fn subscribe(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let topic_name = topic_of(params, "TopicArn")?;
    check_protocol(required_text(params, "Protocol")?)?;
    for (name, value) in text_map(params, "Attributes") {
        check_attribute(&name, &value)?;
    }
    let queue = endpoint_queue(
        text(params, "Endpoint")
            .ok_or_else(|| refused("Endpoint: no value for required parameter"))?,
    )?;
    let topic = read_distributor(ctx, &topic_name).await?;
    // The queue must exist: a prefix with no queue would be a subscription that delivers to nobody, or to a queue made later
    // whose name only starts the same way.
    let found = ctx
        .backend()
        .ok_json_sns(
            ctx.caller,
            Method::GET,
            &format!("/topics/{}", url_encode(&queue.topic)),
            None,
        )
        .await
        .ok()
        .filter(|queue_topic| {
            queue_topic.get("type").and_then(Value::as_str) == Some("queue")
                && queue_topic.get("deadLetter").and_then(Value::as_bool) != Some(true)
                && queue_topic
                    .get("fifo")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    == queue.fifo
        });
    if found.is_none() {
        return Err(refused(format!(
            "Endpoint: the queue {} does not exist",
            queue.name()
        )));
    }
    let mut prefixes = prefixes_of(&topic);
    if !prefixes.contains(&queue.topic) {
        if prefixes.len() >= MAX_SUBSCRIPTIONS {
            return Err(AwsError::sns_subscription_limit(format!(
                "A topic of this gateway has up to {MAX_SUBSCRIPTIONS} subscriptions: a prefix common to the names of several queues covers all of them."
            )));
        }
        prefixes.push(queue.topic.clone());
        set_prefixes(ctx, &topic_name, &prefixes).await?;
    }
    Ok(
        json!({"SubscriptionArn": naming::subscription_arn(&ctx.region, &topic_name, &queue.name())}),
    )
}

/// The topic and the queue a `SubscriptionArn` names.
fn subscription_of(params: &Value) -> Result<(String, Queue), AwsError> {
    let arn = required_text(params, "SubscriptionArn")?;
    let (topic, queue_name) =
        naming::parse_subscription_arn(arn).ok_or_else(|| refused("SubscriptionArn"))?;
    let queue = naming::parse(queue_name).map_err(|_| refused("SubscriptionArn"))?;
    Ok((topic.to_owned(), queue))
}

/// `Unsubscribe`. It is idempotent: a queue that is not subscribed any more is already unsubscribed.
pub async fn unsubscribe(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (topic_name, queue) = subscription_of(params)?;
    let topic = read_distributor(ctx, &topic_name).await?;
    let mut prefixes = prefixes_of(&topic);
    if let Some(at) = prefixes.iter().position(|p| *p == queue.topic) {
        prefixes.remove(at);
        set_prefixes(ctx, &topic_name, &prefixes).await?;
    }
    Ok(json!({}))
}

/// `ListSubscriptionsByTopic`
pub async fn list_subscriptions_by_topic(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let topic_name = topic_of(params, "TopicArn")?;
    let topic = read_distributor(ctx, &topic_name).await?;
    let mut seen = HashMap::new();
    let subscriptions = subscriptions_of(ctx, &topic_name, &prefixes_of(&topic), &mut seen).await;
    Ok(json!({"Subscriptions": subscriptions}))
}

/// `ListSubscriptions`: the subscriptions of the topics a page of the Taba list holds. The `NextToken` is its cursor.
pub async fn list_subscriptions(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (topics, next) = distributor_page(ctx, text(params, "NextToken")).await?;
    let mut seen = HashMap::new();
    let mut subscriptions = Vec::new();
    for topic in &topics {
        let Some(name) = topic.get("name").and_then(Value::as_str) else {
            continue;
        };
        subscriptions.extend(subscriptions_of(ctx, name, &prefixes_of(topic), &mut seen).await);
    }
    let mut result = Map::new();
    result.insert("Subscriptions".into(), Value::Array(subscriptions));
    if let Some(token) = next {
        result.insert("NextToken".into(), json!(token));
    }
    Ok(Value::Object(result))
}

/// The subscription a `SubscriptionArn` names, if the distributor has it.
async fn existing(ctx: &Ctx<'_>, params: &Value) -> Result<(String, Queue), AwsError> {
    let (topic_name, queue) = subscription_of(params)?;
    let topic = read_distributor(ctx, &topic_name).await?;
    if !prefixes_of(&topic).contains(&queue.topic) {
        return Err(AwsError::sns_not_found("Subscription does not exist"));
    }
    Ok((topic_name, queue))
}

/// `GetSubscriptionAttributes`. The delivery is never raw, and a subscription is never pending.
pub async fn get_subscription_attributes(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (topic_name, queue) = existing(ctx, params).await?;
    Ok(json!({"Attributes": {
        "SubscriptionArn": naming::subscription_arn(&ctx.region, &topic_name, &queue.name()),
        "TopicArn": naming::topic_arn(&ctx.region, &topic_name),
        "Protocol": "sqs",
        "Endpoint": queue.arn(&ctx.region),
        "Owner": naming::ACCOUNT,
        "RawMessageDelivery": "false",
        "ConfirmationWasAuthenticated": "true",
        "PendingConfirmation": "false",
    }}))
}

/// `SetSubscriptionAttributes`: only the value the subscription already has is accepted.
pub async fn set_subscription_attributes(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let name = required_text(params, "AttributeName")?;
    check_attribute(name, text(params, "AttributeValue").unwrap_or(""))?;
    existing(ctx, params).await?;
    Ok(json!({}))
}

#[cfg(test)]
mod tests;
