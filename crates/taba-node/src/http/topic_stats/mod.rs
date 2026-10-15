//! Topic metrics in JSON (doc 06b, doc 11): `GET /metrics/topics` (paginated by name),
//! `GET /metrics/topics/{topic}` (one topic, with the largest groups) and
//! `GET /metrics/topics/{topic}/groups` (one group per line).
//!
//! A single node: `scope=cluster` and `scope=node` return the same, and the result is never `partial`. The
//! filter and the `limit` apply after measuring each topic, so `condition` finds the topics with a
//! problem across the whole list. There is no cache (`stats.topics.cache-ttl` only serves the `fan-out`
//! between nodes).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::AppState;
use super::common::ApiError;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::pagination;
use crate::core::time::format_ms;
use crate::engine::messages::stats::{
    self, Condition, DistributorCounters, GroupSnapshot, Percentiles, Severity, TopicSnapshot,
};
use crate::engine::topics::TopicView;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/metrics/topics", get(list))
        .route("/metrics/topics/{topic}", get(detail))
        .route("/metrics/topics/{topic}/groups", get(groups))
}

/// The `TopicStats` fields that `fields` accepts (the queue ones and the distributor one).
const FIELDS: &[&str] = &[
    "id",
    "name",
    "type",
    "tags",
    "createdAt",
    "volatile",
    "idleTimeout",
    "deadLetter",
    "source",
    "published",
    "publishedBytes",
    "publishErrors",
    "publishRate",
    "publishLatency",
    "consumed",
    "acked",
    "consumeRate",
    "ackRate",
    "consumeLatency",
    "visibleMessages",
    "invisibleMessages",
    "depth",
    "oldestMessageAt",
    "oldestAge",
    "bytes",
    "memoryMessages",
    "diskMessages",
    "memoryBytes",
    "diskBytes",
    "memoryShare",
    "groups",
    "noisyShare",
    "distributor",
    "worstCondition",
    "conditions",
    "partial",
    "collectedAt",
];

/// The topic's identification, copied from the definition.
const IDENTITY: &[&str] = &[
    "id",
    "name",
    "type",
    "tags",
    "createdAt",
    "volatile",
    "idleTimeout",
    "deadLetter",
    "source",
];

fn bad_request(field: &str, code: ErrorCode, message: &str) -> TabaError {
    TabaError::new(code, "The request has invalid fields.").with_detail(field, code, message)
}

fn unknown(field: &str) -> TabaError {
    bad_request(
        &format!("query.{field}"),
        ErrorCode::ValidationUnknownField,
        "Unknown parameter.",
    )
}

/// Refuses the parameter the route does not know (`sort`, for example): `400 validation.unknown-field`.
fn check_params(
    params: &HashMap<String, String>,
    allowed: &[&str],
    tags: bool,
) -> Result<(), TabaError> {
    let keys: BTreeSet<&String> = params.keys().collect();
    for key in keys {
        if !allowed.contains(&key.as_str()) && !(tags && key.starts_with("tag.")) {
            return Err(unknown(key));
        }
    }
    Ok(())
}

/// `scope`: `cluster` (the default) or `node`. With a single node, both return the same, except the
/// groups' `band`, which only shows up in `node`.
fn node_scope(params: &HashMap<String, String>) -> Result<bool, TabaError> {
    match params.get("scope").map(String::as_str) {
        None | Some("cluster") => Ok(false),
        Some("node") => Ok(true),
        Some(_) => Err(bad_request(
            "query.scope",
            ErrorCode::ValidationFailed,
            "Expected `cluster` or `node`.",
        )),
    }
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, TabaError> + Send + 'static,
) -> Result<T, ApiError> {
    Ok(tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| TabaError::internal())??)
}

// ---- Measuring a topic ----

enum Stats {
    Queue {
        snapshot: Box<TopicSnapshot>,
        conditions: Vec<Condition>,
    },
    Distributor(DistributorCounters),
}

