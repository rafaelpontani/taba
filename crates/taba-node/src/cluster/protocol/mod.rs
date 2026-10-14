//! As chamadas entre nós como tipos (doc 06c): o que o `Transport` leva e o que as rotas
//! `/cluster/*` e a rede em memória entendem. Uma só definição serve às duas pontas, e o
//! [`http`] traduz cada chamada para a rota HTTP do documento e de volta.

mod backup;
mod definitions;
pub mod http;
mod messages;
mod recovery;
mod signals;
mod vote;

pub use backup::{BACKUP_SAVED_MAX_ITEMS, BackupSaved, BackupSavedItem};
pub use definitions::{
    DefinitionApply, DefinitionKind, DefinitionOperation, DefinitionProposal, StreamEnd,
    StreamPage, StreamQuery, token_from_json, token_to_json,
};
pub use messages::{
    AckRegisterRequest, FIFO_PLACEHOLDER_ID, FifoPublished, IdleResponse, MessageRecord,
    MessageRegisterRequest, TakeRequest, TakeResponse, VisibilityKind, VisibilityRegisterRequest,
    WriteKind, validate_message_path,
};
pub use recovery::{LossAck, LossRecord, RecoverResult, parse_recover_body, recover_body};
pub use signals::{
    ChannelBatch, ChannelFrame, ChannelMessage, Confirm, ForwardRequest, ForwardResponse,
    Keepalive, ReadyCount, Signal, SignalFrame,
};
pub use vote::{Ballot, VoteReason, VoteRequest, VoteResponse};

use serde::{Deserialize, Serialize};

use super::members::{Member, MemberTable};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id;

/// `OperationRef` (doc 06c): a fase 2 e o rollback só dizem de qual operação falam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationRef {
    pub operation_id: String,
}

/// `Ack` (doc 06d, seção 8.3.7): a resposta de `register`, `commit` e `rollback`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ack {
    pub operation_id: String,
    pub accepted: bool,
}

/// `JoinRequest` (doc 06c): o pedido de ingresso que o nó novo faz a um membro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinRequest {
    pub node_id: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub version: String,
}

/// `NodeRegisterRequest` (doc 06c): a fase 1 do ingresso, do coordenador a todos os nós.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeRegisterRequest {
    pub operation_id: String,
    pub coordinator_id: String,
    pub node: Member,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnregisterReason {
    Vote,
    Drain,
}

/// `NodeUnregisterRequest` (doc 06c): a fase 1 da saída de um nó.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeUnregisterRequest {
    pub operation_id: String,
    pub coordinator_id: String,
    pub node_id: String,
    pub reason: UnregisterReason,
}

/// As três fases de uma operação em duas fases: `register` (com o corpo da operação), `commit` e
/// `rollback` (só com o `operationId`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<R> {
    Register(R),
    Commit(OperationRef),
    Rollback(OperationRef),
}

impl<R> Step<R> {
    /// `register`, `commit` ou `rollback`.
    pub const fn phase(&self) -> &'static str {
        match self {
            Step::Register(_) => "register",
            Step::Commit(_) => "commit",
            Step::Rollback(_) => "rollback",
        }
    }
}

