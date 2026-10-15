//! The bodies of the loss and recovery routes (doc 04, section 6.6; doc 06c, "Synchronization and
//! recovery"): the record of a loss, the acknowledgement and the NDJSON body of `recover`.

use serde::{Deserialize, Serialize};

use super::{MessageRecord, StreamPage, require_guid};
use crate::core::error::{ErrorCode, TabaError};

/// `LossRecord` (doc 06c, `POST /cluster/message-loss`): a loss declared in the synchronization,
/// sent to the other nodes responsible for the block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LossRecord {
    pub topic: String,
    pub id: String,
    /// The ring block (16 hexadecimal digits).
    pub block: String,
    pub detected_at: String,
}

impl LossRecord {
    pub fn validate(&self) -> Result<(), TabaError> {
        require_guid("id", &self.id)?;
        if self.topic.trim().is_empty() {
            return Err(TabaError::new(
                ErrorCode::ValidationRequired,
                "The `topic` field is required.",
            ));
        }
        if crate::core::time::parse_ms(&self.detected_at).is_none() {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "`detectedAt` must be a date.",
            ));
        }
        Ok(())
    }
}

/// `LossAck` (doc 06c): the response to recording and acknowledging a loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LossAck {
    /// `true` if the table changed (the loss was new, or it existed and was deleted).
    pub changed: bool,
}

/// `RecoverResult` (doc 06c): the response of `POST /cluster/messages/recover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoverResult {
    pub end: bool,
    /// How many lines were **applied** (stored, or already existing).
    pub count: u64,
}

impl RecoverResult {
    pub fn new(count: u64) -> Self {
        Self { end: true, count }
    }
}

/// The NDJSON body of `recover`: one `MessageRecord` per line and the final line.
pub fn recover_body(records: &[MessageRecord]) -> Vec<u8> {
    let items = records
        .iter()
        .filter_map(|r| serde_json::to_value(r).ok())
        .collect();
    StreamPage::new(items, false).to_ndjson().into_bytes()
}

/// The inverse: without the final line the body is **flawed** (doc 06c, section 8.2.1), and the request is
/// refused so the sender resends everything.
pub fn parse_recover_body(body: &[u8]) -> Result<Vec<MessageRecord>, TabaError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| TabaError::new(ErrorCode::ValidationFailed, "The body is not valid UTF-8."))?;
    let page = StreamPage::from_ndjson(text)?;
    page.items
        .into_iter()
        .map(|item| {
            serde_json::from_value::<MessageRecord>(item).map_err(|e| {
                TabaError::new(
                    ErrorCode::ValidationFailed,
                    format!("Invalid line in the body: {e}."),
                )
            })
        })
        .collect()
}
