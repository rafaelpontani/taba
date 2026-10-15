//! What the messages count about themselves (doc 11): the per-topic and node counters, the
//! snapshots that `GET /metrics/topics*` and `GET /metrics` read and the topic conditions. It is only data:
//! whoever counts is `Messages`, and whoever reads is the HTTP layer, without the `engine` knowing any
//! output format.

use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use crate::config::Config;
use crate::telemetry::histogram::{Histogram, LATENCY_BOUNDS};

/// The buckets of the wait time for the first delivery (`taba.delivery.fair.wait`), in seconds.
pub const WAIT_BOUNDS: &[f64] = &[
    0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 60.0, 300.0,
];

/// The `band` attribute's bands: from `0` to `14`, and `15+`.
pub const BANDS: usize = 16;

/// What a queue topic counts since the node opened the topic.
#[derive(Debug, Clone)]
pub struct TopicCounters {
    pub published: u64,
    pub published_bytes: u64,
    pub publish_errors: u64,
    pub consumed: u64,
    pub acked: u64,
    /// FIFO only: publications discarded as duplicates.
    pub deduplicated: u64,
    pub redelivered: u64,
    pub dead_lettered: u64,
    pub expired: u64,
    pub last_published_ms: Option<u64>,
    pub last_consumed_ms: Option<u64>,
    pub publish_latency: Histogram,
    pub consume_latency: Histogram,
}

impl TopicCounters {
    pub fn new() -> Self {
        Self {
            published: 0,
            published_bytes: 0,
            publish_errors: 0,
            consumed: 0,
            acked: 0,
            deduplicated: 0,
            redelivered: 0,
            dead_lettered: 0,
            expired: 0,
            last_published_ms: None,
            last_consumed_ms: None,
            publish_latency: Histogram::new(LATENCY_BOUNDS),
            consume_latency: Histogram::new(LATENCY_BOUNDS),
        }
    }
}

impl Default for TopicCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// What the distributor counts (doc 15, section 14.7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistributorCounters {
    pub received: u64,
    pub copies: u64,
    pub copy_errors: u64,
}

/// The whole node's counters. Atomic: whoever counts takes no lock.
#[derive(Debug)]
pub struct NodeCounters {
    pub overflow: AtomicU64,
    /// How many `overflow` messages the periodic log summary has already counted.
    pub overflow_reported: AtomicU64,
    pub reads_memory: AtomicU64,
    pub reads_disk: AtomicU64,
    pub disk_errors: AtomicU64,
    pub redeliveries: AtomicU64,
    pub dead_lettered: AtomicU64,
    pub expired: AtomicU64,
    pub deduplicated: AtomicU64,
    pub longpoll_waiting: AtomicI64,
    pub longpoll_delivered: AtomicU64,
    pub longpoll_empty: AtomicU64,
    pub longpoll_disconnected: AtomicU64,
    fair_wait: Mutex<Vec<Histogram>>,
}

impl NodeCounters {
    pub fn new() -> Self {
        Self {
            overflow: AtomicU64::new(0),
            overflow_reported: AtomicU64::new(0),
            reads_memory: AtomicU64::new(0),
            reads_disk: AtomicU64::new(0),
            disk_errors: AtomicU64::new(0),
            redeliveries: AtomicU64::new(0),
            dead_lettered: AtomicU64::new(0),
            expired: AtomicU64::new(0),
            deduplicated: AtomicU64::new(0),
            longpoll_waiting: AtomicI64::new(0),
            longpoll_delivered: AtomicU64::new(0),
            longpoll_empty: AtomicU64::new(0),
            longpoll_disconnected: AtomicU64::new(0),
            fair_wait: Mutex::new((0..BANDS).map(|_| Histogram::new(WAIT_BOUNDS)).collect()),
        }
    }

    pub fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    /// The wait until the first delivery of a message whose group was in the `band` band.
    pub fn observe_wait(&self, band: u32, seconds: f64) {
        let index = (band as usize).min(BANDS - 1);
        self.fair_wait.lock().unwrap_or_else(|e| e.into_inner())[index].record(seconds);
    }

    pub fn fair_wait(&self) -> Vec<Histogram> {
        self.fair_wait
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Default for NodeCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// What `Messages::observe_node` hands to `GET /metrics`.
#[derive(Debug, Clone)]
pub struct NodeObservation {
    pub memory_bytes: u64,
    pub memory_limit_bytes: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
    pub ready: u64,
    pub busy_groups: u64,
    pub overflow: u64,
    pub reads_memory: u64,
    pub reads_disk: u64,
    pub disk_errors: u64,
    pub redeliveries: u64,
    pub dead_lettered: u64,
    pub expired: u64,
    pub deduplicated: u64,
    pub longpoll_waiting: i64,
    pub longpoll_delivered: u64,
    pub longpoll_empty: u64,
    pub longpoll_disconnected: u64,
    pub fair_wait: Vec<Histogram>,
}

/// p50, p95 and p99 of a latency, in seconds. `None`: no observation yet.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Percentiles {
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
}

impl Percentiles {
    pub fn of(histogram: &Histogram) -> Self {
        Self {
            p50: histogram.percentile(0.50),
            p95: histogram.percentile(0.95),
            p99: histogram.percentile(0.99),
        }
    }
}

/// A group of messages (doc 11, "Per-group metrics").
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSnapshot {
    /// `None` on the shared counter of the groups above `delivery.fair.max-groups`. A message
    /// without a `groupId` is the group with an empty name.
    pub group_id: Option<String>,
    pub overflow: bool,
    pub visible: u64,
    pub invisible: u64,
    pub band: u32,
    /// The instant of the group's oldest message.
    pub oldest_ms: Option<u64>,
}

impl GroupSnapshot {
    pub fn messages(&self) -> u64 {
        self.visible + self.invisible
    }
}

/// A queue's snapshot at an instant (doc 11, "Per-topic metrics").
#[derive(Debug, Clone)]
pub struct TopicSnapshot {
    pub counters: TopicCounters,
    pub publish_rate: f64,
    pub consume_rate: f64,
    pub ack_rate: f64,
    pub visible: u64,
    pub invisible: u64,
    /// The oldest message not yet acknowledged.
    pub oldest_ms: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    /// The groups followed, and how many messages the largest has.
    pub groups: u64,
    pub largest_group: u64,
    /// FIFO only: groups with a message in flight.
    pub busy_groups: u64,
    /// FIFO only, and only with `stats.topics.group-stalled-deliveries`: the group whose head went past
    /// the delivery limit.
    pub stalled_group: Option<String>,
    pub publish_latency: Percentiles,
    pub consume_latency: Percentiles,
}

impl TopicSnapshot {
    pub fn depth(&self) -> u64 {
        self.visible + self.invisible
    }

