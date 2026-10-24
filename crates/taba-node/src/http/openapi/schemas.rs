//! The DTOs of the published APIs as OpenAPI schemas, and the error responses the routes share.
//!
//! A DTO that the handlers serialize from a `struct` (`ErrorResponse`, `HealthResponse`, `Message`...) derives
//! `ToSchema` where it is defined, so the document follows the code. The ones the handlers build with `json!` have
//! their twin here; `tests/openapi.rs` checks that real responses match them.

use std::collections::BTreeMap;

use serde::Serialize;
use utoipa::{IntoResponses, ToSchema};

use crate::core::error::ErrorResponse;

// ---------------------------------------------------------------------------------------------------------------
// Errors shared by the routes
// ---------------------------------------------------------------------------------------------------------------

/// The refusals of the authentication layer, on every route that takes a token.
#[derive(IntoResponses)]
#[allow(dead_code)]
pub enum AuthErrors {
    /// `auth.unauthorized`: the token is missing, malformed, unknown, revoked or expired. Only with authentication on.
    #[response(status = 401)]
    Unauthorized(ErrorResponse),
    /// `auth.forbidden` (the token lacks the permission; `required` says which) or `auth.out-of-scope` (the topic is
    /// outside the token's prefixes).
    #[response(status = 403)]
    Forbidden(ErrorResponse),
}

/// `429 rate-limit.exceeded`, with `Retry-After`.
#[derive(IntoResponses)]
#[allow(dead_code)]
pub enum RateLimited {
    /// The rate limit of the route's category was exceeded. `Retry-After` says in how many seconds to try again.
    #[response(status = 429)]
    TooManyRequests(ErrorResponse),
}

/// The routes that need the cluster to agree on something.
#[derive(IntoResponses)]
#[allow(dead_code)]
pub enum ClusterUnavailable {
    /// `cluster.quorum-unavailable`, `cluster.registration-failed`, `cluster.commit-timeout`, `cluster.node-syncing`,
    /// `cluster.node-draining` or `node.failed`: nothing was accepted (or, on `cluster.commit-timeout`, the change
    /// completes in the background).
    #[response(status = 503)]
    ServiceUnavailable(ErrorResponse),
}

// ---------------------------------------------------------------------------------------------------------------
// Topics
// ---------------------------------------------------------------------------------------------------------------

/// The kind of a topic.
#[derive(ToSchema, Serialize)]
#[schema(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum TopicType {
    /// Holds messages until a consumer confirms them.
    Queue,
    /// Copies each message to the queues whose name starts with one of its prefixes.
    Distributor,
}

/// The limits in force for a queue: its own value, or the node default where it has none.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TopicEffective {
    /// Bytes a message may have.
    pub max_message_bytes: u64,
    /// Seconds a message lives.
    pub ttl: u64,
    /// Deliveries without an ack until the message goes to the dead-letter queue.
    pub max_deliveries: u64,
}

/// A topic: a queue (with its dead-letter queue) or a distributor.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Topic {
    /// GUID v7.
    pub id: String,
    pub name: String,
    #[schema(rename = "type")]
    pub kind: TopicType,
    /// Telemetry tags; they do not change the behaviour.
    pub tags: BTreeMap<String, String>,
    #[schema(format = DateTime)]
    pub created_at: String,
    /// Distributor only: the destination prefixes.
    pub prefixes: Option<Vec<String>>,
    /// Queue only.
    pub volatile: Option<bool>,
    /// Volatile queue only: seconds without a consumer until the queue is deleted.
    pub idle_timeout: Option<u64>,
    /// Queue only. Immutable.
    pub fifo: Option<bool>,
    /// Queue only: `null` means the node default (`message.max-bytes`).
    pub max_message_bytes: Option<u64>,
    /// Queue only: `null` means the node default (14 days).
    pub ttl: Option<u64>,
    /// Queue only: `null` means the node default (10).
    pub max_deliveries: Option<u64>,
    /// Queue only: the values that apply.
    pub effective: Option<TopicEffective>,
    /// Only on a dead-letter queue, where it is `true`. A dead-letter queue is read-only and follows its source.
    pub dead_letter: Option<bool>,
    /// Only on a dead-letter queue: the name of the queue it belongs to.
    pub source: Option<String>,
}

