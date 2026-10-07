//! As mensagens em disco (doc 02, seção 3.2): o corpo em `${topico}/${id}.body`, só os bytes
//! como vieram, e o resto numa linha da tabela `mensagens` do banco do tópico,
//! `${topico}/_topico.db`, estritamente local ao nó.
//!
//! **Ordem de gravação:** na escrita, primeiro o arquivo e depois a linha; na remoção,
//! primeiro a linha e depois o arquivo. Uma queda deixa no máximo arquivo sem linha (lixo
//! inofensivo) ou linha sem arquivo (a mensagem se perdeu: é descartada na leitura).
//!
//! **Escritor único:** cada banco tem uma conexão atrás de um `Mutex`; toda escrita espera a
//! sua vez. Os métodos são síncronos: o `engine` os chama fora do executor assíncrono
//! (`spawn_blocking`).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use super::db::to_io;
use crate::config::DbSynchronous;

pub const TOPIC_DB_FILE: &str = "_topico.db";
const BODY_EXT: &str = "body";

/// Tudo o que se sabe de uma mensagem menos o corpo: é o que vai para a linha do banco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageMeta {
    pub id: String,
    pub content_type: String,
    pub group_id: Option<String>,
    pub published_ms: u64,
    pub delivery_count: u32,
    /// Instante a partir do qual a mensagem é entregável. `0`: nunca foi entregue.
    pub invisible_until_ms: u64,
    pub receipt_token: Option<String>,
    /// Quando entrou na dead-letter (e por quê). Dali em diante o `ttl` conta deste instante.
    pub dead_lettered_ms: Option<u64>,
    pub dead_letter_reason: Option<String>,
}

impl MessageMeta {
    /// O instante de onde o `ttl` conta.
    pub fn age_base_ms(&self) -> u64 {
        self.dead_lettered_ms.unwrap_or(self.published_ms)
    }
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS mensagens (
        id                 TEXT PRIMARY KEY NOT NULL,
        content_type       TEXT NOT NULL,
        grupo              TEXT,
        publicada_em       INTEGER NOT NULL,
        delivery_count     INTEGER NOT NULL DEFAULT 0,
        invisible_until    INTEGER NOT NULL DEFAULT 0,
        receipt_token      TEXT,
        dead_lettered_em   INTEGER,
        dead_letter_reason TEXT,
        age_base           INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS mensagens_invisible_until ON mensagens (invisible_until);
    CREATE INDEX IF NOT EXISTS mensagens_age_base ON mensagens (age_base);
";

const COLUMNS: &str = "id, content_type, grupo, publicada_em, delivery_count, invisible_until, \
                       receipt_token, dead_lettered_em, dead_letter_reason";

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageMeta> {
    Ok(MessageMeta {
        id: row.get(0)?,
        content_type: row.get(1)?,
        group_id: row.get(2)?,
        published_ms: row.get::<_, i64>(3)? as u64,
        delivery_count: row.get::<_, i64>(4)? as u32,
        invisible_until_ms: row.get::<_, i64>(5)? as u64,
        receipt_token: row.get(6)?,
        dead_lettered_ms: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        dead_letter_reason: row.get(8)?,
    })
}

/// A pasta de um tópico. Os nomes de tópico diferenciam maiúsculas de minúsculas e o disco do
/// Windows e do macOS não: cada maiúscula vira `~` mais a minúscula (`Pedidos` → `~pedidos`).
/// O `~` não pode estar num nome de tópico (`A-Za-z0-9_-`), então a troca não é ambígua.
pub fn dir_name(topic: &str) -> String {
    let mut stem = String::with_capacity(topic.len());
    for c in topic.chars() {
        if c.is_ascii_uppercase() {
            stem.push('~');
        }
        stem.push(c.to_ascii_lowercase());
    }
    stem
}

/// O nome do tópico de uma pasta (o inverso de `dir_name`); `None` se não é uma pasta nossa.
fn topic_of_dir(dir: &str) -> Option<String> {
    let mut topic = String::with_capacity(dir.len());
    let mut chars = dir.chars();
    while let Some(c) = chars.next() {
        if c == '~' {
            let next = chars.next().filter(char::is_ascii_lowercase)?;
            topic.push(next.to_ascii_uppercase());
        } else if c.is_ascii_uppercase() {
            return None;
        } else {
            topic.push(c);
        }
    }
    Some(topic)
}

/// O disco de um tópico: a pasta e o banco.
#[derive(Debug)]
struct TopicDisk {
    dir: PathBuf,
    conn: Mutex<Connection>,
}

impl TopicDisk {
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn body_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{BODY_EXT}"))
    }
}

/// Todas as mensagens em disco do nó, por tópico. Os bancos abrem sob demanda: um tópico que
/// nunca foi para o disco não tem pasta.
#[derive(Debug)]
pub struct MessageStore {
    root: PathBuf,
    synchronous: DbSynchronous,
    topics: Mutex<HashMap<String, Arc<TopicDisk>>>,
}

impl MessageStore {
    /// `root` é o `storage.dir`; cada tópico ganha uma pasta dentro dele.
    pub fn new(root: &Path, synchronous: DbSynchronous) -> Self {
        Self {
            root: root.to_path_buf(),
            synchronous,
            topics: Mutex::new(HashMap::new()),
        }
    }

