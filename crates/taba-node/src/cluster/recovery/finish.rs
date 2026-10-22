//! The end of the restoration of the previous lives (doc 07, sections 10.8 and 10.7): the nodes tell each other which
//! lives they keep and whether they finished sending them; the restoration starts when the expected nodes announced
//! theirs, or after the grace period `G`; the coordinator (the lowest ID) closes the vote when every present node
//! finished, the status becomes **restored from disk**, the FIFO is released and the folders of the lives are
//! deleted. The operator can force the status (section 4.8).
//!
//! The nodes **poll** each other (`GET /cluster/restore`, a few hundred bytes every half second while it lasts), so
//! a node that joins late, or misses a notice, catches up with the next poll.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::{Recovery, RestoreSummary};
use crate::cluster::members::MemberState;
use crate::cluster::protocol::{Request, Response};
use crate::cluster::restore::{self, Manifest, Status};
use crate::core::error::TabaError;
use crate::storage::messages::MessageStore;

/// How often the nodes ask each other about the restoration while it lasts.
const POLL: Duration = Duration::from_millis(500);

/// What one node answered to `GET /cluster/restore`.
struct PeerState {
    id: String,
    status: Status,
    finished: bool,
    forced: bool,
    lives: Vec<Manifest>,
}

impl PeerState {
    fn parse(id: String, value: &Value) -> Self {
        Self {
            id,
            status: Status::parse(value["status"].as_str().unwrap_or_default()),
            finished: value["finished"].as_bool().unwrap_or(false),
            forced: value["forced"].as_bool().unwrap_or(false),
            lives: value["lives"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|life| serde_json::from_value(life.clone()).ok())
                .collect(),
        }
    }
}

impl Recovery {
    // ---- What this node tells ----

    /// `GET /cluster/restore`: the state of this node's restoration.
    pub fn serve_state(&self) -> Result<Response, TabaError> {
        Ok(Response::Json(self.state_json()))
    }

    fn state_json(&self) -> Value {
        let snapshot = self.service.restoration().snapshot();
        let lives: Vec<Manifest> = self.lives().into_iter().map(|life| life.manifest).collect();
        json!({
            "status": snapshot.status.as_str(),
            "finished": snapshot.finished,
            "forced": snapshot.forced,
            "missing": snapshot.missing,
            "lives": lives,
        })
    }

    /// `POST /cluster/restore/decided`: the vote closed.
    pub async fn serve_decided(self: &Arc<Self>) -> Result<Response, TabaError> {
        self.decide().await;
        Ok(Response::Json(json!({ "status": "restored" })))
    }

    /// `POST /cluster/restore/force` from another node: the operator forced.
    pub async fn serve_force(self: &Arc<Self>) -> Result<Response, TabaError> {
        self.force_here().await;
        Ok(Response::Json(json!({ "forced": true })))
    }

    /// The operator's force (doc 07, section 10.7): every present node changes the status and releases what was
    /// held. Returns the nodes that did not answer.
    pub async fn force_cluster(self: &Arc<Self>) -> Vec<String> {
        self.force_here().await;
        let mut set = JoinSet::new();
        for peer in self.active_peers() {
            let service = self.service.clone();
            set.spawn(async move {
                let timeout = service.rules().call_timeout * 4;
                let answer = service
                    .call_peer(&peer, Request::RestoreForce, timeout)
                    .await;
                (peer.node_id, answer.is_ok())
            });
        }
        let mut silent = Vec::new();
        while let Some(Ok((node, ok))) = set.join_next().await {
            if !ok {
                silent.push(node);
            }
        }
        silent.sort();
        silent
    }

    async fn force_here(self: &Arc<Self>) {
        let state = self.service.restoration();
        if state.is_restoring() {
            tracing::warn!(
                "the restoration of the previous lives was forced: the cluster is declared restored from disk with the nodes that are present"
            );
        }
        state.force();
        self.decide().await;
    }

    // ---- The pass ----