/// A page of topics.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TopicPage {
    pub items: Vec<Topic>,
    /// Absent on the last page; pass it as `cursor` to read the next one.
    pub next_cursor: Option<String>,
}

/// Body of `POST /topics`. A field that does not apply to the type is `400 validation.not-applicable`.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TopicCreateRequest {
    /// `^[A-Za-z0-9_-]{1,100}$`. A name ending in `-dead-letter` is reserved.
    pub name: String,
    #[schema(rename = "type")]
    pub kind: TopicType,
    /// Up to 20 tags: key of 1 to 64 and value of 0 to 128 characters of `A-Za-z0-9_-.:`.
    pub tags: Option<BTreeMap<String, String>>,
    /// Distributor only: up to 20 prefixes of 1 to 100 characters (`A-Za-z0-9_-`).
    pub prefixes: Option<Vec<String>>,
    /// Queue only. Default `false`.
    pub volatile: Option<bool>,
    /// Volatile queue only: 60 to 604800 seconds. Default 600.
    pub idle_timeout: Option<u64>,
    /// Queue only. Immutable. Needs `replication.factor` of at least 2.
    pub fifo: Option<bool>,
    /// Queue only: 1 up to `message.hard-limit`.
    pub max_message_bytes: Option<u64>,
    /// Queue only: 60 to 31536000 seconds.
    pub ttl: Option<u64>,
    /// Queue only: 1 to 1000.
    pub max_deliveries: Option<u64>,
}

/// Body of `PATCH /topics/{topic}`: only what is sent changes. `tags` replaces the whole map, and `null` in
/// `maxMessageBytes`, `ttl` or `maxDeliveries` goes back to the node default. `name`, `id`, `type`, `volatile` and
/// `fifo` are immutable: sending them, even with the current value, is `400 validation.immutable-field`.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TopicUpdateRequest {
    pub tags: Option<BTreeMap<String, String>>,
    /// Distributor only.
    pub prefixes: Option<Vec<String>>,
    /// Volatile queue only.
    pub idle_timeout: Option<u64>,
    pub max_message_bytes: Option<u64>,
    pub ttl: Option<u64>,
    pub max_deliveries: Option<u64>,
}

// ---------------------------------------------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------------------------------------------

/// Answer of a publication to a queue.
#[derive(ToSchema, Serialize)]
#[allow(dead_code)]
pub struct PublishResponse {
    /// The message `id`; on a duplicate, the one of the original.
    pub id: String,
    /// FIFO queues only: `true` if the publication repeated one inside the deduplication window (nothing was
    /// created; the status is then `200`).
    pub duplicate: Option<bool>,
}

/// Why a copy of a message to one destination queue failed.
#[derive(ToSchema, Serialize)]
#[allow(dead_code)]
pub struct DeliveryError {
    /// A code of the catalogue.
    pub code: String,
    pub message: String,
}

/// What happened to one destination queue of a distributor: `id` (a copy was created) or `error`.
#[derive(ToSchema, Serialize)]
#[allow(dead_code)]
pub struct DistributorDelivery {
    pub topic: String,
    pub id: Option<String>,
    pub error: Option<DeliveryError>,
}

/// Answer of a publication to a distributor. Zero matches is not an error.
#[derive(ToSchema, Serialize)]
#[allow(dead_code)]
pub struct DistributorPublishResponse {
    /// Queues whose name matched a prefix.
    pub matched: u64,
    pub deliveries: Vec<DistributorDelivery>,
}

/// What a publication answers with `200`: a duplicate on a FIFO queue, or the result of a distributor.
#[derive(ToSchema, Serialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum PublishOutcome {
    Duplicate(PublishResponse),
    Distributor(DistributorPublishResponse),
}

/// Body of `PUT .../visibility`.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct VisibilityRequest {
    /// The new invisibility, in seconds **from now** (not an increment). `0` releases the message at once. At most
    /// `delivery.visibility-timeout.max`.
    pub visibility_timeout: u64,
}

/// Answer of `PUT .../visibility`.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct VisibilityResponse {
    pub id: String,
    /// The end of the invisibility; with `0`, the current instant.
    #[schema(format = DateTime)]
    pub invisible_until: String,
}

/// What each node removed in a prune.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PruneNode {
    pub node_id: String,
    pub removed: u64,
}

