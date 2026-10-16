//! The routes the screens call (doc 34, section 3), typed, over `Client::call`: each `ApiCall` becomes a
//! `taba_client::Call` and each reply is read into a small struct. Nothing is added to `taba-client`.

use std::time::Duration;
use taba_client::json::{self, Fields};

use taba_client::Call;

/// How long a flush or a stop may take before the call gives up.
const LONG_ACTION: Duration = Duration::from_secs(180);

/// The columns of the topic list that come from the statistics (the rest comes from `GET /topics`).
const STATS_FIELDS: &str = "name,depth,publishRate,consumeRate,oldestAge,worstCondition,conditions";

/// What a call is for; it comes back with the reply so `update` knows what to do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiCall {
    Me,
    Nodes,
    ClusterStatus,
    NodeMetrics(String),
    NodeBackup(String),
    DrainStatus(String),
    DrainStart(String),
    DrainCancel(String),
    FlushNode(String),
    FlushCluster,
    StopCluster,
    /// One page of `GET /tokens`, from a cursor.
    Tokens(Option<String>),
    /// `POST /tokens` with the JSON body of the form.
    TokenCreate(String),
    /// `PATCH /tokens/{id}` with the changed fields.
    TokenUpdate(String, String),
    TokenRevoke(String),
    /// One page of `GET /topics`, from a cursor.
    Topics(Option<String>),
    /// One page of `GET /stats/topics` (the numbers of the list), from a cursor.
    TopicStats(Option<String>),
    /// `POST /topics` with the JSON body of the form.
    TopicCreate(String),
    /// `PATCH /topics/{name}` with the changed fields.
    TopicUpdate(String, String),
    TopicDelete(String),
    /// `GET /stats/topics/{name}`: one reading of a queue for its charts.
    TopicDetail(String),
    /// `POST /topics/{name}/messages`.
    Publish(PublishSpec),
    /// `GET /topics/{name}/messages`: reads up to [`RECEIVE_MAX`] messages and holds them for the given seconds (the form offers [`RECEIVE_HOLD_SECS`]).
    Receive(String, u32, u32),
    /// `DELETE /topics/{name}/messages/{id}` with the receipt: topic, id, receipt.
    Ack(String, String, String),
    /// `PUT /topics/{name}/messages/{id}/visibility` with `0`: gives the message back to the queue now.
    /// Changes the time a held message stays invisible; `0` gives it back to the queue now.
    Visibility(String, String, String, u32),
    /// `DELETE /topics/{name}/messages`: empties the queue on every node.
    Prune(String),
}

/// How many messages one reading takes.
pub const RECEIVE_MAX: u32 = 10;
/// How long the messages of a reading stay invisible to the others, in seconds.
pub const RECEIVE_HOLD_SECS: u32 = 30;

/// What the publish form asks to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishSpec {
    pub topic: String,
    pub body: Vec<u8>,
    pub content_type: String,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
}

impl ApiCall {
    pub fn to_call(&self) -> Call {
        let mut call = self.route();
        if matches!(
            self,
            ApiCall::FlushNode(_) | ApiCall::FlushCluster | ApiCall::StopCluster
        ) {
            // A flush or a stop waits for the nodes to finish, which can take longer than a normal call.
            call.timeout = Some(LONG_ACTION);
        }
        call
    }

