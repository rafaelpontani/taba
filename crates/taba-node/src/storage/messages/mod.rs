//! As mensagens em disco (doc 02, seção 3.2): o corpo em `${topic}/${id}.body`, só os bytes
//! como vieram, e o resto numa linha da tabela `messages` do banco do tópico,
//! `${topic}/_topic.db`, estritamente local ao nó.
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

pub const TOPIC_DB_FILE: &str = "_topic.db";
const BODY_EXT: &str = "body";

/// Tudo o que se sabe de uma mensagem menos o corpo: é o que vai para a linha do banco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageMeta {
    pub id: String,
    pub content_type: String,
    pub group_id: Option<String>,
    /// Só na fila FIFO: a chave de deduplicação dentro do grupo (a informada, ou o SHA-256 do
    /// corpo em hexadecimal).
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    pub delivery_count: u32,
    /// Instante a partir do qual a mensagem é entregável. `0`: nunca foi entregue.
    pub invisible_until_ms: u64,
    pub receipt_token: Option<String>,
    /// Quando entrou na dead-letter (e por quê). Dali em diante o `ttl` conta deste instante.
    pub dead_lettered_ms: Option<u64>,
    pub dead_letter_reason: Option<String>,
    /// A posição no anel de hash (doc 05, seção 7.1): é por ela que se acha o bloco da mensagem.
    /// Quem guarda a mensagem a calcula (a da fila FIFO é a do grupo).
    pub position: u64,
}

/// A posição no anel como o banco a guarda: o `u64` com o bit mais alto trocado, num inteiro com
/// sinal que mantém a mesma ordem (o SQLite não tem inteiro sem sinal).
fn position_to_db(position: u64) -> i64 {
    (position ^ (1 << 63)) as i64
}

fn position_from_db(stored: i64) -> u64 {
    (stored as u64) ^ (1 << 63)
}

impl MessageMeta {
    /// O instante de onde o `ttl` conta.
    pub fn age_base_ms(&self) -> u64 {
        self.dead_lettered_ms.unwrap_or(self.published_ms)
    }
}

/// Uma linha de [`MessageStore::block_rows`]: o que a listagem de IDs de um bloco diz de cada
/// mensagem (doc 06c, `GET /cluster/blocks/{id}/ids`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRow {
    pub id: String,
    /// Só na fila FIFO, na linha curta que o ack deixa.
    pub delete_at_ms: Option<u64>,
}