impl Stats {
    fn conditions(&self) -> &[Condition] {
        match self {
            Stats::Queue { conditions, .. } => conditions,
            Stats::Distributor(_) => &[],
        }
    }

    fn worst(&self) -> &'static str {
        self.conditions().first().map_or("ok", |c| c.as_str())
    }

    /// Does it pass the `condition=` filter (`ok` is the topic with no condition at all)?
    fn matches(&self, wanted: Option<&str>) -> bool {
        match wanted {
            None => true,
            Some("ok") => self.conditions().is_empty(),
            Some(name) => self.conditions().iter().any(|c| c.as_str() == name),
        }
    }
}

fn measure(state: &AppState, view: &TopicView, now: u64) -> Result<Stats, TabaError> {
    let name = &view.definition.name;
    if view.definition.queue().is_none() {
        return Ok(Stats::Distributor(
            state.messages.distributor_counters(name),
        ));
    }
    let snapshot = state.messages.topic_snapshot(name, &state.stats_rules)?;
    let baseline = view.definition.created_ms.max(state.messages.started_ms());
    let conditions = stats::evaluate(&snapshot, &state.stats_rules, now, baseline);
    Ok(Stats::Queue {
        snapshot: Box::new(snapshot),
        conditions,
    })
}

// ---- JSON ----

fn percentiles(p: Percentiles) -> Value {
    json!({ "p50": p.p50, "p95": p.p95, "p99": p.p99 })
}

fn conditions_json(conditions: &[Condition]) -> Value {
    Value::Array(
        conditions
            .iter()
            .map(|c| json!({ "condition": c.as_str(), "severity": c.severity().as_str() }))
            .collect(),
    )
}

fn oldest_fields(out: &mut Map<String, Value>, oldest_ms: Option<u64>, now: u64) {
    if let Some(at) = oldest_ms {
        out.insert("oldestMessageAt".into(), json!(format_ms(at)));
        out.insert(
            "oldestAge".into(),
            json!(now.saturating_sub(at) as f64 / 1000.0),
        );
    }
}

fn identity(view: &TopicView) -> Map<String, Value> {
    let full = view.to_json();
    IDENTITY
        .iter()
        .filter_map(|key| full.get(*key).map(|v| ((*key).to_owned(), v.clone())))
        .collect()
}

/// A topic's `TopicStats` (doc 06b).
fn topic_json(
    view: &TopicView,
    stats: &Stats,
    memory_total: u64,
    now: u64,
    collected_at: &str,
) -> Map<String, Value> {
    let mut out = identity(view);
    match stats {
        Stats::Distributor(d) => {
            out.insert(
                "distributor".into(),
                json!({ "received": d.received, "copies": d.copies, "copyErrors": d.copy_errors }),
            );
        }
        Stats::Queue { snapshot: s, .. } => {
            let c = &s.counters;
            out.insert("published".into(), json!(c.published));
            out.insert("publishedBytes".into(), json!(c.published_bytes));
            out.insert("publishErrors".into(), json!(c.publish_errors));
            out.insert("publishRate".into(), json!(s.publish_rate));
            out.insert("publishLatency".into(), percentiles(s.publish_latency));
            out.insert("consumed".into(), json!(c.consumed));
            out.insert("acked".into(), json!(c.acked));
            out.insert("consumeRate".into(), json!(s.consume_rate));
            out.insert("ackRate".into(), json!(s.ack_rate));
            out.insert("consumeLatency".into(), percentiles(s.consume_latency));
            out.insert("visibleMessages".into(), json!(s.visible));
            out.insert("invisibleMessages".into(), json!(s.invisible));
            out.insert("depth".into(), json!(s.depth()));
            oldest_fields(&mut out, s.oldest_ms, now);
            out.insert("bytes".into(), json!(s.bytes()));
            out.insert("memoryMessages".into(), json!(s.memory_messages));
            out.insert("diskMessages".into(), json!(s.disk_messages));
            out.insert("memoryBytes".into(), json!(s.memory_bytes));
            out.insert("diskBytes".into(), json!(s.disk_bytes));
            let share = if memory_total == 0 {
                0.0
            } else {
                s.memory_bytes as f64 / memory_total as f64
            };
            out.insert("memoryShare".into(), json!(share));
            out.insert("groups".into(), json!(s.groups));
            out.insert("noisyShare".into(), json!(s.noisy_share()));
        }
    }
    out.insert("worstCondition".into(), json!(stats.worst()));
    out.insert("conditions".into(), conditions_json(stats.conditions()));
    out.insert("partial".into(), json!(false));
    out.insert("collectedAt".into(), json!(collected_at));
    out
}

