//! The cluster conditions (doc 11, "Cluster condition"): a **list**, not a single state,
//! derived from the signals each node gathers from what it sees. The signals are the `signals` field of the
//! `NodeStatus` (doc 06b); the conditions come out of them here and in `GET /health/cluster`, which joins the
//! signals of all the nodes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::members::MemberState;
use super::service::ClusterService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Critical,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }
}

/// A condition in force of the cluster. The enum's order is the display order: the critical ones first, each
/// group in the order of doc 11's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Condition {
    WriteQuorumAtRisk,
    QuorumLost,
    MembershipDivergent,
    MemberUnreachable,
    NodeFrozen,
    Syncing,
    UnderReplicated,
    StoragePressure,
}

impl Condition {
    pub const ALL: [Condition; 8] = [
        Condition::WriteQuorumAtRisk,
        Condition::QuorumLost,
        Condition::MembershipDivergent,
        Condition::MemberUnreachable,
        Condition::NodeFrozen,
        Condition::Syncing,
        Condition::UnderReplicated,
        Condition::StoragePressure,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Condition::WriteQuorumAtRisk => "write-quorum-at-risk",
            Condition::QuorumLost => "quorum-lost",
            Condition::MembershipDivergent => "membership-divergent",
            Condition::MemberUnreachable => "member-unreachable",
            Condition::NodeFrozen => "node-frozen",
            Condition::Syncing => "syncing",
            Condition::UnderReplicated => "under-replicated",
            Condition::StoragePressure => "storage-pressure",
        }
    }

    pub const fn severity(self) -> Severity {
        match self {
            Condition::WriteQuorumAtRisk
            | Condition::QuorumLost
            | Condition::MembershipDivergent => Severity::Critical,
            _ => Severity::Warning,
        }
    }

    /// O `ConditionEntry` do doc 06d.
    pub fn entry(self) -> Value {
        json!({ "condition": self.as_str(), "severity": self.severity().as_str() })
    }
}

/// What a node gathers from the cluster (`NodeStatus.signals`, doc 06b), from its own point of view.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSignals {
    /// The version (epoch) of this node's member table.
    pub members_version: u64,
    /// Are there healthy nodes to vote on an exclusion? (`cluster.vote.min-voters`; see [`collect`].)
    pub quorum_available: bool,
    /// Each other member: is the channel to it connected?
    pub channels: BTreeMap<String, bool>,
    /// Blocks with some owner still syncing (delivery is suspended on them).
    pub syncing_blocks: u64,
    /// Blocks with fewer intact replicas than the ring gives (includes those below the quorum).
    pub blocks_below_replication: u64,
    /// Blocks with fewer intact replicas than `write.quorum` or with the primary down.
    pub blocks_below_quorum: u64,
    /// Nodes in a freezing period.
    pub frozen_nodes: u64,
}

impl HealthSignals {
    /// The conditions these signals, alone, show (table divergence only shows up
    /// when several nodes are compared; see [`merge`]).
    pub fn conditions(&self, storage_pressure: bool) -> Vec<Condition> {
        let mut out = Vec::new();
        if self.blocks_below_quorum > 0 {
            out.push(Condition::WriteQuorumAtRisk);
        }
        if !self.quorum_available {
            out.push(Condition::QuorumLost);
        }
        if self.channels.values().any(|up| !up) {
            out.push(Condition::MemberUnreachable);
        }
        if self.frozen_nodes > 0 {
            out.push(Condition::NodeFrozen);
        }
        if self.syncing_blocks > 0 {
            out.push(Condition::Syncing);
        }
        if self.blocks_below_replication > self.blocks_below_quorum {
            out.push(Condition::UnderReplicated);
        }
        if storage_pressure {
            out.push(Condition::StoragePressure);
        }
        out.sort();
        out
    }
}

/// Joins the conditions of several nodes: their union, plus `member-unreachable` if some member did not
/// answer and `membership-divergent` if the nodes that answered have different table versions.
pub fn merge(
    per_node: impl IntoIterator<Item = (u64, Vec<Condition>)>,
    unreachable: bool,
) -> Vec<Condition> {
    let mut all = Vec::new();
    let mut versions = Vec::new();
    for (version, conditions) in per_node {
        versions.push(version);
        all.extend(conditions);
    }
    if unreachable {
        all.push(Condition::MemberUnreachable);
    }
    if versions.windows(2).any(|pair| pair[0] != pair[1]) {
        all.push(Condition::MembershipDivergent);
    }
    all.sort();
    all.dedup();
    all
}

/// Gathers this node's signals. `None` while it does not have the member table.
///
/// `quorumAvailable`: the healthy ones (this node and the peers with a connected channel) reach the minimum of
/// voters. A one-node cluster has no one to exclude, so the minimum is worth at most the
/// number of members.
pub fn collect(service: &ClusterService) -> Option<HealthSignals> {
    let membership = service.membership();
    let table = membership.table()?;
    let rules = service.rules();
    let me = service.node_id();
    let peers = service
        .replication()
        .map(|r| r.channels.peers())
        .unwrap_or_default();
    let reaches = |id: &str| id == me || peers.iter().any(|p| p.node_id == id && p.up);

    let channels: BTreeMap<String, bool> = table
        .members
        .iter()
        .filter(|m| m.node_id != me)
        .map(|m| (m.node_id.clone(), reaches(&m.node_id)))
        .collect();
    let healthy = 1 + channels.values().filter(|up| **up).count();
    let needed = (rules.vote_min_voters as usize).min(table.members.len());

    let ring = membership.ring();
    let wanted = rules.replication_factor.max(1) as usize;
    let state_of = |id: &str| table.member(id).map(|m| m.state);
    let (mut syncing, mut below_replication, mut below_quorum) = (0u64, 0u64, 0u64);
    for block in ring.blocks() {
        let Some(owners) = ring.block_owners(block, wanted) else {
            continue;
        };
        if owners
            .iter()
            .any(|id| state_of(id) == Some(MemberState::Syncing))
        {
            syncing += 1;
        }
        let intact = owners
            .iter()
            .filter(|id| state_of(id) == Some(MemberState::Active) && reaches(id))
            .count();
        let quorum = (rules.quorum as usize).min(owners.len());
        // The write quorum always includes the block's primary (doc 05, section 7.2): with it down
        // and still in the table, the block's writes fail until the exclusion (D107).
        let primary_down = owners
            .first()
            .is_some_and(|id| state_of(id) == Some(MemberState::Active) && !reaches(id));
        if intact < quorum || primary_down {
            below_quorum += 1;
        }
        if intact < owners.len() {
            below_replication += 1;
        }
    }
    Some(HealthSignals {
        members_version: table.version,
        quorum_available: healthy >= needed,
        channels,
        syncing_blocks: syncing,
        blocks_below_replication: below_replication,
        blocks_below_quorum: below_quorum,
        frozen_nodes: membership.frozen_count(service.now(), rules.freezing_duration) as u64,
    })
}

#[cfg(test)]
mod tests;