/// Answer of `DELETE /topics/{topic}/messages`.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PruneResult {
    /// Identifies the operation; it makes the call idempotent on each node.
    pub prune_id: String,
    /// Messages removed, each counted once (the node that owns a message counts it, the replicas do not).
    pub removed: u64,
    pub nodes: Vec<PruneNode>,
}

// ---------------------------------------------------------------------------------------------------------------
// Tokens and who the caller is
// ---------------------------------------------------------------------------------------------------------------

/// What a token may do.
#[derive(ToSchema)]
#[schema(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum Permission {
    /// Publish to the topics within the token's prefixes.
    Publish,
    /// Consume, confirm and change the invisibility of messages.
    Consume,
    /// Create topics (and queues by `autoCreate`).
    Create,
    /// Change, remove and prune the topics within the prefixes.
    Manage,
    /// Manage tokens, and read the cluster and the nodes.
    Admin,
}

/// An access token, without its secret.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Token {
    pub id: String,
    pub name: String,
    pub permissions: Vec<Permission>,
    /// Empty means every topic.
    pub topic_prefixes: Vec<String>,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
}

/// The creation of a token: the `Token` plus its secret, which shows up **only here**.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TokenCreated {
    pub id: String,
    pub name: String,
    pub permissions: Vec<Permission>,
    pub topic_prefixes: Vec<String>,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
    /// `tb_<id>.<secret>`. It is never returned again.
    pub token: String,
}

/// A page of tokens.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TokenPage {
    pub items: Vec<Token>,
    pub next_cursor: Option<String>,
}

/// Body of `POST /tokens`.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TokenCreateRequest {
    pub name: String,
    /// At least one. An `admin` token is only created by the root token.
    pub permissions: Vec<Permission>,
    /// Empty or absent means every topic.
    pub topic_prefixes: Option<Vec<String>>,
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
}

/// Body of `PATCH /tokens/{id}`. Permissions and prefixes are immutable.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TokenUpdateRequest {
    pub name: Option<String>,
    /// A date in the future; `null` removes the deadline.
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
}

/// Who the caller is and what it can do.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Me {
    pub auth_enabled: bool,
    /// `true` for the root token and, with authentication off, for everyone.
    pub root: bool,
    /// `null` for the root token.
    pub id: Option<String>,
    /// `root`, `open` (authentication off) or the token's name.
    pub name: String,
    pub permissions: Vec<Permission>,
    pub topic_prefixes: Vec<String>,
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
}

// ---------------------------------------------------------------------------------------------------------------
// The cluster and the nodes
// ---------------------------------------------------------------------------------------------------------------

/// A node that did not answer a consolidated read.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct MissingNode {
    pub node_id: String,
    /// `timeout`, `unreachable` or `error`.
    pub reason: String,
}

/// One member of the cluster. There are no addresses.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Node {
    pub node_id: String,
    /// `true` for the node that answered.
    #[schema(rename = "self")]
    pub is_self: bool,
    pub reachable: bool,
    /// `active`, `syncing`, `joining`, `returning`, `failed` or `drained`.
    pub state: Option<String>,
    /// `active`, `draining` or `drained`.
    pub drain_state: Option<String>,
    pub version: Option<String>,
    pub member_state: Option<String>,
}

/// The nodes of the cluster.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct NodePage {
    #[schema(format = DateTime)]
    pub collected_at: String,
    pub members_version: u64,
    /// `true` if some node did not answer in time (the numbers are those of the ones that did).
    pub partial: bool,
    pub missing_nodes: Vec<MissingNode>,
    pub items: Vec<Node>,
}

/// What a node stores.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct NodeStorage {
    pub memory_used_bytes: u64,
    pub memory_limit_bytes: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
    pub disk_free_bytes: Option<u64>,
}

/// What a node's registry says about its load.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct NodeRuntime {
    pub active_requests: f64,
    /// Requests refused with `429` since the start.
    pub rate_limit_rejected: f64,
    /// The worst `p95` of the round trip of the node's channels; `null` with no observation.
    pub channel_rtt_p95_seconds: Option<f64>,
}

