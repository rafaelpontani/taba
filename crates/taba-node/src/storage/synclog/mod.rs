//! O banco de eventos de um nó em sincronização: `${storage.dir}/_sync/sync.db` (doc 05, seção
//! 7.3, passo 4). Enquanto o nó carrega os blocos ele já consta na tabela de membros e recebe o
//! protocolo normal do cluster, mas não pode processar os eventos: guarda cada um aqui, na ordem
//! de chegada (número de sequência crescente), e os processa por cima da carga.
//!
//! O evento é a chamada HTTP como chegou (método, caminho, cabeçalhos e corpo): quem a processa
//! lê pelo mesmo caminho que leria uma chamada de rede. A tabela `loaded` guarda os IDs que a
//! carga trouxe, para a verificação final saber o que conferir. O arquivo existe só durante a
//! sincronização e é apagado no fim.
//!
//! Uma única conexão atrás de um `Mutex` é a thread única de gravação do doc.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use super::db::to_io;

pub const SYNC_DIR: &str = "_sync";
pub const SYNC_DB_FILE: &str = "sync.db";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS events (
        seq     INTEGER PRIMARY KEY AUTOINCREMENT,
        method  TEXT NOT NULL,
        path    TEXT NOT NULL,
        headers TEXT NOT NULL,
        body    BLOB NOT NULL
    );
    CREATE TABLE IF NOT EXISTS loaded (
        id    TEXT PRIMARY KEY NOT NULL,
        topic TEXT NOT NULL,
        block TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS loaded_block ON loaded (block, id);
";

/// Um evento guardado: a chamada como chegou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: i64,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct SyncLog {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl SyncLog {
    /// Cria o banco, apagando o que uma sincronização anterior deixou.
    pub fn create(storage_dir: &Path) -> io::Result<Self> {
        let dir = storage_dir.join(SYNC_DIR);
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(SYNC_DB_FILE);
        let conn = Connection::open(&path).map_err(to_io)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(to_io)?;
        // Um banco que existe só até o fim da sincronização: a durabilidade não vale o custo.
        conn.pragma_update(None, "synchronous", "OFF")
            .map_err(to_io)?;
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        Ok(Self {
            path,
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Acrescenta o evento ao fim e devolve o número de sequência.
    pub fn append(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> io::Result<i64> {
        let headers = serde_json::to_string(headers).map_err(io::Error::other)?;
        let conn = self.conn();
        conn.execute(
            "INSERT INTO events (method, path, headers, body) VALUES (?1, ?2, ?3, ?4)",
            params![method, path, headers, body],
        )
        .map_err(to_io)?;
        Ok(conn.last_insert_rowid())
    }

    /// O evento mais antigo (menor número de sequência).
    pub fn oldest(&self) -> io::Result<Option<StoredEvent>> {
        let row = self
            .conn()
            .query_row(
                "SELECT seq, method, path, headers, body FROM events ORDER BY seq LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(to_io)?;
        row.map(|(seq, method, path, headers, body)| {
            Ok(StoredEvent {
                seq,
                method,
                path,
                headers: serde_json::from_str(&headers).map_err(io::Error::other)?,
                body,
            })
        })
        .transpose()
    }

    /// Apaga o evento já processado.
    pub fn delete(&self, seq: i64) -> io::Result<()> {
        self.conn()
            .execute("DELETE FROM events WHERE seq = ?1", params![seq])
            .map(|_| ())
            .map_err(to_io)
    }

    /// Quantos eventos faltam processar.
    pub fn len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    pub fn is_empty(&self) -> io::Result<bool> {
        self.len().map(|n| n == 0)
    }

    /// O tamanho do banco em disco (páginas × tamanho da página), para `cluster.sync.log.max-bytes`.
    pub fn size_bytes(&self) -> io::Result<u64> {
        let conn = self.conn();
        let pages: i64 = conn
            .pragma_query_value(None, "page_count", |row| row.get(0))
            .map_err(to_io)?;
        let size: i64 = conn
            .pragma_query_value(None, "page_size", |row| row.get(0))
            .map_err(to_io)?;
        Ok((pages.max(0) * size.max(0)) as u64)
    }

    /// Guarda que a carga trouxe a mensagem (do bloco), para a verificação final.
    pub fn mark_loaded(&self, block: &str, items: &[(String, String)]) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO loaded (id, topic, block) VALUES (?1, ?2, ?3)",
                )
                .map_err(to_io)?;
            for (topic, id) in items {
                stmt.execute(params![id, topic, block]).map_err(to_io)?;
            }
        }
        tx.commit().map_err(to_io)
    }

    /// As mensagens que a carga trouxe do bloco, `(tópico, id)` em ordem de `id`.
    pub fn loaded_in(&self, block: &str) -> io::Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT topic, id FROM loaded WHERE block = ?1 ORDER BY id")
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![block], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// Fecha o banco e apaga a pasta `_sync` (a sincronização terminou).
    pub fn discard(&self) -> io::Result<()> {
        // Troca a conexão por uma em memória: a do arquivo fecha (o Windows não apaga arquivo aberto).
        *self.conn() = Connection::open_in_memory().map_err(to_io)?;
        match self.path.parent().map(std::fs::remove_dir_all) {
            Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
