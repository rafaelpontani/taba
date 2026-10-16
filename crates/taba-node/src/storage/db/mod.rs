//! The node database: `${storage.dir}/_node.db`, strictly local SQLite (doc 02, section 3.2).
//!
//! It keeps the `topics` table (the definitions) and `state` (key and value of the node's state,
//! like the save mark) and `tokens` (the access tokens, only with the salted hash of the secret);
//! `losses` (the messages declared lost in synchronization, doc 05, section 7.3). A single connection behind a `Mutex` is the database's
//! single writer: whoever writes waits their turn, and SQLite never sees two writes at the same time.
//! The methods are synchronous: the `engine` calls them outside the async executor.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use rusqlite::Connection;

use crate::config::DbSynchronous;

pub const NODE_DB_FILE: &str = "_node.db";

/// Schema version, kept in `PRAGMA user_version`. A database of a greater version belongs to a
/// newer node: it is not touched.
const SCHEMA_VERSION: i64 = 5;

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

/// Who is warned of a fatal disk error in the node database (full or corrupt).
pub type FailureHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Hook(OnceLock<FailureHook>);

impl std::fmt::Debug for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hook")
    }
}

#[derive(Debug)]
pub struct NodeDb {
    conn: Mutex<Connection>,
    on_failure: Hook,
}

impl NodeDb {
    /// Opens (creating, if needed) `<storage_dir>/_node.db` in WAL mode, with the configured
    /// `storage.db.synchronous`. An unreadable database or one of an unknown version
    /// is an error: the node does not start (doc 02, section 3.2).
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
                    "{NODE_DB_FILE} is schema version {version}, newer than this node's {SCHEMA_VERSION}"
                ),
            ));
        }
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        if version < 5 {
            rename_admin_to_manage(&conn).map_err(to_io)?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(to_io)?;
        Ok(Self {
            conn: Mutex::new(conn),
            on_failure: Hook::default(),
        })
    }

    /// Registers who is warned of a fatal disk error (doc 04, section 6.3, D78.1). Only the
    /// first call counts.
    pub fn on_failure(&self, hook: FailureHook) {
        let _ = self.on_failure.0.set(hook);
    }

    /// The SQLite error as `io::Error`. A full disk or a corrupt database also warns the node,
    /// which goes to `failed`: without `_node.db` it does not keep what the cluster already acknowledged.
    pub fn fail(&self, e: rusqlite::Error) -> io::Error {
        let error = to_io(e);
        if super::is_fatal(&error)
            && let Some(hook) = self.on_failure.0.get()
        {
            hook();
        }
        error
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Value of a read `PRAGMA` (for the tests and diagnostics).
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

/// Schema 5: the topic manager, which was called `admin`, is now `manage`; `admin` became the general
/// administrator and no stored token may gain it by the rename. Runs once, in a single transaction.
fn rename_admin_to_manage(conn: &Connection) -> rusqlite::Result<()> {
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT id, permissions FROM tokens")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let tx = conn.unchecked_transaction()?;
    for (id, permissions) in rows {
        if !permissions.split(',').any(|p| p == "admin") {
            continue;
        }
        let renamed = permissions
            .split(',')
            .map(|p| if p == "admin" { "manage" } else { p })
            .collect::<Vec<_>>()
            .join(",");
        tx.execute(
            "UPDATE tokens SET permissions = ?1 WHERE id = ?2",
            rusqlite::params![renamed, id],
        )?;
    }
    tx.commit()
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