/// The state of one node.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct NodeStatus {
    pub node_id: String,
    pub version: String,
    /// `joining`, `active`, `syncing`, `returning` or `failed`.
    pub state: String,
    /// `active`, `draining` or `drained`.
    pub drain_state: String,
    #[schema(format = DateTime)]
    pub started_at: String,
    #[schema(format = DateTime)]
    pub collected_at: String,
    #[schema(format = DateTime)]
    pub excluded_since: String,
    pub storage: NodeStorage,
    pub runtime: NodeRuntime,
    /// The raw signals the conditions come from.
    #[schema(value_type = Object)]
    pub signals: serde_json::Value,
}

/// A condition of the cluster.
#[derive(ToSchema)]
#[allow(dead_code)]
pub struct Condition {
    /// `quorum-lost`, `write-quorum-at-risk`, `member-unreachable`, `under-replicated`, `node-frozen`...
    pub condition: String,
    /// `warning` or `critical`.
    pub severity: String,
}

/// The cluster in numbers.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ClusterSummary {
    /// How many members are in each state.
    pub members: BTreeMap<String, u64>,
    pub members_version: u64,
    pub quorum_available: bool,
    pub syncing_blocks: u64,
    /// Nodes that are active and out of drain.
    pub serving_nodes: u64,
    /// `true` if every node that answered is in drain.
    pub stopped: bool,
    /// `true` if the stop was `POST /cluster/stop` (it left a stop list in S3).
    pub planned_stop: bool,
    #[schema(format = DateTime)]
    pub stopped_at: Option<String>,
    #[schema(format = DateTime)]
    pub excluded_cutoff: Option<String>,
}

/// One node in the cluster status.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ClusterStatusNode {
    pub node_id: String,
    pub reachable: bool,
    /// `null` if the node did not answer.
    pub status: Option<NodeStatus>,
}

/// The state and the conditions of the cluster.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ClusterStatus {
    /// `cluster` (asks every node) or `node` (only the one that answers).
    pub scope: String,
    #[schema(format = DateTime)]
    pub collected_at: String,
    pub partial: bool,
    pub missing_nodes: Vec<MissingNode>,
    /// The condition of highest severity; `ok` if none.
    pub worst_condition: String,
    pub conditions: Vec<Condition>,
    pub summary: ClusterSummary,
    pub nodes: Vec<ClusterStatusNode>,
}

/// What the stop of the whole cluster did.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct StopReport {
    pub stopped: bool,
    /// `true` if the stop list was written to S3.
    pub stop_list: bool,
    pub warnings: Vec<String>,
    /// One entry per node: `nodeId`, `drained` and, if it failed, `error`.
    #[schema(value_type = Vec<Object>)]
    pub nodes: Vec<serde_json::Value>,
}

/// What forcing the restoration did.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct RestoreForce {
    pub was_restoring: bool,
    pub status: String,
    pub silent_nodes: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------------------------------------------

/// Latency percentiles, in seconds; `null` with no observation.
#[derive(ToSchema, Serialize)]
#[allow(dead_code)]
pub struct Latency {
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
}

/// What a distributor received and copied.
#[derive(ToSchema, Serialize)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct DistributorStats {
    pub received: u64,
    pub copies: u64,
    pub copy_errors: u64,
}

/// A condition found on a topic.
#[derive(ToSchema, Serialize)]
#[allow(dead_code)]
pub struct TopicCondition {
    /// `backlog`, `stalled`, `idle`, `noisy-group`, `dead-letter-growing`...
    pub condition: String,
    /// `warning` or `critical`.
    pub severity: String,
}

