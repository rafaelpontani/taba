//! The topic actions of SNS (doc 39): create, delete, list, attributes and tags.
//!
//! A topic is a Taba distributor with the same name; the gateway keeps nothing of its own. A topic name that is also a
//! queue, a dead-letter queue or a name Taba does not accept is refused. The attributes SNS has and Taba cannot honor are
//! refused, never accepted and ignored; the two exceptions are `DisplayName` (only for SMS and e-mail, which the gateway does
//! not deliver) and `Policy` (Taba applies none; the SQS gateway does the same with the policy of a queue).

use axum::http::Method;
use serde_json::{Map, Value, json};

use super::Ctx;
use crate::gateway::aws::errors::AwsError;
use crate::gateway::aws::params::{required_text, text, text_map, texts, url_encode};
use crate::gateway::naming;

const MAX_TAGS: usize = 20;
const TAG_KEY_MAX: usize = 64;
const TAG_VALUE_MAX: usize = 128;
/// How many Taba pages a list reads looking for a page with a topic of SNS in it (queues come in the same list).
const MAX_PAGES: usize = 20;

/// The topic name inside a `TopicArn` parameter.
pub fn topic_of(params: &Value, parameter: &str) -> Result<String, AwsError> {
    let arn = text(params, parameter).ok_or_else(|| {
        AwsError::sns_invalid_parameter(format!(
            "Invalid parameter: {parameter} Reason: no value for required parameter"
        ))
    })?;
    naming::topic_in_arn(arn)
        .map(str::to_owned)
        .ok_or_else(|| AwsError::sns_invalid_parameter(format!("Invalid parameter: {parameter}")))
}

/// Reads a distributor. A topic that is a queue is not an SNS topic.
pub async fn read_distributor(ctx: &Ctx<'_>, name: &str) -> Result<Value, AwsError> {
    let value = ctx
        .backend()
        .ok_json_sns(
            ctx.caller,
            Method::GET,
            &format!("/topics/{}", url_encode(name)),
            None,
        )
        .await?;
    if value.get("type").and_then(Value::as_str) != Some("distributor") {
        return Err(AwsError::sns_not_found("Topic does not exist"));
    }
    Ok(value)
}

/// The prefixes a distributor copies to.
pub fn prefixes_of(topic: &Value) -> Vec<String> {
    topic
        .get("prefixes")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Checks tags the application chose against what a Taba tag can hold.
fn check_tags(tags: &[(String, String)]) -> Result<(), AwsError> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':');
    for (key, value) in tags {
        if key.is_empty() || key.len() > TAG_KEY_MAX || !key.chars().all(allowed) {
            return Err(AwsError::sns_invalid_parameter(format!(
                "The tag key `{key}` is not valid: 1 to {TAG_KEY_MAX} letters, digits, `_`, `-`, `.` or `:`."
            )));
        }
        if value.len() > TAG_VALUE_MAX || !value.chars().all(allowed) {
            return Err(AwsError::sns_invalid_parameter(format!(
                "The value of the tag `{key}` is not valid: up to {TAG_VALUE_MAX} letters, digits, `_`, `-`, `.` or `:`."
            )));
        }
    }
    Ok(())
}