    fn route(&self) -> Call {
        let node = |id: &str, tail: &str| format!("/nodes/{}{tail}", encode_segment(id));
        match self {
            ApiCall::Me => Call::new("GET", "/auth/me"),
            ApiCall::Nodes => Call::new("GET", "/nodes"),
            ApiCall::ClusterStatus => Call::new("GET", "/cluster/status"),
            ApiCall::NodeMetrics(id) => {
                let mut call = Call::new("GET", node(id, "/metrics"));
                call.headers
                    .push(("Accept".to_owned(), "text/plain".to_owned()));
                call
            }
            ApiCall::NodeBackup(id) => Call::new("GET", node(id, "/backup")),
            ApiCall::DrainStatus(id) => Call::new("GET", node(id, "/drain")),
            ApiCall::DrainStart(id) => Call::new("POST", node(id, "/drain")),
            ApiCall::DrainCancel(id) => Call::new("DELETE", node(id, "/drain")),
            ApiCall::FlushNode(id) => Call::new("POST", node(id, "/backup/flush")),
            ApiCall::FlushCluster => Call::new("POST", "/cluster/backup/flush"),
            ApiCall::StopCluster => Call::new("POST", "/cluster/stop"),
            ApiCall::Tokens(cursor) => {
                let mut call = Call::new("GET", "/tokens").query("limit", 200);
                if let Some(cursor) = cursor {
                    call = call.query("cursor", cursor);
                }
                call
            }
            ApiCall::TokenCreate(body) => Call::new("POST", "/tokens").json_body(body.clone()),
            ApiCall::TokenUpdate(id, body) => {
                Call::new("PATCH", format!("/tokens/{}", encode_segment(id)))
                    .json_body(body.clone())
            }
            ApiCall::TokenRevoke(id) => {
                Call::new("DELETE", format!("/tokens/{}", encode_segment(id)))
            }
            ApiCall::Topics(cursor) => {
                let mut call = Call::new("GET", "/topics").query("limit", 200);
                if let Some(cursor) = cursor {
                    call = call.query("cursor", cursor);
                }
                call
            }
            ApiCall::TopicStats(cursor) => {
                let mut call = Call::new("GET", "/stats/topics")
                    .query("limit", 200)
                    .query("fields", STATS_FIELDS);
                if let Some(cursor) = cursor {
                    call = call.query("cursor", cursor);
                }
                call
            }
            ApiCall::TopicCreate(body) => Call::new("POST", "/topics").json_body(body.clone()),
            ApiCall::TopicUpdate(name, body) => {
                Call::new("PATCH", format!("/topics/{}", encode_segment(name)))
                    .json_body(body.clone())
            }
            ApiCall::TopicDelete(name) => {
                Call::new("DELETE", format!("/topics/{}", encode_segment(name)))
            }
            ApiCall::TopicDetail(name) => {
                Call::new("GET", format!("/stats/topics/{}", encode_segment(name)))
            }
            ApiCall::Publish(spec) => {
                let mut call = Call::new(
                    "POST",
                    format!("/topics/{}/messages", encode_segment(&spec.topic)),
                );
                call.body = spec.body.clone();
                call.content_type = Some(spec.content_type.clone());
                if let Some(group) = &spec.group_id {
                    call = call.query("groupId", group);
                }
                if let Some(dedup) = &spec.deduplication_id {
                    call = call.query("deduplicationId", dedup);
                }
                call
            }
            ApiCall::Receive(name, hold, max) => {
                Call::new("GET", format!("/topics/{}/messages", encode_segment(name)))
                    .query("maxMessages", max)
                    .query("visibilityTimeout", hold)
            }
            ApiCall::Ack(topic, id, receipt) => Call::new(
                "DELETE",
                format!(
                    "/topics/{}/messages/{}",
                    encode_segment(topic),
                    encode_segment(id)
                ),
            )
            .query("receiptToken", receipt),
            ApiCall::Visibility(topic, id, receipt, secs) => Call::new(
                "PUT",
                format!(
                    "/topics/{}/messages/{}/visibility",
                    encode_segment(topic),
                    encode_segment(id)
                ),
            )
            .query("receiptToken", receipt)
            .json_body(format!("{{\"visibilityTimeout\":{secs}}}")),
            ApiCall::Prune(name) => Call::new(
                "DELETE",
                format!("/topics/{}/messages", encode_segment(name)),
            ),
        }
    }

