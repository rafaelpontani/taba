//! The member table and the two-phase register of a node join or leave (doc 04,
//! sections 6.2 and 6.3; doc 06c, "Members and channel").
//!
//! A member change is decided only with the confirmation of **all** nodes (no reduced
//! quorum): phase 1 (`register`) leaves the change pending, and phase 2 (`commit`) makes it effective and
//! raises the table's `version`. What is pending does not change the table, and there is **one change at a time**
//! (section 6.8): a second `register` with another `operationId` is a conflict. The three steps are
//! idempotent by `operationId`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::ring::Ring;

/// How many already-decided operations the node remembers, to repeat the answer of a repeated `commit` or
/// `rollback` instead of saying the operation is unknown.
const FINISHED_MEMORY: usize = 256;

/// How many versions-ahead changes the node keeps, waiting for the ones missing before them.
const HELD_LIMIT: usize = 64;

/// Milliseconds since 1970.
fn millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberState {
    Active,
    Syncing,
    Draining,
    Suspect,
}

impl MemberState {
    pub const fn as_str(self) -> &'static str {
        match self {
            MemberState::Active => "active",
            MemberState::Syncing => "syncing",
            MemberState::Draining => "draining",
            MemberState::Suspect => "suspect",
        }
    }
}

/// A table node (`Member`, doc 06c).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Member {
    pub node_id: String,
    /// The node's internal IPs.
    pub addresses: Vec<String>,
    /// A porta do cluster.
    pub port: u16,
    pub state: MemberState,
}

/// The member table (`MemberTable`, doc 06c). The `version` is the epoch: it rises on each change
/// made effective, and different versions between nodes indicate incomplete propagation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberTable {
    pub version: u64,
    pub members: Vec<Member>,
    /// The nodes in freezing that whoever answers knows (D106). It only travels in the responses that bring the
    /// table to a new node; the stored table never has it (freezing lives apart, without raising the
    /// `version`), and it does not count in table equality.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frozen: Vec<FrozenNode>,
}

impl PartialEq for MemberTable {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.members == other.members
    }
}

impl Eq for MemberTable {}

/// A node excluded by vote and the instant of the exclusion (doc 04, section 6.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenNode {
    pub node_id: String,
    /// Milliseconds since 1970.
    pub frozen_at: u64,
}

impl MemberTable {
    pub fn member(&self, node_id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.node_id == node_id)
    }

    fn sort(&mut self) {
        self.members.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    }
}

