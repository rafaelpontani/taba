//! The S3 backup calls between nodes (doc 23, "S3 backup"): the notice of messages already saved
//! in the bucket (`BackupSaved`), the flush on a node (`BackupFlush`) and the stop drain (`BackupStop`).

use serde::{Deserialize, Serialize};

use super::require_guid;
use crate::core::error::{ErrorCode, TabaError};

/// A saved message and where it is in the bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSavedItem {
    pub id: String,
    pub segment: String,
    pub offset: u64,
    pub len: u64,
    /// When the primary confirmed the PUT (its clock); `0` if the notice does not carry it.
    #[serde(default)]
    pub saved_at_ms: u64,
}

/// `POST /cluster/backup/saved`: the primary tells the replica, in a batch, which messages of the topic
/// already went into a segment. It only applies to what the replica keeps: what it does not know is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSaved {
    pub topic: String,
    pub items: Vec<BackupSavedItem>,
}

/// The ceiling of items in a notice: the primary splits larger batches.
pub const BACKUP_SAVED_MAX_ITEMS: usize = 5_000;

impl BackupSaved {
    pub fn validate(&self) -> Result<(), TabaError> {
        if self.items.len() > BACKUP_SAVED_MAX_ITEMS {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "The saved-messages notice has too many items.",
            )
            .with_detail("items", ErrorCode::ValidationOutOfRange, "Too many items."));
        }
        if self.topic.is_empty() {
            return Err(TabaError::new(
                ErrorCode::ValidationRequired,
                "The saved-messages notice does not say the topic.",
            ));
        }
        for item in &self.items {
            require_guid("items[].id", &item.id)?;
        }
        Ok(())
    }
}
