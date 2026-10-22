//! The calls between nodes as types (doc 06c): what the `Transport` carries and what the
//! `/cluster/*` routes and the in-memory network understand. A single definition serves both ends, and
//! [`http`] translates each call to the document's HTTP route and back.

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

/// `OperationRef` (doc 06c): phase 2 and the rollback only say which operation they speak of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationRef {
    pub operation_id: String,
}

/// `Ack` (doc 06d, section 8.3.7): the response of `register`, `commit` and `rollback`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ack {
    pub operation_id: String,
    pub accepted: bool,
}

/// `JoinRequest` (doc 06c): the join request the new node makes to a member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinRequest {
    pub node_id: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub version: String,
}

/// `NodeRegisterRequest` (doc 06c): phase 1 of the join, from the coordinator to all nodes.
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

/// `NodeUnregisterRequest` (doc 06c): phase 1 of a node's departure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeUnregisterRequest {
    pub operation_id: String,
    pub coordinator_id: String,
    pub node_id: String,
    pub reason: UnregisterReason,
}

/// The three phases of a two-phase operation: `register` (with the operation body), `commit` and
/// `rollback` (only with the `operationId`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<R> {
    Register(R),
    Commit(OperationRef),
    Rollback(OperationRef),
}

impl<R> Step<R> {
    /// `register`, `commit` or `rollback`.
    pub const fn phase(&self) -> &'static str {
        match self {
            Step::Register(_) => "register",
            Step::Commit(_) => "commit",
            Step::Rollback(_) => "rollback",
        }
    }
}

/// A call to another node. The largest variant (a message's register) carries the whole body; the
/// enum is built and consumed without copying, so the indirection is not worth it.
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
    /// `POST /cluster/definitions/propose`: the proposal of a topic or token change, from whoever
    /// received the client's call to the definition's owner.
    DefinitionPropose(DefinitionProposal),
    /// `POST /cluster/definitions/apply`: the change decided by the owner, with its version.
    DefinitionApply(DefinitionApply),
    /// `GET /cluster/topics`
    ListTopics(StreamQuery),
    /// `GET /cluster/tokens`
    ListTokens(StreamQuery),
    /// `GET /cluster/definitions/tombstones`: the tombstones (removed definitions, with the version) that the
    /// node still keeps.
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
    /// `GET /cluster/status`: the node's `NodeStatus`, for the cluster's `GET /health/cluster`.
    Status,
    /// `* /cluster/forward/{path}`
    Forward(ForwardRequest),
    /// `GET /cluster/blocks/{id}/ids`
    BlockIds { block: String, query: StreamQuery },
    /// `GET /cluster/blocks/{id}/excluded`: the block's log of excluded messages (doc 37, section 4.4).
    BlockExcluded { block: String, query: StreamQuery },
    /// `GET /cluster/blocks/{id}/messages`
    BlockMessages { block: String, query: StreamQuery },
    /// `GET /cluster/blocks/{id}/messages/{messageId}`
    BlockMessage { block: String, id: String },
    /// `GET /cluster/message-loss`: the node's `losses` table (accepts whoever is not yet a member).
    MessageLoss(StreamQuery),
    /// `POST /cluster/message-loss`: records a loss declared by another node.
    LossRegister(LossRecord),
    /// `DELETE /cluster/message-loss/{topic}/{id}`: acknowledges a recovered loss.
    LossRemove { topic: String, id: String },
    /// `POST /cluster/topics/{topic}/fifo/publish`: the forward of a FIFO publication to the group's primary.
    /// The `MessageRecord` carries the content, the `groupId` and the `deduplicationId`; the `id` is a
    /// placeholder (`FIFO_PLACEHOLDER_ID`), because whoever generates the `id` is the primary.
    FifoPublish {
        topic: String,
        record: MessageRecord,
    },
    /// `POST /cluster/messages/recover`: the messages the returning node found (accepts whoever is
    /// not yet a member). With `replicate`, whoever receives it also writes to the other responsible nodes;
    /// without it (`?replicate=false`), it only writes to its own disk.
    Recover {
        records: Vec<MessageRecord>,
        replicate: bool,
    },
    /// `POST /cluster/backup/flush`: the S3 backup flush on this node (doc 23, section 1.11).
    BackupFlush,
    /// `POST /cluster/backup/stop`: this node's drain for the cluster's planned stop, with the
    /// upload to S3; answers when the node is done.
    BackupStop,
    /// `POST /cluster/backup/saved`: the primary tells the replica which messages are already in the
    /// bucket.
    BackupSaved(BackupSaved),
    /// `* /cluster/node-call/{path}`: a call of the **operations** API (`/metrics`, `/node/drain`,
    /// `/node/backup`...) that the receiving node runs against itself, for a client call addressed to
    /// this node (`/nodes/{nodeId}/...`). The token was already checked by whoever sent it. The response
    /// is the one the route gave, as it came.
    NodeCall(ForwardRequest),
    /// `GET /cluster/stats/topics`: what **this node** sees of every topic (the numbers, not the
    /// presentation), for the client API's `/stats/topics` to consolidate.
    TopicStats,
    /// `GET /cluster/stats/topics/{topic}/groups`: the groups of a queue as this node sees them.
    TopicGroups { topic: String },
    /// `GET /cluster/backup/status`: the S3 backup state on this node.
    BackupStatus,
    /// `GET /cluster/departed`: the nodes that left by drain, as **this node** remembers them (id, addresses, port),
    /// for `GET /nodes` to unite what every member knows.
    Departed,
    /// `POST /cluster/topics/{topic}/prune`: empties the queue on this node. Idempotent by `prune_id`.
    Prune { topic: String, prune_id: String },
}

