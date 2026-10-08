//! As chamadas entre nós como tipos (doc 06c): o que o `Transport` leva e o que as rotas
//! `/cluster/*` e a rede em memória entendem. Uma só definição serve às duas pontas.

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

/// Uma chamada a outro nó.
#[derive(Debug, Clone, PartialEq, Eq)]
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
        }
    }
}

/// A resposta de sucesso (`200`) de uma chamada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Members(MemberTable),
    Ack(Ack),
}

fn invalid(field: &str, code: ErrorCode, message: &str) -> TabaError {
    TabaError::new(code, "A requisição tem campos inválidos.").with_detail(field, code, message)
}

fn require_guid(field: &str, value: &str) -> Result<(), TabaError> {
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

#[cfg(test)]
mod tests;
