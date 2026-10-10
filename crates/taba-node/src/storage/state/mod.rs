//! O estado do nó na tabela `state` do `_node.db` (doc 04, seção 6.10): hoje a marca
//! `save_completed`, o instante do fim do drain. É o que separa a parada limpa (o save
//! terminou) da queda (a partida seguinte avisa que o que só estava em memória se perdeu).

use std::io;
use std::sync::Arc;

use rusqlite::{OptionalExtension, params};

use super::db::NodeDb;

const SAVE_COMPLETED: &str = "save_completed";

#[derive(Debug, Clone)]
pub struct NodeStateStore {
    db: Arc<NodeDb>,
}

impl NodeStateStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }

    /// Grava a marca com o instante (ms) do fim do save.
    pub fn mark_save_completed(&self, at_ms: u64) -> io::Result<()> {
        self.db
            .conn()
            .execute(
                "INSERT INTO state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![SAVE_COMPLETED, at_ms.to_string()],
            )
            .map(|_| ())
            .map_err(|e| self.db.fail(e))
    }

    /// Lê a marca **e a apaga**: quem parte a consome, para uma queda depois da partida não
    /// passar por save limpo. `None`: não houve save concluído.
    pub fn take_save_mark(&self) -> io::Result<Option<u64>> {
        let conn = self.db.conn();
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM state WHERE key = ?1",
                params![SAVE_COMPLETED],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| self.db.fail(e))?;
        conn.execute("DELETE FROM state WHERE key = ?1", params![SAVE_COMPLETED])
            .map_err(|e| self.db.fail(e))?;
        Ok(value.and_then(|v| v.parse().ok()))
    }

    /// Apaga a marca sem ler (o drain foi cancelado: voltou a gravar em memória).
    pub fn clear_save_mark(&self) -> io::Result<()> {
        self.take_save_mark().map(|_| ())
    }
}

#[cfg(test)]
mod tests;
