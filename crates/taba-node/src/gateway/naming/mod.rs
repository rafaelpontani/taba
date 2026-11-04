//! Names between the AWS side and the Taba side (doc 39): the queue name in the `QueueUrl`, the account and the ARN.
//!
//! A standard SQS queue name (1 to 80 letters, digits, hyphens and underscores) is already a valid Taba topic name, so
//! the two sides share it as it is and a native client reaches the same queue by the same name. A FIFO queue name ends in
//! `.fifo`, which a topic name cannot hold: the topic is the name **without** the suffix and has `fifo: true`, so
//! `orders.fifo` and the FIFO topic `orders` are the same queue (and a standard `orders` cannot exist beside it, because a
//! topic name is unique). The name of a dead-letter queue (`-dead-letter` at the end) belongs to Taba and is refused here.

use super::aws::errors::AwsError;
use super::envelope::sha256_hex;

/// The account id in the URLs and ARNs: the gateway has no accounts, so it is a fixed, well-formed one.
pub const ACCOUNT: &str = "000000000000";
/// The region of the ARNs when the request does not say one.
pub const DEFAULT_REGION: &str = "us-east-1";
/// The longest SQS queue name.
pub const MAX_NAME: usize = 80;
const DEAD_LETTER_SUFFIX: &str = "-dead-letter";
const FIFO_SUFFIX: &str = ".fifo";

/// A queue as the two sides name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    /// The Taba topic.
    pub topic: String,
    /// The topic is FIFO, and the SQS name ends in `.fifo`.
    pub fifo: bool,
}

impl Queue {
    /// The queue of a Taba topic, when its name is also a valid SQS name.
    pub fn of_topic(topic: &str, fifo: bool) -> Option<Self> {
        let queue = Self {
            topic: topic.to_owned(),
            fifo,
        };
        parse(&queue.name()).ok()
    }

    /// The SQS name.
    pub fn name(&self) -> String {
        if self.fifo {
            format!("{}{FIFO_SUFFIX}", self.topic)
        } else {
            self.topic.clone()
        }
    }

    /// `http://host:9324/000000000000/orders`
    pub fn url(&self, base: &str) -> String {
        format!("{}/{ACCOUNT}/{}", base.trim_end_matches('/'), self.name())
    }

    /// `arn:aws:sqs:us-east-1:000000000000:orders`
    pub fn arn(&self, region: &str) -> String {
        queue_arn(region, &self.name())
    }

    /// The Taba dead-letter queue of this one.
    pub fn dead_letter_topic(&self) -> String {
        format!("{}{DEAD_LETTER_SUFFIX}", self.topic)
    }
}

fn bad_name(fifo: bool) -> AwsError {
    AwsError::invalid_parameter(if fifo {
        "Can only include alphanumeric characters, hyphens, or underscores and must end with .fifo. 1 to 80 in length."
    } else {
        "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length."
    })
}

/// Checks an SQS queue name and returns the queue.
pub fn parse(name: &str) -> Result<Queue, AwsError> {
    let (topic, fifo) = match name.strip_suffix(FIFO_SUFFIX) {
        Some(topic) => (topic, true),
        None => (name, false),
    };
    if name.is_empty()
        || name.len() > MAX_NAME
        || topic.is_empty()
        || !topic
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(bad_name(fifo));
    }
    if topic.ends_with(DEAD_LETTER_SUFFIX) {
        return Err(AwsError::invalid_parameter(
            "The suffix -dead-letter is reserved for the dead-letter queues of Taba.",
        ));
    }
    Ok(Queue {
        topic: topic.to_owned(),
        fifo,
    })
}

/// The queue name inside a `QueueUrl`: the last path segment, after the account. A wrong URL is `QueueDoesNotExist`.
pub fn name_in_url(url: &str) -> Result<String, AwsError> {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let path = match path.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
        None => path.trim_start_matches('/'),
    };
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    match (segments.next(), segments.next(), segments.next()) {
        (Some(_account), Some(name), None) => Ok(name.to_owned()),
        _ => Err(AwsError::queue_does_not_exist()),
    }
}

/// `arn:aws:sqs:us-east-1:000000000000:orders`
pub fn queue_arn(region: &str, name: &str) -> String {
    format!("arn:aws:sqs:{region}:{ACCOUNT}:{name}")
}