/// Uma chamada a outro nó. A maior variante (o registro de uma mensagem) leva o corpo inteiro; a
/// enum é montada e consumida sem copiar, então não vale a indireção.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum Request {
    /// `GET /cluster/nodes`
    ListNodes,
    /// `POST /cluster/nodes/join`
    Join(JoinRequest),
    /// `POST /cluster/nodes/register`
    Register(NodeRegisterRequest),
    /// `POST /cluster/nodes/unregister`
    Unregister(NodeUnregisterRequest),
    /// `POST /cluster/nodes/commit`
    Commit(OperationRef),
    /// `POST /cluster/nodes/rollback`
    Rollback(OperationRef),
    /// `POST /cluster/definitions/propose`: a proposta de uma mudança de tópico ou de token, de quem
    /// recebeu a chamada do cliente ao dono da definição.
    DefinitionPropose(DefinitionProposal),
    /// `POST /cluster/definitions/apply`: a mudança decidida pelo dono, com a versão dele.
    DefinitionApply(DefinitionApply),
    /// `GET /cluster/topics`
    ListTopics(StreamQuery),
    /// `GET /cluster/tokens`
    ListTokens(StreamQuery),
    /// `GET /cluster/definitions/tombstones`: as lápides (definições removidas, com a versão) que o
    /// nó ainda guarda.
    ListTombstones(StreamQuery),
    /// `POST /cluster/topics/{topic}/messages/{id}/register|commit|rollback`
    Write {
        topic: String,
        id: String,
        step: Step<MessageRegisterRequest>,
    },
    /// `POST /cluster/topics/{topic}/messages/{id}/visibility/register|commit|rollback`
    Visibility {
        topic: String,
        id: String,
        step: Step<VisibilityRegisterRequest>,
    },
    /// `POST /cluster/topics/{topic}/messages/{id}/ack/register|commit|rollback`
    Ack {
        topic: String,
        id: String,
        step: Step<AckRegisterRequest>,
    },
    /// `GET /cluster/topics/{id}/idle`
    Idle { topic_id: String },
    /// `POST /cluster/topics/{topic}/messages/take`
    Take { topic: String, request: TakeRequest },
    /// `POST /cluster/vote-remove`
    Vote(VoteRequest),
    /// `GET /cluster/status`: o `NodeStatus` do nó, para o `GET /health/cluster` do cluster.
    Status,
    /// `* /cluster/forward/{caminho}`
    Forward(ForwardRequest),
    /// `GET /cluster/blocks/{id}/ids`
    BlockIds { block: String, query: StreamQuery },
    /// `GET /cluster/blocks/{id}/messages`
    BlockMessages { block: String, query: StreamQuery },
    /// `GET /cluster/blocks/{id}/messages/{messageId}`
    BlockMessage { block: String, id: String },
    /// `GET /cluster/message-loss`: a tabela `losses` do nó (aceita quem ainda não é membro).
    MessageLoss(StreamQuery),
    /// `POST /cluster/message-loss`: registra uma perda declarada por outro nó.
    LossRegister(LossRecord),
    /// `DELETE /cluster/message-loss/{topic}/{id}`: dá baixa numa perda recuperada.
    LossRemove { topic: String, id: String },
    /// `POST /cluster/topics/{topic}/fifo/publish`: o repasse de uma publicação FIFO ao primário
    /// do grupo. O `MessageRecord` leva o conteúdo, o `groupId` e o `deduplicationId`; o `id` é um
    /// marcador (`FIFO_PLACEHOLDER_ID`), porque quem gera o `id` é o primário.
    FifoPublish {
        topic: String,
        record: MessageRecord,
    },
    /// `POST /cluster/messages/recover`: as mensagens que o nó que volta encontrou (aceita quem
    /// ainda não é membro). Com `replicate`, quem recebe grava também nos outros responsáveis;
    /// sem ele (`?replicate=false`), só grava no próprio disco.
    Recover {
        records: Vec<MessageRecord>,
        replicate: bool,
    },
    /// `POST /cluster/backup/flush`: o flush do backup em S3 neste nó (doc 23, seção 1.11).
    BackupFlush,
    /// `POST /cluster/backup/stop`: o drain deste nó para a parada programada do cluster, com o
    /// envio ao S3; responde quando o nó concluiu.
    BackupStop,
    /// `POST /cluster/backup/saved`: o primário avisa a réplica de quais mensagens já estão no
    /// bucket.
    BackupSaved(BackupSaved),
}

impl Request {
    /// Um nome curto, para log e para conferir o que passou pela rede em memória.
    pub const fn name(&self) -> &'static str {
        match self {
            Request::ListNodes => "list-nodes",
            Request::Join(_) => "join",
            Request::Register(_) => "register",
            Request::Unregister(_) => "unregister",
            Request::Commit(_) => "commit",
            Request::Rollback(_) => "rollback",
            Request::ListTopics(_) => "list-topics",
            Request::DefinitionPropose(_) => "definition-propose",
            Request::DefinitionApply(_) => "definition-apply",
            Request::ListTokens(_) => "list-tokens",
            Request::ListTombstones(_) => "list-tombstones",
            Request::Write {
                step: Step::Register(_),
                ..
            } => "write-register",
            Request::Write {
                step: Step::Commit(_),
                ..
            } => "write-commit",
            Request::Write {
                step: Step::Rollback(_),
                ..
            } => "write-rollback",
            Request::Visibility {
                step: Step::Register(_),
                ..
            } => "visibility-register",
            Request::Visibility {
                step: Step::Commit(_),
                ..
            } => "visibility-commit",
            Request::Visibility {
                step: Step::Rollback(_),
                ..
            } => "visibility-rollback",
            Request::Ack {
                step: Step::Register(_),
                ..
            } => "ack-register",
            Request::Ack {
                step: Step::Commit(_),
                ..
            } => "ack-commit",
            Request::Ack {
                step: Step::Rollback(_),
                ..
            } => "ack-rollback",
            Request::Idle { .. } => "idle",
            Request::Take { .. } => "take",
            Request::Vote(_) => "vote",
            Request::Status => "status",
            Request::Forward(_) => "forward",
            Request::BlockIds { .. } => "block-ids",
            Request::BlockMessages { .. } => "block-messages",
            Request::BlockMessage { .. } => "block-message",
            Request::MessageLoss(_) => "message-loss",
            Request::LossRegister(_) => "loss-register",
            Request::LossRemove { .. } => "loss-remove",
            Request::Recover { .. } => "recover",
            Request::BackupFlush => "backup-flush",
            Request::BackupStop => "backup-stop",
            Request::BackupSaved(_) => "backup-saved",
            Request::FifoPublish { .. } => "fifo-publish",
        }
    }
}

