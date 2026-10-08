//! Persistência das definições de tópico: a tabela `topics` do `_node.db`, uma linha por
//! tópico, gravada no commit (doc 02, seções 3.2 e 3.3).
//!
//! O trait mora aqui, na camada mais baixa de que o `engine` precisa, porque as
//! dependências só andam para a direita (doc 18, seção 17.3). Os métodos são
//! síncronos: o `engine` os chama fora do executor assíncrono (`spawn_blocking`).

use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::params;

use super::db::{NodeDb, to_io};
use crate::core::topic::TopicDefinition;

pub trait TopicStore: Send + Sync + 'static {
    /// Todas as definições gravadas. Uma linha ilegível é erro: o nó não sobe com
    /// uma tabela de tópicos pela metade.
    fn load_all(&self) -> io::Result<Vec<TopicDefinition>>;

    /// Grava (ou substitui) a definição, de forma atômica: nunca uma linha pela metade.
    fn save(&self, definition: &TopicDefinition) -> io::Result<()>;

    /// Apaga a definição. Apagar o que não existe não é erro.
    fn delete(&self, name: &str) -> io::Result<()>;
}

/// A implementação real, no banco do nó. O nome é a chave e o SQLite compara texto byte a
/// byte, então `Pedidos` e `pedidos` são tópicos diferentes (UC-TOP-011) em qualquer disco.
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
            .map_err(to_io)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(to_io)?;
        let mut out = Vec::new();
        for row in rows {
            let (name, text) = row.map_err(to_io)?;
            let invalid = |reason: String| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("tópico `{name}` em _node.db: {reason}"),
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
            .map_err(to_io)?;
        Ok(())
    }

    fn delete(&self, name: &str) -> io::Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM topics WHERE name = ?1", params![name])
            .map_err(to_io)?;
        Ok(())
    }
}

/// A implementação de teste: memória, com falha programável.
#[derive(Debug, Default)]
pub struct MemoryTopicStore {
    topics: Mutex<BTreeMap<String, TopicDefinition>>,
    failing: AtomicBool,
}

impl MemoryTopicStore {
    /// Com `true`, `save` e `delete` falham, como um disco cheio.
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
            return Err(io::Error::other("falha programada pelo teste"));
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
