//! As chamadas do backup em S3 entre nós (doc 23, "Backup em S3"): o aviso das mensagens já salvas
//! no bucket (`BackupSaved`), o flush num nó (`BackupFlush`) e o drain de parada (`BackupStop`).

use serde::{Deserialize, Serialize};

use super::require_guid;
use crate::core::error::{ErrorCode, TabaError};

/// Uma mensagem salva e onde ela está no bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSavedItem {
    pub id: String,
    pub segment: String,
    pub offset: u64,
    pub len: u64,
    /// Quando o primário confirmou o PUT (relógio dele); `0` se o aviso não o traz.
    #[serde(default)]
    pub saved_at_ms: u64,
}

/// `POST /cluster/backup/saved`: o primário avisa a réplica, em lote, de quais mensagens do tópico
/// já foram para um segmento. Só vale para o que a réplica guarda: o que ela não conhece é ignorado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSaved {
    pub topic: String,
    pub items: Vec<BackupSavedItem>,
}

/// O teto de itens de um aviso: o primário parte lotes maiores.
pub const BACKUP_SAVED_MAX_ITEMS: usize = 5_000;

impl BackupSaved {
    pub fn validate(&self) -> Result<(), TabaError> {
        if self.items.len() > BACKUP_SAVED_MAX_ITEMS {
            return Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "O aviso de mensagens salvas tem itens demais.",
            )
            .with_detail("items", ErrorCode::ValidationOutOfRange, "Itens demais."));
        }
        if self.topic.is_empty() {
            return Err(TabaError::new(
                ErrorCode::ValidationRequired,
                "O aviso de mensagens salvas não diz o tópico.",
            ));
        }
        for item in &self.items {
            require_guid("items[].id", &item.id)?;
        }
        Ok(())
    }
}
