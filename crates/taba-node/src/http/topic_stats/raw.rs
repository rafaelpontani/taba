//! What a node says of its topics to the others (`GET /cluster/stats/topics`) and how the node that
//! answers a client joins what every node said. The numbers cross the cluster port, not the presentation:
//! the client API builds its JSON from the joined numbers, with the conditions evaluated again on them.
//!
//! How each field joins (a message is counted by the node that **owns** it, so the counts add up):
//! the counts and rates **add**; `oldestMessageAt` is the **minimum**; the latency percentiles are the
//! **maximum** of the nodes' (percentiles do not add up, and the maximum errs on the cautious side);
//! `groups` and the largest group are the **maximum** (the same group can have messages on several nodes,
//! so adding would count it twice: it is a lower bound).

use serde::{Deserialize, Serialize};

use crate::engine::messages::stats::{
    DistributorCounters, GroupSnapshot, Percentiles, TopicCounters, TopicSnapshot,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStats {
    pub node_id: String,
    /// The node's memory in use by messages, for `memoryShare`.
    pub memory_total_bytes: u64,
    pub topics: Vec<RawTopic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTopic {
    pub name: String,
    #[serde(flatten)]
    pub kind: RawKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RawKind {
    Queue(Box<RawQueue>),
    Distributor(RawDistributor),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawDistributor {
    pub received: u64,
    pub copies: u64,
    pub copy_errors: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RawPercentiles {
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawQueue {
    pub published: u64,
    pub published_bytes: u64,
    pub publish_errors: u64,
    pub consumed: u64,
    pub acked: u64,
    pub last_published_ms: Option<u64>,
    pub last_consumed_ms: Option<u64>,
    pub publish_rate: f64,
    pub consume_rate: f64,
    pub ack_rate: f64,
    pub visible: u64,
    pub invisible: u64,
    pub oldest_ms: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub groups: u64,
    pub largest_group: u64,
    pub busy_groups: u64,
    pub stalled_group: Option<String>,
    pub publish_latency: RawPercentiles,
    pub consume_latency: RawPercentiles,
}

fn pct(p: Percentiles) -> RawPercentiles {
    RawPercentiles {
        p50: p.p50,
        p95: p.p95,
        p99: p.p99,
    }
}

fn unpct(p: RawPercentiles) -> Percentiles {
    Percentiles {
        p50: p.p50,
        p95: p.p95,
        p99: p.p99,
    }
}

fn max_opt<T: PartialOrd>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b > a { b } else { a }),
        (a, b) => a.or(b),
    }
}

fn min_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

impl RawQueue {
    pub fn of(s: &TopicSnapshot) -> Self {
        Self {
            published: s.counters.published,
            published_bytes: s.counters.published_bytes,
            publish_errors: s.counters.publish_errors,
            consumed: s.counters.consumed,
            acked: s.counters.acked,
            last_published_ms: s.counters.last_published_ms,
            last_consumed_ms: s.counters.last_consumed_ms,
            publish_rate: s.publish_rate,
            consume_rate: s.consume_rate,
            ack_rate: s.ack_rate,
            visible: s.visible,
            invisible: s.invisible,
            oldest_ms: s.oldest_ms,
            memory_messages: s.memory_messages,
            disk_messages: s.disk_messages,
            memory_bytes: s.memory_bytes,
            disk_bytes: s.disk_bytes,
            groups: s.groups,
            largest_group: s.largest_group,
            busy_groups: s.busy_groups,
            stalled_group: s.stalled_group.clone(),
            publish_latency: pct(s.publish_latency),
            consume_latency: pct(s.consume_latency),
        }
    }

    /// The snapshot the conditions are evaluated on. The latency histograms stay empty: the percentiles
    /// were already read by whoever measured.
    pub fn snapshot(&self) -> TopicSnapshot {
        let mut counters = TopicCounters::new();
        counters.published = self.published;
        counters.published_bytes = self.published_bytes;
        counters.publish_errors = self.publish_errors;
        counters.consumed = self.consumed;
        counters.acked = self.acked;
        counters.last_published_ms = self.last_published_ms;
        counters.last_consumed_ms = self.last_consumed_ms;
        TopicSnapshot {
            counters,
            publish_rate: self.publish_rate,
            consume_rate: self.consume_rate,
            ack_rate: self.ack_rate,
            visible: self.visible,
            invisible: self.invisible,
            oldest_ms: self.oldest_ms,
            memory_messages: self.memory_messages,
            disk_messages: self.disk_messages,
            memory_bytes: self.memory_bytes,
            disk_bytes: self.disk_bytes,
            groups: self.groups,
            largest_group: self.largest_group,
            busy_groups: self.busy_groups,
            stalled_group: self.stalled_group.clone(),
            publish_latency: unpct(self.publish_latency),
            consume_latency: unpct(self.consume_latency),
        }
    }

    /// Joins what another node saw of the same queue.
    pub fn join(&mut self, other: &RawQueue) {
        self.published += other.published;
        self.published_bytes += other.published_bytes;
        self.publish_errors += other.publish_errors;
        self.consumed += other.consumed;
        self.acked += other.acked;
        self.last_published_ms = max_opt(self.last_published_ms, other.last_published_ms);
        self.last_consumed_ms = max_opt(self.last_consumed_ms, other.last_consumed_ms);
        self.publish_rate += other.publish_rate;
        self.consume_rate += other.consume_rate;
        self.ack_rate += other.ack_rate;
        self.visible += other.visible;
        self.invisible += other.invisible;
        self.oldest_ms = min_opt(self.oldest_ms, other.oldest_ms);
        self.memory_messages += other.memory_messages;
        self.disk_messages += other.disk_messages;
        self.memory_bytes += other.memory_bytes;
        self.disk_bytes += other.disk_bytes;
        self.groups = self.groups.max(other.groups);
        self.largest_group = self.largest_group.max(other.largest_group);
        self.busy_groups += other.busy_groups;
        if self.stalled_group.is_none() {
            self.stalled_group.clone_from(&other.stalled_group);
        }
        for (mine, theirs) in [
            (&mut self.publish_latency, other.publish_latency),
            (&mut self.consume_latency, other.consume_latency),
        ] {
            mine.p50 = max_opt(mine.p50, theirs.p50);
            mine.p95 = max_opt(mine.p95, theirs.p95);
            mine.p99 = max_opt(mine.p99, theirs.p99);
        }
    }
}

impl RawDistributor {
    pub fn of(c: DistributorCounters) -> Self {
        Self {
            received: c.received,
            copies: c.copies,
            copy_errors: c.copy_errors,
        }
    }

    pub fn counters(self) -> DistributorCounters {
        DistributorCounters {
            received: self.received,
            copies: self.copies,
            copy_errors: self.copy_errors,
        }
    }

    pub fn join(&mut self, other: &RawDistributor) {
        self.received += other.received;
        self.copies += other.copies;
        self.copy_errors += other.copy_errors;
    }
}

/// A group as a node sees it, on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawGroup {
    pub group_id: Option<String>,
    pub overflow: bool,
    pub visible: u64,
    pub invisible: u64,
    pub band: u32,
    pub oldest_ms: Option<u64>,
}

impl RawGroup {
    pub fn of(g: &GroupSnapshot) -> Self {
        Self {
            group_id: g.group_id.clone(),
            overflow: g.overflow,
            visible: g.visible,
            invisible: g.invisible,
            band: g.band,
            oldest_ms: g.oldest_ms,
        }
    }

    pub fn snapshot(&self) -> GroupSnapshot {
        GroupSnapshot {
            group_id: self.group_id.clone(),
            overflow: self.overflow,
            visible: self.visible,
            invisible: self.invisible,
            band: self.band,
            oldest_ms: self.oldest_ms,
        }
    }
}

/// Joins the groups of every node: the same group adds its messages, keeps the oldest and the highest band.
pub fn join_groups(nodes: Vec<Vec<GroupSnapshot>>) -> Vec<GroupSnapshot> {
    let mut joined: Vec<GroupSnapshot> = Vec::new();
    for group in nodes.into_iter().flatten() {
        match joined
            .iter_mut()
            .find(|g| g.group_id == group.group_id && g.overflow == group.overflow)
        {
            Some(found) => {
                found.visible += group.visible;
                found.invisible += group.invisible;
                found.band = found.band.max(group.band);
                found.oldest_ms = min_opt(found.oldest_ms, group.oldest_ms);
            }
            None => joined.push(group),
        }
    }
    joined
}
