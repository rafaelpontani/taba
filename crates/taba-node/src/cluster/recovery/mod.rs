//! The recovery of losses (doc 04, section 6.6; doc 06c, "Synchronization and recovery").
//!
//! Two sides:
//!
//! - **Who serves:** the node's `losses` table (`GET /cluster/message-loss`), the record and acknowledgement of
//!   a loss (`POST`/`DELETE /cluster/message-loss`) and the acceptance of the messages a returning
//!   node found (`POST /cluster/messages/recover`). The first two routes and the last
//!   accept whoever is not yet a member.
//! - **Who returns:** the node that finds out it was excluded, or that comes back from the drain, queries the loss
//!   list of all the nodes it reaches, looks for each message in what it still keeps and sends it to the
//!   block's responsible party; only then does it clean the local state and redo the join.
//!
//! The restoration of the past lives (doc 37, sections 4.5 and 4.6) is in [`restoration`]: every node sends the
//! whole folder of each life inside the window of the log of excluded messages to the new primary of each block,
//! and the primary checks the topic and the dead before it activates. Its end is in [`finish`]: the nodes poll each
//! other, the coordinator closes the vote, the FIFO queues are released and the folders of the lives are deleted
//! (sections 4.7 and 4.8).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::members::{Member, MemberTable};
use super::protocol::{
    LossAck, LossRecord, MessageRecord, RecoverResult, Request, Response, StreamPage, StreamQuery,
};
use super::service::ClusterService;
use super::state::NodeState;
use super::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring::Ring;
use crate::core::time;
use crate::engine::messages::{Loaded, Messages};
use crate::storage::losses::{Loss, LossStore};
use crate::telemetry::metrics::{CLUSTER_RECOVER, RECOVER_MESSAGES};

mod finish;
mod restoration;
pub use restoration::RestoreSummary;

/// The IDs a topic's journal registers as removed, and when it was read.
type CachedJournal = (Instant, Arc<HashSet<String>>);

/// What the return recovered, for the log and the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Distinct losses the queried nodes listed.
    pub losses: usize,
    /// How many of those the node still had.
    pub found: usize,
    /// How many the responsible party applied (write quorum reached).
    pub recovered: usize,
    /// All the nodes answered the query before `cluster.recovery.timeout`.
    pub complete: bool,
}

pub struct Recovery {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    losses: LossStore,
    /// The ceiling of lines in a page of the loss list and in a send (`cluster.sync.stream.max-items`).
    max_items: usize,
    /// The journal of each topic in S3, read for a restoration (the IDs it registers as removed), and when.
    journals: Mutex<HashMap<String, CachedJournal>>,
    /// One decision of the vote at a time (the coordinator's call and the node's own poll may meet).
    decision: tokio::sync::Mutex<()>,
}

fn disk(error: &std::io::Error) -> TabaError {
    tracing::error!(error = %error, "failed to access the loss table");
    TabaError::internal()
}

fn node_syncing() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterNodeSyncing,
        "The node is still syncing and does not accept recovered messages.",
    )
    .with_retry_after(5)
}

