//! A votação de exclusão de um nó (doc 04, seção 6.3; doc 06c, `POST /cluster/vote-remove`).

use serde::{Deserialize, Serialize};

use super::require_guid;
use crate::core::error::TabaError;

/// Por que a votação foi aberta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoteReason {
    /// O canal com o nó ficou fora depois das tentativas de reconexão.
    ChannelDown,
    /// A fila de saída para o nó estourou o limite.
    QueueOverflow,
    /// O nó entrou em falha e avisou.
    NodeFailed,
}

impl VoteReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            VoteReason::ChannelDown => "channel-down",
            VoteReason::QueueOverflow => "queue-overflow",
            VoteReason::NodeFailed => "node-failed",
        }
    }
}

/// `VoteRequest` (doc 06c).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoteRequest {
    pub vote_id: String,
    pub candidate_node_id: String,
    pub opened_by: String,
    pub reason: VoteReason,
}

impl VoteRequest {
    pub fn validate(&self) -> Result<(), TabaError> {
        require_guid("voteId", &self.vote_id)?;
        require_guid("candidateNodeId", &self.candidate_node_id)?;
        require_guid("openedBy", &self.opened_by)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ballot {
    Yes,
    No,
}

/// `VoteResponse` (doc 06c): o voto, conforme o que o votante enxerga do candidato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoteResponse {
    pub vote: Ballot,
}