/// `GroupStats` (doc 06b). Without the per-group rates (PRB-024).
fn group_json(group: &GroupSnapshot, depth: u64, node: bool, now: u64) -> Value {
    let mut out = Map::new();
    if let Some(id) = &group.group_id {
        out.insert("groupId".into(), json!(id));
    }
    if group.overflow {
        out.insert("overflow".into(), json!(true));
    }
    out.insert("messages".into(), json!(group.messages()));
    out.insert("visibleMessages".into(), json!(group.visible));
    out.insert("invisibleMessages".into(), json!(group.invisible));
    let share = if depth == 0 {
        0.0
    } else {
        group.messages() as f64 / depth as f64
    };
    out.insert("share".into(), json!(share));
    if node {
        out.insert("band".into(), json!(group.band));
    }
    oldest_fields(&mut out, group.oldest_ms, now);
    Value::Object(out)
}

/// The groups from largest to smallest (and by name, to break ties).
fn largest_first(groups: &mut [GroupSnapshot]) {
    groups.sort_by(|a, b| {
        b.messages()
            .cmp(&a.messages())
            .then_with(|| a.group_id.cmp(&b.group_id))
    });
}

/// `fields=`: only the requested fields, plus those that always come.
fn parse_fields(params: &HashMap<String, String>) -> Result<Option<BTreeSet<String>>, TabaError> {
    let Some(raw) = params.get("fields") else {
        return Ok(None);
    };
    let mut wanted = BTreeSet::new();
    for field in raw.split(',').map(str::trim).filter(|f| !f.is_empty()) {
        if !FIELDS.contains(&field) {
            return Err(bad_request(
                "query.fields",
                ErrorCode::ValidationUnknownField,
                &format!("Unknown field `{field}`."),
            ));
        }
        wanted.insert(field.to_owned());
    }
    Ok(Some(wanted))
}

fn project(
    mut item: Map<String, Value>,
    fields: Option<&BTreeSet<String>>,
    always: &[&str],
) -> Value {
    if let Some(fields) = fields {
        item.retain(|key, _| fields.contains(key) || always.contains(&key.as_str()));
    }
    Value::Object(item)
}

fn page(items: Vec<Value>, next_cursor: Option<String>) -> Value {
    let mut out = Map::new();
    out.insert("items".into(), Value::Array(items));
    if let Some(cursor) = next_cursor {
        out.insert("nextCursor".into(), Value::String(cursor));
    }
    Value::Object(out)
}

// ---- GET /metrics/topics ----

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    /// The last key read: the topic name, or the tag value with `groupBy`.
    n: String,
    /// The filters that generated the cursor: with others, it is refused.
    f: String,
}

fn stale_cursor() -> TabaError {
    TabaError::new(
        ErrorCode::ValidationInvalidCursor,
        "The cursor does not apply to these filters.",
    )
    .with_detail(
        "cursor",
        ErrorCode::ValidationInvalidCursor,
        "Cursor generated with other filters.",
    )
}

/// The conditions in severity order and then the table's, with no repeats.
fn ordered(mut conditions: Vec<Condition>) -> Vec<Condition> {
    let rank = |c: &Condition| {
        (
            c.severity() != Severity::Warning,
            Condition::ALL.iter().position(|x| x == c),
        )
    };
    conditions.sort_by_key(rank);
    conditions.dedup();
    conditions
}