/// The tags of a topic as the application sees them.
fn tags_of(topic: &Value) -> Vec<(String, String)> {
    topic
        .get("tags")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Checks the attributes of a topic: the ones that cannot be honored are refused.
fn check_attribute(name: &str, value: &str) -> Result<(), AwsError> {
    match name {
        // Not used by the gateway: there is no SMS or e-mail delivery, and Taba applies no policy.
        "DisplayName" | "Policy" => Ok(()),
        "FifoTopic" if value == "false" => Ok(()),
        "FifoTopic" | "ContentBasedDeduplication" => Err(AwsError::sns_invalid_parameter(
            "FIFO topics are not supported by this gateway: the distributor of Taba is not FIFO.",
        )),
        other => Err(AwsError::sns_invalid_parameter(format!(
            "Invalid parameter: Attributes Reason: the attribute {other} is not supported by this gateway."
        ))),
    }
}

/// `CreateTopic`. It is idempotent, as in SNS: the name of an existing topic answers with its ARN.
pub async fn create_topic(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let name = naming::parse_topic(required_text(params, "Name")?)?;
    for (attribute, value) in text_map(params, "Attributes") {
        check_attribute(&attribute, &value)?;
    }
    let tags = text_map(params, "Tags");
    check_tags(&tags)?;
    if tags.len() > MAX_TAGS {
        return Err(AwsError::sns_invalid_parameter(
            "A topic can have up to 20 tags.",
        ));
    }
    let tag_map: Map<String, Value> = tags.into_iter().map(|(k, v)| (k, json!(v))).collect();
    let body = json!({"name": name, "type": "distributor", "tags": tag_map});
    let reply = ctx
        .backend()
        .json(ctx.caller, Method::POST, "/topics", Some(&body))
        .await?;
    if !reply.is_success() {
        if reply.error_code().as_deref() != Some("topic.already-exists") {
            return Err(reply.sns_error());
        }
        // The topic exists: SNS answers with its ARN, unless the name is taken by a queue.
        let existing = ctx
            .backend()
            .ok_json_sns(
                ctx.caller,
                Method::GET,
                &format!("/topics/{}", url_encode(&name)),
                None,
            )
            .await?;
        if existing.get("type").and_then(Value::as_str) != Some("distributor") {
            return Err(AwsError::sns_invalid_parameter(format!(
                "The name {name} is already used by a queue: a topic and a queue of the gateway cannot share a name."
            )));
        }
    }
    Ok(json!({"TopicArn": naming::topic_arn(&ctx.region, &name)}))
}

/// `DeleteTopic`. It is idempotent: a topic that does not exist, or is not a topic, is already gone for SNS.
pub async fn delete_topic(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let name = topic_of(params, "TopicArn")?;
    match read_distributor(ctx, &name).await {
        Ok(_) => {}
        Err(error) if error.query_code == "NotFound" => return Ok(json!({})),
        Err(error) => return Err(error),
    }
    ctx.backend()
        .ok_json_sns(
            ctx.caller,
            Method::DELETE,
            &format!("/topics/{}", url_encode(&name)),
            None,
        )
        .await?;
    Ok(json!({}))
}

/// The topics a page of Taba topics holds, in the shape of the answer.
fn topics_of(page: &Value, region: &str) -> Vec<Value> {
    page.get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("distributor"))
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(|name| json!({"TopicArn": naming::topic_arn(region, name)}))
        .collect()
}