    /// Calls that change something: the screen never repeats them by itself.
    pub fn is_action(&self) -> bool {
        matches!(
            self,
            ApiCall::DrainStart(_)
                | ApiCall::DrainCancel(_)
                | ApiCall::FlushNode(_)
                | ApiCall::FlushCluster
                | ApiCall::StopCluster
                | ApiCall::TokenCreate(_)
                | ApiCall::TokenUpdate(..)
                | ApiCall::TokenRevoke(_)
                | ApiCall::TopicCreate(_)
                | ApiCall::TopicUpdate(..)
                | ApiCall::TopicDelete(_)
                | ApiCall::Publish(_)
                | ApiCall::Receive(..)
                | ApiCall::Ack(..)
                | ApiCall::Visibility(..)
                | ApiCall::Prune(_)
        )
    }
}

/// A node id is a GUID; anything else is percent-encoded so it cannot change the path.
fn encode_segment(id: &str) -> String {
    id.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn fields(text: &str) -> Result<Fields<'_>, String> {
    Fields::parse(text).map_err(|e| e.to_string())
}

fn num(f: &Fields<'_>, name: &str) -> Option<f64> {
    f.raw(name)?.parse().ok()
}

fn whole(f: &Fields<'_>, name: &str) -> Option<u64> {
    num(f, name).filter(|n| *n >= 0.0).map(|n| n as u64)
}

/// A count the server gives either as a number or as a map of entries (the channels of the node): the entries are counted.
fn count_of(f: &Fields<'_>, name: &str) -> u64 {
    match f.raw(name) {
        Some(raw) if raw.trim_start().starts_with('{') => {
            json::object_fields(raw).map_or(0, |entries| entries.len() as u64)
        }
        Some(_) => whole(f, name).unwrap_or(0),
        None => 0,
    }
}

fn items<'a>(f: &Fields<'a>, name: &str) -> Vec<&'a str> {
    f.raw(name)
        .and_then(|raw| json::array_items(raw).ok())
        .unwrap_or_default()
}

fn texts(f: &Fields<'_>, name: &str) -> Vec<String> {
    items(f, name)
        .into_iter()
        .filter_map(|s| json::decode_string(s).ok())
        .collect()
}

/// Who the token is (`GET /auth/me`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Me {
    pub auth_enabled: bool,
    pub root: bool,
    pub name: Option<String>,
    pub permissions: Vec<String>,
}

impl Me {
    /// Root, or no authentication at all: the cluster-wide reads are allowed.
    pub fn is_root(&self) -> bool {
        self.root || !self.auth_enabled
    }

    /// Root, no authentication, or a token with the `admin` permission: the cluster and the tokens are open to it.
    pub fn is_admin(&self) -> bool {
        self.is_root() || self.permissions.iter().any(|p| p == "admin")
    }

    /// `manage` includes `create`, and `admin` includes `manage` and `create` (the same rule as the server).
    pub fn can(&self, permission: &str) -> bool {
        let has = |name: &str| self.permissions.iter().any(|p| p == name);
        self.is_root()
            || has(permission)
            || (matches!(permission, "create" | "publish" | "consume")
                && (has("manage") || has("admin")))
            || (permission == "manage" && has("admin"))
    }
}

pub fn parse_me(text: &str) -> Result<Me, String> {
    let f = fields(text)?;
    Ok(Me {
        auth_enabled: f.boolean("authEnabled").unwrap_or(true),
        root: f.boolean("root").unwrap_or(false),
        name: f.string("name"),
        permissions: texts(&f, "permissions"),
    })
}

/// A node that did not answer in a consolidated read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub node_id: String,
    pub reason: String,
}

fn parse_missing(f: &Fields<'_>) -> Vec<Missing> {
    items(f, "missingNodes")
        .into_iter()
        .filter_map(|raw| {
            let m = Fields::parse(raw).ok()?;
            Some(Missing {
                node_id: m.string("nodeId")?,
                reason: m.string("reason").unwrap_or_default(),
            })
        })
        .collect()
}

