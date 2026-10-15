//! Rules the topic registry reads from the configuration, and the requests it receives.

use std::collections::BTreeMap;

use crate::config::Config;

/// The configuration values the topics depend on (doc 10).
#[derive(Debug, Clone)]
pub struct TopicRules {
    /// `topics.dead-letter.suffix`.
    pub dead_letter_suffix: String,
    /// `message.max-bytes`: the default of `maxMessageBytes`.
    pub max_message_bytes_default: u64,
    /// `message.hard-limit`: without it, there is no ceiling for `maxMessageBytes`.
    pub hard_limit: Option<u64>,
    /// `retention.ttl.default`, in seconds.
    pub ttl_default: u64,
    /// `retention.max-deliveries.default`.
    pub max_deliveries_default: u64,
    /// `topics.volatile.idle-timeout.default`, in seconds.
    pub idle_timeout_default: u64,
    /// `topics.volatile.max-count`: the ceiling of volatile queues for `autoCreate`; `None`, no ceiling.
    pub volatile_max_count: Option<u32>,
    /// A FIFO queue requires a `replication.factor` of at least 2; in development it does not apply.
    pub fifo_needs_replication: bool,
    /// `api.pagination.default-limit` e `max-limit`.
    pub default_limit: u32,
    pub max_limit: u32,
}

impl TopicRules {
    pub fn from_config(config: &Config) -> Self {
        Self {
            dead_letter_suffix: config.topics.dead_letter_suffix.clone(),
            max_message_bytes_default: config.messages.max_bytes,
            hard_limit: config.messages.hard_limit,
            ttl_default: config.retention.ttl_default.as_secs(),
            max_deliveries_default: u64::from(config.retention.max_deliveries_default),
            idle_timeout_default: config.topics.volatile_idle_timeout_default.as_secs(),
            volatile_max_count: config.topics.volatile_max_count,
            fifo_needs_replication: !config.is_development() && config.write.replication_factor < 2,
            default_limit: config.pagination.default_limit,
            max_limit: config.pagination.max_limit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Queue,
    Distributor,
}

/// `POST /topics`. The numbers arrive as `i64` so the range `400` also applies
/// to negative values.
#[derive(Debug, Clone)]
pub struct CreateTopic {
    pub name: String,
    pub kind: Kind,
    pub tags: Option<BTreeMap<String, String>>,
    pub prefixes: Option<Vec<String>>,
    pub volatile: Option<bool>,
    pub idle_timeout: Option<i64>,
    pub fifo: Option<bool>,
    pub max_message_bytes: Option<i64>,
    pub ttl: Option<i64>,
    pub max_deliveries: Option<i64>,
}

/// A `PATCH` field: absent (does not touch), `null` (goes back to the default) or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Patch<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

/// `PATCH /topics/{topic}`: partial update.
#[derive(Debug, Clone, Default)]
pub struct UpdateTopic {
    /// Immutable fields (`name`, `id`, `type`, `volatile`, `fifo`) that came in the body.
    pub immutable_present: Vec<String>,
    pub tags: Option<BTreeMap<String, String>>,
    pub prefixes: Option<Vec<String>>,
    pub idle_timeout: Option<i64>,
    pub max_message_bytes: Patch<i64>,
    pub ttl: Patch<i64>,
    pub max_deliveries: Patch<i64>,
}

/// The query parameters of `GET /topics`, as they came, for the `engine` to validate.
#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    pub limit: Option<String>,
    pub cursor: Option<String>,
    pub sort: Option<String>,
    pub name: Option<String>,
    /// Only the topics whose name starts with any of these prefixes (the token's); empty, all.
    pub scope: Vec<String>,
}
