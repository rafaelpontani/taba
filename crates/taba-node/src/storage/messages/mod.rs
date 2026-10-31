//! The messages on disk (doc 02, section 3.2): everything in the topic's database,
//! `${topic}/_topic.db`, strictly local to the node. The row of the `messages` table holds the metadata
//! and, up to `storage.body.inline-max` bytes, the body itself (`body` column); a larger body goes to the
//! `bodies` table (`id`, `body`), linked by `id` and written in the same transaction as the row.
//!
//! **Write order:** one transaction writes (or removes) the row and the body, so a crash leaves both or
//! neither. A topic folder from before this layout still has `${id}.body` files: they are read when the row
//! has no body, and removed with the message, but nothing writes a file any more.
//!
//! **Um escritor, commit em grupo (D192):** cada banco tem uma conexão de escrita. As escritas quentes (publicar,
//! entregar, confirmar, os pares da `expiry`) passam pela fila de escrita do tópico (`writer`), que grava tudo o que
//! espera numa transação; as outras pegam a conexão de escrita depois de a fila esvaziar. As leituras usam conexões
//! só de leitura, em paralelo com a escrita, e veem o que ainda está na fila. Os métodos são síncronos: o `engine`
//! os chama fora do executor assíncrono (`spawn_blocking`).

use std::collections::HashMap;
use std::io;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, OnceLock, RwLock, RwLockWriteGuard};

use rusqlite::{Connection, OptionalExtension, params};

use super::db::to_io;
use crate::config::DbSynchronous;

mod excluded;
mod writer;
use excluded::ExcludedLog;
pub use excluded::{
    DEFAULT_MAX_AGE_MS, DEFAULT_MAX_BYTES, ExcludedLimits, ExcludedRow, merged_since,
};
pub use writer::Written;
use writer::{Op, Pending};

pub const TOPIC_DB_FILE: &str = "_topic.db";
/// The extension of the body files of the layout before the bodies went into the database.
const BODY_EXT: &str = "body";
/// The largest body kept in the `messages` row itself, when the store is not told otherwise: a bigger one
/// lives in `bodies`, so the state updates of the row (delivery, deadline) do not rewrite it.
pub const DEFAULT_BODY_INLINE_MAX: usize = 256;
/// The page cache of each topic's database, in the negative form of `PRAGMA cache_size` (KiB): 32 MB.
const TOPIC_CACHE_KIB: i64 = -32 * 1024;
/// Páginas do WAL a partir das quais a própria escrita faz o checkpoint: só uma rede de segurança (uns 400 MB), porque a
/// fila de escrita já pede o checkpoint em segundo plano (`writer`), e o feito na escrita segurava o escritor (D192).
const TOPIC_CHECKPOINT_PAGES: i64 = 100_000;

/// Everything known about a message except the body: it is what goes into the database row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageMeta {
    pub id: String,
    pub content_type: String,
    pub group_id: Option<String>,
    /// FIFO queue only: the deduplication key within the group (the given one, or the SHA-256 of the
    /// body in hexadecimal).
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    pub delivery_count: u32,
    /// Instant from which the message is deliverable. `0`: it was never delivered.
    pub invisible_until_ms: u64,
    pub receipt_token: Option<String>,
    /// When it entered the dead-letter (and why). From then on the `ttl` counts from this instant.
    pub dead_lettered_ms: Option<u64>,
    pub dead_letter_reason: Option<String>,
    /// The position on the hash ring (doc 05, section 7.1): it is how the message's block is found.
    /// Whoever stores the message computes it (a FIFO queue's is the group's).
    pub position: u64,
}

/// The position on the ring as the database stores it: the `u64` with the highest bit flipped, in a signed
/// integer that keeps the same order (SQLite has no unsigned integer).
fn position_to_db(position: u64) -> i64 {
    (position ^ (1 << 63)) as i64
}

fn position_from_db(stored: i64) -> u64 {
    (stored as u64) ^ (1 << 63)
}

impl MessageMeta {
    /// The instant the `ttl` counts from.
    pub fn age_base_ms(&self) -> u64 {
        self.dead_lettered_ms.unwrap_or(self.published_ms)
    }
}

/// A row of [`MessageStore::block_rows`]: what a block's ID listing says about each
/// message (doc 06c, `GET /cluster/blocks/{id}/ids`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRow {
    pub id: String,
    /// FIFO queue only, on the short row the ack leaves.
    pub delete_at_ms: Option<u64>,
}

/// Which live rows [`MessageStore::page`] brings: a page of the topic's messages in `id` order, without the
/// short row the FIFO queue's ack leaves.
#[derive(Debug, Clone, Default)]
pub struct PageQuery {
    /// Only `id` after this (exclusive).
    pub after: Option<String>,
    /// Only `id` before this (exclusive).
    pub before: Option<String>,
    /// Descending `id` order.
    pub desc: bool,
    pub limit: usize,
    /// Only messages delivered at least once.
    pub delivered: bool,
    /// Leaves out the ones that have a pair in the `expiry` table (those in processing).
    pub off_table: bool,
    /// Only this group (empty: the messages without a group).
    pub group: Option<String>,
    /// Also the short row the FIFO queue's ack leaves (`delete_at`).
    pub tombstones: bool,
}

/// A row of [`MessageStore::scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRow {
    pub id: String,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub published_ms: u64,
    /// The short row the FIFO queue's ack leaves for deduplication (`deleteAt`): no body, no
    /// delivery and outside the index.
    pub delete_at_ms: Option<u64>,
    pub age_ms: u64,
    pub delivery_count: u32,
    pub invisible_until_ms: u64,
}

/// The `ScanRow` of a `SELECT id, group_id, age_base, delivery_count, invisible_until,
/// deduplication_id, published_at, delete_at`.
fn scan_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScanRow> {
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
}

/// A linha de uma página lida por `id` (a que a fila pendente fez o SQL deixar de fora).
const SCAN_BY_ID: &str = "SELECT id, group_id, age_base, delivery_count, invisible_until, deduplication_id, \
                          published_at, delete_at FROM messages WHERE id = ?1 AND delete_at IS NULL";

/// O `ScanRow` de uma mensagem viva que ainda está na fila de escrita.
fn scan_of(meta: &MessageMeta) -> ScanRow {
    ScanRow {
        id: meta.id.clone(),
        group_id: meta.group_id.clone(),
        deduplication_id: meta.deduplication_id.clone(),
        published_ms: meta.published_ms,
        delete_at_ms: None,
        age_ms: meta.age_base_ms(),
        delivery_count: meta.delivery_count,
        invisible_until_ms: meta.invisible_until_ms,
    }
}