/// The sum of the topics of a tag value (`groupBy`).
#[derive(Default)]
struct Aggregate {
    topics: u64,
    published: u64,
    published_bytes: u64,
    publish_errors: u64,
    publish_rate: f64,
    consumed: u64,
    acked: u64,
    consume_rate: f64,
    ack_rate: f64,
    visible: u64,
    invisible: u64,
    oldest_ms: Option<u64>,
    memory_messages: u64,
    disk_messages: u64,
    memory_bytes: u64,
    disk_bytes: u64,
    groups: u64,
    conditions: Vec<Condition>,
}

impl Aggregate {
    fn add(&mut self, s: &TopicSnapshot, conditions: &[Condition]) {
        self.topics += 1;
        self.published += s.counters.published;
        self.published_bytes += s.counters.published_bytes;
        self.publish_errors += s.counters.publish_errors;
        self.publish_rate += s.publish_rate;
        self.consumed += s.counters.consumed;
        self.acked += s.counters.acked;
        self.consume_rate += s.consume_rate;
        self.ack_rate += s.ack_rate;
        self.visible += s.visible;
        self.invisible += s.invisible;
        self.oldest_ms = match (self.oldest_ms, s.oldest_ms) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.memory_messages += s.memory_messages;
        self.disk_messages += s.disk_messages;
        self.memory_bytes += s.memory_bytes;
        self.disk_bytes += s.disk_bytes;
        self.groups += s.groups;
        self.conditions.extend_from_slice(conditions);
    }

    fn json(self, tag: &str, value: &str, now: u64, collected_at: &str) -> Map<String, Value> {
        let conditions = ordered(self.conditions);
        let mut out = Map::new();
        out.insert("tag".into(), json!(tag));
        out.insert("value".into(), json!(value));
        out.insert("topics".into(), json!(self.topics));
        out.insert("published".into(), json!(self.published));
        out.insert("publishedBytes".into(), json!(self.published_bytes));
        out.insert("publishErrors".into(), json!(self.publish_errors));
        out.insert("publishRate".into(), json!(self.publish_rate));
        out.insert("consumed".into(), json!(self.consumed));
        out.insert("acked".into(), json!(self.acked));
        out.insert("consumeRate".into(), json!(self.consume_rate));
        out.insert("ackRate".into(), json!(self.ack_rate));
        out.insert("visibleMessages".into(), json!(self.visible));
        out.insert("invisibleMessages".into(), json!(self.invisible));
        out.insert("depth".into(), json!(self.visible + self.invisible));
        oldest_fields(&mut out, self.oldest_ms, now);
        out.insert("bytes".into(), json!(self.memory_bytes + self.disk_bytes));
        out.insert("memoryMessages".into(), json!(self.memory_messages));
        out.insert("diskMessages".into(), json!(self.disk_messages));
        out.insert("memoryBytes".into(), json!(self.memory_bytes));
        out.insert("diskBytes".into(), json!(self.disk_bytes));
        out.insert("groups".into(), json!(self.groups));
        out.insert(
            "worstCondition".into(),
            json!(conditions.first().map_or("ok", |c| c.as_str())),
        );
        out.insert("conditions".into(), conditions_json(&conditions));
        out.insert("partial".into(), json!(false));
        out.insert("collectedAt".into(), json!(collected_at));
        out
    }
}

async fn list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(blocking(move || list_topics(&state, &params)).await?))
}

