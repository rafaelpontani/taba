//! O banco do nó: `${storage.dir}/_node.db`, SQLite estritamente local (doc 02, seção 3.2).
//!
//! Guarda a tabela `topics` (as definições) e a `state` (chave e valor do estado do nó,
//! como a marca do save) e a `tokens` (os tokens de acesso, só com o hash salgado do segredo);
//! `losses` (as mensagens declaradas perdidas na sincronização, doc 05, seção 7.3). Uma única conexão atrás de um `Mutex` é o escritor único do
//! banco: quem grava espera a vez, e o SQLite nunca vê duas escritas ao mesmo tempo.
//! Os métodos são síncronos: o `engine` os chama fora do executor assíncrono.

use std::io;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::config::DbSynchronous;

pub const NODE_DB_FILE: &str = "_node.db";

/// Versão do esquema, guardada em `PRAGMA user_version`. Um banco de versão maior é de um
/// nó mais novo: não se mexe nele.
const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS topics (
        name      TEXT PRIMARY KEY NOT NULL,
        definition TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS state (
        key TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS tokens (
        id             TEXT PRIMARY KEY NOT NULL,
        name           TEXT NOT NULL,
        permissions    TEXT NOT NULL,
        topic_prefixes TEXT NOT NULL,
        created_at     INTEGER NOT NULL,
        expires_at     INTEGER,
        salt           TEXT NOT NULL,
        secret_hash    TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS losses (
        topic       TEXT NOT NULL,
        id          TEXT NOT NULL,
        block       TEXT NOT NULL,
        detected_at INTEGER NOT NULL,
        PRIMARY KEY (topic, id)
    ) WITHOUT ROWID;
";

#[derive(Debug)]
pub struct NodeDb {
    conn: Mutex<Connection>,
}

impl NodeDb {
    /// Abre (criando, se preciso) `<storage_dir>/_node.db` em modo WAL, com o
    /// `storage.db.synchronous` configurado. Banco ilegível ou de versão desconhecida
    /// é erro: o nó não sobe (doc 02, seção 3.2).
    pub fn open(storage_dir: &Path, synchronous: DbSynchronous) -> io::Result<Self> {
        std::fs::create_dir_all(storage_dir)?;
        let conn = Connection::open(storage_dir.join(NODE_DB_FILE)).map_err(to_io)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(to_io)?;
        conn.pragma_update(None, "synchronous", synchronous_name(synchronous))
            .map_err(to_io)?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(to_io)?;
        if version > SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{NODE_DB_FILE} é da versão {version} do esquema, mais nova que a {SCHEMA_VERSION} deste nó"
                ),
            ));
        }
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(to_io)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Valor de um `PRAGMA` de leitura (para os testes e o diagnóstico).
    pub fn pragma(&self, name: &str) -> io::Result<String> {
        self.conn()
            .pragma_query_value(None, name, |row| {
                row.get::<_, rusqlite::types::Value>(0).map(|v| match v {
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    rusqlite::types::Value::Text(t) => t,
                    other => format!("{other:?}"),
                })
            })
            .map_err(to_io)
    }
}

pub fn to_io(e: rusqlite::Error) -> io::Error {
    io::Error::other(e)
}

pub(super) fn synchronous_name(s: DbSynchronous) -> &'static str {
    match s {
        DbSynchronous::Off => "OFF",
        DbSynchronous::Normal => "NORMAL",
        DbSynchronous::Full => "FULL",
        DbSynchronous::Extra => "EXTRA",
    }
}

#[cfg(test)]
mod tests;