/// A linha passa nos filtros da página que não são a faixa nem a `expiry`.
fn page_fits(query: &PageQuery, row: &ScanRow) -> bool {
    (query.tombstones || row.delete_at_ms.is_none())
        && (!query.delivered || row.delivery_count > 0)
        && query.group.as_deref().is_none_or(|group| {
            if group.is_empty() {
                row.group_id.as_deref().is_none_or(str::is_empty)
            } else {
                row.group_id.as_deref() == Some(group)
            }
        })
}

/// A mensagem tem par gravado na `expiry`.
fn expiry_has(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.prepare_cached("SELECT EXISTS (SELECT 1 FROM expiry WHERE id = ?1)")?
        .query_row(params![id], |row| row.get(0))
}

/// O escopo atômico de uma operação: uma transação própria quando a conexão está livre, e nada quando já há uma
/// aberta (a rodada da fila de escrita, `writer`). Um ponto de salvamento dentro do lote custaria um subjournal
/// por página tocada (23% do CPU de um servidor publicando em lote); o lote já é atômico.
pub(super) struct Scope<'c> {
    conn: &'c Connection,
    own: bool,
}

impl<'c> Scope<'c> {
    pub(super) fn begin(conn: &'c mut Connection) -> rusqlite::Result<Self> {
        let own = conn.is_autocommit();
        if own {
            conn.execute_batch("BEGIN")?;
        }
        Ok(Self { conn, own })
    }

    pub(super) fn commit(mut self) -> rusqlite::Result<()> {
        if std::mem::take(&mut self.own) {
            self.conn.execute_batch("COMMIT")?;
        }
        Ok(())
    }
}