/// An item of `GET /nodes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub node_id: String,
    pub is_self: bool,
    pub reachable: bool,
    pub state: String,
    pub drain_state: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Nodes {
    pub items: Vec<NodeInfo>,
    pub partial: bool,
    pub missing: Vec<Missing>,
}

pub fn parse_nodes(text: &str) -> Result<Nodes, String> {
    let f = fields(text)?;
    let list = items(&f, "items")
        .into_iter()
        .filter_map(|raw| {
            let n = Fields::parse(raw).ok()?;
            Some(NodeInfo {
                node_id: n.string("nodeId")?,
                is_self: n.boolean("self").unwrap_or(false),
                reachable: n.boolean("reachable").unwrap_or(true),
                state: n.string("state").unwrap_or_default(),
                drain_state: n.string("drainState").unwrap_or_default(),
                version: n.string("version").unwrap_or_default(),
            })
        })
        .collect();
    Ok(Nodes {
        items: list,
        partial: f.boolean("partial").unwrap_or(false),
        missing: parse_missing(&f),
    })
}

/// `warning` or `critical`; the order is the severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Severity {
    #[default]
    Ok,
    Warning,
    Critical,
}

impl Severity {
    pub fn parse(text: &str) -> Severity {
        match text {
            "critical" => Severity::Critical,
            "warning" => Severity::Warning,
            _ => Severity::Ok,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub name: String,
    pub severity: Severity,
}

/// One member of `GET /cluster/status`: the node, and its `NodeStatus` when it answered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeStatus {
    pub node_id: String,
    pub reachable: bool,
    pub cluster_name: Option<String>,
    pub version: String,
    pub mode: String,
    pub state: String,
    pub drain_state: String,
    pub started_at: Option<String>,
    pub memory_used: u64,
    pub memory_limit: u64,
    pub memory_messages: u64,
    pub disk_messages: u64,
    pub disk_free: u64,
    pub quorum: bool,
    pub channels: u64,
    pub syncing_blocks: u64,
    pub below_replication: u64,
    pub below_quorum: u64,
    pub frozen_nodes: u64,
    pub active_requests: f64,
    pub rate_limited: f64,
    pub rtt_p95: Option<f64>,
}

impl NodeStatus {
    /// Memory in use, from 0 to 1 (0 without a limit).
    pub fn memory_ratio(&self) -> f64 {
        if self.memory_limit == 0 {
            0.0
        } else {
            (self.memory_used as f64 / self.memory_limit as f64).clamp(0.0, 1.0)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClusterStatus {
    pub collected_at: String,
    pub partial: bool,
    pub missing: Vec<Missing>,
    pub worst: String,
    pub conditions: Vec<Condition>,
    pub members: Vec<(String, u64)>,
    pub members_version: u64,
    pub quorum: bool,
    pub syncing_blocks: u64,
    /// Every node that answered is in drain: the cluster is stopped (a planned stop, or all drained by hand).
    pub stopped: bool,
    /// The S3 stop list exists: the stop was `POST /cluster/stop`.
    pub planned_stop: bool,
    pub stopped_at: Option<String>,
    pub nodes: Vec<NodeStatus>,
}

fn parse_node_status(node_id: &str, reachable: bool, raw: Option<&str>) -> NodeStatus {
    let mut node = NodeStatus {
        node_id: node_id.to_owned(),
        reachable,
        ..NodeStatus::default()
    };
    let Some(f) = raw.and_then(|r| Fields::parse(r).ok()) else {
        return node;
    };
    node.cluster_name = f.string("clusterName");
    node.version = f.string("version").unwrap_or_default();
    node.mode = f.string("mode").unwrap_or_default();
    node.state = f.string("state").unwrap_or_default();
    node.drain_state = f.string("drainState").unwrap_or_default();
    node.started_at = f.string("startedAt");
    if let Some(s) = f.raw("storage").and_then(|r| Fields::parse(r).ok()) {
        node.memory_used = whole(&s, "memoryUsedBytes").unwrap_or(0);
        node.memory_limit = whole(&s, "memoryLimitBytes").unwrap_or(0);
        node.memory_messages = whole(&s, "memoryMessages").unwrap_or(0);
        node.disk_messages = whole(&s, "diskMessages").unwrap_or(0);
        node.disk_free = whole(&s, "diskFreeBytes").unwrap_or(0);
    }
    if let Some(s) = f.raw("signals").and_then(|r| Fields::parse(r).ok()) {
        node.quorum = s.boolean("quorumAvailable").unwrap_or(false);
        node.channels = count_of(&s, "channels");
        node.syncing_blocks = whole(&s, "syncingBlocks").unwrap_or(0);
        node.below_replication = whole(&s, "blocksBelowReplication").unwrap_or(0);
        node.below_quorum = whole(&s, "blocksBelowQuorum").unwrap_or(0);
        node.frozen_nodes = whole(&s, "frozenNodes").unwrap_or(0);
    }
    if let Some(r) = f.raw("runtime").and_then(|r| Fields::parse(r).ok()) {
        node.active_requests = num(&r, "activeRequests").unwrap_or(0.0);
        node.rate_limited = num(&r, "rateLimitRejected").unwrap_or(0.0);
        node.rtt_p95 = num(&r, "channelRttP95Seconds");
    }
    node
}

pub fn parse_cluster_status(text: &str) -> Result<ClusterStatus, String> {
    let f = fields(text)?;
    let conditions = items(&f, "conditions")
        .into_iter()
        .filter_map(|raw| {
            let c = Fields::parse(raw).ok()?;
            Some(Condition {
                name: c.string("condition")?,
                severity: Severity::parse(&c.string("severity").unwrap_or_default()),
            })
        })
        .collect();
    let mut status = ClusterStatus {
        collected_at: f.string("collectedAt").unwrap_or_default(),
        partial: f.boolean("partial").unwrap_or(false),
        missing: parse_missing(&f),
        worst: f
            .string("worstCondition")
            .unwrap_or_else(|| "ok".to_owned()),
        conditions,
        ..ClusterStatus::default()
    };
    if let Some(s) = f.raw("summary").and_then(|r| Fields::parse(r).ok()) {
        status.members_version = whole(&s, "membersVersion").unwrap_or(0);
        status.quorum = s.boolean("quorumAvailable").unwrap_or(false);
        status.syncing_blocks = whole(&s, "syncingBlocks").unwrap_or(0);
        status.stopped = s.boolean("stopped").unwrap_or(false);
        status.planned_stop = s.boolean("plannedStop").unwrap_or(false);
        status.stopped_at = s.string("stoppedAt");
        if let Some(m) = s.raw("members").and_then(|r| json::object_fields(r).ok()) {
            status.members = m
                .into_iter()
                .map(|(state, n)| (state, n.parse().unwrap_or(0)))
                .collect();
        }
    }
    status.nodes = items(&f, "nodes")
        .into_iter()
        .filter_map(|raw| {
            let n = Fields::parse(raw).ok()?;
            let id = n.string("nodeId")?;
            let reachable = n.boolean("reachable").unwrap_or(false);
            Some(parse_node_status(&id, reachable, n.raw("status")))
        })
        .collect();
    Ok(status)
}

/// `GET`, `POST` and `DELETE /nodes/{id}/drain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drain {
    pub state: String,
    pub pending: u64,
}

impl Drain {
    pub fn is_active(&self) -> bool {
        self.state == "active"
    }

    pub fn is_done(&self) -> bool {
        self.state == "drained"
    }
}

pub fn parse_drain(text: &str) -> Result<Drain, String> {
    let f = fields(text)?;
    Ok(Drain {
        state: f.string("drainState").unwrap_or_default(),
        pending: whole(&f, "pendingMessages").unwrap_or(0),
    })
}

/// The S3 backup of a node (`GET /nodes/{id}/backup`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Backup {
    pub reachable: Option<bool>,
    pub pending: Option<u64>,
    pub last_success: Option<String>,
    pub segments: Option<u64>,
}

pub fn parse_backup(text: &str) -> Result<Backup, String> {
    let f = fields(text)?;
    Ok(Backup {
        reachable: f.boolean("s3Reachable").or_else(|| f.boolean("reachable")),
        pending: whole(&f, "outboxPending").or_else(|| whole(&f, "pending")),
        last_success: f
            .string("lastSuccessAt")
            .or_else(|| f.string("lastSuccess")),
        segments: whole(&f, "segments").or_else(|| whole(&f, "segmentsUploaded")),
    })
}

/// What one node reported on a backup flush.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlushNode {
    pub node_id: Option<String>,
    pub complete: bool,
    pub pending: bool,
    pub timed_out: bool,
    pub outbox_pending: u64,
    pub memory_remaining: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlushReport {
    pub complete: bool,
    pub nodes: Vec<FlushNode>,
}

fn flush_node(f: &Fields<'_>) -> FlushNode {
    FlushNode {
        node_id: f.string("nodeId"),
        complete: f.boolean("complete").unwrap_or(false),
        pending: f.boolean("pending").unwrap_or(false),
        timed_out: f.boolean("timedOut").unwrap_or(false),
        outbox_pending: whole(f, "outboxPending").unwrap_or(0),
        memory_remaining: whole(f, "memoryRemaining").unwrap_or(0),
        error: f.string("error"),
    }
}

/// The reply of a flush of one node (the report itself) or of the cluster (a `nodes` list).
pub fn parse_flush(text: &str) -> Result<FlushReport, String> {
    let f = fields(text)?;
    let complete = f.boolean("complete").unwrap_or(false);
    let nodes = if f.raw("nodes").is_some() {
        items(&f, "nodes")
            .into_iter()
            .filter_map(|raw| Fields::parse(raw).ok().map(|n| flush_node(&n)))
            .collect()
    } else {
        vec![flush_node(&f)]
    };
    Ok(FlushReport { complete, nodes })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StopReport {
    pub stopped: Vec<String>,
    pub stop_list: bool,
    pub warnings: Vec<String>,
    pub nodes: Vec<(String, bool)>,
}

pub fn parse_stop(text: &str) -> Result<StopReport, String> {
    let f = fields(text)?;
    let nodes = items(&f, "nodes")
        .into_iter()
        .filter_map(|raw| {
            let n = Fields::parse(raw).ok()?;
            Some((n.string("nodeId")?, n.boolean("drained").unwrap_or(false)))
        })
        .collect();
    Ok(StopReport {
        stopped: texts(&f, "stopped"),
        stop_list: f.boolean("stopList").unwrap_or(false),
        warnings: texts(&f, "warnings"),
        nodes,
    })
}

/// A token of `GET /tokens` (never with the secret).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Token {
    pub id: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub prefixes: Vec<String>,
    pub created_at: Option<String>,
    pub expires_at: Option<String>,
}

fn token_of(f: &Fields<'_>) -> Option<Token> {
    Some(Token {
        id: f.string("id")?,
        name: f.string("name").unwrap_or_default(),
        permissions: texts(f, "permissions"),
        prefixes: texts(f, "topicPrefixes"),
        created_at: f.string("createdAt"),
        expires_at: f.string("expiresAt"),
    })
}

/// A page of tokens and where the next one starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenPage {
    pub items: Vec<Token>,
    pub next_cursor: Option<String>,
}

pub fn parse_tokens(text: &str) -> Result<TokenPage, String> {
    let f = fields(text)?;
    let list = items(&f, "items")
        .into_iter()
        .filter_map(|raw| token_of(&Fields::parse(raw).ok()?))
        .collect();
    Ok(TokenPage {
        items: list,
        next_cursor: f.string("nextCursor"),
    })
}

/// The reply of `POST /tokens`: the token and its secret, which exists only here.
pub fn parse_token_created(text: &str) -> Result<(Token, String), String> {
    let f = fields(text)?;
    let token = token_of(&f).ok_or_else(|| "the reply has no token id".to_owned())?;
    let secret = f
        .string("token")
        .ok_or_else(|| "the reply has no secret".to_owned())?;
    Ok((token, secret))
}

/// A topic of `GET /topics` (doc 16): its definition, as the server keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Topic {
    pub id: String,
    pub name: String,
    /// `queue` or `distributor`.
    pub kind: String,
    pub fifo: bool,
    pub volatile: bool,
    pub tags: Vec<(String, String)>,
    pub prefixes: Vec<String>,
    pub idle_timeout: Option<u64>,
    /// What was set on the topic itself; `None` is the node's default.
    pub max_message_bytes: Option<u64>,
    pub ttl: Option<u64>,
    pub max_deliveries: Option<u64>,
    /// The values that apply (`effective`): the topic's own, or the node's default.
    pub effective_max_message_bytes: Option<u64>,
    pub effective_ttl: Option<u64>,
    pub effective_max_deliveries: Option<u64>,
    pub dead_letter: bool,
    /// The queue a dead-letter queue belongs to.
    pub source: Option<String>,
    pub created_at: Option<String>,
}