/// Uma linha de [`MessageStore::scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRow {
    pub id: String,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    /// A linha curta que o ack da fila FIFO deixa para a deduplicação (`deleteAt`): sem corpo, sem
    /// entrega e fora do índice.
    pub delete_at_ms: Option<u64>,
    pub age_ms: u64,
    pub delivery_count: u32,
    pub invisible_until_ms: u64,
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS messages (
        id                 TEXT PRIMARY KEY NOT NULL,
        content_type       TEXT NOT NULL,
        group_id              TEXT,
        published_at       INTEGER NOT NULL,
        delivery_count     INTEGER NOT NULL DEFAULT 0,
        invisible_until    INTEGER NOT NULL DEFAULT 0,
        receipt_token      TEXT,
        dead_lettered_at   INTEGER,
        dead_letter_reason TEXT,
        age_base           INTEGER NOT NULL,
        deduplication_id   TEXT,
        delete_at          INTEGER,
        position           INTEGER NOT NULL DEFAULT 0
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS messages_invisible_until ON messages (invisible_until);
    CREATE INDEX IF NOT EXISTS messages_age_base ON messages (age_base);
";

/// Os índices das colunas da fila FIFO, criados depois de `migrate` (um banco de antes delas
/// ainda não as tem).
const FIFO_INDEXES: &str = "
    CREATE INDEX IF NOT EXISTS messages_group_dedup ON messages (group_id, deduplication_id);
    CREATE INDEX IF NOT EXISTS messages_delete_at ON messages (delete_at);
    CREATE INDEX IF NOT EXISTS messages_position ON messages (position);
";

/// Acrescenta ao banco de uma versão anterior as colunas que ele não tem.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('messages')")?;
    let have: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    for (name, kind) in [
        ("deduplication_id", "TEXT"),
        ("delete_at", "INTEGER"),
        ("position", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !have.iter().any(|c| c == name) {
            conn.execute_batch(&format!("ALTER TABLE messages ADD COLUMN {name} {kind}"))?;
        }
    }
    Ok(())
}

const COLUMNS: &str = "id, content_type, group_id, published_at, delivery_count, invisible_until, \
                       receipt_token, dead_lettered_at, dead_letter_reason, deduplication_id, \
                       position";

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
        deduplication_id: row.get(9)?,
        position: position_from_db(row.get::<_, i64>(10)?),
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
        migrate(&conn).map_err(to_io)?;
        conn.execute_batch(FIFO_INDEXES).map_err(to_io)?;
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
                "INSERT OR REPLACE INTO messages (id, content_type, group_id, published_at, \
                 delivery_count, invisible_until, receipt_token, dead_lettered_at, \
                 dead_letter_reason, age_base, deduplication_id, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                    meta.deduplication_id,
                    position_to_db(meta.position),
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
                &format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"),
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
                "UPDATE messages SET delivery_count = ?2, invisible_until = ?3, \
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
            .execute("DELETE FROM messages WHERE id = ?1", params![id])
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

    /// O que o índice do tópico precisa de cada mensagem em disco (sem o corpo), para refazê-lo
    /// quando o tópico é aberto: `id`, grupo, idade, entregas e fim da invisibilidade.
    pub fn scan(&self, topic: &str) -> io::Result<Vec<ScanRow>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        let conn = disk.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, group_id, age_base, delivery_count, invisible_until, deduplication_id, \
                 published_at, delete_at FROM messages ORDER BY id",
            )
            .map_err(to_io)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ScanRow {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    age_ms: row.get::<_, i64>(2)? as u64,
                    delivery_count: row.get::<_, i64>(3)? as u32,
                    invisible_until_ms: row.get::<_, i64>(4)? as u64,
                    deduplication_id: row.get(5)?,
                    published_ms: row.get::<_, i64>(6)? as u64,
                    delete_at_ms: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                })
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// As mensagens do tópico cuja posição cai em alguma das `ranges` (os arcos de um bloco), com
    /// `id` entre `from` e `to` (inclusivos), em ordem de `id`, no máximo `limit`. Inclui a linha
    /// curta da fila FIFO (`delete_at`).
    pub fn block_rows(
        &self,
        topic: &str,
        ranges: &[(u64, u64)],
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<BlockRow>> {
        use rusqlite::types::Value;
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        if ranges.is_empty() {
            return Ok(Vec::new());
        }
        let mut args: Vec<Value> = Vec::new();
        let mut sql = String::from("SELECT id, delete_at FROM messages WHERE (");
        for (i, (low, high)) in ranges.iter().enumerate() {
            if i > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str("position BETWEEN ? AND ?");
            args.push(Value::Integer(position_to_db(*low)));
            args.push(Value::Integer(position_to_db(*high)));
        }
        sql.push(')');
        if let Some(from) = from {
            sql.push_str(" AND id >= ?");
            args.push(Value::Text(from.to_owned()));
        }
        if let Some(to) = to {
            sql.push_str(" AND id <= ?");
            args.push(Value::Text(to.to_owned()));
        }
        sql.push_str(" ORDER BY id LIMIT ?");
        args.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
        let conn = disk.conn();
        let mut stmt = conn.prepare(&sql).map_err(to_io)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |row| {
                Ok(BlockRow {
                    id: row.get(0)?,
                    delete_at_ms: row.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                })
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// Os `limit` primeiros ids entregáveis (prazo vencido), em ordem de `id`.
    pub fn ready_ids(&self, topic: &str, now_ms: u64, limit: usize) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM messages WHERE invisible_until <= ?1 ORDER BY id LIMIT ?2",
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
                "SELECT MIN(invisible_until) FROM messages WHERE invisible_until > ?1",
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
            "SELECT id FROM messages WHERE age_base <= ?1",
            &[limit],
        )
    }

    /// As mensagens que gastaram as entregas e cuja invisibilidade acabou.
    pub fn exhausted_ids(&self, topic: &str, now_ms: u64, max: u64) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM messages WHERE delivery_count > 0 AND delivery_count >= ?1 \
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
            .execute("DELETE FROM messages WHERE id = ?1", params![meta.id])
            .map_err(to_io)?;
        Ok(())
    }

    /// O ack da fila FIFO: a mensagem vira uma linha curta com `delete_at` (doc 17, seção 16.7).
    /// Ficam `id`, grupo, `deduplicationId` e as datas; saem o corpo e os metadados de entrega. A
    /// linha vale mesmo que a mensagem estivesse só em memória. Primeiro a linha, depois o arquivo.
    pub fn tombstone(&self, topic: &str, meta: &MessageMeta, delete_at_ms: u64) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("criado");
        disk.conn()
            .execute(
                "INSERT OR REPLACE INTO messages (id, content_type, group_id, published_at, \
                 delivery_count, invisible_until, receipt_token, dead_lettered_at, \
                 dead_letter_reason, age_base, deduplication_id, delete_at, position) \
                 VALUES (?1, '', ?2, ?3, 0, 0, NULL, NULL, NULL, ?4, ?5, ?6, ?7)",
                params![
                    meta.id,
                    meta.group_id,
                    meta.published_ms as i64,
                    meta.published_ms as i64,
                    meta.deduplication_id,
                    delete_at_ms as i64,
                    position_to_db(meta.position),
                ],
            )
            .map_err(to_io)?;
        match std::fs::remove_file(disk.body_path(&meta.id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Apaga as linhas curtas cujo `delete_at` passou, pelo índice, e devolve quantas.
    pub fn sweep_tombstones(&self, topic: &str, now_ms: u64) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let removed = disk
            .conn()
            .execute(
                "DELETE FROM messages WHERE delete_at IS NOT NULL AND delete_at <= ?1",
                params![now_ms as i64],
            )
            .map_err(to_io)?;
        Ok(removed as u64)
    }

    /// O espaço livre, em bytes, no volume de `storage.dir` (o que um processo sem privilégio pode
    /// usar).
    pub fn free_bytes(&self) -> io::Result<u64> {
        fs4::available_space(&self.root)
    }

    /// A soma dos tamanhos dos arquivos de corpo do tópico (lê só os metadados dos arquivos, em
    /// ordem de pasta: custa uma chamada por mensagem em disco).
    pub fn body_bytes(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let mut total = 0;
        for entry in std::fs::read_dir(&disk.dir)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == BODY_EXT) {
                match entry.metadata() {
                    Ok(meta) => total += meta.len(),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(total)
    }

    pub fn count(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let n: i64 = disk
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE delete_at IS NULL",
                [],
                |row| row.get(0),
            )
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