impl Request {
    /// A short name, for logs and to check what passed through the in-memory network.
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
            Request::BlockExcluded { .. } => "block-excluded",
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
            Request::NodeCall(_) => "node-call",
            Request::TopicStats => "topic-stats",
            Request::TopicGroups { .. } => "topic-groups",
            Request::BackupStatus => "backup-status",
            Request::Departed => "departed",
            Request::Prune { .. } => "prune",
        }
    }
}

/// The success response (`200`) of a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Members(MemberTable),
    Ack(Ack),
    /// A definition's change as the owner decided it (response of `propose` and `apply`).
    Definition(DefinitionApply),
    /// A stretch of `GET /cluster/topics` or `/cluster/tokens`.
    Stream(StreamPage),
    Taken(TakeResponse),
    Idle(IdleResponse),
    /// The exclusion vote.
    Vote(VoteResponse),
    /// The `NodeStatus` (doc 06b), as JSON: whoever builds it is the operations layer.
    Status(serde_json::Value),
    /// The response of a forwarded client call, as the destination gave it.
    Forwarded(ForwardResponse),
    /// A whole message (`GET /cluster/blocks/{id}/messages/{messageId}`).
    Record(MessageRecord),
    /// The record or acknowledgement of a loss.
    Loss(LossAck),
    /// The result of `recover`.
    Recovered(RecoverResult),
    /// The primary's response to a forwarded FIFO publication.
    FifoPublished(FifoPublished),
    /// The S3 backup report (flush or stop), as JSON.
    Backup(serde_json::Value),
    /// A small JSON answer (the result of a prune).
    Json(serde_json::Value),
}

fn invalid(field: &str, code: ErrorCode, message: &str) -> TabaError {
    TabaError::new(code, "The request has invalid fields.").with_detail(field, code, message)
}

pub(crate) fn require_guid(field: &str, value: &str) -> Result<(), TabaError> {
    if id::is_guid_v7(value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            ErrorCode::ValidationFailed,
            "Expected a lowercase GUID v7.",
        ))
    }
}

fn require_text(field: &str, value: &str) -> Result<(), TabaError> {
    if value.trim().is_empty() {
        Err(invalid(
            field,
            ErrorCode::ValidationRequired,
            "Required field.",
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
            "Provide at least one address.",
        ));
    }
    if port == 0 {
        return Err(invalid(
            "port",
            ErrorCode::ValidationOutOfRange,
            "The port must be from 1 to 65535.",
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
    /// Validates the `operationId` of phase 2 and of the rollback; the `register` body is validated by whoever
    /// knows it.
    pub fn validate_reference(&self) -> Result<(), TabaError> {
        match self {
            Step::Register(_) => Ok(()),
            Step::Commit(op) | Step::Rollback(op) => op.validate(),
        }
    }
}

#[cfg(test)]
mod tests;
