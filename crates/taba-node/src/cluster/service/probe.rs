//! The probe of the old IPs and the regroup of the groups that took over the disk (doc 37, section 4.14).
//!
//! Two groups that came up without talking to each other never excluded one another, so the reunion by exclusion
//! (`check_reunion`) does not find them. While the cluster has fewer members than the table of the previous life, each
//! node tries the old IPs that are not in its table; the group that answers with a table of its own is another
//! group. The smaller one (D101) **loses**, but it does not wipe anything: its disk goes to the folder of its life
//! (section 4.5), its definitions are offered to the winner and it redoes the join with a new ID.

use super::*;

impl ClusterService {
    /// Reads the table of the previous life once, before anything moves its manifest (the root of the disk or the
    /// folder of the life). The probe looks for these addresses.
    pub(super) fn remember_previous_table(&self) {
        self.previous_table.get_or_init(|| {
            self.replication
                .get()
                .and_then(|replication| replication.recovery.latest_manifest())
                .map(|manifest| manifest.members)
                .unwrap_or_default()
        });
    }

    /// The probe ended with nodes of the previous table still missing: the `members-missing` condition (the node
    /// that is missing enters later through the normal join, and the condition goes away with the table complete).
    pub fn members_missing(&self) -> bool {
        self.probe_gave_up.load(Ordering::SeqCst)
            && self.previous_table.get().is_some_and(|previous| {
                self.membership.table().is_none_or(|table| {
                    table
                        .members
                        .iter()
                        .filter(|m| m.state == MemberState::Active)
                        .count()
                        < previous.len()
                })
            })
    }

    /// The probe of the old IPs (section 4.14): every `cluster.restore.probe.interval`, up to
    /// `cluster.restore.probe.timeout` after the start, while the table has fewer active members than the previous one.
    pub async fn run_probe(self: Arc<Self>) {
        let started = Instant::now();
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => return,
                () = tokio::time::sleep(self.rules.probe_interval) => {}
            }
            let Some(previous) = self.previous_table.get() else {
                continue;
            };
            // A previous life of one node (or none) has no one to look for.
            if previous.len() < 2 || self.probe_round(previous).await {
                return;
            }
            if started.elapsed() >= self.rules.probe_timeout {
                tracing::warn!(
                    expected = previous.len(),
                    "the probe of the old addresses ended and the table is still incomplete: the missing nodes enter through the normal join"
                );
                self.probe_gave_up.store(true, Ordering::SeqCst);
                return;
            }
        }
    }

    /// One round: returns whether the table is complete. An answer from another group, if this side is the
    /// loser, starts the regroup.
    async fn probe_round(self: &Arc<Self>, previous: &[restore::ManifestMember]) -> bool {
        if self.node.state() != NodeState::Active
            || self.node.drain_state() != crate::cluster::state::DrainState::Active
        {
            return false;
        }
        let Some(mine) = self.membership.table() else {
            return false;
        };
        let active = mine
            .members
            .iter()
            .filter(|m| m.state == MemberState::Active)
            .count();
        if active >= previous.len() {
            return true;
        }
        let known: HashSet<&str> = mine
            .members
            .iter()
            .flat_map(|m| m.addresses.iter().map(String::as_str))
            .collect();
        let me = self.node_id();
        for old in previous {
            for address in old.addresses.iter().filter(|a| !known.contains(a.as_str())) {
                let to = PeerAddr::new(address.clone(), old.port);
                let Ok(Response::Members(theirs)) = self
                    .call_addr(&to, Request::ListNodes, self.rules.call_timeout)
                    .await
                else {
                    continue;
                };
                if is_other_group(&me, &mine, &theirs) && loses_reunion(&mine, &theirs) {
                    tracing::warn!(
                        node_id = %me,
                        other = %address,
                        "two groups took over the disk without talking and met again: this side is the smaller and regroups"
                    );
                    self.regroup(theirs).await;
                    return false;
                }
                break;
            }
        }
        false
    }

    /// The losing group's node leaves the other group's table to the winner: its disk becomes the folder of a life
    /// (so what the group accepted while it was apart is restored and checked against the dead), its definitions go
    /// to the winner and it joins again with a new ID. Nothing is wiped before the move succeeds.
    async fn regroup(self: &Arc<Self>, winner: MemberTable) {
        if self.node.state() != NodeState::Active
            || self.node.drain_state() != crate::cluster::state::DrainState::Active
            || self.excluded.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let Some(replication) = self.replication.get().cloned() else {
            self.excluded.store(false, Ordering::SeqCst);
            return;
        };
        // The manifest that goes with the disk says when this life was last seen.
        self.write_manifest();
        let messages = replication.messages.messages().clone();
        match tokio::task::spawn_blocking(move || messages.set_aside_old_disk()).await {
            Ok(Ok(_)) => {}
            other => {
                tracing::warn!(
                    ?other,
                    "could not set aside the disk of the group that lost: it stays as it is"
                );
                self.excluded.store(false, Ordering::SeqCst);
                return;
            }
        }
        let stored = replication.definitions.stored_changes().await;
        self.offered_definitions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(stored);
        replication.reset_local().await;
        let new_id = id::new_at(self.clock.now_millis());
        self.local
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .node_id = new_id.clone();
        self.node.set_node_id(new_id.clone());
        self.membership.clear();
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        // The table that answered is where the node joins (the old IPs of the manifest are the ones of this group).
        *self.join_hint.lock().unwrap_or_else(|e| e.into_inner()) = Some(winner);
        self.node.set_state(NodeState::Joining);
        replication.channels.sync_members();
        tracing::info!(node_id = %new_id, "the group lost the reunion: the node joins the other one as a new node");
        let me = self.clone();
        tokio::spawn(async move {
            let result = me.bootstrap().await;
            me.excluded.store(false, Ordering::SeqCst);
            if let Err(e) = result {
                tracing::error!(error = %e, "the node could not get back into the cluster");
            }
        });
    }
}

/// The table is another group's: it has members, none of them is in this one and this node is not in it.
pub(super) fn is_other_group(me: &str, mine: &MemberTable, theirs: &MemberTable) -> bool {
    !theirs.members.is_empty()
        && theirs.member(me).is_none()
        && theirs
            .members
            .iter()
            .all(|m| mine.member(&m.node_id).is_none())
}