impl Recovery {
    pub fn new(
        service: Arc<ClusterService>,
        messages: Arc<Messages>,
        losses: LossStore,
        max_items: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            messages,
            losses,
            max_items: max_items.max(2),
            journals: Mutex::new(HashMap::new()),
            decision: tokio::sync::Mutex::new(()),
        })
    }

    /// This node's loss table.
    pub fn losses(&self) -> &LossStore {
        &self.losses
    }

    fn count(&self, def: &'static crate::telemetry::metrics::Def, label: (&str, &str), n: usize) {
        if n > 0 {
            self.service
                .telemetry()
                .registry
                .add(def, &[label], n as f64);
        }
    }

    // ---- Who serves ----

    /// Deletes the records older than `cluster.message-loss.ttl`.
    fn expire(&self) {
        let ttl =
            u64::try_from(self.service.rules().message_loss_ttl.as_millis()).unwrap_or(u64::MAX);
        let cutoff = self.service.clock().now_millis().saturating_sub(ttl);
        if let Err(e) = self.losses.expire(cutoff) {
            tracing::warn!(error = %e, "failed to expire the loss table");
        }
    }

    /// `GET /cluster/message-loss`: the `losses` table, in ID order.
    pub fn serve_losses(&self, query: &StreamQuery) -> Result<Response, TabaError> {
        self.expire();
        let limit = query
            .limit
            .map_or(self.max_items, |n| (n as usize).clamp(2, self.max_items));
        let rows = self
            .losses
            .list(query.from.as_deref(), limit + 1)
            .map_err(|e| disk(&e))?;
        let more = rows.len() > limit;
        let items = rows
            .into_iter()
            .take(limit)
            .filter(|loss| query.to.as_deref().is_none_or(|to| loss.id.as_str() <= to))
            .map(|loss| json!({ "id": loss.id, "topic": loss.topic }))
            .collect();
        Ok(Response::Stream(StreamPage::new(items, more)))
    }

    /// `POST /cluster/message-loss`: another node declared a loss in a block this one also keeps.
    pub fn register_loss(&self, record: LossRecord) -> Result<Response, TabaError> {
        record.validate()?;
        let detected_ms = time::parse_ms(&record.detected_at).unwrap_or(0);
        let changed = self
            .losses
            .record(&Loss {
                topic: record.topic,
                id: record.id,
                block: record.block,
                detected_ms,
            })
            .map_err(|e| disk(&e))?;
        Ok(Response::Loss(LossAck { changed }))
    }

    /// `DELETE /cluster/message-loss/{topic}/{id}`: the message was recovered on another node.
    pub fn remove_loss(&self, topic: &str, id: &str) -> Result<Response, TabaError> {
        super::protocol::validate_message_path(topic, id)?;
        let changed = self.losses.remove(topic, id).map_err(|e| disk(&e))?;
        Ok(Response::Loss(LossAck { changed }))
    }

    /// Stores the recovered message on this node, without the two phases: the message does not exist on the owners,
    /// nobody competes for it. The invisibility is zeroed and the receipt of an old delivery disappears;
    /// the `deliveryCount` stays.
    async fn apply_local(
        &self,
        record: &MessageRecord,
        durable: bool,
    ) -> Result<Loaded, TabaError> {
        let messages = self.messages.clone();
        let topic = record.topic.clone();
        let mut meta = record.to_meta();
        meta.invisible_until_ms = 0;
        meta.receipt_token = None;
        let body = record.body.clone();
        tokio::task::spawn_blocking(move || messages.apply_load_with(&topic, meta, body, durable))
            .await
            .map_err(|_| TabaError::internal())?
    }

    /// `POST /cluster/messages/recover`. Each record goes to the nodes responsible for the block; with
    /// `replicate` this node stores its own and sends to the others, and the row only counts as applied when
    /// `write.quorum` nodes stored it. Without `replicate` it only stores here (it is another responsible node's
    /// request). Whoever accepts acknowledges the loss and notifies the others (best effort).
    pub async fn accept(
        self: &Arc<Self>,
        records: Vec<MessageRecord>,
        replicate: bool,
        durable: bool,
    ) -> Result<Response, TabaError> {
        if self.service.node_state() != NodeState::Active {
            return Err(node_syncing());
        }
        let me = self.service.node_id();
        let quorum = self.service.rules().quorum.max(1) as usize;
        // Confirmed writes per record, and the responsible parties of each.
        let mut confirmed = vec![0usize; records.len()];
        let mut needed = vec![quorum; records.len()];
        let mut remote: BTreeMap<String, (Member, Vec<usize>)> = BTreeMap::new();
        let mut applied = 0usize;
        let mut skipped = 0usize;
        let mut local_ok = vec![false; records.len()];
        for (index, record) in records.iter().enumerate() {
            let position =
                self.messages
                    .position_for(&record.topic, &record.id, record.group_id.as_deref());
            let owners = self.service.owners_of(position)?;
            needed[index] = quorum.min(owners.len()).max(1);
            for owner in owners {
                if owner.node_id == me {
                    match self.apply_local(record, durable).await {
                        Ok(Loaded::Stored) => {
                            applied += 1;
                            confirmed[index] += 1;
                            local_ok[index] = true;
                        }
                        // It already existed, expired by `ttl` or the topic ceased to exist: there is nothing more
                        // to store and the row is resolved.
                        Ok(_) => {
                            skipped += 1;
                            confirmed[index] += 1;
                            local_ok[index] = true;
                        }
                        Err(e) => {
                            tracing::warn!(topic = %record.topic, id = %record.id, error = %e.message, "could not store the recovered message");
                        }
                    }
                } else if replicate {
                    remote
                        .entry(owner.node_id.clone())
                        .or_insert_with(|| (owner.clone(), Vec::new()))
                        .1
                        .push(index);
                }
            }
        }
        self.count(&RECOVER_MESSAGES, ("taba.outcome", "applied"), applied);
        self.count(&RECOVER_MESSAGES, ("taba.outcome", "skipped"), skipped);
        self.count(
            &CLUSTER_RECOVER,
            ("taba.direction", "received"),
            applied + skipped,
        );

        if !replicate {
            let count = local_ok.iter().filter(|ok| **ok).count();
            return Ok(Response::Recovered(RecoverResult::new(count as u64)));
        }
        let mut set = JoinSet::new();
        for (member, indexes) in remote.into_values() {
            let service = self.service.clone();
            let batch: Vec<MessageRecord> = indexes.iter().map(|i| records[*i].clone()).collect();
            set.spawn(async move {
                let stored = Self::send_to_owner(&service, &member, batch, durable).await;
                (indexes, stored)
            });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((indexes, stored)) = joined {
                for (index, ok) in indexes.into_iter().zip(stored) {
                    if ok {
                        confirmed[index] += 1;
                    }
                }
            }
        }
        let mut count = 0u64;
        for (index, record) in records.iter().enumerate() {
            if confirmed[index] >= needed[index] {
                count += 1;
                self.settle(&record.topic, &record.id);
            }
        }
        Ok(Response::Recovered(RecoverResult::new(count)))
    }

    /// Sends the batch to a responsible node (`replicate=false`) and says, per record, whether it stored it. If the
    /// responsible node applied only part, each record is asked for again on its own to find out which.
    async fn send_to_owner(
        service: &Arc<ClusterService>,
        owner: &Member,
        batch: Vec<MessageRecord>,
        durable: bool,
    ) -> Vec<bool> {
        let call = |records: Vec<MessageRecord>| {
            let service = service.clone();
            let owner = owner.clone();
            async move {
                let request = Request::Recover {
                    records,
                    replicate: false,
                    durable,
                };
                // Calls through the `Transport` (never the node itself, which is not responsible here): avoids
                // `accept` appearing inside its own future type.
                let rules = service.rules();
                for attempt in 1..=rules.retry_attempts.max(1) {
                    match service
                        .call_peer(&owner, request.clone(), rules.call_timeout)
                        .await
                    {
                        Ok(Response::Recovered(result)) => return Some(result.count as usize),
                        Err(CallError::Remote(e)) if e.status() < 500 => return None,
                        _ if attempt < rules.retry_attempts => {
                            tokio::time::sleep(rules.retry_backoff).await;
                        }
                        _ => {}
                    }
                }
                None
            }
        };
        match call(batch.clone()).await {
            Some(count) if count == batch.len() => vec![true; batch.len()],
            Some(0) | None if batch.len() == 1 => vec![false],
            _ => {
                let mut out = Vec::with_capacity(batch.len());
                for record in batch {
                    out.push(call(vec![record]).await == Some(1));
                }
                out
            }
        }
    }

    /// Acknowledges the loss here and notifies the other members, without waiting.
    fn settle(self: &Arc<Self>, topic: &str, id: &str) {
        if let Err(e) = self.losses.remove(topic, id) {
            tracing::warn!(error = %e, "failed to acknowledge the loss");
        }
        tracing::info!(
            event.name = "taba.message.recovered",
            messaging.destination.name = %topic,
            message.id = %id,
            "lost message recovered"
        );
        let Some(table) = self.service.membership().table() else {
            return;
        };
        let me = self.service.node_id();
        for member in table.members.into_iter().filter(|m| m.node_id != me) {
            let service = self.service.clone();
            let request = Request::LossRemove {
                topic: topic.to_owned(),
                id: id.to_owned(),
            };
            tokio::spawn(async move {
                let _ = service.call_member(&member, request).await;
            });
        }
    }

    // ---- Who returns ----

    /// The procedure of doc 04, section 6.6, steps 1 to 5: returns the summary when the loss list
    /// was handled or the `cluster.recovery.timeout` deadline ran out. Cleaning the local state and
    /// the new join (step 6) are up to the caller.
    pub async fn run(self: &Arc<Self>, peers: Vec<Member>) -> Summary {
        self.run_with(peers, false).await
    }

    /// Like `run`, for the process restart (PRB-048): the previous life's disk does not get in the way of
    /// anything, so what each answering node's list asks for is returned **right away**, and only the
    /// mute node is called again until the deadline.
    pub async fn run_streaming(self: &Arc<Self>, peers: Vec<Member>) -> Summary {
        self.run_with(peers, true).await
    }

    async fn run_with(self: &Arc<Self>, peers: Vec<Member>, early: bool) -> Summary {
        let rules = self.service.rules().clone();
        let deadline = Instant::now() + rules.recovery_timeout;
        let mut summary = Summary::default();
        if peers.is_empty() {
            summary.complete = true;
            return summary;
        }

        // Step 1: the loss list of all the nodes that answer. Whoever did not answer is
        // queried again every `cluster.bootstrap.retry.interval`, until the deadline.
        let mut pending = peers;
        let mut lost: BTreeSet<(String, String)> = BTreeSet::new();
        let mut handled: BTreeSet<(String, String)> = BTreeSet::new();
        let mut newest: Option<MemberTable> = None;
        loop {
            let mut failed = Vec::new();
            for peer in pending {
                match self.fetch_losses(&peer).await {
                    Some(found) => lost.extend(found),
                    None => {
                        failed.push(peer);
                        continue;
                    }
                }
                if let Some(table) = self.fetch_table(&peer).await
                    && newest.as_ref().is_none_or(|n| table.version > n.version)
                {
                    newest = Some(table);
                }
            }
            if early {
                let fresh: Vec<_> = lost.difference(&handled).cloned().collect();
                self.send_found(&fresh, newest.clone(), deadline, &mut summary)
                    .await;
                handled.extend(fresh);
            }
            pending = failed;
            if pending.is_empty() {
                summary.complete = true;
                break;
            }
            let now = Instant::now();
            if now >= deadline || self.service.cancel_token().is_cancelled() {
                tracing::warn!(
                    missing = pending.len(),
                    "the recovery went on without the loss list of all the nodes"
                );
                break;
            }
            let wait = rules.bootstrap_retry_interval.min(deadline - now);
            tokio::select! {
                () = self.service.cancel_token().cancelled() => break,
                () = tokio::time::sleep(wait) => {}
            }
        }
        summary.losses = lost.len();
        let rest: Vec<_> = lost.difference(&handled).cloned().collect();
        self.send_found(&rest, newest, deadline, &mut summary).await;
        tracing::info!(
            losses = summary.losses,
            found = summary.found,
            recovered = summary.recovered,
            complete = summary.complete,
            "loss recovery completed"
        );
        summary
    }

    /// Steps 2 and 3: looks for each loss in what the node keeps and sends what it found to the responsible party.
    async fn send_found(
        &self,
        lost: &[(String, String)],
        table: Option<MemberTable>,
        deadline: Instant,
        summary: &mut Summary,
    ) {
        if lost.is_empty() {
            return;
        }
        let rules = self.service.rules().clone();
        let table = table.or_else(|| self.service.membership().table());
        let Some(table) = table else {
            return;
        };
        // The destination is never the node itself: a peer's table may still list it (the exit
        // commit did not reach everyone), but it no longer serves anyone. On a process restart, the
        // table still has the old ID, at the same address (D111).
        let me = self.service.node_id();
        let local = self.service.local();
        let is_me = |m: &&Member| {
            m.node_id == me
                || (m.port == local.port && m.addresses.iter().any(|a| local.addresses.contains(a)))
        };
        let ring = Ring::new(
            table
                .members
                .iter()
                .filter(|m| !is_me(m))
                .map(|m| m.node_id.as_str()),
            rules.virtual_nodes,
        );
        let mut by_owner: BTreeMap<String, Vec<MessageRecord>> = BTreeMap::new();
        for (topic, id) in lost {
            let messages = self.messages.clone();
            let (t, i) = (topic.clone(), id.clone());
            // What the node keeps today counts first; then, the previous life's disk (PRB-048).
            let held = tokio::task::spawn_blocking(move || match messages.held_message(&t, &i) {
                Ok(None) => messages.held_previous_life(&t, &i),
                other => other,
            })
            .await;
            let Ok(Ok(Some(found))) = held else {
                continue;
            };
            summary.found += 1;
            let position = self
                .messages
                .position_for(topic, id, found.meta.group_id.as_deref());
            let Some(primary) = ring.primary(position) else {
                continue;
            };
            by_owner
                .entry(primary.to_owned())
                .or_default()
                .push(MessageRecord::from_stored(topic, found.meta, found.body));
        }
        let sent: usize = by_owner.values().map(Vec::len).sum();
        self.count(&RECOVER_MESSAGES, ("taba.outcome", "sent"), sent);
        self.count(&CLUSTER_RECOVER, ("taba.direction", "sent"), sent);
        for (owner, records) in by_owner {
            let Some(member) = table.member(&owner).cloned() else {
                continue;
            };
            for chunk in records.chunks(self.max_items) {
                summary.recovered += self.send_chunk(&member, chunk, deadline).await;
            }
        }
    }

    /// A full pass through a node's loss list, page by page. `None` if the node did not
    /// answer.
    async fn fetch_losses(&self, peer: &Member) -> Option<Vec<(String, String)>> {
        let mut found = Vec::new();
        let mut from: Option<String> = None;
        loop {
            let request = Request::MessageLoss(StreamQuery {
                from: from.clone(),
                to: None,
                limit: None,
            });
            let page = match self
                .service
                .call_peer(peer, request, self.service.rules().call_timeout)
                .await
            {
                Ok(Response::Stream(page)) => page,
                _ => return None,
            };
            for item in &page.items {
                if let (Some(id), Some(topic)) = (
                    item.get("id").and_then(|v| v.as_str()),
                    item.get("topic").and_then(|v| v.as_str()),
                ) {
                    found.push((topic.to_owned(), id.to_owned()));
                }
            }
            match (page.end.has_more, page.end.last) {
                (true, Some(last)) if from.as_deref() != Some(last.as_str()) => from = Some(last),
                _ => return Some(found),
            }
        }
    }

    /// The member table the node sees (to find each block's responsible party today).
    async fn fetch_table(&self, peer: &Member) -> Option<MemberTable> {
        match self
            .service
            .call_peer(peer, Request::ListNodes, self.service.rules().call_timeout)
            .await
        {
            Ok(Response::Members(table)) => Some(table),
            _ => None,
        }
    }

    /// Sends a batch to the responsible party and repeats everything (the resend is idempotent) every
    /// `cluster.bootstrap.retry.interval` until the deadline. Returns how many rows were applied.
    async fn send_chunk(
        &self,
        owner: &Member,
        chunk: &[MessageRecord],
        deadline: Instant,
    ) -> usize {
        let interval: Duration = self.service.rules().bootstrap_retry_interval;
        loop {
            let request = Request::Recover {
                records: chunk.to_vec(),
                replicate: true,
                durable: false,
            };
            match self
                .service
                .call_peer(owner, request, self.service.rules().call_timeout * 4)
                .await
            {
                Ok(Response::Recovered(result)) if result.count as usize == chunk.len() => {
                    return chunk.len();
                }
                outcome => {
                    let now = Instant::now();
                    if now >= deadline || self.service.cancel_token().is_cancelled() {
                        tracing::warn!(owner = %owner.node_id, ?outcome, "the recovered batch was not accepted in full");
                        return match outcome {
                            Ok(Response::Recovered(result)) => result.count as usize,
                            _ => 0,
                        };
                    }
                    tokio::select! {
                        () = self.service.cancel_token().cancelled() => {}
                        () = tokio::time::sleep(interval.min(deadline - now)) => {}
                    }
                }
            }
        }
    }
}