impl std::ops::Deref for Scope<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if self.own {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
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
        position           INTEGER NOT NULL DEFAULT 0,
        body               BLOB
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS messages_invisible_until ON messages (invisible_until);
    CREATE INDEX IF NOT EXISTS messages_age_base ON messages (age_base);
    -- The end of a group (oldest or newest ready message) is read by this index, in `id` order, without
    -- walking the topic.
    CREATE INDEX IF NOT EXISTS messages_group ON messages (group_id);
    -- The bodies above the inline limit: a table with a rowid on purpose (a `WITHOUT ROWID` one would copy the
    -- body into the interior pages of its tree), written once and deleted once, without a second index.
    CREATE TABLE IF NOT EXISTS bodies (
        id   TEXT PRIMARY KEY NOT NULL,
        body BLOB NOT NULL
    );
    CREATE TABLE IF NOT EXISTS excluded (
        id       TEXT PRIMARY KEY NOT NULL,
        position INTEGER NOT NULL,
        at_ms    INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS excluded_at ON excluded (at_ms);
    -- O índice por posição (`excluded_position`) não nasce aqui: só a leitura de um bloco o usa, e mantê-lo custa
    -- uma inserção aleatória por ack. É criado na primeira leitura de bloco (`excluded_rows`).
    CREATE TABLE IF NOT EXISTS topic_meta (
        key   TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS expiry (
        invisible_until INTEGER NOT NULL,
        id              TEXT NOT NULL,
        PRIMARY KEY (invisible_until, id)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS expiry_id ON expiry (id);
";

/// Key of the `topic_meta` table that holds the `topicId` the folder was created for (doc 07, section 10.6).
const TOPIC_ID_KEY: &str = "topic_id";

/// Tells the `topicId` of a topic name, when the folder of the topic is created.
pub type TopicIdResolver = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The resolver, wrapped to let the store derive `Debug`.
struct Resolver(TopicIdResolver);

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Resolver")
    }
}

/// The indexes of the FIFO queue columns, created after `migrate` (a database from before them
/// does not have them yet).
const FIFO_INDEXES: &str = "
    CREATE INDEX IF NOT EXISTS messages_group_dedup ON messages (group_id, deduplication_id)
        WHERE deduplication_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS messages_delete_at ON messages (delete_at) WHERE delete_at IS NOT NULL;
    CREATE INDEX IF NOT EXISTS messages_position ON messages (position);
";

/// Os índices que passaram a ser parciais (D192): só as linhas da fila FIFO têm `deduplication_id`, e só a linha curta
/// do ack FIFO tem `delete_at`; nas outras, cada escrita e cada remoção pagavam uma entrada inútil. Um banco de antes
/// tem a versão inteira, que sai aqui para `FIFO_INDEXES` criar a parcial (uma vez). Uma versão anterior que abra o
/// banco depois acha o índice pelo nome e não o recria; as consultas dela continuam corretas.
const PARTIAL_INDEXES: [&str; 2] = ["messages_group_dedup", "messages_delete_at"];

fn drop_full_indexes(conn: &Connection) -> rusqlite::Result<()> {
    for name in PARTIAL_INDEXES {
        let sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                params![name],
                |row| row.get(0),
            )
            .optional()?;
        if sql.is_some_and(|sql| !sql.contains("WHERE")) {
            conn.execute_batch(&format!("DROP INDEX {name}"))?;
        }
    }
    Ok(())
}

/// Adds to the database of an earlier version the columns it lacks.
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
        ("body", "BLOB"),
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

/// The reads of a message by `id`, with the statement kept prepared on the connection.
static SELECT_BY_ID: LazyLock<String> =
    LazyLock::new(|| format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"));
static SELECT_LIVE_BY_ID: LazyLock<String> =
    LazyLock::new(|| format!("SELECT {COLUMNS} FROM messages WHERE id = ?1 AND delete_at IS NULL"));

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

/// A topic's folder. Topic names are case-sensitive and the disk of Windows and macOS is not: each
/// capital becomes `~` plus the lowercase (`Orders` → `~orders`).
/// The `~` cannot be in a topic name (`A-Za-z0-9_-`), so the swap is not ambiguous.
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

/// The topic name of a folder (the inverse of `dir_name`); `None` if it is not one of our folders.
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

/// A topic's disk: the folder and the database.
#[derive(Debug)]
struct TopicDisk {
    dir: PathBuf,
    /// A conexão de escrita: a da líder da fila e a das escritas que não passam por ela.
    conn: Mutex<Connection>,
    /// A fila de escrita (`writer`) e a vez de quem espera por ela.
    queue: Mutex<writer::Queue>,
    turn: Condvar,
    /// As conexões só de leitura, guardadas para a próxima leitura.
    readers: Mutex<Vec<Connection>>,
    /// Um checkpoint em segundo plano está rodando.
    checkpointing: AtomicBool,
    /// A thread que espera as escritas desligadas (`Written::detach`) está rodando.
    flushing: AtomicBool,
    /// O maior corpo guardado na linha (ver [`DEFAULT_BODY_INLINE_MAX`]).
    inline_max: usize,
    /// Exclusions of messages that were only in memory, waiting to be written in one transaction
    /// (see `excluded`).
    pending_excluded: Mutex<Vec<excluded::PendingExclusion>>,
    /// The writer of the exclusions already has this disk in its queue.
    flush_queued: AtomicBool,
    /// The topic was dropped: the writer no longer writes here.
    dropped: AtomicBool,
    /// The table may have a live message: read when the database opens and set before every row written.
    /// Without one, asking whether an `id` exists reads nothing (a topic whose database only holds the
    /// expiry table and the log of excluded messages, the usual case when everything fits in memory).
    may_hold_messages: AtomicBool,
    /// The same for the `expiry` table: read when the database opens, set before every pair written and
    /// cleared with the table. Without a pair, the look for one reads nothing.
    may_hold_expiry: AtomicBool,
    /// The `bodies` table may have a row: read when the database opens and set before every body written there.
    /// Without one, removing a message does not look for its body.
    may_hold_bodies: AtomicBool,
    /// The folder still has `.body` files of the layout before the bodies went into the database: read when the
    /// database opens. Without them nothing touches the file system to read or remove a body.
    legacy_files: bool,
}

impl TopicDisk {
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A conexão de escrita para uma escrita que não passa pela fila: depois de tudo o que já está nela.
    fn exclusive(self: &Arc<Self>) -> MutexGuard<'_, Connection> {
        self.flush();
        self.conn()
    }

    /// Writes the message: the row and the body in one transaction (or one statement, when the body fits in the row).
    fn insert_row(&self, conn: &mut Connection, meta: &MessageMeta, body: &[u8]) -> io::Result<()> {
        self.may_hold_messages.store(true, Ordering::SeqCst);
        let inline = body.len() <= self.inline_max;
        if !inline {
            self.may_hold_bodies.store(true, Ordering::SeqCst);
        }
        // A body in the row, and no row of `bodies` that a body of the same `id` written before could have left:
        // one statement. Otherwise the two tables change together.
        if inline && !self.may_hold_bodies.load(Ordering::SeqCst) {
            return write_row(conn, meta, Some(body));
        }
        let tx = Scope::begin(conn).map_err(to_io)?;
        write_row(&tx, meta, inline.then_some(body))?;
        if inline {
            self.drop_body(&tx, &meta.id)?;
        } else {
            tx.prepare_cached("INSERT OR REPLACE INTO bodies (id, body) VALUES (?1, ?2)")
                .and_then(|mut stmt| stmt.execute(params![meta.id, body]))
                .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    fn body_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{BODY_EXT}"))
    }

    /// Deletes the message's row in `bodies` (if the table may have one). Inside the transaction of the row's own
    /// removal.
    fn drop_body(&self, conn: &Connection, id: &str) -> io::Result<()> {
        if self.may_hold_bodies.load(Ordering::SeqCst) {
            conn.prepare_cached("DELETE FROM bodies WHERE id = ?1")
                .and_then(|mut stmt| stmt.execute(params![id]))
                .map_err(to_io)?;
        }
        Ok(())
    }

    /// Removes the body file of the layout before the database, once the row is gone.
    fn drop_legacy_file(&self, id: &str) -> io::Result<()> {
        if !self.legacy_files {
            return Ok(());
        }
        match std::fs::remove_file(self.body_path(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// All the node's messages on disk, by topic. The databases open on demand: a topic that
/// never went to disk has no folder.
#[derive(Debug)]
pub struct MessageStore {
    root: PathBuf,
    synchronous: DbSynchronous,
    /// The topics already looked up: the open disk, or `None` for a topic with no database yet. Only this
    /// store creates a topic's database (`disk` with `create`), so a topic seen without one stays without it
    /// until then, and the reads of a publication (does this `id` exist?) do not ask the file system each time.
    topics: RwLock<HashMap<String, Option<Arc<TopicDisk>>>>,
    /// The log of excluded messages (doc 07, section 10.3).
    excluded: ExcludedLog,
    /// Records the `topicId` in each folder created, so a restoration can tell a recreated topic.
    topic_ids: OnceLock<Resolver>,
    /// The largest body kept in the row (see [`DEFAULT_BODY_INLINE_MAX`]).
    inline_max: usize,
    /// O nó parou e fechou os bancos ([`release_files`](Self::release_files)): nenhum banco abre de novo.
    released: AtomicBool,
}

impl MessageStore {
    /// `root` is the `storage.dir`; each topic gets a folder inside it.
    pub fn new(root: &Path, synchronous: DbSynchronous) -> Self {
        Self {
            root: root.to_path_buf(),
            synchronous,
            topics: RwLock::new(HashMap::new()),
            excluded: ExcludedLog::default(),
            topic_ids: OnceLock::new(),
            inline_max: DEFAULT_BODY_INLINE_MAX,
            released: AtomicBool::new(false),
        }
    }

    /// Sets the largest body kept in the `messages` row; a bigger one goes to `bodies` (`0`: all of them).
    pub fn with_body_inline_max(mut self, bytes: usize) -> Self {
        self.inline_max = bytes;
        self
    }

    /// From now on, each topic folder created records the `topicId` the resolver tells. Only the first call counts.
    pub fn set_topic_id_resolver(&self, resolver: TopicIdResolver) {
        let _ = self.topic_ids.set(Resolver(resolver));
    }

    /// The `topicId` the topic's folder was created for; `None` for a folder from before the record.
    pub fn topic_id(&self, topic: &str) -> io::Result<Option<String>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        disk.read(|conn| {
            conn.query_row(
                "SELECT value FROM topic_meta WHERE key = ?1",
                params![TOPIC_ID_KEY],
                |row| row.get(0),
            )
            .optional()
        })
    }

    /// A store over another folder (the folder of a past life), with the same durability.
    pub fn open_life(&self, dir: &Path) -> MessageStore {
        MessageStore::new(dir, self.synchronous).with_body_inline_max(self.inline_max)
    }

    /// Closes the topic databases the store holds open. Daí em diante nenhum banco abre de novo: uma tarefa do nó
    /// parado que ainda lê (um `reown` depois do drain) seguraria os arquivos, e um nó iniciado no mesmo disco no
    /// mesmo processo (os testes) não conseguiria movê-los para a pasta da vida anterior.
    pub fn release_files(&self) {
        self.flush_writes();
        let _ = self.flush_excluded();
        self.released.store(true, Ordering::SeqCst);
        self.topics().clear();
    }

    /// Espera as filas de escrita de todos os tópicos chegarem ao banco.
    pub fn flush_writes(&self) {
        let disks: Vec<Arc<TopicDisk>> = self
            .topics
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .flatten()
            .cloned()
            .collect();
        for disk in disks {
            disk.flush();
        }
    }

    fn topics(&self) -> RwLockWriteGuard<'_, HashMap<String, Option<Arc<TopicDisk>>>> {
        self.topics.write().unwrap_or_else(|e| e.into_inner())
    }

    /// The topic was already looked up and has no live message on disk (no database, or a database
    /// whose table never had one): asking whether an `id` exists touches nothing on disk.
    pub fn known_without_disk(&self, topic: &str) -> bool {
        match self
            .topics
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(topic)
        {
            Some(None) => true,
            Some(Some(disk)) => !disk.may_hold_messages.load(Ordering::SeqCst),
            None => false,
        }
    }

    /// The topic's disk. Without `create`, a topic without a folder is `None` (reads create nothing).
    fn disk(&self, topic: &str, create: bool) -> io::Result<Option<Arc<TopicDisk>>> {
        {
            let topics = self.topics.read().unwrap_or_else(|e| e.into_inner());
            match topics.get(topic) {
                Some(Some(disk)) => return Ok(Some(disk.clone())),
                Some(None) if !create => return Ok(None),
                _ => {}
            }
        }
        let mut topics = self.topics();
        match topics.get(topic) {
            Some(Some(disk)) => return Ok(Some(disk.clone())),
            Some(None) if !create => return Ok(None),
            _ => {}
        }
        if self.released.load(Ordering::SeqCst) {
            return if create {
                Err(io::Error::other(
                    "the node stopped: the topic databases are closed",
                ))
            } else {
                Ok(None)
            };
        }
        let dir = self.root.join(dir_name(topic));
        if !create && !dir.join(TOPIC_DB_FILE).is_file() {
            topics.insert(topic.to_owned(), None);
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
        // The page cache holds the indexes of the log of excluded messages (random by position): with the 2 MB
        // of the default, each insert evicted a page and read another from the file (D191). A ceiling, not a
        // reservation: the pages are only taken as they are read.
        conn.pragma_update(None, "cache_size", TOPIC_CACHE_KIB)
            .map_err(to_io)?;
        // The checkpoint copies the last version of each page of the WAL to the file and syncs it: the same pages
        // of those indexes are rewritten by many transactions, so a less frequent checkpoint copies each one once
        // instead of many times, and syncs less.
        conn.pragma_update(None, "wal_autocheckpoint", TOPIC_CHECKPOINT_PAGES)
            .map_err(to_io)?;
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        migrate(&conn).map_err(to_io)?;
        drop_full_indexes(&conn).map_err(to_io)?;
        conn.execute_batch(FIFO_INDEXES).map_err(to_io)?;
        if let Some(id) = self.topic_ids.get().and_then(|resolve| (resolve.0)(topic)) {
            conn.execute(
                "INSERT OR IGNORE INTO topic_meta (key, value) VALUES (?1, ?2)",
                params![TOPIC_ID_KEY, id],
            )
            .map_err(to_io)?;
        }
        let holds: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM messages WHERE delete_at IS NULL)",
                [],
                |row| row.get(0),
            )
            .map_err(to_io)?;
        let pairs: bool = conn
            .query_row("SELECT EXISTS (SELECT 1 FROM expiry)", [], |row| row.get(0))
            .map_err(to_io)?;
        let bodies: bool = conn
            .query_row("SELECT EXISTS (SELECT 1 FROM bodies)", [], |row| row.get(0))
            .map_err(to_io)?;
        let legacy_files = std::fs::read_dir(&dir)?
            .filter_map(Result::ok)
            .any(|entry| entry.path().extension().is_some_and(|e| e == BODY_EXT));
        let disk = Arc::new(TopicDisk {
            dir,
            conn: Mutex::new(conn),
            queue: Mutex::new(writer::Queue::default()),
            turn: Condvar::new(),
            readers: Mutex::new(Vec::new()),
            checkpointing: AtomicBool::new(false),
            flushing: AtomicBool::new(false),
            inline_max: self.inline_max,
            pending_excluded: Mutex::new(Vec::new()),
            flush_queued: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
            may_hold_messages: AtomicBool::new(holds),
            may_hold_expiry: AtomicBool::new(pairs),
            may_hold_bodies: AtomicBool::new(bodies),
            legacy_files,
        });
        topics.insert(topic.to_owned(), Some(disk.clone()));
        Ok(Some(disk))
    }

    /// Roda `work` num escopo adiado (D192): as escritas da fila feitas dentro dele só entram na fila, e o
    /// [`Written`] devolvido espera o commit delas. O motor solta o lock do tópico antes de esperar, e as chamadas
    /// que chegam nesse meio-tempo entram no mesmo commit. Uma leitura dentro do escopo vê o que ele escreveu.
    pub fn deferred<R>(&self, work: impl FnOnce() -> R) -> (R, Written) {
        writer::deferred(work)
    }

    /// Writes the message: the row and the body in one transaction (or one statement, when the body fits in the row).
    pub fn put(&self, topic: &str, meta: &MessageMeta, body: &[u8]) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("created");
        // Antes de entrar na fila: quem pergunta se o `id` existe não pula o banco (`known_without_disk`).
        disk.may_hold_messages.store(true, Ordering::SeqCst);
        disk.submit(Op::Put {
            meta: meta.clone(),
            body: Arc::from(body),
        })
    }

    pub fn get(&self, topic: &str, id: &str) -> io::Result<Option<MessageMeta>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        match disk.pending(id) {
            Some(Pending::Row { meta, .. } | Pending::Meta(meta)) => return Ok(Some(meta)),
            Some(Pending::Gone) => return Ok(None),
            None => {}
        }
        disk.read(|conn| {
            conn.prepare_cached(&SELECT_BY_ID)
                .and_then(|mut stmt| stmt.query_row(params![id], from_row).optional())
        })
    }

    /// Like [`MessageStore::get`], but only the live message: not the short row the FIFO queue's ack leaves.
    pub fn get_live(&self, topic: &str, id: &str) -> io::Result<Option<MessageMeta>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        match disk.pending(id) {
            Some(Pending::Row { meta, .. } | Pending::Meta(meta)) => return Ok(Some(meta)),
            Some(Pending::Gone) => return Ok(None),
            None => {}
        }
        if !disk.may_hold_messages.load(Ordering::SeqCst) {
            return Ok(None);
        }
        disk.read(|conn| {
            conn.prepare_cached(&SELECT_LIVE_BY_ID)
                .and_then(|mut stmt| stmt.query_row(params![id], from_row).optional())
        })
    }