impl Topic {
    pub fn is_distributor(&self) -> bool {
        self.kind == "distributor"
    }
}

fn pairs_of(f: &Fields<'_>, name: &str) -> Vec<(String, String)> {
    f.raw(name)
        .and_then(|raw| json::object_fields(raw).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(key, value)| Some((key, json::decode_string(value).ok()?)))
        .collect()
}

fn topic_of(f: &Fields<'_>) -> Option<Topic> {
    let effective = f.raw("effective").and_then(|r| Fields::parse(r).ok());
    let own = |name: &str| effective.as_ref().and_then(|e| whole(e, name));
    Some(Topic {
        id: f.string("id").unwrap_or_default(),
        name: f.string("name")?,
        kind: f.string("type").unwrap_or_else(|| "queue".to_owned()),
        fifo: f.boolean("fifo").unwrap_or(false),
        volatile: f.boolean("volatile").unwrap_or(false),
        tags: pairs_of(f, "tags"),
        prefixes: texts(f, "prefixes"),
        idle_timeout: whole(f, "idleTimeout"),
        max_message_bytes: whole(f, "maxMessageBytes"),
        ttl: whole(f, "ttl"),
        max_deliveries: whole(f, "maxDeliveries"),
        effective_max_message_bytes: own("maxMessageBytes"),
        effective_ttl: own("ttl"),
        effective_max_deliveries: own("maxDeliveries"),
        dead_letter: f.boolean("deadLetter").unwrap_or(false),
        source: f.string("source"),
        created_at: f.string("createdAt"),
    })
}

