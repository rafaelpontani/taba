//! The vote to exclude a node (doc 04, section 6.3).
//!
//! When the channel with a node stays down after the reconnection attempts, when the outgoing queue
//! to it overflows or when it itself announces it entered failure, the node that detected it opens a
//! vote among the **healthy** nodes: the ones it sees and that answer the vote request. Nodes
//! that are down do not vote and do not enter the base. Each voter says `yes` if it also cannot reach the
//! candidate, and `no` if it can. The exclusion requires `ceil(S × cluster.vote.quorum-percent)` votes
//! of `S` healthy, with at least `cluster.vote.min-voters` voters; once the quorum is reached, the
//! unregistration follows the two-phase protocol (`reason: vote`), and the excluded node enters freezing.
//! One vote at a time on each node (pending exclusions are voted one at a time).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::Instrument;

use super::channel::Channels;
use super::protocol::{
    Ballot, Request, Response, UnregisterReason, VoteReason, VoteRequest, VoteResponse,
};
use super::service::{ClusterService, conflict_wait};
use super::state::NodeState;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::metrics::CLUSTER_VOTE;
use crate::telemetry::trace;

/// How many votes are required of `healthy` voters: `ceil(healthy × percent / 100)`.
pub fn required_votes(healthy: u32, percent: u32) -> u32 {
    (healthy * percent).div_ceil(100).max(1)
}

pub struct Voting {
    service: Arc<ClusterService>,
    channels: Arc<Channels>,
    /// One vote at a time on this node.
    turn: Mutex<()>,
}

impl Voting {
    pub fn new(service: Arc<ClusterService>, channels: Arc<Channels>) -> Arc<Self> {
        Arc::new(Self {
            service,
            channels,
            turn: Mutex::new(()),
        })
    }

    fn count(&self, role: &str, outcome: &str) {
        self.service.telemetry().registry.add(
            &CLUSTER_VOTE,
            &[("taba.role", role), ("taba.outcome", outcome)],
            1.0,
        );
    }

    /// Opens the vote without waiting (what the channel and the failure signal call).
    pub fn open_in_background(self: &Arc<Self>, candidate: String, reason: VoteReason) {
        let me = self.clone();
        let span = trace::operation_span("taba.vote.open", &candidate);
        tokio::spawn(async move { me.open(candidate, reason).await }.instrument(span));
    }

    /// Opens the vote to exclude `candidate` and, if the quorum closes, excludes it.
    pub async fn open(self: Arc<Self>, candidate: String, reason: VoteReason) {
        // Only a sound node runs a vote; a node that is still syncing or is in failure does not.
        if self.service.node_state() != NodeState::Active
            || candidate == self.service.node_id()
            || self.service.cancel_token().is_cancelled()
        {
            return;
        }
        // With the S3 backup, "unreachable" is not enough: the node may be alive in another partition, and
        // its heartbeat in the bucket says so. The exclusion waits for the heartbeat to expire (doc 23, 1.6).
        if reason != VoteReason::NodeFailed
            && let Some(port) = self.service.backup()
        {
            let channels = self.channels.clone();
            let who = candidate.clone();
            if !port
                .backup()
                .confirm_dead(&candidate, move || channels.is_live(&who))
                .await
            {
                return;
            }
        }
        let _turn = self.turn.lock().await;
        let Some(table) = self.service.membership().table() else {
            return;
        };
        if table.member(&candidate).is_none() {
            return;
        }
        // If the channel came back while waiting for its turn, nothing changes (doc 04, section 6.3).
        if reason != VoteReason::NodeFailed && self.channels.is_live(&candidate) {
            return;
        }
        let me = self.service.node_id();
        let vote_id = self.service.new_operation_id();
        self.service.mark_vote(&candidate);
        self.count("opened", "started");
        tracing::warn!(
            candidate = %candidate,
            reason = reason.as_str(),
            "exclusion vote opened"
        );

        // The voters: the other members that are not the candidate. Whoever does not answer does not enter the base.
        let mut set = JoinSet::new();
        for member in table
            .members
            .iter()
            .filter(|m| m.node_id != candidate && m.node_id != me)
        {
            let (service, member) = (self.service.clone(), member.clone());
            let request = Request::Vote(VoteRequest {
                vote_id: vote_id.clone(),
                candidate_node_id: candidate.clone(),
                opened_by: me.clone(),
                reason,
            });
            let timeout = service.rules().call_timeout;
            let span = trace::peer_span("taba.vote.request", &member.node_id, false);
            set.spawn(
                async move {
                    let answer = service.call_peer(&member, request, timeout).await;
                    (member.node_id, answer)
                }
                .instrument(span),
            );
        }
        // I vote last and always `yes`: whoever opens the vote has already seen the node fall.
        let (mut healthy, mut yes) = (1u32, 1u32);
        let mut voters = vec![me.clone()];
        while let Some(joined) = set.join_next().await {
            if let Ok((voter, Ok(Response::Vote(VoteResponse { vote })))) = joined {
                healthy += 1;
                voters.push(voter);
                if vote == Ballot::Yes {
                    yes += 1;
                }
            }
        }
        let rules = self.service.rules();
        // The sole survivor: all the other members are dead to the bucket, so there is no
        // one to vote, and the node may exclude them even below `cluster.vote.min-voters`.
        let alone = match self.service.backup() {
            Some(port) if healthy < rules.vote_min_voters => {
                port.backup().wait_all_others_dead().await
            }
            _ => false,
        };
        let outcome = if healthy < rules.vote_min_voters && !alone {
            tracing::error!(
                candidate = %candidate,
                healthy,
                min = rules.vote_min_voters,
                "fewer healthy nodes than the minimum of voters: no exclusion takes effect (quorum-lost)"
            );
            "quorum-lost"
        } else if yes < required_votes(healthy, rules.vote_quorum_percent) {
            tracing::info!(candidate = %candidate, yes, healthy, "the vote did not reach the quorum");
            "rejected"
        } else {
            tracing::warn!(candidate = %candidate, yes, healthy, "quorum reached: excluding the node");
            self.exclude(&candidate, voters).await
        };
        self.count("opened", outcome);
        self.service.clear_vote(&candidate);
        if outcome == "failed" {
            self.channels.allow_new_vote(&candidate);
        }
    }