    fn topics(&self) -> MutexGuard<'_, HashMap<String, Arc<TopicDisk>>> {
        self.topics.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// O disco do tópico. Sem `create`, um tópico sem pasta é `None` (leituras não criam nada).
    fn disk(&self, topic: &str, create: bool) -> io::Result<Option<Arc<TopicDisk>>> {
        let mut topics = self.topics();
        if let Some(disk) = topics.get(topic) {
            return Ok(Some(disk.clone()));
        }
        let dir = self.root.join(dir_name(topic));
        if !create && !dir.join(TOPIC_DB_FILE).is_file() {
            return Ok(None);
        }
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join(TOPIC_DB_FILE)).map_err(to_io)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(to_io)?;
        conn.pragma_update(
            None,
            "synchronous",
            super::db::synchronous_name(self.synchronous),
        )
        .map_err(to_io)?;
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        let disk = Arc::new(TopicDisk {
            dir,
            conn: Mutex::new(conn),
        });
        topics.insert(topic.to_owned(), disk.clone());
        Ok(Some(disk))
    }

    /// Grava a mensagem: primeiro o arquivo do corpo, depois a linha.
    pub fn put(&self, topic: &str, meta: &MessageMeta, body: &[u8]) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("criado");
        let target = disk.body_path(&meta.id);
        let tmp = target.with_extension(format!("{BODY_EXT}.tmp"));
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &target)?;
        Self::insert_row(&disk, meta)
    }

    fn insert_row(disk: &TopicDisk, meta: &MessageMeta) -> io::Result<()> {
        disk.conn()
            .execute(
                "INSERT OR REPLACE INTO mensagens (id, content_type, grupo, publicada_em, \
                 delivery_count, invisible_until, receipt_token, dead_lettered_em, \
                 dead_letter_reason, age_base) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    meta.id,
                    meta.content_type,
                    meta.group_id,
                    meta.published_ms as i64,
                    meta.delivery_count,
                    meta.invisible_until_ms as i64,
                    meta.receipt_token,
                    meta.dead_lettered_ms.map(|v| v as i64),
                    meta.dead_letter_reason,
                    meta.age_base_ms() as i64,
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    pub fn get(&self, topic: &str, id: &str) -> io::Result<Option<MessageMeta>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        disk.conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM mensagens WHERE id = ?1"),
                params![id],
                from_row,
            )
            .optional()
            .map_err(to_io)
    }

    /// Grava de novo os campos que mudam na entrega: contador, prazo e `receiptToken`.
    pub fn update_delivery(&self, topic: &str, meta: &MessageMeta) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        disk.conn()
            .execute(
                "UPDATE mensagens SET delivery_count = ?2, invisible_until = ?3, \
                 receipt_token = ?4 WHERE id = ?1",
                params![
                    meta.id,
                    meta.delivery_count,
                    meta.invisible_until_ms as i64,
                    meta.receipt_token
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// Os bytes do corpo, como foram gravados.
    pub fn body(&self, topic: &str, id: &str) -> io::Result<Vec<u8>> {
        match self.disk(topic, false)? {
            Some(disk) => std::fs::read(disk.body_path(id)),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    /// Remove a mensagem: primeiro a linha, depois o arquivo.
    pub fn remove(&self, topic: &str, id: &str) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        disk.conn()
            .execute("DELETE FROM mensagens WHERE id = ?1", params![id])
            .map_err(to_io)?;
        match std::fs::remove_file(disk.body_path(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn ids(&self, topic: &str, sql: &str, args: &[i64]) -> io::Result<Vec<String>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        let conn = disk.conn();
        let mut stmt = conn.prepare_cached(sql).map_err(to_io)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |row| row.get(0))
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// Os `limit` primeiros ids entregáveis (prazo vencido), em ordem de `id`.
    pub fn ready_ids(&self, topic: &str, now_ms: u64, limit: usize) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM mensagens WHERE invisible_until <= ?1 ORDER BY id LIMIT ?2",
            &[now_ms as i64, limit as i64],
        )
    }

    /// O instante em que a próxima invisível volta a ser entregável.
    pub fn next_invisible_until(&self, topic: &str, now_ms: u64) -> io::Result<Option<u64>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        let next: Option<i64> = disk
            .conn()
            .query_row(
                "SELECT MIN(invisible_until) FROM mensagens WHERE invisible_until > ?1",
                params![now_ms as i64],
                |row| row.get(0),
            )
            .map_err(to_io)?;
        Ok(next.map(|v| v as u64))
    }

    /// As mensagens cujo `ttl` venceu: `age_base + ttl ≤ agora`.
    pub fn expired_ids(&self, topic: &str, now_ms: u64, ttl_ms: u64) -> io::Result<Vec<String>> {
        let limit = i64::try_from(now_ms.saturating_sub(ttl_ms)).unwrap_or(i64::MAX);
        if ttl_ms > now_ms {
            return Ok(Vec::new());
        }
        self.ids(
            topic,
            "SELECT id FROM mensagens WHERE age_base <= ?1",
            &[limit],
        )
    }

    /// As mensagens que gastaram as entregas e cuja invisibilidade acabou.
    pub fn exhausted_ids(&self, topic: &str, now_ms: u64, max: u64) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM mensagens WHERE delivery_count > 0 AND delivery_count >= ?1 \
             AND invisible_until <= ?2",
            &[i64::try_from(max).unwrap_or(i64::MAX), now_ms as i64],
        )
    }

    /// Move a mensagem para a pasta de outro tópico (a dead-letter), trocando os metadados.
    /// O arquivo é renomeado, sem copiar o corpo.
    pub fn move_to(&self, from: &str, to: &str, meta: &MessageMeta) -> io::Result<()> {
        let source = self.disk(from, true)?.expect("criado");
        let target = self.disk(to, true)?.expect("criado");
        std::fs::rename(source.body_path(&meta.id), target.body_path(&meta.id))?;
        Self::insert_row(&target, meta)?;
        source
            .conn()
            .execute("DELETE FROM mensagens WHERE id = ?1", params![meta.id])
            .map_err(to_io)?;
        Ok(())
    }

    pub fn count(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let n: i64 = disk
            .conn()
            .query_row("SELECT COUNT(*) FROM mensagens", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// Os tópicos que têm pasta com banco em `storage.dir`.
    pub fn topics_on_disk(&self) -> io::Result<Vec<String>> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if !path.join(TOPIC_DB_FILE).is_file() {
                continue;
            }
            if let Some(topic) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(topic_of_dir)
            {
                out.push(topic);
            }
        }
        Ok(out)
    }

    /// Apaga a pasta do tópico, com o banco e os corpos.
    pub fn drop_topic(&self, topic: &str) -> io::Result<()> {
        self.topics().remove(topic);
        match std::fs::remove_dir_all(self.root.join(dir_name(topic))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