fn list_topics(state: &AppState, params: &HashMap<String, String>) -> Result<Value, TabaError> {
    check_params(
        params,
        &["limit", "cursor", "scope", "condition", "fields", "groupBy"],
        true,
    )?;
    node_scope(params)?;
    let rules = state.topics.rules();
    let limit = pagination::parse_limit(
        params.get("limit").map(String::as_str),
        rules.default_limit,
        rules.max_limit,
    )?;
    let fields = parse_fields(params)?;
    let condition = params.get("condition").map(String::as_str);
    if let Some(name) = condition
        && name != "ok"
        && Condition::parse(name).is_none()
    {
        return Err(bad_request(
            "query.condition",
            ErrorCode::ValidationFailed,
            "Unknown condition.",
        ));
    }
    let group_by = params.get("groupBy").map(String::as_str);
    let tag_filters: BTreeMap<&str, &str> = params
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix("tag.")?, v.as_str())))
        .collect();
    let canonical = format!(
        "{}|{}|{}",
        condition.unwrap_or(""),
        tag_filters
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(";"),
        group_by.unwrap_or("")
    );
    let cursor = match params.get("cursor") {
        Some(raw) => {
            let cursor: Cursor = pagination::decode_cursor(raw)?;
            if cursor.f != canonical {
                return Err(stale_cursor());
            }
            Some(cursor)
        }
        None => None,
    };

    let now = state.clock.now_millis();
    let collected_at = format_ms(now);
    let memory_total = state.messages.observe_node().memory_bytes;
    let views = state.topics.all_views().into_iter().filter(|view| {
        tag_filters
            .iter()
            .all(|(k, v)| view.definition.tags.get(*k).is_some_and(|t| t == v))
    });

    if let Some(tag) = group_by {
        let mut groups: BTreeMap<String, Aggregate> = BTreeMap::new();
        for view in views {
            let Some(value) = view.definition.tags.get(tag) else {
                continue;
            };
            let Ok(stats) = measure(state, &view, now) else {
                continue;
            };
            if let (
                Stats::Queue {
                    snapshot,
                    conditions,
                },
                true,
            ) = (&stats, stats.matches(condition))
            {
                groups
                    .entry(value.clone())
                    .or_default()
                    .add(snapshot, conditions);
            }
        }
        let mut rows: Vec<(String, Aggregate)> = groups
            .into_iter()
            .filter(|(value, _)| cursor.as_ref().is_none_or(|c| *value > c.n))
            .collect();
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next = has_more
            .then(|| rows.last().map(|(value, _)| value.clone()))
            .flatten()
            .map(|n| {
                pagination::encode_cursor(&Cursor {
                    n,
                    f: canonical.clone(),
                })
            });
        let items = rows
            .into_iter()
            .map(|(value, aggregate)| {
                project(
                    aggregate.json(tag, &value, now, &collected_at),
                    fields.as_ref(),
                    &["tag", "value", "topics"],
                )
            })
            .collect();
        return Ok(page(items, next));
    }

    let mut items: Vec<(TopicView, Stats)> = Vec::new();
    for view in views {
        if cursor.as_ref().is_some_and(|c| view.definition.name <= c.n) {
            continue;
        }
        if items.len() > limit {
            break;
        }
        let Ok(stats) = measure(state, &view, now) else {
            continue;
        };
        if stats.matches(condition) {
            items.push((view, stats));
        }
    }
    let has_more = items.len() > limit;
    items.truncate(limit);
    let next = has_more
        .then(|| items.last().map(|(view, _)| view.definition.name.clone()))
        .flatten()
        .map(|n| {
            pagination::encode_cursor(&Cursor {
                n,
                f: canonical.clone(),
            })
        });
    let items = items
        .iter()
        .map(|(view, stats)| {
            project(
                topic_json(view, stats, memory_total, now, &collected_at),
                fields.as_ref(),
                &["id", "name"],
            )
        })
        .collect();
    Ok(page(items, next))
}

// ---- GET /metrics/topics/{topic} ----

async fn detail(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        blocking(move || topic_detail(&state, &topic, &params)).await?,
    ))
}