/// A page of topics and where the next one starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TopicPage {
    pub items: Vec<Topic>,
    pub next_cursor: Option<String>,
}

pub fn parse_topics(text: &str) -> Result<TopicPage, String> {
    let f = fields(text)?;
    let list = items(&f, "items")
        .into_iter()
        .filter_map(|raw| topic_of(&Fields::parse(raw).ok()?))
        .collect();
    Ok(TopicPage {
        items: list,
        next_cursor: f.string("nextCursor"),
    })
}

/// The reply of `POST /topics` and `PATCH /topics/{name}`: the topic as it is now.
pub fn parse_topic(text: &str) -> Result<Topic, String> {
    topic_of(&fields(text)?).ok_or_else(|| "the reply has no topic name".to_owned())
}

/// The numbers of one topic in the list (`GET /stats/topics`). A distributor has no queue, so no numbers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TopicNumbers {
    pub depth: Option<u64>,
    pub publish_rate: Option<f64>,
    pub consume_rate: Option<f64>,
    /// Seconds; absent when the topic has no messages.
    pub oldest_age: Option<f64>,
    /// A condition name (`ok`, `backlog`, `stalled`...), not translated.
    pub worst: String,
    pub severity: Severity,
}

/// A page of statistics and where the next one starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TopicStatsPage {
    pub items: Vec<(String, TopicNumbers)>,
    pub next_cursor: Option<String>,
    pub partial: bool,
    pub missing: Vec<Missing>,
}

