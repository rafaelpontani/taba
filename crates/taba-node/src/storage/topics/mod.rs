//! Persistence of the topic definitions: the `topics` table of `_node.db`, one row per
//! topic, written at commit (doc 02, sections 3.2 and 3.3).
//!
//! The trait lives here, in the lowest layer the `engine` needs, because
//! dependencies only go to the right (doc 18, section 17.3). The methods are
//! synchronous: the `engine` calls them outside the async executor (`spawn_blocking`).

use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::params;

use super::db::NodeDb;
use crate::core::topic::TopicDefinition;

pub trait TopicStore: Send + Sync + 'static {
    /// All the stored definitions. An unreadable row is an error: the node does not start with
    /// a half-built topic table.
    fn load_all(&self) -> io::Result<Vec<TopicDefinition>>;

    /// Writes (or replaces) the definition, atomically: never a half row.
    fn save(&self, definition: &TopicDefinition) -> io::Result<()>;

    /// Deletes the definition. Deleting what does not exist is no error.
    fn delete(&self, name: &str) -> io::Result<()>;
}

/// The real implementation, on the node database. The name is the key and SQLite compares text byte by
/// byte, so `Orders` and `orders` are different topics (UC-TOP-011) on any disk.
#[derive(Debug)]
pub struct SqliteTopicStore {
    db: Arc<NodeDb>,
}

impl SqliteTopicStore {
    pub fn new(db: Arc<NodeDb>) -> Self {
        Self { db }
    }
}

impl TopicStore for SqliteTopicStore {
    fn load_all(&self) -> io::Result<Vec<TopicDefinition>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare("SELECT name, definition FROM topics ORDER BY name")
            .map_err(|e| self.db.fail(e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| self.db.fail(e))?;
        let mut out = Vec::new();
        for row in rows {
            let (name, text) = row.map_err(|e| self.db.fail(e))?;
            let invalid = |reason: String| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("topic `{name}` in _node.db: {reason}"),
                )
            };
            let value = serde_json::from_str(&text).map_err(|e| invalid(e.to_string()))?;
            out.push(TopicDefinition::from_json(&value).map_err(invalid)?);
        }
        Ok(out)
    }

    fn save(&self, definition: &TopicDefinition) -> io::Result<()> {
        let text = serde_json::to_string(&definition.to_json()).map_err(io::Error::other)?;
        self.db
            .conn()
            .execute(
                "INSERT INTO topics (name, definition) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET definition = excluded.definition",
                params![definition.name, text],
            )
            .map_err(|e| self.db.fail(e))?;
        Ok(())
    }

    fn delete(&self, name: &str) -> io::Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM topics WHERE name = ?1", params![name])
            .map_err(|e| self.db.fail(e))?;
        Ok(())
    }
}

/// The test implementation: memory, with a programmable failure.
#[derive(Debug, Default)]
pub struct MemoryTopicStore {
    topics: Mutex<BTreeMap<String, TopicDefinition>>,
    failing: AtomicBool,
}

impl MemoryTopicStore {
    /// With `true`, `save` and `disk` fail, like a full disk.
    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    pub fn names(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, TopicDefinition>> {
        self.topics.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn check(&self) -> io::Result<()> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(io::Error::other("failure programmed by the test"));
        }
        Ok(())
    }
}

impl TopicStore for MemoryTopicStore {
    fn load_all(&self) -> io::Result<Vec<TopicDefinition>> {
        Ok(self.lock().values().cloned().collect())
    }

    fn save(&self, definition: &TopicDefinition) -> io::Result<()> {
        self.check()?;
        self.lock()
            .insert(definition.name.clone(), definition.clone());
        Ok(())
    }

    fn delete(&self, name: &str) -> io::Result<()> {
        self.check()?;
        self.lock().remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
