//! The restoration of the past lives (doc 37, sections 4.5 and 4.6), the two sides of it:
//!
//! - **Who sends:** every node reads the folder of each life it keeps (`${storage.dir}/_lives/<old ID>`) whose
//!   last activity is inside the window of the cluster's log of excluded messages ([`restore::life_restorable`])
//!   and sends the messages, with their original ID, to the **new primary** of each block. The node decides
//!   nothing about a message: it only sends. What the primary confirms is deleted from the folder of the life,
//!   which is how the node marks it as sent.
//! - **Who receives:** the new primary checks the topic (deleted, or recreated with another `topicId`), then the
//!   dead, from the cheapest source to the dearest (its own log, the log of the other present nodes, the journal
//!   in S3), and activates what is alive through the path of the recovery (`Recover`, with the write quorum).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use serde_json::json;
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::{Recovery, node_syncing};
use crate::cluster::members::{Member, MemberState};
use crate::cluster::protocol::{MessageRecord, RecoverResult, Request, Response};
use crate::cluster::restore::{Manifest, life_restorable};
use crate::cluster::state::NodeState;
use crate::cluster::transport::CallError;
use crate::core::error::TabaError;
use crate::core::time;
use crate::engine::messages::TopicCheck;
use crate::storage::messages::MessageStore;
use crate::telemetry::metrics::RECOVER_MESSAGES;

/// How long the journal of a topic in S3, once read, serves the next batches of the same restoration.
const JOURNAL_CACHE: std::time::Duration = std::time::Duration::from_secs(30);

/// What a restoration pass did, for the log and the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestoreSummary {
    /// Lives found in the folder of the lives.
    pub lives: usize,
    /// Lives outside the window of the log (they follow the path of the loss list).
    pub outside_window: usize,
    /// Messages sent to a primary.
    pub sent: usize,
    /// Messages the primaries resolved (activated, already there, or discarded as dead).
    pub resolved: usize,
    /// The pass reached its end (it was not cancelled).
    pub done: bool,
}

impl RestoreSummary {
    /// Everything this node sent was resolved by the primaries: the folders of the lives can go.
    pub fn complete(&self) -> bool {
        self.done && self.sent == self.resolved
    }
}

/// A life found on disk.
pub(super) struct Life {
    pub(super) id: String,
    pub(super) dir: std::path::PathBuf,
    pub(super) manifest: Manifest,
}

impl Recovery {
    // ---- Who receives ----

    /// `POST /cluster/topics/{topic}/messages/dead`: which of the IDs this node's log of excluded messages has.
    pub async fn serve_dead(&self, topic: String, ids: Vec<String>) -> Result<Response, TabaError> {
        let messages = self.messages.clone();
        let dead = tokio::task::spawn_blocking(move || messages.excluded_among(&topic, &ids))
            .await
            .map_err(|_| TabaError::internal())?;
        Ok(Response::Json(json!({ "dead": dead })))
    }

