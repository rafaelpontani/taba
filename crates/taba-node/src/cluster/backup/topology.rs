//! What the backup needs to know about the cluster, behind a trait: who owns what and who is up.

use std::sync::Arc;

use crate::cluster::members::MemberState;
use crate::cluster::protocol::{BACKUP_SAVED_MAX_ITEMS, BackupSaved, BackupSavedItem, Request};
use crate::cluster::service::ClusterService;
use crate::cluster::state::NodeState;
use crate::core::ring::definition_position;
use crate::storage::backup::SavedRef;

pub trait Topology: Send + Sync + 'static {
    fn node_id(&self) -> String;

    /// The IPs the node presents to the others.
    fn addresses(&self) -> Vec<String>;

    /// The node is active (already synchronized): only then does it own anything.
    fn is_active(&self) -> bool;

    /// The node's cluster port.
    fn port(&self) -> u16;

    /// The node is the primary of the ring position (the owner of a message).
    fn owns_position(&self, position: u64) -> bool;

    /// The nodes responsible for the position, the primary first.
    fn owners(&self, position: u64) -> Vec<String>;

    /// The node is the definition's **manager** (its owner, as in `09-topicos.md`): `kind` is `topic` or
    /// `token`.
    fn manages_definition(&self, kind: &str, key: &str) -> bool;

    /// The `id`s of the table's members, itself included.
    fn members(&self) -> Vec<String>;

    /// Does the node talk to `node_id` right now (the channel is open and heard the peer a moment ago)?
    fn reaches(&self, node_id: &str) -> bool;

    /// Tells `node_id`, a replica, which of the topic's messages are already in the bucket (without waiting for an
    /// answer: the notice is best effort, the replica only gets the eviction with it).
    fn send_saved(&self, _node_id: &str, _topic: &str, _items: Vec<(String, SavedRef)>) {}
}

/// The real cluster, through the `ClusterService`.
pub struct ClusterTopology(pub Arc<ClusterService>);

impl Topology for ClusterTopology {
    fn node_id(&self) -> String {
        self.0.node_id()
    }

    fn addresses(&self) -> Vec<String> {
        self.0.local().addresses
    }

    fn port(&self) -> u16 {
        self.0.local().port
    }

    fn is_active(&self) -> bool {
        self.0.node_state() == NodeState::Active
    }

    fn owns_position(&self, position: u64) -> bool {
        self.is_active()
            && self
                .0
                .membership()
                .ring()
                .primary(position)
                .is_some_and(|primary| primary == self.0.node_id())
    }

    fn owners(&self, position: u64) -> Vec<String> {
        let factor = self.0.rules().replication_factor.max(1) as usize;
        self.0
            .membership()
            .ring()
            .owners(position, factor)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn manages_definition(&self, kind: &str, key: &str) -> bool {
        if !self.is_active() {
            return false;
        }
        // The owner is the first **active** node of the position: a node that is still syncing does not have the
        // complete copy (the same criterion as `Definitions::owners`).
        let Ok(owners) = self.0.owners_of(definition_position(kind, key)) else {
            return false;
        };
        owners
            .iter()
            .find(|m| m.state == MemberState::Active)
            .is_some_and(|m| m.node_id == self.0.node_id())
    }

    fn members(&self) -> Vec<String> {
        self.0
            .membership()
            .table()
            .map(|t| t.members.into_iter().map(|m| m.node_id).collect())
            .unwrap_or_default()
    }

    fn reaches(&self, node_id: &str) -> bool {
        self.0
            .replication()
            .is_some_and(|r| r.channels.is_live(node_id))
    }

    fn send_saved(&self, node_id: &str, topic: &str, items: Vec<(String, SavedRef)>) {
        let Some(member) = self
            .0
            .membership()
            .table()
            .and_then(|t| t.member(node_id).cloned())
        else {
            return;
        };
        for chunk in items.chunks(BACKUP_SAVED_MAX_ITEMS) {
            let request = Request::BackupSaved(BackupSaved {
                topic: topic.to_owned(),
                items: chunk
                    .iter()
                    .map(|(id, saved)| BackupSavedItem {
                        id: id.clone(),
                        segment: saved.segment.clone(),
                        offset: saved.offset,
                        len: saved.len,
                        saved_at_ms: saved.saved_at_ms,
                    })
                    .collect(),
            });
            let (service, member) = (self.0.clone(), member.clone());
            tokio::spawn(async move {
                // Best effort: without the notice the replica just cannot do the eviction.
                let _ = service.call_member(&member, request).await;
            });
        }
    }
}