    /// The share of `depth` the largest group takes up, from 0 to 1.
    pub fn noisy_share(&self) -> f64 {
        match self.depth() {
            0 => 0.0,
            depth => self.largest_group as f64 / depth as f64,
        }
    }

    pub fn bytes(&self) -> u64 {
        self.memory_bytes + self.disk_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
        }
    }
}

/// The topic's conditions, in severity order and then by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Condition {
    Backlog,
    Stalled,
    GroupStalled,
    NoisyGroup,
    MemoryPressure,
    Idle,
}

impl Condition {
    pub const ALL: [Condition; 6] = [
        Condition::Backlog,
        Condition::Stalled,
        Condition::GroupStalled,
        Condition::NoisyGroup,
        Condition::MemoryPressure,
        Condition::Idle,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Condition::Backlog => "backlog",
            Condition::Stalled => "stalled",
            Condition::GroupStalled => "group-stalled",
            Condition::NoisyGroup => "noisy-group",
            Condition::MemoryPressure => "memory-pressure",
            Condition::Idle => "idle",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == text)
    }

    pub const fn severity(self) -> Severity {
        match self {
            Condition::Backlog | Condition::Stalled | Condition::GroupStalled => Severity::Warning,
            Condition::NoisyGroup | Condition::MemoryPressure | Condition::Idle => Severity::Info,
        }
    }
}

/// The conditions' limits and the size of the detail (`stats.topics.*`). A limit that was not
/// set turns off the condition that depends on it (D90).
#[derive(Debug, Clone)]
pub struct StatsRules {
    pub top_groups: usize,
    pub backlog_depth: Option<u64>,
    pub backlog_age_ms: Option<u64>,
    pub noisy_group_share: u32,
    pub group_stalled_deliveries: Option<u32>,
    pub stalled_after_ms: Option<u64>,
    pub idle_after_ms: Option<u64>,
    /// `stats.disk.free-min`: below this much free space, the node's `storage-pressure` condition.
    pub disk_free_min: Option<u64>,
}

fn millis(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl StatsRules {
    pub fn from_config(config: &Config) -> Self {
        let s = &config.stats;
        Self {
            top_groups: s.topics_top_groups as usize,
            backlog_depth: s.topics_backlog_depth,
            backlog_age_ms: s.topics_backlog_age.map(millis),
            noisy_group_share: s.topics_noisy_group_share,
            group_stalled_deliveries: s.topics_group_stalled_deliveries,
            stalled_after_ms: s.topics_stalled_after.map(millis),
            idle_after_ms: s.topics_idle_after.map(millis),
            disk_free_min: s.disk_free_min,
        }
    }
}

/// The topic's conditions in force, the most serious first. `baseline_ms` is the start of the count
/// for a topic that has not had activity yet: the greater of the creation and the node's start.
pub fn evaluate(
    snapshot: &TopicSnapshot,
    rules: &StatsRules,
    now_ms: u64,
    baseline_ms: u64,
) -> Vec<Condition> {
    let depth = snapshot.depth();
    let last_consumed = snapshot
        .counters
        .last_consumed_ms
        .unwrap_or(0)
        .max(baseline_ms);
    let last_activity = last_consumed.max(snapshot.counters.last_published_ms.unwrap_or(0));
    let too_old = match (rules.backlog_age_ms, snapshot.oldest_ms) {
        (Some(limit), Some(at)) => now_ms.saturating_sub(at) > limit,
        _ => false,
    };
    let active = |condition: Condition| match condition {
        Condition::Backlog => rules.backlog_depth.is_some_and(|limit| depth > limit) || too_old,
        Condition::Stalled => {
            depth > 0
                && rules
                    .stalled_after_ms
                    .is_some_and(|after| now_ms.saturating_sub(last_consumed) > after)
        }
        Condition::GroupStalled => snapshot.stalled_group.is_some(),
        Condition::NoisyGroup => {
            snapshot.groups >= 2
                && depth > 0
                && snapshot.largest_group * 100 > depth * u64::from(rules.noisy_group_share)
        }
        Condition::MemoryPressure => snapshot.disk_messages > snapshot.memory_messages,
        Condition::Idle => rules
            .idle_after_ms
            .is_some_and(|after| now_ms.saturating_sub(last_activity) > after),
    };
    let mut out: Vec<Condition> = Condition::ALL.into_iter().filter(|c| active(*c)).collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.severity() == Severity::Warning));
    out
}

#[cfg(test)]
mod tests;