    /// Carries out the exclusion. When the node falls, all the healthy ones notice it together and open the same
    /// vote; if all coordinated the unregistration, each would refuse the other's (`cluster.conflict`)
    /// and no one would finish it. So the one with the lowest ID among the healthy coordinates, and the others
    /// wait their turn (the one of order `k` waits `k × write.commit.timeout`) and only then try, in case
    /// the node is still in the table.
    async fn exclude(&self, candidate: &str, mut voters: Vec<String>) -> &'static str {
        voters.sort();
        let me = self.service.node_id();
        let rank = voters.iter().position(|v| *v == me).unwrap_or(0) as u32;
        let rules = self.service.rules();
        let started = tokio::time::Instant::now();
        let turn_at = started + rules.commit_timeout * rank;
        let give_up_at = turn_at + rules.join_timeout;
        while tokio::time::Instant::now() < turn_at {
            if !self.service.membership().is_member(candidate) {
                return "excluded";
            }
            if self.service.cancel_token().is_cancelled() {
                return "failed";
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        loop {
            match self
                .service
                .unregister_node(candidate, UnregisterReason::Vote)
                .await
            {
                Ok(()) => return "excluded",
                // Another member change under way: waits a little and sees whether the node already left.
                Err(error)
                    if error.code == ErrorCode::ClusterConflict
                        && tokio::time::Instant::now() < give_up_at
                        && !self.service.cancel_token().is_cancelled() =>
                {
                    tokio::time::sleep(conflict_wait()).await;
                    if !self.service.membership().is_member(candidate) {
                        return "excluded";
                    }
                }
                Err(error) => {
                    tracing::error!(candidate = %candidate, %error.message, "the exclusion did not take effect");
                    return "failed";
                }
            }
        }
    }

    /// `POST /cluster/vote-remove`: this node's vote according to what it sees of the candidate.
    pub async fn cast(&self, request: VoteRequest) -> Result<Response, TabaError> {
        request.validate()?;
        let candidate = request.candidate_node_id.clone();
        let known = self.service.membership().is_member(&candidate);
        let vote = if !known || candidate == self.service.node_id() {
            // Whoever is not in the table already left; and I am alive.
            Ballot::No
        } else if request.reason == VoteReason::NodeFailed
            && self.channels.peer_state(&candidate).as_deref() == Some("failed")
        {
            // The candidate announced it entered failure: it is alive, but does not serve the cluster.
            Ballot::Yes
        } else if self.reaches(&candidate).await {
            Ballot::No
        } else {
            Ballot::Yes
        };
        if vote == Ballot::Yes {
            self.service.mark_vote(&candidate);
        }
        self.count(
            "cast",
            match vote {
                Ballot::Yes => "yes",
                Ballot::No => "no",
            },
        );
        Ok(Response::Vote(VoteResponse { vote }))
    }

    /// Does the voter reach the candidate? Its channel is open and the peer spoke a moment ago.
    async fn reaches(&self, candidate: &str) -> bool {
        self.channels.is_live(candidate)
    }
}

#[cfg(test)]
mod tests;