/// What a pending operation does to the table. It also travels in the channel's `membership` frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Change {
    Add(Member),
    /// The voluntary exit (drain): no freezing.
    Remove {
        node_id: String,
    },
    /// The exclusion by vote: the node enters freezing (doc 04, section 6.7).
    Exclude {
        node_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipError {
    /// The node does not have the table yet (it is joining).
    NoTable,
    /// There is another pending change, or the current state prevents this one (already a member, not a member).
    Conflict(String),
    /// The `commit` or `rollback` of an operation the node does not know (it never arrived or expired).
    UnknownOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finished {
    Committed,
    RolledBack,
}

#[derive(Debug)]
struct Pending {
    operation_id: String,
    change: Change,
    registered_at: SystemTime,
    /// This node is the one coordinating the change (and not just a participant in it).
    coordinated: bool,
}

#[derive(Debug, Default)]
struct Inner {
    table: Option<MemberTable>,
    ring: Arc<Ring>,
    pending: Option<Pending>,
    finished: VecDeque<(String, Finished)>,
    /// The nodes that left by drain: they can still forward client calls to whoever is in the
    /// cluster (doc 04, section 6.9). In memory only; it goes away when a new node joins at the same address.
    departed: Vec<Member>,
    /// The nodes excluded by vote and when, for the freezing period (doc 04, section 6.7).
    frozen: HashMap<String, SystemTime>,
    /// The nodes excluded by vote, with the addresses: after a partition in which both sides
    /// excluded each other, it is through them that one side looks for the other (D101). In memory only, up to 64.
    excluded: Vec<Member>,
    /// The effected changes that arrived through the channel before the version prior to them, by version
    /// (`operationId` and change). In memory only, up to [`HELD_LIMIT`].
    held: std::collections::BTreeMap<u64, (String, Change)>,
}

/// A node's member table, with the pending change and the ring derived from it.
#[derive(Debug)]
pub struct Membership {
    inner: Mutex<Inner>,
    /// `write.pending.ttl`: after it a register with neither commit nor rollback disappears by itself.
    pending_ttl: Duration,
    virtual_nodes: u32,
}

impl Membership {
    pub fn new(pending_ttl: Duration, virtual_nodes: u32) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            pending_ttl,
            virtual_nodes,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn rebuild_ring(&self, inner: &mut Inner) {
        inner.ring = Arc::new(match &inner.table {
            Some(table) => Ring::new(
                table.members.iter().map(|m| m.node_id.as_str()),
                self.virtual_nodes,
            ),
            None => Ring::default(),
        });
    }

    /// Installs the table: the first node's (itself alone) or the one `join` returned.
    pub fn install(&self, mut table: MemberTable) {
        table.sort();
        let mut inner = self.lock();
        // The freezing that came with the table is inherited; what the node already knows counts for more.
        for node in std::mem::take(&mut table.frozen) {
            inner
                .frozen
                .entry(node.node_id)
                .or_insert(UNIX_EPOCH + Duration::from_millis(node.frozen_at));
        }
        inner.table = Some(table);
        inner.pending = None;
        self.rebuild_ring(&mut inner);
    }

    /// Forgets the table (the node goes back to being new).
    pub fn clear(&self) {
        let mut inner = self.lock();
        *inner = Inner::default();
    }

    pub fn table(&self) -> Option<MemberTable> {
        self.lock().table.clone()
    }

    /// The table with the nodes in freezing that still apply, for whoever is joining (D106).
    pub fn table_with_frozen(&self, now: SystemTime, duration: Duration) -> Option<MemberTable> {
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, at| now.duration_since(*at).unwrap_or_default() < duration);
        let mut table = inner.table.clone()?;
        table.frozen = inner
            .frozen
            .iter()
            .map(|(node_id, at)| FrozenNode {
                node_id: node_id.clone(),
                frozen_at: at
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            })
            .collect();
        table.frozen.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Some(table)
    }

    pub fn version(&self) -> Option<u64> {
        self.lock().table.as_ref().map(|t| t.version)
    }

    /// The ring of the current members (empty while the node has no table).
    pub fn ring(&self) -> Arc<Ring> {
        self.lock().ring.clone()
    }

    pub fn is_member(&self, node_id: &str) -> bool {
        self.lock()
            .table
            .as_ref()
            .is_some_and(|t| t.member(node_id).is_some())
    }

    /// Is the member in the table and `active`? (A syncing node does not serve anything yet.)
    pub fn is_active(&self, node_id: &str) -> bool {
        self.lock().table.as_ref().is_some_and(|t| {
            t.member(node_id)
                .is_some_and(|m| m.state == MemberState::Active)
        })
    }

    /// Is some member syncing? While there is one, no other join, leave or
    /// rebalancing is made effective (doc 04, section 6.8).
    pub fn syncing(&self) -> Vec<String> {
        self.lock()
            .table
            .as_ref()
            .map(|t| {
                t.members
                    .iter()
                    .filter(|m| m.state == MemberState::Syncing)
                    .map(|m| m.node_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Changes a member's state in the table, without raising the `version` (it is not a member change:
    /// it is the node saying where it stands). `true` if it changed.
    pub fn set_state(&self, node_id: &str, state: MemberState) -> bool {
        let mut inner = self.lock();
        let Some(member) = inner
            .table
            .as_mut()
            .and_then(|t| t.members.iter_mut().find(|m| m.node_id == node_id))
        else {
            return false;
        };
        let changed = member.state != state;
        member.state = state;
        changed
    }

    /// Does some member have this address? It is how the cluster port recognizes who is calling.
    pub fn is_member_address(&self, address: &str) -> bool {
        self.lock().table.as_ref().is_some_and(|t| {
            t.members
                .iter()
                .any(|m| m.addresses.iter().any(|a| a == address))
        })
    }

    /// Is the address that of a node that left by drain and may still forward calls?
    pub fn is_departed_address(&self, address: &str) -> bool {
        self.lock()
            .departed
            .iter()
            .any(|m| m.addresses.iter().any(|a| a == address))
    }

    /// The nodes that left by drain and have not come back (up to 64, in memory only): a client can still
    /// address them to return them from the drain.
    pub fn departed_members(&self) -> Vec<Member> {
        self.lock().departed.clone()
    }

    /// The nodes this node saw excluded by vote and that did not return to the table (D101).
    pub fn excluded_members(&self) -> Vec<Member> {
        let inner = self.lock();
        inner
            .excluded
            .iter()
            .filter(|m| {
                inner
                    .table
                    .as_ref()
                    .is_none_or(|t| t.member(&m.node_id).is_none())
            })
            .cloned()
            .collect()
    }

    /// How much of the node's freezing (excluded by vote) is left, or `None` if it is not in freezing.
    pub fn frozen_remaining(
        &self,
        node_id: &str,
        now: SystemTime,
        duration: Duration,
    ) -> Option<Duration> {
        let inner = self.lock();
        let at = inner.frozen.get(node_id)?;
        let elapsed = now.duration_since(*at).unwrap_or_default();
        duration.checked_sub(elapsed).filter(|left| !left.is_zero())
    }

    /// Stores the freezing of a node that another node saw excluded (the channel's `frozen` frame).
    /// Returns `true` if it was news: what the node already knows counts for more (the echo does not change the instant),
    /// and a freezing that already ended is not stored.
    pub fn learn_frozen(
        &self,
        node_id: &str,
        frozen_at: u64,
        now: SystemTime,
        duration: Duration,
    ) -> bool {
        let at = UNIX_EPOCH + Duration::from_millis(frozen_at);
        if now.duration_since(at).unwrap_or_default() >= duration {
            return false;
        }
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, known| now.duration_since(*known).unwrap_or_default() < duration);
        if inner.frozen.contains_key(node_id) {
            return false;
        }
        inner.frozen.insert(node_id.to_owned(), at);
        true
    }

    /// The instant (ms since 1970) the node entered freezing, if it is stored.
    pub fn frozen_at(&self, node_id: &str) -> Option<u64> {
        self.lock().frozen.get(node_id).map(|at| millis(*at))
    }

    /// The nodes still in freezing, in ID order (the expired ones are forgotten).
    pub fn frozen_nodes(&self, now: SystemTime, duration: Duration) -> Vec<FrozenNode> {
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, at| now.duration_since(*at).unwrap_or_default() < duration);
        let mut nodes: Vec<FrozenNode> = inner
            .frozen
            .iter()
            .map(|(node_id, at)| FrozenNode {
                node_id: node_id.clone(),
                frozen_at: millis(*at),
            })
            .collect();
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        nodes
    }
    /// How many nodes are in freezing now (the expired ones are forgotten).
    pub fn frozen_count(&self, now: SystemTime, duration: Duration) -> usize {
        let mut inner = self.lock();
        inner
            .frozen
            .retain(|_, at| now.duration_since(*at).unwrap_or_default() < duration);
        inner.frozen.len()
    }

    /// Is there a pending change (and not yet expired)?
    pub fn has_pending(&self, now: SystemTime) -> bool {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        inner.pending.is_some()
    }

    fn drop_expired(&self, inner: &mut Inner, now: SystemTime) {
        let expired = inner.pending.as_ref().is_some_and(|p| {
            now.duration_since(p.registered_at)
                .is_ok_and(|age| age >= self.pending_ttl)
        });
        if expired {
            inner.pending = None;
        }
    }

    fn remember(inner: &mut Inner, operation_id: String, outcome: Finished) {
        inner.finished.retain(|(id, _)| *id != operation_id);
        inner.finished.push_back((operation_id, outcome));
        while inner.finished.len() > FINISHED_MEMORY {
            inner.finished.pop_front();
        }
    }

    fn finished(inner: &Inner, operation_id: &str) -> Option<Finished> {
        inner
            .finished
            .iter()
            .find(|(id, _)| id == operation_id)
            .map(|(_, f)| *f)
    }

    /// Phase 1 as a participant: leaves the change pending. Repeating the same `operationId` changes
    /// nothing. If the pending one is one **this node coordinates** and the one arriving has the lower `operationId` (the
    /// oldest), this node yields: discards its own and accepts the other, so two simultaneous
    /// coordinators always have a winner (D94).
    pub fn register(
        &self,
        operation_id: &str,
        change: Change,
        now: SystemTime,
    ) -> Result<(), MembershipError> {
        self.register_as(operation_id, change, now, false)
    }

    /// Phase 1 of whoever coordinates the change.
    pub fn register_coordinating(
        &self,
        operation_id: &str,
        change: Change,
        now: SystemTime,
    ) -> Result<(), MembershipError> {
        self.register_as(operation_id, change, now, true)
    }

    /// Is the change with that `operationId` still pending here? Whoever coordinates checks before the
    /// commit: an older participant may have displaced it.
    pub fn is_pending(&self, operation_id: &str) -> bool {
        self.lock()
            .pending
            .as_ref()
            .is_some_and(|p| p.operation_id == operation_id)
    }

    fn register_as(
        &self,
        operation_id: &str,
        change: Change,
        now: SystemTime,
        coordinated: bool,
    ) -> Result<(), MembershipError> {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        if let Some(pending) = &inner.pending {
            if pending.operation_id == operation_id {
                return Ok(());
            }
            let yields =
                !coordinated && pending.coordinated && operation_id < pending.operation_id.as_str();
            if !yields {
                return Err(MembershipError::Conflict(
                    "There is another member change in progress.".into(),
                ));
            }
            let displaced = inner.pending.take().expect("checked above");
            Self::remember(&mut inner, displaced.operation_id, Finished::RolledBack);
        }
        if Self::finished(&inner, operation_id).is_some() {
            return Ok(());
        }
        let Some(table) = &inner.table else {
            return Err(MembershipError::NoTable);
        };
        match &change {
            Change::Add(member) if table.member(&member.node_id).is_some() => {
                return Err(MembershipError::Conflict(format!(
                    "Node {} is already a member.",
                    member.node_id
                )));
            }
            Change::Remove { node_id } | Change::Exclude { node_id }
                if table.member(node_id).is_none() =>
            {
                return Err(MembershipError::Conflict(format!(
                    "Node {node_id} is not a member."
                )));
            }
            _ => {}
        }
        inner.pending = Some(Pending {
            operation_id: operation_id.to_owned(),
            change,
            registered_at: now,
            coordinated,
        });
        Ok(())
    }

    /// The pending change with that `operationId` (for the commit and rollback to know what they count).
    pub fn pending_change(&self, operation_id: &str) -> Option<Change> {
        let inner = self.lock();
        inner
            .pending
            .as_ref()
            .filter(|p| p.operation_id == operation_id)
            .map(|p| p.change.clone())
    }

    /// Phase 2: makes the pending change effective and returns the new table.
    pub fn commit(
        &self,
        operation_id: &str,
        now: SystemTime,
    ) -> Result<MemberTable, MembershipError> {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        let matches = inner
            .pending
            .as_ref()
            .is_some_and(|p| p.operation_id == operation_id);
        if !matches {
            return match Self::finished(&inner, operation_id) {
                Some(Finished::Committed) => {
                    inner.table.clone().ok_or(MembershipError::UnknownOperation)
                }
                Some(Finished::RolledBack) => Err(MembershipError::Conflict(
                    "The operation was cancelled (rollback).".into(),
                )),
                None => Err(MembershipError::UnknownOperation),
            };
        }
        let pending = inner.pending.take().expect("checked above");
        let table = self.apply_change(&mut inner, pending.change, now)?;
        Self::remember(&mut inner, operation_id.to_owned(), Finished::Committed);
        Ok(table)
    }

    /// Applies the change to the table: the same thing for the commit and for the change that arrives through the channel.
    /// Raises the version, stores the freezing and who left, rebuilds the ring and returns the new table.
    fn apply_change(
        &self,
        inner: &mut Inner,
        change: Change,
        now: SystemTime,
    ) -> Result<MemberTable, MembershipError> {
        let excluded = matches!(change, Change::Exclude { .. });
        let mut frozen = std::mem::take(&mut inner.frozen);
        let mut inner_departed = std::mem::take(&mut inner.departed);
        let mut excluded_members = std::mem::take(&mut inner.excluded);
        let Some(table) = inner.table.as_mut() else {
            inner.excluded = excluded_members;
            inner.departed = inner_departed;
            inner.frozen = frozen;
            return Err(MembershipError::NoTable);
        };
        match change {
            Change::Add(member) => {
                inner_departed.retain(|d| d.addresses != member.addresses || d.port != member.port);
                excluded_members
                    .retain(|d| d.addresses != member.addresses || d.port != member.port);
                table.members.push(member);
            }
            Change::Remove { node_id } | Change::Exclude { node_id } => {
                if excluded {
                    frozen.insert(node_id.clone(), now);
                }
                if let Some(left) = table.members.iter().find(|m| m.node_id == node_id) {
                    if excluded {
                        excluded_members.push(left.clone());
                        if excluded_members.len() > 64 {
                            excluded_members.remove(0);
                        }
                    }
                    inner_departed.push(left.clone());
                    if inner_departed.len() > 64 {
                        inner_departed.remove(0);
                    }
                }
                table.members.retain(|m| m.node_id != node_id);
            }
        }
        table.version += 1;
        table.sort();
        let table = table.clone();
        inner.excluded = excluded_members;
        inner.departed = inner_departed;
        inner.frozen = frozen;
        self.rebuild_ring(inner);
        Ok(table)
    }

    /// An already-effected change that a peer said through the channel (`membership` frame): the node that lost the
    /// register or the commit (D126) applies it without depending on the pending change. Only the **next** version
    /// after the table's applies; an equal or lower one was already applied and is ignored, and one ahead stays
    /// stored until the earlier ones arrive. Returns what was applied now, in order:
    /// `(operationId, version, change)`.
    pub fn learn(
        &self,
        operation_id: &str,
        version: u64,
        change: Change,
        now: SystemTime,
    ) -> Vec<(String, u64, Change)> {
        let mut inner = self.lock();
        let Some(current) = inner.table.as_ref().map(|t| t.version) else {
            return Vec::new();
        };
        if version <= current {
            return Vec::new();
        }
        inner
            .held
            .insert(version, (operation_id.to_owned(), change));
        while inner.held.len() > HELD_LIMIT {
            inner.held.pop_last();
        }
        self.apply_held(&mut inner, now)
    }

    /// Applies the stored changes that are now the next ones after the table (the normal commit may have
    /// advanced the version while they waited). Same return as [`Membership::learn`].
    pub fn learn_held(&self, now: SystemTime) -> Vec<(String, u64, Change)> {
        let mut inner = self.lock();
        self.apply_held(&mut inner, now)
    }

    fn apply_held(&self, inner: &mut Inner, now: SystemTime) -> Vec<(String, u64, Change)> {
        let mut applied = Vec::new();
        while let Some(current) = inner.table.as_ref().map(|t| t.version) {
            inner.held.retain(|version, _| *version > current);
            let Some((operation_id, change)) = inner.held.remove(&(current + 1)) else {
                break;
            };
            let fits = inner.table.as_ref().is_some_and(|table| match &change {
                Change::Add(member) => table.member(&member.node_id).is_none(),
                Change::Remove { node_id } | Change::Exclude { node_id } => {
                    table.member(node_id).is_some()
                }
            });
            if !fits {
                tracing::warn!(%operation_id, version = current + 1, "member change that does not fit the table: ignored");
                continue;
            }
            let Ok(table) = self.apply_change(inner, change.clone(), now) else {
                break;
            };
            if inner
                .pending
                .as_ref()
                .is_some_and(|p| p.operation_id == operation_id)
            {
                inner.pending = None;
            }
            Self::remember(inner, operation_id.clone(), Finished::Committed);
            applied.push((operation_id, table.version, change));
        }
        applied
    }

    /// Discards the pending change (only before the commit).
    pub fn rollback(&self, operation_id: &str, now: SystemTime) -> Result<(), MembershipError> {
        let mut inner = self.lock();
        self.drop_expired(&mut inner, now);
        let matches = inner
            .pending
            .as_ref()
            .is_some_and(|p| p.operation_id == operation_id);
        if matches {
            inner.pending = None;
            Self::remember(&mut inner, operation_id.to_owned(), Finished::RolledBack);
            return Ok(());
        }
        match Self::finished(&inner, operation_id) {
            Some(Finished::RolledBack) => Ok(()),
            Some(Finished::Committed) => Err(MembershipError::Conflict(
                "The operation has already taken effect: there is no rollback after the commit."
                    .into(),
            )),
            None => Err(MembershipError::UnknownOperation),
        }
    }
}

#[cfg(test)]
mod tests;