pub fn parse_topic_stats(text: &str) -> Result<TopicStatsPage, String> {
    let f = fields(text)?;
    let list = items(&f, "items")
        .into_iter()
        .filter_map(|raw| {
            let t = Fields::parse(raw).ok()?;
            let severity = items(&t, "conditions")
                .first()
                .and_then(|c| Fields::parse(c).ok())
                .and_then(|c| c.string("severity"))
                .map_or(Severity::Ok, |s| Severity::parse(&s));
            Some((
                t.string("name")?,
                TopicNumbers {
                    depth: whole(&t, "depth"),
                    publish_rate: num(&t, "publishRate"),
                    consume_rate: num(&t, "consumeRate"),
                    oldest_age: num(&t, "oldestAge"),
                    worst: t
                        .string("worstCondition")
                        .unwrap_or_else(|| "ok".to_owned()),
                    severity,
                },
            ))
        })
        .collect();
    Ok(TopicStatsPage {
        items: list,
        next_cursor: f.string("nextCursor"),
        partial: f.boolean("partial").unwrap_or(false),
        missing: parse_missing(&f),
    })
}

/// One reading of a queue for its charts (`GET /stats/topics/{name}`): the counts, the rates, the age of the oldest
/// message and that of the largest groups.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TopicSample {
    pub visible: u64,
    pub invisible: u64,
    /// Messages published, delivered and acknowledged (deleted) per second.
    pub publish_rate: f64,
    pub consume_rate: f64,
    pub ack_rate: f64,
    /// Seconds; absent when the queue has no messages.
    pub oldest_age: Option<f64>,
    /// The largest groups with messages, largest first: the group and the age of its oldest message.
    pub groups: Vec<(String, f64)>,
    pub partial: bool,
}