    /// `POST /cluster/topics/{topic}/messages/restore`: this node is the primary of the block of these messages of a
    /// past life. The response counts the messages **resolved** (activated, already here, or discarded), so the
    /// sender can tell the batch is settled.
    pub async fn accept_restore(
        self: &Arc<Self>,
        topic: String,
        topic_id: Option<String>,
        records: Vec<MessageRecord>,
    ) -> Result<Response, TabaError> {
        if self.service.node_state() != NodeState::Active {
            return Err(node_syncing());
        }
        let total = records.len();
        // Before everything: the topic. A deleted or recreated topic discards the whole batch, with no query.
        if self.messages.check_topic(&topic, topic_id.as_deref()) != TopicCheck::Alive {
            self.count(&RECOVER_MESSAGES, ("taba.outcome", "discarded"), total);
            return Ok(Response::Recovered(RecoverResult::new(total as u64)));
        }
        // First source: this node's own log.
        let messages = self.messages.clone();
        let name = topic.clone();
        let alive = tokio::task::spawn_blocking(move || {
            records
                .into_iter()
                .filter(|r| !messages.is_excluded(&name, &r.id))
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|_| TabaError::internal())?;
        // The other sources: the other nodes and S3.
        let ids: Vec<String> = alive.iter().map(|r| r.id.clone()).collect();
        let dead = self.dead_elsewhere(&topic, &ids).await;
        let survivors: Vec<MessageRecord> = alive
            .into_iter()
            .filter(|r| !dead.contains(&r.id))
            .collect();
        let discarded = total - survivors.len();
        self.count(&RECOVER_MESSAGES, ("taba.outcome", "discarded"), discarded);
        if survivors.is_empty() {
            return Ok(Response::Recovered(RecoverResult::new(total as u64)));
        }
        // What is alive is activated by the path of the recovery: stored here, replicated to the other owners
        // with the write quorum, and `Existing` is skipped.
        match self.accept(survivors, true, true).await? {
            Response::Recovered(result) => Ok(Response::Recovered(RecoverResult::new(
                discarded as u64 + result.count,
            ))),
            other => Ok(other),
        }
    }

    /// The IDs among `ids` that the other present nodes or the journal in S3 say are dead. A source that does not
    /// answer is evidence that is absent: the primary decides with the others and leaves a warning.
    async fn dead_elsewhere(self: &Arc<Self>, topic: &str, ids: &[String]) -> HashSet<String> {
        let mut dead = HashSet::new();
        if ids.is_empty() {
            return dead;
        }
        let me = self.service.node_id();
        let peers: Vec<Member> = self
            .service
            .membership()
            .table()
            .map(|table| {
                table
                    .members
                    .into_iter()
                    .filter(|m| m.node_id != me && m.state == MemberState::Active)
                    .collect()
            })
            .unwrap_or_default();
        // Every present node, not only the owners of the block: the log lives in the nodes, not in the ring.
        for chunk in ids.chunks(self.max_items) {
            let mut set = JoinSet::new();
            for peer in &peers {
                let service = self.service.clone();
                let peer = peer.clone();
                let request = Request::Dead {
                    topic: topic.to_owned(),
                    ids: chunk.to_vec(),
                };
                set.spawn(async move {
                    let timeout = service.rules().call_timeout;
                    let answer = service.call_peer(&peer, request, timeout).await;
                    (peer.node_id.clone(), answer)
                });
            }
            while let Some(Ok((node, answer))) = set.join_next().await {
                match answer {
                    Ok(Response::Json(value)) => {
                        let found = value["dead"].as_array().into_iter().flatten();
                        dead.extend(found.filter_map(|v| v.as_str().map(str::to_owned)));
                    }
                    other => {
                        tracing::warn!(node = %node, ?other, topic = %topic, "a node did not answer the query of the dead; deciding without it");
                    }
                }
            }
        }
        if let Some(removed) = self.journal_removed(topic).await {
            dead.extend(ids.iter().filter(|id| removed.contains(*id)).cloned());
        }
        dead
    }

    /// The IDs the journal in S3 registers as removed for the topic; `None` without a backup or with S3 down.
    async fn journal_removed(&self, topic: &str) -> Option<Arc<HashSet<String>>> {
        let backup = self.service.backup()?.backup();
        let fresh = {
            let cache = self.journals.lock().unwrap_or_else(|e| e.into_inner());
            cache
                .get(topic)
                .filter(|(at, _)| at.elapsed() < JOURNAL_CACHE)
                .map(|(_, removed)| removed.clone())
        };
        if let Some(removed) = fresh {
            return Some(removed);
        }
        let Some(removed) = backup.removed_in_journal(topic).await else {
            tracing::warn!(topic = %topic, "S3 did not answer the query of the dead; deciding with the other sources");
            return None;
        };
        let removed = Arc::new(removed);
        self.journals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(topic.to_owned(), (Instant::now(), removed.clone()));
        Some(removed)
    }

    // ---- Who sends ----

    /// The cut-off date of the cluster's log: the latest start of the recording among the active members (doc 37,
    /// section 4.4). `None` if a member does not tell (it runs an earlier version, or does not answer): the log
    /// cannot be trusted and no life is restorable.
    pub async fn cluster_cutoff(self: &Arc<Self>) -> Option<u64> {
        let table = self.service.membership().table()?;
        let mut cutoff: Option<u64> = None;
        for member in table
            .members
            .iter()
            .filter(|m| m.state == MemberState::Active)
        {
            let since = if self.service.is_self(member) {
                Some(self.messages.excluded_since())
            } else {
                let timeout = self.service.rules().call_timeout;
                match self
                    .service
                    .call_peer(member, Request::Status, timeout)
                    .await
                {
                    Ok(Response::Status(status)) => {
                        status["excludedSince"].as_str().and_then(time::parse_ms)
                    }
                    _ => None,
                }
            };
            let since = since?;
            cutoff = Some(cutoff.map_or(since, |c| c.max(since)));
        }
        cutoff
    }

    /// The lives this node keeps, from the oldest to the newest.
    pub(super) fn lives(&self) -> Vec<Life> {
        let root = self.messages.storage_root().join(MessageStore::LIVES);
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let mut lives: Vec<Life> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let dir = entry.path();
                let manifest = Manifest::read(&dir)?;
                let id = dir.file_name()?.to_str()?.to_owned();
                Some(Life { id, dir, manifest })
            })
            .collect();
        lives.sort_by(|a, b| {
            (a.manifest.last_seen_ms, &a.id).cmp(&(b.manifest.last_seen_ms, &b.id))
        });
        lives
    }

    /// Sends the messages of every restorable life this node keeps to the new primaries (doc 37, section 4.6).
    /// A life outside the window stays where it is and follows the path of the loss list.
    pub async fn restore_lives(self: &Arc<Self>) -> RestoreSummary {
        let mut summary = RestoreSummary::default();
        let lives = self.lives();
        summary.lives = lives.len();
        if lives.is_empty() {
            summary.done = true;
            return summary;
        }
        let cutoff = self.cluster_cutoff().await;
        let slack = self.service.rules().restore.clock_slack;
        for life in lives {
            if !life_restorable(life.manifest.last_seen_ms, cutoff, slack) {
                summary.outside_window += 1;
                tracing::info!(life = %life.id, ?cutoff, "the life is outside the window of the log of excluded messages; it follows the loss list");
                continue;
            }
            self.send_life(&life, &mut summary).await;
        }
        summary.done = !self.service.cancel_token().is_cancelled();
        tracing::info!(
            lives = summary.lives,
            outside_window = summary.outside_window,
            sent = summary.sent,
            resolved = summary.resolved,
            "restoration of the past lives completed"
        );
        summary
    }

    /// One life: incorporates its log of excluded messages and sends its messages, a window of rows at a time.
    async fn send_life(self: &Arc<Self>, life: &Life, summary: &mut RestoreSummary) {
        let store = Arc::new(self.messages.open_life(&life.dir));
        {
            let (messages, store) = (self.messages.clone(), store.clone());
            let (since, seen) = (life.manifest.excluded_since_ms, life.manifest.last_seen_ms);
            let merged =
                tokio::task::spawn_blocking(move || messages.import_life_log(&store, since, seen))
                    .await;
            if !matches!(merged, Ok(Ok(()))) {
                tracing::warn!(life = %life.id, "could not incorporate the log of excluded messages of the life");
            }
        }
        let Ok(Ok(topics)) = ({
            let store = store.clone();
            tokio::task::spawn_blocking(move || store.topics_on_disk()).await
        }) else {
            return;
        };
        for topic in topics {
            if self.service.cancel_token().is_cancelled() {
                return;
            }
            self.send_topic(&store, &topic, summary).await;
        }
    }

    async fn send_topic(
        self: &Arc<Self>,
        store: &Arc<MessageStore>,
        topic: &str,
        summary: &mut RestoreSummary,
    ) {
        let rows = {
            let (store, topic) = (store.clone(), topic.to_owned());
            tokio::task::spawn_blocking(move || {
                let topic_id = store.topic_id(&topic).ok().flatten();
                store.scan(&topic).map(|rows| (topic_id, rows))
            })
            .await
        };
        let Ok(Ok((topic_id, rows))) = rows else {
            return;
        };
        // The FIFO's short rows only serve deduplication: they have no body and are not messages to restore.
        let ids: Vec<String> = rows
            .into_iter()
            .filter(|row| row.delete_at_ms.is_none())
            .map(|row| row.id)
            .collect();
        for window in ids.chunks(self.max_items) {
            let mut by_owner: BTreeMap<String, (Member, Vec<MessageRecord>)> = BTreeMap::new();
            for id in window {
                let (messages, life, name, id) = (
                    self.messages.clone(),
                    store.clone(),
                    topic.to_owned(),
                    id.clone(),
                );
                let found =
                    tokio::task::spawn_blocking(move || messages.life_message(&life, &name, &id))
                        .await;
                let Ok(Ok(Some(found))) = found else {
                    continue;
                };
                let position = self.messages.position_for(
                    topic,
                    &found.meta.id,
                    found.meta.group_id.as_deref(),
                );
                let Some(primary) = self
                    .service
                    .owners_of(position)
                    .ok()
                    .and_then(|owners| owners.into_iter().next())
                else {
                    // No primary to send to: it counts as sent and not resolved, and the folder stays.
                    summary.sent += 1;
                    continue;
                };
                by_owner
                    .entry(primary.node_id.clone())
                    .or_insert_with(|| (primary, Vec::new()))
                    .1
                    .push(MessageRecord::from_stored(topic, found.meta, found.body));
            }
            for (owner, records) in by_owner.into_values() {
                let sent = records.len();
                summary.sent += sent;
                self.count(&RECOVER_MESSAGES, ("taba.outcome", "sent"), sent);
                match self
                    .send_restore(&owner, topic, topic_id.as_deref(), &records)
                    .await
                {
                    true => {
                        summary.resolved += sent;
                        // The primary has them on disk and replicated: this is how they are marked as sent.
                        let (store, topic) = (store.clone(), topic.to_owned());
                        let _ = tokio::task::spawn_blocking(move || {
                            for record in &records {
                                let _ = store.remove(&topic, &record.id);
                            }
                        })
                        .await;
                    }
                    false => {
                        tracing::warn!(owner = %owner.node_id, topic = %topic, "the restored batch was not accepted in full; it stays in the folder of the life");
                    }
                }
            }
        }
    }

    /// Sends a batch to the primary and repeats it (the resend is idempotent) until it answers that it resolved all
    /// of it or `cluster.recovery.timeout` runs out.
    async fn send_restore(
        self: &Arc<Self>,
        owner: &Member,
        topic: &str,
        topic_id: Option<&str>,
        records: &[MessageRecord],
    ) -> bool {
        let rules = self.service.rules();
        let (deadline, interval) = (
            Instant::now() + rules.recovery_timeout,
            rules.bootstrap_retry_interval,
        );
        let count = records.len();
        loop {
            // The primary may be this very node: then it does not go through the network.
            let answer: Result<Response, CallError> = if self.service.is_self(owner) {
                self.accept_restore(
                    topic.to_owned(),
                    topic_id.map(str::to_owned),
                    records.to_vec(),
                )
                .await
                .map_err(CallError::Remote)
            } else {
                let request = Request::Restore {
                    topic: topic.to_owned(),
                    topic_id: topic_id.map(str::to_owned),
                    records: records.to_vec(),
                };
                self.service
                    .call_peer(owner, request, rules.call_timeout * 4)
                    .await
            };
            match answer {
                Ok(Response::Recovered(result)) if result.count as usize >= count => return true,
                outcome => {
                    let now = Instant::now();
                    if now >= deadline || self.service.cancel_token().is_cancelled() {
                        tracing::warn!(owner = %owner.node_id, ?outcome, "gave up sending a restored batch");
                        return false;
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
