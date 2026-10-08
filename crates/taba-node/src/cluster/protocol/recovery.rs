//! Os corpos das rotas de perdas e de recuperação (doc 04, seção 6.6; doc 06c, "Sincronização e
//! recuperação"): o registro de uma perda, a baixa e o corpo NDJSON do `recover`.

use serde::{Deserialize, Serialize};

use super::{MessageRecord, StreamPage, require_guid};
use crate::core::error::{ErrorCode, TabaError};

/// `LossRecord` (doc 06c, `POST /cluster/message-loss`): uma perda declarada na sincronização,
/// enviada aos outros nós responsáveis pelo bloco.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LossRecord {
    pub topic: String,
    pub id: String,
    /// O bloco do anel (16 dígitos hexadecimais).
    pub block: String,
    pub detected_at: String,
}

impl LossRecord {
    pub fn validate(&self) -> Result<(), TabaError> {
        require_guid("id", &self.id)?;
        if self.topic.trim().is_empty() {
            return Err(TabaError::new(
                ErrorCode::ValidationRequired,
                "O campo `topic` é obrigatório.",
            ));
        }
        if crate::core::time::parse_ms(&self.detected_at).is_none() {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "`detectedAt` deve ser uma data.",
            ));
        }
        Ok(())
    }
}

/// `LossAck` (doc 06c): a resposta do registro e da baixa de uma perda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LossAck {
    /// `true` se a tabela mudou (a perda era nova, ou existia e foi apagada).
    pub changed: bool,
}

/// `RecoverResult` (doc 06c): a resposta do `POST /cluster/messages/recover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoverResult {
    pub end: bool,
    /// Quantas linhas foram **aplicadas** (gravadas, ou já existentes).
    pub count: u64,
}

impl RecoverResult {
    pub fn new(count: u64) -> Self {
        Self { end: true, count }
    }
}

/// O corpo NDJSON do `recover`: um `MessageRecord` por linha e a linha final.
pub fn recover_body(records: &[MessageRecord]) -> Vec<u8> {
    let items = records
        .iter()
        .filter_map(|r| serde_json::to_value(r).ok())
        .collect();
    StreamPage::new(items, false).to_ndjson().into_bytes()
}

/// O inverso: sem a linha final o corpo é **falho** (doc 06c, seção 8.2.1), e a requisição é
/// recusada para quem envia reenviar tudo.
pub fn parse_recover_body(body: &[u8]) -> Result<Vec<MessageRecord>, TabaError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| TabaError::new(ErrorCode::ValidationFailed, "O corpo não é UTF-8 válido."))?;
    let page = StreamPage::from_ndjson(text)?;
    page.items
        .into_iter()
        .map(|item| {
            serde_json::from_value::<MessageRecord>(item).map_err(|e| {
                TabaError::new(
                    ErrorCode::ValidationFailed,
                    format!("Linha inválida no corpo: {e}."),
                )
            })
        })
        .collect()
}