    /// The whole restoration of this node, from the moment it is active to the moment the cluster is restored from
    /// disk and the node has sent its lives. Returns what the pass did; the folders are deleted afterwards
    /// ([`drop_lives`](Self::drop_lives)).
    pub async fn restoration(self: &Arc<Self>) -> RestoreSummary {
        self.raise_floor_from(
            &self
                .lives()
                .into_iter()
                .map(|l| l.manifest)
                .collect::<Vec<_>>(),
        );
        let state = self.service.restoration();
        if !state.is_restoring() {
            // The joining node asks if the cluster it joined is restoring.
            let peers = self.poll().await;
            if peers.iter().any(|p| p.status == Status::Restoring) {
                state.begin(self.service.clock().now_millis());
                self.messages.hold_fifo();
            } else {
                state.complete();
            }
        }
        if !state.is_restoring() {
            // A cluster already restored: this node's lives are restored on their own, with no vote.
            let summary = self.restore_lives().await;
            state.finish();
            return summary;
        }
        self.guided().await
    }

    /// The restoration with the vote: waits for the expected nodes (or `G`), sends the lives, and follows (or, as
    /// the coordinator, closes) the vote.
    async fn guided(self: &Arc<Self>) -> RestoreSummary {
        let state = self.service.restoration();
        let me = self.service.node_id();
        let mut summary = RestoreSummary::default();
        let mut passed = false;
        let mut warned = false;
        loop {
            if self.service.cancel_token().is_cancelled() {
                return summary;
            }
            let peers = self.poll().await;
            let manifests: Vec<Manifest> = peers.iter().flat_map(|p| p.lives.clone()).collect();
            self.raise_floor_from(&manifests);
            if state.is_restoring() && peers.iter().any(|p| p.status == Status::Restored) {
                self.decide().await;
            }
            if !passed {
                let expected = restore::expected_nodes(&manifests);
                let mut announced: Vec<String> =
                    manifests.iter().map(|m| m.node_id.clone()).collect();
                announced.sort();
                announced.dedup();
                let elapsed =
                    Duration::from_millis(state.elapsed_ms(self.service.clock().now_millis()));
                let forced = state.is_forced() || peers.iter().any(|p| p.forced);
                let start = restore::start(
                    &expected,
                    &announced,
                    elapsed,
                    &self.service.rules().restore,
                    forced,
                );
                let run = match start {
                    restore::Start::Go => true,
                    restore::Start::GoWithMissing(missing) => {
                        if !warned {
                            warned = true;
                            tracing::warn!(
                                ?missing,
                                "restoring the previous lives without the nodes that did not come back"
                            );
                        }
                        state.set_missing(missing);
                        true
                    }
                    restore::Start::Wait(missing) => {
                        state.set_missing(missing);
                        false
                    }
                    restore::Start::Held(missing) => {
                        if !warned {
                            warned = true;
                            tracing::warn!(
                                ?missing,
                                "the restoration waits for the operator to force it (strict mode)"
                            );
                        }
                        state.set_missing(missing);
                        false
                    }
                };
                if run {
                    summary = self.restore_lives().await;
                    if summary.done {
                        passed = true;
                        state.finish();
                    }
                }
            }
            if passed {
                if state.is_restoring() {
                    let members = self.active_ids();
                    let finished: Vec<String> = peers
                        .iter()
                        .filter(|p| p.finished || p.id == me)
                        .map(|p| p.id.clone())
                        .collect();
                    if restore::coordinator(&members).is_some_and(|id| *id == me)
                        && restore::vote_closes(&members, &finished)
                    {
                        self.announce_decision().await;
                    }
                }
                if !state.is_restoring() {
                    return summary;
                }
            }
            tokio::select! {
                () = self.service.cancel_token().cancelled() => {}
                () = tokio::time::sleep(POLL) => {}
            }
        }
    }