/// The queue name an ARN points to (its last field).
pub fn name_in_arn(arn: &str) -> Option<&str> {
    let mut fields = arn.split(':');
    match (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) {
        (Some("arn"), Some(_), Some("sqs"), Some(_), Some(_), Some(name), None)
            if !name.is_empty() =>
        {
            Some(name)
        }
        _ => None,
    }
}

// ---- SNS topics ---------------------------------------------------------------------------------------------------------

/// The longest SNS topic name Taba takes (a topic name has up to 100 characters; SNS itself allows 256).
pub const MAX_TOPIC_NAME: usize = 100;

/// Checks an SNS topic name. A distributor is a Taba topic, so the name is a topic name: letters, digits, hyphens and
/// underscores, up to 100, and not ending in `-dead-letter`. A `.fifo` topic is refused: the distributor is not FIFO.
pub fn parse_topic(name: &str) -> Result<String, AwsError> {
    if name.ends_with(FIFO_SUFFIX) {
        return Err(AwsError::sns_invalid_parameter(
            "FIFO topics are not supported by this gateway: the distributor of Taba is not FIFO.",
        ));
    }
    if name.is_empty()
        || name.len() > MAX_TOPIC_NAME
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AwsError::sns_invalid_parameter(
            "Topic names must be made up of only uppercase and lowercase ASCII letters, numbers, underscores, and hyphens, and must be between 1 and 100 characters long.",
        ));
    }
    if name.ends_with(DEAD_LETTER_SUFFIX) {
        return Err(AwsError::sns_invalid_parameter(
            "The suffix -dead-letter is reserved for the dead-letter queues of Taba.",
        ));
    }
    Ok(name.to_owned())
}

/// `arn:aws:sns:us-east-1:000000000000:orders`
pub fn topic_arn(region: &str, name: &str) -> String {
    format!("arn:aws:sns:{region}:{ACCOUNT}:{name}")
}

/// The topic name an SNS topic ARN points to. Anything else (another service, a subscription ARN) is not a topic ARN.
pub fn topic_in_arn(arn: &str) -> Option<&str> {
    let mut fields = arn.split(':');
    match (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) {
        (Some("arn"), Some(_), Some("sns"), Some(_), Some(_), Some(name), None)
            if !name.is_empty() =>
        {
            Some(name)
        }
        _ => None,
    }
}

/// The ARN of a subscription: the topic ARN and the name of the queue. The gateway keeps no state, so the subscription is
/// what the topic and the queue say, and its ARN says both.
pub fn subscription_arn(region: &str, topic: &str, queue_name: &str) -> String {
    format!("{}:{queue_name}", topic_arn(region, topic))
}

/// The topic and the queue name inside a subscription ARN.
pub fn parse_subscription_arn(arn: &str) -> Option<(&str, &str)> {
    let mut fields = arn.split(':');
    match (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) {
        (Some("arn"), Some(_), Some("sns"), Some(_), Some(_), Some(topic), Some(queue), None)
            if !topic.is_empty() && !queue.is_empty() =>
        {
            Some((topic, queue))
        }
        _ => None,
    }
}

// ---- the group and the deduplication id --------------------------------------------------------------------------------

/// Whether the text is a valid Taba `groupId` or `deduplicationId`: 1 to 128 of `A-Za-z0-9_-.:`.
fn is_taba_id(text: &str) -> bool {
    (1..=128).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

/// Whether the text is a valid SQS `MessageGroupId` or `MessageDeduplicationId`: 1 to 128 letters, digits and the
/// punctuation ``!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~``.
pub fn is_aws_id(text: &str) -> bool {
    (1..=128).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || (c.is_ascii_punctuation()))
}

/// The id Taba gets for an SQS group or deduplication id. One that Taba accepts goes as it is; one with other punctuation
/// is replaced by `h.` and 40 hexadecimal digits of its SHA-256 (the same text always gives the same id), and the
/// original travels in the envelope of the message so that the receiver gets it back. The second value is that original.
pub fn taba_id(original: &str) -> (String, Option<String>) {
    if is_taba_id(original) {
        (original.to_owned(), None)
    } else {
        let hash = sha256_hex(original.as_bytes());
        (format!("h.{}", &hash[..40]), Some(original.to_owned()))
    }
}

#[cfg(test)]
mod tests;