/// The numbers of a topic. With `fields`, only the requested ones come (`id` and `name` always do).
#[derive(ToSchema, Serialize)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TopicStats {
    pub id: String,
    pub name: String,
    #[schema(rename = "type")]
    pub kind: Option<TopicType>,
    pub tags: Option<BTreeMap<String, String>>,
    #[schema(format = DateTime)]
    pub created_at: Option<String>,
    pub volatile: Option<bool>,
    pub idle_timeout: Option<u64>,
    pub dead_letter: Option<bool>,
    pub source: Option<String>,
    pub published: Option<u64>,
    pub published_bytes: Option<u64>,
    pub publish_errors: Option<u64>,
    /// Messages per second.
    pub publish_rate: Option<f64>,
    pub publish_latency: Option<Latency>,
    pub consumed: Option<u64>,
    pub acked: Option<u64>,
    pub consume_rate: Option<f64>,
    pub ack_rate: Option<f64>,
    pub consume_latency: Option<Latency>,
    pub visible_messages: Option<u64>,
    pub invisible_messages: Option<u64>,
    /// Messages in the queue, visible and invisible.
    pub depth: Option<u64>,
    #[schema(format = DateTime)]
    pub oldest_message_at: Option<String>,
    /// Seconds since `oldestMessageAt`.
    pub oldest_age: Option<f64>,
    pub bytes: Option<u64>,
    pub memory_messages: Option<u64>,
    pub disk_messages: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub disk_bytes: Option<u64>,
    pub memory_share: Option<f64>,
    pub groups: Option<u64>,
    pub noisy_share: Option<f64>,
    /// Only on a distributor.
    pub distributor: Option<DistributorStats>,
    /// `ok`, `warning` or `critical`.
    pub worst_condition: Option<String>,
    pub conditions: Option<Vec<TopicCondition>>,
    /// Only in the client API: some node did not answer, so the numbers are those of the ones that did.
    pub partial: Option<bool>,
    #[schema(format = DateTime)]
    pub collected_at: Option<String>,
    /// Only on the detail of one topic: up to `stats.topics.top-groups` (default 10) groups, largest first.
    pub top_groups: Option<Vec<GroupStats>>,
}

/// A page of topic numbers (or, with `groupBy`, of rows per tag value).
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TopicStatsPage {
    pub items: Vec<TopicStatsRow>,
    pub next_cursor: Option<String>,
    pub partial: Option<bool>,
    pub missing_nodes: Option<Vec<MissingNode>>,
}

/// The numbers of one group (`groupId`) of a topic.
#[derive(ToSchema, Serialize)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GroupStats {
    pub group_id: Option<String>,
    /// `true` for the row that sums the groups past the ceiling.
    pub overflow: Option<bool>,
    pub messages: u64,
    pub visible_messages: u64,
    pub invisible_messages: u64,
    /// The fraction of the topic's messages that are in the group.
    pub share: f64,
    #[schema(format = DateTime)]
    pub oldest_message_at: Option<String>,
    pub oldest_age: Option<f64>,
}

/// A page of group numbers.
#[derive(ToSchema)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GroupStatsPage {
    pub items: Vec<GroupStats>,
    pub next_cursor: Option<String>,
    pub partial: Option<bool>,
    pub missing_nodes: Option<Vec<MissingNode>>,
}

/// The bytes of a message, of any media type: the request body of a publication.
#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
pub struct MessageBytes(Vec<u8>);

/// With `groupBy`, one row per value of the tag: the sums of the topics that have that value.
#[derive(ToSchema, Serialize)]
#[schema(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TopicStatsGroup {
    /// The tag the rows are grouped by.
    pub tag: String,
    /// The value of the tag in this row.
    pub value: String,
    /// How many topics have that value.
    pub topics: u64,
    pub published: Option<u64>,
    pub published_bytes: Option<u64>,
    pub publish_errors: Option<u64>,
    pub publish_rate: Option<f64>,
    pub consumed: Option<u64>,
    pub acked: Option<u64>,
    pub consume_rate: Option<f64>,
    pub ack_rate: Option<f64>,
    pub visible_messages: Option<u64>,
    pub invisible_messages: Option<u64>,
    pub depth: Option<u64>,
    #[schema(format = DateTime)]
    pub oldest_message_at: Option<String>,
    pub oldest_age: Option<f64>,
    pub bytes: Option<u64>,
    pub memory_messages: Option<u64>,
    pub disk_messages: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub disk_bytes: Option<u64>,
    pub groups: Option<u64>,
    pub worst_condition: Option<String>,
    pub conditions: Option<Vec<TopicCondition>>,
    pub partial: Option<bool>,
    #[schema(format = DateTime)]
    pub collected_at: Option<String>,
}

/// An item of a page of topic numbers: a topic, or, with `groupBy`, a row per tag value.
#[derive(ToSchema, Serialize)]
#[serde(untagged)]
#[allow(dead_code, clippy::large_enum_variant)] // only for the schema: never built
pub enum TopicStatsRow {
    Topic(TopicStats),
    Group(TopicStatsGroup),
}