/// A resposta de sucesso (`200`) de uma chamada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Members(MemberTable),
    Ack(Ack),
    /// A mudança de uma definição como o dono a decidiu (resposta do `propose` e do `apply`).
    Definition(DefinitionApply),
    /// Um trecho de `GET /cluster/topics` ou `/cluster/tokens`.
    Stream(StreamPage),
    Taken(TakeResponse),
    Idle(IdleResponse),
    /// O voto de exclusão.
    Vote(VoteResponse),
    /// O `NodeStatus` (doc 06b), como JSON: quem o monta é a camada de operação.
    Status(serde_json::Value),
    /// A resposta de uma chamada de cliente repassada, como o destino a deu.
    Forwarded(ForwardResponse),
    /// Uma mensagem inteira (`GET /cluster/blocks/{id}/messages/{messageId}`).
    Record(MessageRecord),
    /// O registro ou a baixa de uma perda.
    Loss(LossAck),
    /// O resultado do `recover`.
    Recovered(RecoverResult),
    /// A resposta do primário a uma publicação FIFO repassada.
    FifoPublished(FifoPublished),
    /// O relatório do backup em S3 (flush ou parada), como JSON.
    Backup(serde_json::Value),
}

fn invalid(field: &str, code: ErrorCode, message: &str) -> TabaError {
    TabaError::new(code, "A requisição tem campos inválidos.").with_detail(field, code, message)
}

pub(crate) fn require_guid(field: &str, value: &str) -> Result<(), TabaError> {
    if id::is_guid_v7(value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            ErrorCode::ValidationFailed,
            "Esperado um GUID v7 em minúsculas.",
        ))
    }
}

fn require_text(field: &str, value: &str) -> Result<(), TabaError> {
    if value.trim().is_empty() {
        Err(invalid(
            field,
            ErrorCode::ValidationRequired,
            "Campo obrigatório.",
        ))
    } else {
        Ok(())
    }
}

fn validate_endpoint(addresses: &[String], port: u16) -> Result<(), TabaError> {
    if addresses.is_empty() || addresses.iter().any(|a| a.trim().is_empty()) {
        return Err(invalid(
            "addresses",
            ErrorCode::ValidationRequired,
            "Informe ao menos um endereço.",
        ));
    }
    if port == 0 {
        return Err(invalid(
            "port",
            ErrorCode::ValidationOutOfRange,
            "A porta deve ser de 1 a 65535.",
        ));
    }
    Ok(())
}

impl JoinRequest {
    pub fn validate(&self) -> Result<(), TabaError> {
        require_guid("nodeId", &self.node_id)?;
        validate_endpoint(&self.addresses, self.port)?;
        require_text("version", &self.version)
    }
}

impl NodeRegisterRequest {
    pub fn validate(&self) -> Result<(), TabaError> {
        require_guid("operationId", &self.operation_id)?;
        require_guid("coordinatorId", &self.coordinator_id)?;
        require_guid("node.nodeId", &self.node.node_id)?;
        validate_endpoint(&self.node.addresses, self.node.port)
    }
}

impl NodeUnregisterRequest {
    pub fn validate(&self) -> Result<(), TabaError> {
        require_guid("operationId", &self.operation_id)?;
        require_guid("coordinatorId", &self.coordinator_id)?;
        require_guid("nodeId", &self.node_id)
    }
}

impl OperationRef {
    pub fn validate(&self) -> Result<(), TabaError> {
        require_guid("operationId", &self.operation_id)
    }
}

impl<R> Step<R> {
    /// Valida o `operationId` da fase 2 e do rollback; o corpo do `register` é validado por quem
    /// o conhece.
    pub fn validate_reference(&self) -> Result<(), TabaError> {
        match self {
            Step::Register(_) => Ok(()),
            Step::Commit(op) | Step::Rollback(op) => op.validate(),
        }
    }
}

#[cfg(test)]
mod tests;