/// The pages of the Taba topics, read until one has a distributor in it or the list ends: the queues come in the same
/// list, and a page of SNS must not be empty only because the Taba page was full of queues. Returns the distributors
/// (as the items of the Taba page) and the cursor of the next page.
pub async fn distributor_page(
    ctx: &Ctx<'_>,
    token: Option<&str>,
) -> Result<(Vec<Value>, Option<String>), AwsError> {
    let mut cursor = token.map(str::to_owned);
    for _ in 0..MAX_PAGES {
        let mut path = String::from("/topics?limit=100");
        if let Some(c) = &cursor {
            path.push_str("&cursor=");
            path.push_str(&url_encode(c));
        }
        let page = ctx
            .backend()
            .ok_json_sns(ctx.caller, Method::GET, &path, None)
            .await
            .map_err(|error| {
                if error.query_code == "NotFound" {
                    AwsError::sns_invalid_parameter("Invalid parameter: NextToken")
                } else {
                    error
                }
            })?;
        let items: Vec<Value> = page
            .get("items")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter(|item| item.get("type").and_then(Value::as_str) == Some("distributor"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let following = page
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if !items.is_empty() || following.is_none() {
            return Ok((items, following));
        }
        cursor = following;
    }
    Ok((Vec::new(), cursor))
}

/// `ListTopics`. The `NextToken` is the cursor of the Taba list.
pub async fn list_topics(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (items, next) = distributor_page(ctx, text(params, "NextToken")).await?;
    let topics = topics_of(&json!({"items": items}), &ctx.region);
    let mut result = Map::new();
    result.insert("Topics".into(), Value::Array(topics));
    if let Some(token) = next {
        result.insert("NextToken".into(), json!(token));
    }
    Ok(Value::Object(result))
}

/// `GetTopicAttributes`. The subscriptions are the prefixes of the distributor, all of them confirmed.
pub async fn get_topic_attributes(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let name = topic_of(params, "TopicArn")?;
    let topic = read_distributor(ctx, &name).await?;
    let subscriptions = prefixes_of(&topic).len();
    Ok(json!({"Attributes": {
        "TopicArn": naming::topic_arn(&ctx.region, &name),
        "Owner": naming::ACCOUNT,
        "DisplayName": "",
        "SubscriptionsConfirmed": subscriptions.to_string(),
        "SubscriptionsPending": "0",
        "SubscriptionsDeleted": "0",
    }}))
}

/// `SetTopicAttributes`
pub async fn set_topic_attributes(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let name = topic_of(params, "TopicArn")?;
    let attribute = required_text(params, "AttributeName")?;
    check_attribute(attribute, text(params, "AttributeValue").unwrap_or(""))?;
    read_distributor(ctx, &name).await?;
    Ok(json!({}))
}

/// The topic a `ResourceArn` names.
async fn resource_topic(ctx: &Ctx<'_>, params: &Value) -> Result<(String, Value), AwsError> {
    let name = topic_of(params, "ResourceArn")?;
    let topic = read_distributor(ctx, &name).await?;
    Ok((name, topic))
}

async fn replace_tags(
    ctx: &Ctx<'_>,
    name: &str,
    tags: Vec<(String, String)>,
) -> Result<(), AwsError> {
    if tags.len() > MAX_TAGS {
        return Err(AwsError::sns_invalid_parameter(
            "A topic can have up to 20 tags.",
        ));
    }
    let map: Map<String, Value> = tags.into_iter().map(|(k, v)| (k, json!(v))).collect();
    ctx.backend()
        .ok_json_sns(
            ctx.caller,
            Method::PATCH,
            &format!("/topics/{}", url_encode(name)),
            Some(&json!({"tags": map})),
        )
        .await?;
    Ok(())
}

/// `TagResource`: the new tags are added to the ones the topic has, and a key that repeats gets the new value.
pub async fn tag_resource(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (name, topic) = resource_topic(ctx, params).await?;
    let new = text_map(params, "Tags");
    check_tags(&new)?;
    let mut tags = tags_of(&topic);
    for (key, value) in new {
        match tags.iter_mut().find(|(k, _)| *k == key) {
            Some(found) => found.1 = value,
            None => tags.push((key, value)),
        }
    }
    replace_tags(ctx, &name, tags).await?;
    Ok(json!({}))
}

/// `UntagResource`
pub async fn untag_resource(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (name, topic) = resource_topic(ctx, params).await?;
    let keys = texts(params, "TagKeys");
    let tags: Vec<(String, String)> = tags_of(&topic)
        .into_iter()
        .filter(|(k, _)| !keys.contains(k))
        .collect();
    replace_tags(ctx, &name, tags).await?;
    Ok(json!({}))
}

/// `ListTagsForResource`
pub async fn list_tags_for_resource(ctx: &Ctx<'_>, params: &Value) -> Result<Value, AwsError> {
    let (_, topic) = resource_topic(ctx, params).await?;
    let tags: Vec<Value> = tags_of(&topic)
        .into_iter()
        .map(|(key, value)| json!({"Key": key, "Value": value}))
        .collect();
    Ok(json!({"Tags": tags}))
}

#[cfg(test)]
mod tests;