pub fn parse_topic_sample(text: &str) -> Result<TopicSample, String> {
    let f = fields(text)?;
    let groups = items(&f, "topGroups")
        .into_iter()
        .filter_map(|raw| {
            let g = Fields::parse(raw).ok()?;
            Some((g.string("groupId")?, num(&g, "oldestAge")?))
        })
        .collect();
    Ok(TopicSample {
        visible: whole(&f, "visibleMessages").unwrap_or(0),
        invisible: whole(&f, "invisibleMessages").unwrap_or(0),
        publish_rate: num(&f, "publishRate").unwrap_or(0.0),
        consume_rate: num(&f, "consumeRate").unwrap_or(0.0),
        ack_rate: num(&f, "ackRate").unwrap_or(0.0),
        oldest_age: num(&f, "oldestAge"),
        groups,
        partial: f.boolean("partial").unwrap_or(false),
    })
}

/// The messages of a reading (`GET /topics/{name}/messages`).
pub fn parse_received(text: &str) -> Result<Vec<taba_client::Message>, String> {
    let f = fields(text)?;
    items(&f, "messages")
        .into_iter()
        .map(|raw| taba_client::Message::parse(raw).map_err(|e| e.to_string()))
        .collect()
}

/// What a publication answered: the id (queue), whether it was a duplicate (FIFO) and how many queues matched
/// (distributor).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Published {
    pub id: Option<String>,
    pub duplicate: bool,
    pub matched: Option<u64>,
}

pub fn parse_published(text: &str) -> Result<Published, String> {
    let f = fields(text)?;
    Ok(Published {
        id: f.string("id"),
        duplicate: f.boolean("duplicate").unwrap_or(false),
        matched: whole(&f, "matched"),
    })
}

/// How many messages a prune removed.
pub fn parse_pruned(text: &str) -> Result<u64, String> {
    let f = fields(text)?;
    Ok(whole(&f, "removed").unwrap_or(0))
}

#[cfg(test)]
mod tests;
