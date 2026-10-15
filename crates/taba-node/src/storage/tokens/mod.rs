//! Persistence of the access tokens: the `tokens` table of `_node.db`, one row per token
//! (doc 02, section 3.4). The row keeps the secret's **salted hash** (`salt` and `secret_hash`),
//! never the secret. The methods are synchronous, like the topics'.

use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::params;

use super::db::NodeDb;
use crate::core::token::{Permission, TokenRecord};

pub trait TokenStore: Send + Sync + 'static {
    /// All the stored tokens. An unreadable row is an error: the node does not start with a half-built table.
    fn load_all(&self) -> io::Result<Vec<TokenRecord>>;

    /// Writes (or replaces) the token, atomically.
    fn save(&self, record: &TokenRecord) -> io::Result<()>;

    /// Deletes the token. Deleting what does not exist is no error.
    fn delete(&self, id: &str) -> io::Result<()>;
}

#[derive(Debug)]
pub struct SqliteTokenStore {
    db: Arc<NodeDb>,
}

impl SqliteTokenStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }
}

fn invalid(id: &str, reason: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("token `{id}` in _node.db: {reason}"),
    )
}

/// A table row, as it came from SQLite.
type Row = (
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    String,
    String,
);

fn record_of(row: Row) -> io::Result<TokenRecord> {
    let (id, name, permissions, prefixes, created, expires, salt, secret_hash) = row;
    let permissions = permissions
        .split(',')
        .map(|p| Permission::parse(p).ok_or_else(|| invalid(&id, format!("permission `{p}`"))))
        .collect::<Result<Vec<_>, _>>()?;
    let topic_prefixes: Vec<String> =
        serde_json::from_str(&prefixes).map_err(|e| invalid(&id, e))?;
    let created_ms = u64::try_from(created).map_err(|e| invalid(&id, e))?;
    let expires_ms = expires
        .map(u64::try_from)
        .transpose()
        .map_err(|e| invalid(&id, e))?;
    Ok(TokenRecord {
        id,
        name,
        permissions,
        topic_prefixes,
        created_ms,
        expires_ms,
        salt,
        secret_hash,
    })
}

impl TokenStore for SqliteTokenStore {
    fn load_all(&self) -> io::Result<Vec<TokenRecord>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, permissions, topic_prefixes, created_at, expires_at, salt, \
                 secret_hash FROM tokens ORDER BY id",
            )
            .map_err(|e| self.db.fail(e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })
            .map_err(|e| self.db.fail(e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(record_of(row.map_err(|e| self.db.fail(e))?)?);
        }
        Ok(out)
    }

    fn save(&self, record: &TokenRecord) -> io::Result<()> {
        let permissions = record
            .permissions
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let prefixes = serde_json::to_string(&record.topic_prefixes).map_err(io::Error::other)?;
        self.db
            .conn()
            .execute(
                "INSERT INTO tokens (id, name, permissions, topic_prefixes, created_at, \
                 expires_at, salt, secret_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, \
                 permissions = excluded.permissions, topic_prefixes = excluded.topic_prefixes, \
                 created_at = excluded.created_at, expires_at = excluded.expires_at, \
                 salt = excluded.salt, secret_hash = excluded.secret_hash",
                params![
                    record.id,
                    record.name,
                    permissions,
                    prefixes,
                    record.created_ms as i64,
                    record.expires_ms.map(|ms| ms as i64),
                    record.salt,
                    record.secret_hash,
                ],
            )
            .map_err(|e| self.db.fail(e))?;
        Ok(())
    }

    fn delete(&self, id: &str) -> io::Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM tokens WHERE id = ?1", params![id])
            .map_err(|e| self.db.fail(e))?;
        Ok(())
    }
}

/// The test implementation: memory, with a programmable failure.
#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<BTreeMap<String, TokenRecord>>,
    failing: AtomicBool,
}

impl MemoryTokenStore {
    /// With `true`, `save` and `delete` fail, like a full disk.
    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    pub fn ids(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, TokenRecord>> {
        self.tokens.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn check(&self) -> io::Result<()> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(io::Error::other("failure programmed by the test"));
        }
        Ok(())
    }
}

impl TokenStore for MemoryTokenStore {
    fn load_all(&self) -> io::Result<Vec<TokenRecord>> {
        Ok(self.lock().values().cloned().collect())
    }

    fn save(&self, record: &TokenRecord) -> io::Result<()> {
        self.check()?;
        self.lock().insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn delete(&self, id: &str) -> io::Result<()> {
        self.check()?;
        self.lock().remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