    /// Writes again the fields that change on delivery: counter, deadline and `receiptToken`.
    pub fn update_delivery(&self, topic: &str, meta: &MessageMeta) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        disk.submit(Op::Delivery(meta.clone()))
    }

    /// Applies in order the operations on the `expiry` table (the invisible pairs that did not fit
    /// in the owner's list): `true` writes the pair, `false` deletes it. All in one transaction.
    pub fn expiry_apply(&self, topic: &str, ops: &[(bool, u64, String)]) -> io::Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        let Some(disk) = self.disk(topic, true)? else {
            return Ok(());
        };
        disk.submit(Op::Expiry(ops.to_vec()))
    }

    /// The `limit` pairs of the `expiry` table with the earliest deadline (ordered query by the table's
    /// own key).
    pub fn expiry_earliest(&self, topic: &str, limit: usize) -> io::Result<Vec<(u64, String)>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        // Sem esperar a fila: os pares dela entram por cima do banco, e um `id` com par na fila deixa de valer pelo
        // do banco (D193).
        disk.read_pending(Bound::Unbounded, Bound::Unbounded, |conn, view| {
            let mut stmt = conn.prepare_cached(
                "SELECT invisible_until, id FROM expiry ORDER BY invisible_until, id LIMIT ?1",
            )?;
            let wanted = limit.saturating_add(view.expiry.len());
            let rows = stmt.query_map(params![wanted as i64], |row| {
                Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
            })?;
            let mut pairs: Vec<(u64, String)> = rows
                .filter(|row| {
                    row.as_ref()
                        .map_or(true, |(_, id)| !view.expiry.contains_key(id))
                })
                .collect::<rusqlite::Result<_>>()?;
            pairs.extend(
                view.expiry
                    .iter()
                    .filter_map(|(id, until)| until.map(|until| (until, id.clone()))),
            );
            pairs.sort();
            pairs.truncate(limit);
            Ok(pairs)
        })
    }

    /// The deadline of the message's pair in the `expiry` table, if it has one.
    pub fn expiry_find(&self, topic: &str, id: &str) -> io::Result<Option<u64>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        // O par pode estar na fila: vale o último dela, sem esperar o commit (D193).
        if let Some(until) = disk.pending_expiry(id) {
            return Ok(until);
        }
        if !disk.may_hold_expiry.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let until: Option<i64> = disk.read(|conn| {
            conn.prepare_cached("SELECT invisible_until FROM expiry WHERE id = ?1 LIMIT 1")
                .and_then(|mut stmt| stmt.query_row(params![id], |row| row.get(0)).optional())
        })?;
        Ok(until.map(|v| v as u64))
    }

    /// How many pairs the `expiry` table holds.
    pub fn expiry_count(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        disk.flush();
        disk.read(|conn| {
            conn.query_row("SELECT COUNT(*) FROM expiry", [], |row| {
                row.get::<_, i64>(0)
            })
        })
        .map(|n| n as u64)
    }

    /// Empties the `expiry` table: it is derived state and is rebuilt from the messages when the topic
    /// is opened.
    pub fn expiry_clear(&self, topic: &str) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        // Cleared with the connection held: a pair written at the same time sets it again after.
        let conn = disk.exclusive();
        conn.execute("DELETE FROM expiry", []).map_err(to_io)?;
        disk.may_hold_expiry.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// The body bytes, as they were written.
    pub fn body(&self, topic: &str, id: &str) -> io::Result<Vec<u8>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Err(io::ErrorKind::NotFound.into());
        };
        match disk.pending(id) {
            Some(Pending::Row { body, .. }) => return Ok(body.to_vec()),
            Some(Pending::Gone) => return Err(io::ErrorKind::NotFound.into()),
            Some(Pending::Meta(_)) | None => {}
        }
        // In the row, or else in `bodies`: one statement.
        let found: Option<Option<Vec<u8>>> = disk.read(|conn| {
            conn.prepare_cached(
                "SELECT COALESCE(body, (SELECT body FROM bodies WHERE bodies.id = messages.id)) \
                 FROM messages WHERE id = ?1",
            )
            .and_then(|mut stmt| stmt.query_row(params![id], |row| row.get(0)).optional())
        })?;
        match found.flatten() {
            Some(body) => Ok(body),
            None if disk.legacy_files => std::fs::read(disk.body_path(id)),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    /// Removes the message: the row and its body in one transaction.
    pub fn remove(&self, topic: &str, id: &str) -> io::Result<()> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(());
        };
        disk.submit(Op::Remove {
            id: id.to_owned(),
            excluded_at: None,
        })
    }

    fn ids(&self, topic: &str, sql: &str, args: &[i64]) -> io::Result<Vec<String>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        disk.flush();
        disk.read(|conn| {
            let mut stmt = conn.prepare_cached(sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| row.get(0))?;
            rows.collect()
        })
    }

    /// What the topic's index needs from each message on disk (without the body), to rebuild it
    /// when the topic is opened: `id`, group, age, deliveries and end of invisibility.
    pub fn scan(&self, topic: &str) -> io::Result<Vec<ScanRow>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        disk.flush();
        disk.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, group_id, age_base, delivery_count, invisible_until, deduplication_id, \
                 published_at, delete_at FROM messages ORDER BY id",
            )?;
            let rows = stmt.query_map([], scan_row)?;
            rows.collect()
        })
    }

    /// A page of the topic's live messages (see [`PageQuery`]), without the body. Não espera a fila de escrita
    /// (D193): o que está nela por cima do banco, num instantâneo fixado com a fila parada. A página volta cheia
    /// (`limit` linhas) só quando pode haver mais depois da última.
    pub fn page(&self, topic: &str, query: &PageQuery) -> io::Result<Vec<ScanRow>> {
        use rusqlite::types::Value;
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        if !query.tombstones && !disk.may_hold_messages.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        // A faixa caminha a partir de `start` (exclusivo) até `far` (exclusivo).
        let (start, far) = if query.desc {
            (query.before.as_deref(), query.after.as_deref())
        } else {
            (query.after.as_deref(), query.before.as_deref())
        };
        let mut sql = String::from(
            "SELECT id, group_id, age_base, delivery_count, invisible_until, deduplication_id, \
             published_at, delete_at FROM messages WHERE 1 = 1",
        );
        if !query.tombstones {
            sql.push_str(" AND delete_at IS NULL");
        }
        sql.push_str(if query.desc {
            " AND id < ? AND id > ?"
        } else {
            " AND id > ? AND id < ?"
        });
        if query.delivered {
            sql.push_str(" AND delivery_count > 0");
        }
        let mut group_arg: Option<Value> = None;
        match query.group.as_deref() {
            Some("") => sql.push_str(" AND (group_id IS NULL OR group_id = '')"),
            Some(group) => {
                sql.push_str(" AND group_id = ?");
                group_arg = Some(Value::Text(group.to_owned()));
            }
            None => {}
        }
        // Sem par nenhum gravado, a subconsulta não tem o que achar.
        let table = query.off_table && disk.may_hold_expiry.load(Ordering::SeqCst);
        if table {
            sql.push_str(" AND NOT EXISTS (SELECT 1 FROM expiry WHERE expiry.id = messages.id)");
        }
        sql.push_str(if query.desc {
            " ORDER BY id DESC LIMIT ?"
        } else {
            " ORDER BY id LIMIT ?"
        });
        let (low, high) = (
            query
                .after
                .as_deref()
                .map_or(Bound::Unbounded, Bound::Excluded),
            query
                .before
                .as_deref()
                .map_or(Bound::Unbounded, Bound::Excluded),
        );
        // Os limites abertos viram os extremos do texto: nenhum `id` é vazio, e todos ficam abaixo de U+10FFFF.
        let bound = |b: Option<&str>, open: &str| Value::Text(b.unwrap_or(open).to_owned());
        let edges = if query.desc {
            ("\u{10FFFF}", "")
        } else {
            ("", "\u{10FFFF}")
        };
        disk.read_pending(low, high, |conn, view| {
            let mut stmt = conn.prepare_cached(&sql)?;
            let mut fetch = |from: Option<&str>| -> rusqlite::Result<Vec<ScanRow>> {
                let mut args = vec![bound(from, edges.0), bound(far, edges.1)];
                args.extend(group_arg.clone());
                args.push(Value::Integer(
                    i64::try_from(query.limit).unwrap_or(i64::MAX),
                ));
                stmt.query_map(rusqlite::params_from_iter(args), scan_row)?
                    .collect()
            };
            if view.is_empty() {
                return fetch(start);
            }
            let fits = |row: &ScanRow| page_fits(query, row);
            let in_table = |id: &str| matches!(view.expiry.get(id), Some(Some(_)));
            let mut out: Vec<ScanRow> = Vec::new();
            let mut cursor = start.map(str::to_owned);
            // Onde a última leitura cheia do banco parou: depois dela o banco pode ter mais.
            let mut horizon: Option<String>;
            loop {
                let rows = fetch(cursor.as_deref())?;
                let full = rows.len() >= query.limit;
                horizon = if full {
                    rows.last().map(|r| r.id.clone())
                } else {
                    None
                };
                for row in rows {
                    let row = match view.rows.get(&row.id) {
                        Some(Pending::Gone) => continue,
                        Some(Pending::Row { meta, .. } | Pending::Meta(meta)) => scan_of(meta),
                        None => row,
                    };
                    if fits(&row) && !(query.off_table && in_table(&row.id)) {
                        out.push(row);
                    }
                }
                if !full || out.len() >= query.limit {
                    break;
                }
                cursor.clone_from(&horizon);
            }
            let before_horizon = |id: &str| {
                horizon
                    .as_deref()
                    .is_none_or(|h| if query.desc { id >= h } else { id <= h })
            };
            let seen: std::collections::HashSet<String> =
                out.iter().map(|r| r.id.clone()).collect();
            // O que a fila escreveu e o banco ainda não tem (ou tem com valores que o filtro do SQL tirou).
            for (id, pending) in &view.rows {
                let (Pending::Row { meta, .. } | Pending::Meta(meta)) = pending else {
                    continue;
                };
                if seen.contains(id) || !before_horizon(id) {
                    continue;
                }
                let row = scan_of(meta);
                if !fits(&row) {
                    continue;
                }
                if query.off_table {
                    let paired = match view.expiry.get(id) {
                        Some(until) => until.is_some(),
                        None => table && expiry_has(conn, id)?,
                    };
                    if paired {
                        continue;
                    }
                }
                out.push(row);
            }
            // A linha que o SQL deixou de fora por um par que a fila apaga.
            if table {
                for (id, until) in &view.expiry {
                    if until.is_some()
                        || seen.contains(id)
                        || view.rows.contains_key(id)
                        || !before_horizon(id)
                    {
                        continue;
                    }
                    let row = conn
                        .prepare_cached(SCAN_BY_ID)?
                        .query_row(params![id], scan_row)
                        .optional()?;
                    if let Some(row) = row.filter(|row| fits(row)) {
                        out.push(row);
                    }
                }
            }
            if query.desc {
                out.sort_by(|a, b| b.id.cmp(&a.id));
            } else {
                out.sort_by(|a, b| a.id.cmp(&b.id));
            }
            out.truncate(query.limit);
            Ok(out)
        })
    }

    /// A page of the live messages by age, oldest first: `(age, id)` of those with `age ≤ up_to_ms`,
    /// after the `(age, id)` given, at most `limit`.
    pub fn by_age(
        &self,
        topic: &str,
        up_to_ms: u64,
        after: Option<&(u64, String)>,
        limit: usize,
    ) -> io::Result<Vec<(u64, String)>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(Vec::new());
        };
        let (age, id) = after.map_or((-1, String::new()), |(a, i)| (*a as i64, i.clone()));
        disk.flush();
        disk.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT age_base, id FROM messages WHERE delete_at IS NULL AND age_base <= ?1                  AND (age_base > ?2 OR (age_base = ?2 AND id > ?3))                  ORDER BY age_base, id LIMIT ?4",
            )?;
            let rows = stmt.query_map(
                params![
                    i64::try_from(up_to_ms).unwrap_or(i64::MAX),
                    age,
                    id,
                    limit as i64
                ],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?)),
            )?;
            rows.collect()
        })
    }

    /// The topic's messages whose position falls in any of the `ranges` (a block's arcs), with
    /// `id` between `from` and `to` (inclusive), in `id` order, at most `limit`. Includes the FIFO queue's
    /// short row (`delete_at`).
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
        disk.flush();
        disk.read(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| {
                Ok(BlockRow {
                    id: row.get(0)?,
                    delete_at_ms: row.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                })
            })?;
            rows.collect()
        })
    }

    /// The first `limit` deliverable ids (deadline expired), in `id` order.
    pub fn ready_ids(&self, topic: &str, now_ms: u64, limit: usize) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM messages WHERE invisible_until <= ?1 ORDER BY id LIMIT ?2",
            &[now_ms as i64, limit as i64],
        )
    }

    /// The instant the next invisible one becomes deliverable again.
    pub fn next_invisible_until(&self, topic: &str, now_ms: u64) -> io::Result<Option<u64>> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(None);
        };
        disk.flush();
        let next: Option<i64> = disk.read(|conn| {
            conn.query_row(
                "SELECT MIN(invisible_until) FROM messages WHERE invisible_until > ?1",
                params![now_ms as i64],
                |row| row.get(0),
            )
        })?;
        Ok(next.map(|v| v as u64))
    }

    /// The messages whose `ttl` expired: `age_base + ttl ≤ now`.
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

    /// The messages that used up their deliveries and whose invisibility ended.
    pub fn exhausted_ids(&self, topic: &str, now_ms: u64, max: u64) -> io::Result<Vec<String>> {
        self.ids(
            topic,
            "SELECT id FROM messages WHERE delivery_count > 0 AND delivery_count >= ?1 \
             AND invisible_until <= ?2",
            &[i64::try_from(max).unwrap_or(i64::MAX), now_ms as i64],
        )
    }

    /// Moves the message to another topic (the dead-letter), swapping the metadata. The body is read from the
    /// source and written to the target in the same transaction as its row.
    pub fn move_to(&self, from: &str, to: &str, meta: &MessageMeta) -> io::Result<()> {
        let source = self.disk(from, true)?.expect("created");
        let target = self.disk(to, true)?.expect("created");
        let body = self.body(from, &meta.id)?;
        // As duas filas esvaziam antes de pegar qualquer conexão de escrita: quem espera a fila não segura nenhuma.
        source.flush();
        target.flush();
        target.insert_row(&mut target.conn(), meta, &body)?;
        // It leaves the source dead: the exclusion is recorded in the transaction that deletes it.
        {
            let mut conn = source.conn();
            let tx = Scope::begin(&mut conn).map_err(to_io)?;
            tx.execute(
                excluded::RECORD_FROM_ROW,
                params![meta.id, meta.dead_lettered_ms.unwrap_or_default() as i64],
            )
            .map_err(to_io)?;
            tx.execute("DELETE FROM messages WHERE id = ?1", params![meta.id])
                .map_err(to_io)?;
            source.drop_body(&tx, &meta.id)?;
            tx.commit().map_err(to_io)?;
        }
        source.drop_legacy_file(&meta.id)
    }

    /// The FIFO queue's ack: the message becomes a short row with `delete_at` (doc 17, section 16.7).
    /// What stays is `id`, group, `deduplicationId` and the dates; the body and delivery metadata go. The
    /// row applies even if the message was only in memory. The row and the body go in one transaction.
    pub fn tombstone(&self, topic: &str, meta: &MessageMeta, delete_at_ms: u64) -> io::Result<()> {
        let disk = self.disk(topic, true)?.expect("created");
        {
            let mut conn = disk.exclusive();
            let tx = Scope::begin(&mut conn).map_err(to_io)?;
            write_tombstone(&tx, meta, delete_at_ms)?;
            disk.drop_body(&tx, &meta.id)?;
            tx.commit().map_err(to_io)?;
        }
        disk.drop_legacy_file(&meta.id)
    }

    /// Deletes the short rows whose `delete_at` has passed, by the index, and returns how many.
    pub fn sweep_tombstones(&self, topic: &str, now_ms: u64) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let removed = disk
            .exclusive()
            .execute(
                "DELETE FROM messages WHERE delete_at IS NOT NULL AND delete_at <= ?1",
                params![now_ms as i64],
            )
            .map_err(to_io)?;
        Ok(removed as u64)
    }

    /// The free space, in bytes, on the `storage.dir` volume (what an unprivileged process can
    /// use).
    pub fn free_bytes(&self) -> io::Result<u64> {
        fs4::available_space(&self.root)
    }

    /// The usage of the `storage.dir` volume, as a percentage (of what an unprivileged process sees).
    pub fn disk_usage_percent(&self) -> io::Result<u32> {
        let total = fs4::total_space(&self.root)?;
        if total == 0 {
            return Ok(0);
        }
        let free = fs4::available_space(&self.root)?.min(total);
        Ok(((total - free) * 100 / total) as u32)
    }

    /// Deletes only the body, keeping the row (the backup's eviction, doc 23, section 1.8). Returns the bytes
    /// freed; `0` if the body was no longer on disk.
    pub fn evict_body(&self, topic: &str, id: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        let mut freed = 0u64;
        {
            let mut conn = disk.exclusive();
            let tx = Scope::begin(&mut conn).map_err(to_io)?;
            let inline: Option<i64> = tx
                .query_row(
                    "SELECT length(body) FROM messages WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(to_io)?
                .flatten();
            if let Some(size) = inline {
                tx.execute("UPDATE messages SET body = NULL WHERE id = ?1", params![id])
                    .map_err(to_io)?;
                freed += size as u64;
            }
            if disk.may_hold_bodies.load(Ordering::SeqCst) {
                let big: Option<i64> = tx
                    .query_row(
                        "SELECT length(body) FROM bodies WHERE id = ?1",
                        params![id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(to_io)?;
                if let Some(size) = big {
                    disk.drop_body(&tx, id)?;
                    freed += size as u64;
                }
            }
            tx.commit().map_err(to_io)?;
        }
        if disk.legacy_files {
            let path = disk.body_path(id);
            match std::fs::metadata(&path) {
                Ok(meta) => {
                    std::fs::remove_file(&path)?;
                    freed += meta.len();
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(freed)
    }

    /// Does the message have its body on disk?
    pub fn has_body(&self, topic: &str, id: &str) -> bool {
        let Some(disk) = self.disk(topic, false).ok().flatten() else {
            return false;
        };
        match disk.pending(id) {
            Some(Pending::Row { .. }) => return true,
            Some(Pending::Gone) => return false,
            Some(Pending::Meta(_)) | None => {}
        }
        let found: Option<bool> = disk
            .read(|conn| {
                conn.query_row(
                    "SELECT body IS NOT NULL OR EXISTS (SELECT 1 FROM bodies WHERE bodies.id = messages.id) \
                     FROM messages WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()
            })
            .unwrap_or(None);
        found == Some(true) || (disk.legacy_files && disk.body_path(id).is_file())
    }

    /// The sum of the sizes of the topic's bodies (the lengths in the rows and in `bodies`, which SQLite reads
    /// without loading the bodies, plus the legacy files, which cost one call each).
    pub fn body_bytes(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        disk.flush();
        let mut total: i64 = disk.read(|conn| {
            let inline: i64 = conn.query_row(
                "SELECT COALESCE(SUM(length(body)), 0) FROM messages",
                [],
                |row| row.get(0),
            )?;
            let big: i64 = conn.query_row(
                "SELECT COALESCE(SUM(length(body)), 0) FROM bodies",
                [],
                |row| row.get(0),
            )?;
            Ok(inline + big)
        })?;
        if disk.legacy_files {
            for entry in std::fs::read_dir(&disk.dir)? {
                let entry = entry?;
                if entry.path().extension().is_some_and(|e| e == BODY_EXT) {
                    match entry.metadata() {
                        Ok(meta) => total += meta.len() as i64,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        Ok(total as u64)
    }

    pub fn count(&self, topic: &str) -> io::Result<u64> {
        let Some(disk) = self.disk(topic, false)? else {
            return Ok(0);
        };
        disk.flush();
        let n: i64 = disk.read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE delete_at IS NULL",
                [],
                |row| row.get(0),
            )
        })?;
        Ok(n as u64)
    }

    /// The topics that have a folder with a database in `storage.dir`.
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

    /// The folder of a disk with no manifest (a version before the quarantine per life): it waits for the restart
    /// recovery of the losses and is deleted at the end of it (PRB-048; doc 23, rolling update).
    pub const PREVIOUS_LIFE: &str = "_previous-life";

    /// The folder that holds one subfolder per previous life, named with the node ID of that life (doc 07,
    /// section 10.4). Nothing here is deleted by the recovery.
    pub const LIVES: &str = "_lives";

    /// The manifest of a life, in the root of the disk of the running life and, afterwards, in its folder.
    pub const LIFE_MANIFEST: &str = "manifest.json";

    /// The `storage.dir`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Sets aside what was left on the previous life's disk: each topic folder (and the manifest) goes to
    /// `${root}/_lives/<life>`, and the node goes on with an empty disk. Without `life` (a disk with no manifest),
    /// the folders go to `${root}/_previous-life`. A topic folder that was already there (a restart that crashed
    /// halfway through the move) stays as it is and the new one is deleted: the first is the one that may have
    /// what only it kept; the new one only brings copies the cluster already has. Returns the previous life's
    /// disk, or `None` if there are no messages on it.
    pub fn set_aside_old_disk(&self, life: Option<&str>) -> io::Result<Option<MessageStore>> {
        if life.is_some_and(|id| id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.')) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the ID of the life is not a folder name",
            ));
        }
        let aside = match life {
            Some(id) => self.root.join(Self::LIVES).join(id),
            None => self.root.join(Self::PREVIOUS_LIFE),
        };
        for topic in self.topics_on_disk()? {
            let from = self.root.join(dir_name(&topic));
            let to = aside.join(dir_name(&topic));
            self.topics().remove(&topic);
            if to.exists() {
                std::fs::remove_dir_all(&from)?;
                continue;
            }
            std::fs::create_dir_all(&aside)?;
            move_dir(&from, &to)?;
        }
        let manifest = self.root.join(Self::LIFE_MANIFEST);
        if life.is_some() && manifest.is_file() {
            std::fs::create_dir_all(&aside)?;
            let to = aside.join(Self::LIFE_MANIFEST);
            // The one from the running disk is the latest: it replaces the one of a move that crashed.
            let _ = std::fs::remove_file(&to);
            std::fs::rename(&manifest, &to)?;
        }
        let previous = MessageStore::new(&aside, self.synchronous);
        Ok((!previous.topics_on_disk()?.is_empty()).then_some(previous))
    }

    /// Deletes the folder of a disk with no manifest (its recovery is over). The folders of the lives stay.
    pub fn drop_old_disk(&self) -> io::Result<()> {
        match std::fs::remove_dir_all(self.root.join(Self::PREVIOUS_LIFE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Deletes the topic's folder, with the database and the bodies.
    pub fn drop_topic(&self, topic: &str) -> io::Result<()> {
        // Taken out first: the lock of the map is not held while waiting for the writer.
        let removed = self.topics().remove(topic);
        if let Some(Some(disk)) = removed {
            disk.dropped.store(true, Ordering::Release);
            // The writer of the exclusions may hold the disk for a moment: the database closes only when it lets
            // go, and an open file does not leave the folder on Windows.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while Arc::strong_count(&disk) > 1 && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        match std::fs::remove_dir_all(self.root.join(dir_name(topic))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Writes the message's row (replacing the one with the same `id`); `body` is the body kept in the row, if it fits.
fn write_row(conn: &Connection, meta: &MessageMeta, body: Option<&[u8]>) -> io::Result<()> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO messages (id, content_type, group_id, published_at, \
         delivery_count, invisible_until, receipt_token, dead_lettered_at, \
         dead_letter_reason, age_base, deduplication_id, position, body) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )
    .and_then(|mut stmt| {
        stmt.execute(params![
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
            body,
        ])
    })
    .map(|_| ())
    .map_err(to_io)
}

/// Writes the FIFO queue's short row (the SQL of `tombstone`, shared with the one that also records the
/// exclusion in the same transaction).
fn write_tombstone(conn: &Connection, meta: &MessageMeta, delete_at_ms: u64) -> io::Result<()> {
    conn.execute(
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
    .map(|_| ())
    .map_err(to_io)
}

/// Moves a folder. Where the system does not allow renaming (on Windows, with a file of the folder still
/// open), it copies and deletes the original, repeating the delete for up to three seconds.
fn move_dir(from: &Path, to: &Path) -> io::Result<()> {
    // A handle that is still closing (the previous owner of the folder, an antivirus) makes the rename fail for a
    // moment; copying meanwhile would miss what is still in the databases' write-ahead logs.
    for _ in 0..20 {
        if std::fs::rename(from, to).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    copy_dir(from, to)?;
    let mut result = std::fs::remove_dir_all(from);
    for _ in 0..30 {
        if result.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        result = std::fs::remove_dir_all(from);
    }
    result
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
