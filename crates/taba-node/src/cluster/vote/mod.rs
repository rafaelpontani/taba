//! A votação de exclusão de um nó (doc 04, seção 6.3).
//!
//! Quando o canal com um nó fica fora depois das tentativas de reconexão, quando a fila de saída
//! para ele estoura ou quando ele mesmo avisa que entrou em falha, o nó que detectou abre uma
//! votação entre os nós **saudáveis**: os que ele enxerga e que respondem ao pedido de voto. Os nós
//! fora do ar não votam e não entram na base. Cada votante diz `yes` se também não alcança o
//! candidato, e `no` se o alcança. A exclusão exige `ceil(S × cluster.vote.quorum-percent)` votos
//! de `S` saudáveis, com pelo menos `cluster.vote.min-voters` votantes; atingido o quórum, o
//! desregistro segue o protocolo de duas fases (`reason: vote`), e o nó excluído entra em freezing.
//! Uma votação por vez em cada nó (as exclusões pendentes são votadas uma de cada vez).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::Instrument;

use super::channel::Channels;
use super::protocol::{
    Ballot, ChannelBatch, Request, Response, UnregisterReason, VoteReason, VoteRequest,
    VoteResponse,
};
use super::service::{ClusterService, conflict_wait};
use super::state::NodeState;
use crate::core::error::{ErrorCode, TabaError};
use crate::telemetry::metrics::CLUSTER_VOTE;
use crate::telemetry::trace;

/// Quantos votos são exigidos de `healthy` votantes: `ceil(healthy × percent / 100)`.
pub fn required_votes(healthy: u32, percent: u32) -> u32 {
    (healthy * percent).div_ceil(100).max(1)
}

pub struct Voting {
    service: Arc<ClusterService>,
    channels: Arc<Channels>,
    /// Uma votação por vez neste nó.
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

    /// Abre a votação sem esperar (é o que o canal e o sinal de falha chamam).
    pub fn open_in_background(self: &Arc<Self>, candidate: String, reason: VoteReason) {
        let me = self.clone();
        let span = trace::operation_span("taba.vote.open", &candidate);
        tokio::spawn(async move { me.open(candidate, reason).await }.instrument(span));
    }

    /// Abre a votação de exclusão de `candidate` e, se o quórum fecha, o exclui.
    pub async fn open(self: Arc<Self>, candidate: String, reason: VoteReason) {
        // Só um nó íntegro conduz votação; um nó que ainda sincroniza ou está em falha não.
        if self.service.node_state() != NodeState::Active
            || candidate == self.service.node_id()
            || self.service.cancel_token().is_cancelled()
        {
            return;
        }
        let _turn = self.turn.lock().await;
        let Some(table) = self.service.membership().table() else {
            return;
        };
        if table.member(&candidate).is_none() {
            return;
        }
        // Se o canal voltou enquanto esperava a vez, nada muda (doc 04, seção 6.3).
        if reason != VoteReason::NodeFailed && self.channels.is_up(&candidate) {
            return;
        }
        let me = self.service.node_id();
        let vote_id = self.service.new_operation_id();
        self.service.mark_vote(&candidate);
        self.count("opened", "started");
        tracing::warn!(
            candidate = %candidate,
            reason = reason.as_str(),
            "votação de exclusão aberta"
        );

        // Os votantes: os outros membros que o candidato não é. Quem não responde não entra na base.
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
        // Eu voto por último e sempre `yes`: quem abre a votação já viu o nó cair.
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
        let outcome = if healthy < rules.vote_min_voters {
            tracing::error!(
                candidate = %candidate,
                healthy,
                min = rules.vote_min_voters,
                "menos nós saudáveis que o mínimo de votantes: nenhuma exclusão se efetiva (quorum-lost)"
            );
            "quorum-lost"
        } else if yes < required_votes(healthy, rules.vote_quorum_percent) {
            tracing::info!(candidate = %candidate, yes, healthy, "a votação não atingiu o quórum");
            "rejected"
        } else {
            tracing::warn!(candidate = %candidate, yes, healthy, "quórum atingido: excluindo o nó");
            self.exclude(&candidate, voters).await
        };
        self.count("opened", outcome);
        self.service.clear_vote(&candidate);
        if outcome == "failed" {
            self.channels.allow_new_vote(&candidate);
        }
    }

    /// Efetiva a exclusão. Quando o nó cai, todos os saudáveis o percebem juntos e abrem a mesma
    /// votação; se todos coordenassem o desregistro, cada um recusaria o do outro (`cluster.conflict`)
    /// e ninguém o concluiria. Por isso coordena o de menor ID entre os saudáveis, e os outros
    /// esperam a sua vez (o de ordem `k` espera `k × write.commit.timeout`) e só então tentam, caso
    /// o nó ainda conste da tabela.
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
                // Outra mudança de membros em curso: espera um pouco e vê se o nó já saiu.
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
                    tracing::error!(candidate = %candidate, %error.message, "a exclusão não se efetivou");
                    return "failed";
                }
            }
        }
    }

    /// `POST /cluster/vote-remove`: o voto deste nó conforme o que ele enxerga do candidato.
    pub async fn cast(&self, request: VoteRequest) -> Result<Response, TabaError> {
        request.validate()?;
        let candidate = request.candidate_node_id.clone();
        let known = self.service.membership().is_member(&candidate);
        let vote = if !known || candidate == self.service.node_id() {
            // Quem não consta na tabela já saiu; e eu estou vivo.
            Ballot::No
        } else if request.reason == VoteReason::NodeFailed
            && self.channels.peer_state(&candidate).as_deref() == Some("failed")
        {
            // O candidato avisou que entrou em falha: está vivo, mas não serve ao cluster.
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

    /// O votante alcança o candidato? Se o canal dele está conectado, confere na hora.
    async fn reaches(&self, candidate: &str) -> bool {
        if !self.channels.is_up(candidate) {
            return false;
        }
        let Some(member) = self
            .service
            .membership()
            .table()
            .and_then(|t| t.member(candidate).cloned())
        else {
            return false;
        };
        let probe = Request::Channel(ChannelBatch {
            from: self.service.node_id(),
            to: candidate.to_owned(),
            keepalive: None,
            frames: Vec::new(),
        });
        let timeout = (self.service.rules().keepalive_interval * 2).max(Duration::from_millis(500));
        self.service
            .call_peer(&member, probe, timeout)
            .await
            .is_ok()
    }
}

#[cfg(test)]
mod tests;