fn topic_detail(
    state: &AppState,
    topic: &str,
    params: &HashMap<String, String>,
) -> Result<Value, TabaError> {
    check_params(params, &["scope"], false)?;
    let node = node_scope(params)?;
    let view = state.topics.get(topic)?;
    let now = state.clock.now_millis();
    let stats = measure(state, &view, now)?;
    let memory_total = state.messages.observe_node().memory_bytes;
    let mut out = topic_json(&view, &stats, memory_total, now, &format_ms(now));
    if matches!(stats, Stats::Queue { .. }) {
        let mut groups = state.messages.group_snapshots(topic)?;
        let depth: u64 = groups.iter().map(GroupSnapshot::messages).sum();
        largest_first(&mut groups);
        groups.truncate(state.stats_rules.top_groups);
        out.insert(
            "topGroups".into(),
            Value::Array(
                groups
                    .iter()
                    .map(|g| group_json(g, depth, node, now))
                    .collect(),
            ),
        );
    }
    Ok(Value::Object(out))
}

// ---- GET /metrics/topics/{topic}/groups ----

#[derive(Debug, Serialize, Deserialize)]
struct GroupCursor {
    /// The shared counter's group comes after all the named ones.
    o: bool,
    n: String,
}

async fn groups(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        blocking(move || topic_groups(&state, &topic, &params)).await?,
    ))
}

fn topic_groups(
    state: &AppState,
    topic: &str,
    params: &HashMap<String, String>,
) -> Result<Value, TabaError> {
    check_params(
        params,
        &["limit", "cursor", "groupId", "top", "scope"],
        false,
    )?;
    let node = node_scope(params)?;
    let rules = state.topics.rules();
    let limit = pagination::parse_limit(
        params.get("limit").map(String::as_str),
        rules.default_limit,
        rules.max_limit,
    )?;
    let top = match params.get("top") {
        None => None,
        Some(raw) => {
            let n: u64 = raw.trim().parse().map_err(|_| {
                bad_request(
                    "query.top",
                    ErrorCode::ValidationFailed,
                    "Expected an integer.",
                )
            })?;
            if n == 0 {
                return Err(bad_request(
                    "query.top",
                    ErrorCode::ValidationOutOfRange,
                    "top must be at least 1.",
                ));
            }
            Some(usize::try_from(n).unwrap_or(usize::MAX))
        }
    };
    if top.is_some() && params.contains_key("cursor") {
        return Err(bad_request(
            "query.top",
            ErrorCode::ValidationFailed,
            "`top` does not combine with `cursor`: the response is not paginated.",
        ));
    }
    let cursor = params
        .get("cursor")
        .map(|raw| pagination::decode_cursor::<GroupCursor>(raw))
        .transpose()?;

    let view = state.topics.get(topic)?;
    if view.definition.queue().is_none() {
        return Err(TabaError::new(
            ErrorCode::TopicWrongType,
            "A distributor topic has no groups.",
        ));
    }
    let now = state.clock.now_millis();
    let mut all = state.messages.group_snapshots(topic)?;
    let depth: u64 = all.iter().map(GroupSnapshot::messages).sum();
    if let Some(wanted) = params.get("groupId") {
        all.retain(|g| g.group_id.as_deref() == Some(wanted.as_str()));
    }
    let json = |g: &GroupSnapshot| group_json(g, depth, node, now);

    if let Some(top) = top {
        largest_first(&mut all);
        all.truncate(top);
        return Ok(page(all.iter().map(json).collect(), None));
    }
    let key = |g: &GroupSnapshot| (g.overflow, g.group_id.clone().unwrap_or_default());
    all.sort_by_key(key);
    if let Some(c) = &cursor {
        all.retain(|g| key(g) > (c.o, c.n.clone()));
    }
    let has_more = all.len() > limit;
    all.truncate(limit);
    let next = has_more
        .then(|| all.last().map(key))
        .flatten()
        .map(|(o, n)| pagination::encode_cursor(&GroupCursor { o, n }));
    Ok(page(all.iter().map(json).collect(), next))
}