    /// The coordinator tells every present node the vote closed, and decides here.
    async fn announce_decision(self: &Arc<Self>) {
        let mut set = JoinSet::new();
        for peer in self.active_peers() {
            let service = self.service.clone();
            set.spawn(async move {
                let timeout = service.rules().call_timeout * 4;
                let answer = service
                    .call_peer(&peer, Request::RestoreDecided, timeout)
                    .await;
                (peer.node_id, answer.is_ok())
            });
        }
        while let Some(Ok((node, ok))) = set.join_next().await {
            if !ok {
                tracing::warn!(node = %node, "a node did not hear the decision of the restoration; it learns it on its next poll");
            }
        }
        self.decide().await;
    }

    /// The cluster is restored from disk (doc 07, section 10.8): the group's primary deduplicates what the FIFO
    /// accepted meanwhile, in `id` order, discards the duplicates, and only then the FIFO is released. Idempotent.
    pub async fn decide(self: &Arc<Self>) {
        let _turn = self.decision.lock().await;
        let state = self.service.restoration();
        if !state.is_restoring() {
            return;
        }
        let duplicates = self.messages.fifo_duplicates();
        if !duplicates.is_empty()
            && let Some(replication) = self.service.replication().cloned()
        {
            for (topic, id) in &duplicates {
                if let Err(e) = replication.messages.discard(topic, id).await {
                    tracing::warn!(topic = %topic, id = %id, error = %e, "could not discard a duplicate of the FIFO queue");
                }
            }
        }
        self.messages.release_fifo();
        state.complete();
        tracing::info!(
            duplicates = duplicates.len(),
            "the cluster is restored from disk; the FIFO queues are released"
        );
    }

    /// Deletes the folders of the lives once the pass resolved everything it sent and the cluster is restored (doc
    /// 37, section 4.7). An incomplete pass leaves them for the next start.
    pub fn drop_lives(&self, summary: &RestoreSummary) {
        if !summary.complete() || self.service.restoration().is_restoring() {
            if summary.lives > 0 {
                tracing::warn!(
                    ?summary,
                    "the folders of the previous lives stay: the restoration did not resolve everything"
                );
            }
            return;
        }
        let root = self.messages.storage_root().join(MessageStore::LIVES);
        for life in self.lives() {
            if let Err(e) = std::fs::remove_dir_all(&life.dir) {
                tracing::warn!(life = %life.id, error = %e, "could not delete the folder of a restored life");
            }
        }
        let _ = std::fs::remove_dir(&root);
    }

    // ---- Who is present ----

    fn active_ids(&self) -> Vec<String> {
        self.service
            .membership()
            .table()
            .map(|table| {
                table
                    .members
                    .into_iter()
                    .filter(|m| m.state == MemberState::Active)
                    .map(|m| m.node_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn active_peers(&self) -> Vec<crate::cluster::members::Member> {
        let me = self.service.node_id();
        self.service
            .membership()
            .table()
            .map(|table| {
                table
                    .members
                    .into_iter()
                    .filter(|m| m.node_id != me && m.state == MemberState::Active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// This node's own state and what every other active member answered.
    async fn poll(self: &Arc<Self>) -> Vec<PeerState> {
        let mut out = vec![PeerState::parse(self.service.node_id(), &self.state_json())];
        let mut set = JoinSet::new();
        for peer in self.active_peers() {
            let service = self.service.clone();
            set.spawn(async move {
                let timeout = service.rules().call_timeout;
                let answer = service
                    .call_peer(&peer, Request::RestoreState, timeout)
                    .await;
                (peer.node_id, answer)
            });
        }
        while let Some(Ok((node, answer))) = set.join_next().await {
            if let Ok(Response::Json(value)) = answer {
                out.push(PeerState::parse(node, &value));
            }
        }
        out
    }

    /// The generator of IDs stays above the watermark of every old life announced (doc 07, section 10.9).
    fn raise_floor_from(&self, manifests: &[Manifest]) {
        for manifest in manifests {
            if let Some(watermark) = &manifest.watermark {
                self.messages.raise_id_floor(watermark);
            }
        }
    }
}
