//! A tabela `losses` do `_node.db` (doc 02, seção 3.2; doc 04, seção 6.6): os pares (tópico, ID)
//! que alguma sincronização declarou perdidos. O nó que volta de uma exclusão a consulta nos
//! outros nós para recuperar as mensagens que ainda tem. Os registros expiram depois de
//! `cluster.message-loss.ttl`.

use std::io;
use std::sync::Arc;

use rusqlite::params;

use super::db::{NodeDb, to_io};

/// Uma perda registrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loss {
    pub topic: String,
    pub id: String,
    /// O bloco do anel a que a mensagem pertence (16 dígitos hexadecimais).
    pub block: String,
    pub detected_ms: u64,
}

#[derive(Debug, Clone)]
pub struct LossStore {
    db: Arc<NodeDb>,
}

impl LossStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }

    /// Grava a perda. A chave é (tópico, ID): repetir não muda nada. `true` se era nova.
    pub fn record(&self, loss: &Loss) -> io::Result<bool> {
        let changed = self
            .db
            .conn()
            .execute(
                "INSERT OR IGNORE INTO losses (topic, id, block, detected_at) VALUES (?1, ?2, ?3, ?4)",
                params![loss.topic, loss.id, loss.block, loss.detected_ms as i64],
            )
            .map_err(to_io)?;
        Ok(changed > 0)
    }

    /// Dá baixa na perda (a mensagem foi recuperada). `true` se existia.
    pub fn remove(&self, topic: &str, id: &str) -> io::Result<bool> {
        let changed = self
            .db
            .conn()
            .execute(
                "DELETE FROM losses WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(to_io)?;
        Ok(changed > 0)
    }

    /// As perdas com `id` a partir de `from` (inclusivo), em ordem de `id`, no máximo `limit`.
    pub fn list(&self, from: Option<&str>, limit: usize) -> io::Result<Vec<Loss>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT topic, id, block, detected_at FROM losses WHERE id >= ?1 ORDER BY id, topic LIMIT ?2",
            )
            .map_err(to_io)?;
        let rows = stmt
            .query_map(
                params![from.unwrap_or(""), i64::try_from(limit).unwrap_or(i64::MAX)],
                |row| {
                    Ok(Loss {
                        topic: row.get(0)?,
                        id: row.get(1)?,
                        block: row.get(2)?,
                        detected_ms: row.get::<_, i64>(3)? as u64,
                    })
                },
            )
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// Apaga os registros detectados antes de `before_ms`; devolve quantos.
    pub fn expire(&self, before_ms: u64) -> io::Result<u64> {
        let removed = self
            .db
            .conn()
            .execute(
                "DELETE FROM losses WHERE detected_at < ?1",
                params![before_ms as i64],
            )
            .map_err(to_io)?;
        Ok(removed as u64)
    }

    pub fn count(&self) -> io::Result<u64> {
        let n: i64 = self
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM losses", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }
}

#[cfg(test)]
mod tests;
