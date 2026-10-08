//! As condições do cluster (doc 11, "Condição do cluster"): uma **lista**, não um estado único,
//! derivada dos sinais que cada nó colhe do que enxerga. Os sinais são o campo `signals` do
//! `NodeStatus` (doc 06b); as condições saem deles aqui e no `GET /health/cluster`, que junta os
//! sinais de todos os nós.

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

/// Uma condição vigente do cluster. A ordem da enum é a de exibição: as críticas primeiro, cada
/// grupo na ordem da tabela do doc 11.
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

/// O que um nó colhe do cluster (`NodeStatus.signals`, doc 06b), do ponto de vista dele.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSignals {
    /// A versão (época) da tabela de membros deste nó.
    pub members_version: u64,
    /// Há saudáveis para votar uma exclusão? (`cluster.vote.min-voters`; ver [`collect`].)
    pub quorum_available: bool,
    /// Cada outro membro: o canal com ele está conectado?
    pub channels: BTreeMap<String, bool>,
    /// Blocos com algum dono ainda em sincronização (a entrega está suspensa neles).
    pub syncing_blocks: u64,
    /// Blocos com menos réplicas íntegras do que o anel dá (inclui os que estão abaixo do quórum).
    pub blocks_below_replication: u64,
    /// Blocos com menos réplicas íntegras do que `write.quorum`.
    pub blocks_below_quorum: u64,
    /// Nós em período de freezing.
    pub frozen_nodes: u64,
}

impl HealthSignals {
    /// As condições que estes sinais, sozinhos, mostram (a divergência de tabelas só aparece
    /// quando se comparam vários nós; ver [`merge`]).
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

/// Junta as condições de vários nós: a união delas, mais `member-unreachable` se algum membro não
/// respondeu e `membership-divergent` se os nós que responderam têm versões diferentes da tabela.
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

/// Colhe os sinais deste nó. `None` enquanto ele não tem a tabela de membros.
///
/// `quorumAvailable`: os saudáveis (este nó e os pares com o canal conectado) alcançam o mínimo de
/// votantes. Um cluster de um nó só não tem ninguém a excluir, então o mínimo vale no máximo o
/// número de membros.
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
        if intact < quorum {
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
