//! O `_backup.db` (doc 23, "Backup em S3"): o banco SQLite local do backup, ao lado do `_node.db`.
//!
//! - **`outbox`**: as mensagens que o primário já confirmou e ainda não foram para um segmento.
//!   Só o `id`: o corpo e os metadados vêm de onde a mensagem está. O commit da mensagem só insere
//!   uma linha aqui; nada espera o S3.
//! - **`saved`**: onde cada mensagem salva está no bucket (segmento, deslocamento e tamanho).
//!   Com isto o nó apaga o corpo do disco (evicção) e lê de volta por `Range`.
//! - **`segments`**: para cada segmento que **este nó** gravou, quantas mensagens ainda vivem. Com
//!   isto o primário sabe que um segmento morreu inteiro **sem consultar o S3**.
//! - **`journal`**: as mensagens com estado alterado (ou removidas) desde o último diário.
//!
//! Sem teto para o `outbox` (Q9): se o disco enche, o nó entra em `failed`, como com qualquer
//! outra falta de espaço. Os métodos são síncronos: quem chama os roda em `spawn_blocking`.

use std::io;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::db::to_io;

pub const BACKUP_DB_FILE: &str = "_backup.db";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS outbox (
        topic    TEXT NOT NULL,
        id       TEXT NOT NULL,
        added_at INTEGER NOT NULL,
        PRIMARY KEY (topic, id)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS outbox_added_at ON outbox (added_at);
    CREATE TABLE IF NOT EXISTS saved (
        topic   TEXT NOT NULL,
        id      TEXT NOT NULL,
        segment TEXT NOT NULL,
        off     INTEGER NOT NULL,
        len     INTEGER NOT NULL,
        saved_at INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (topic, id)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS saved_segment ON saved (segment);
    CREATE TABLE IF NOT EXISTS segments (
        key        TEXT PRIMARY KEY NOT NULL,
        topic      TEXT NOT NULL,
        topic_id   TEXT NOT NULL,
        total      INTEGER NOT NULL,
        live       INTEGER NOT NULL,
        bytes      INTEGER NOT NULL,
        created_at INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS journal (
        topic   TEXT NOT NULL,
        id      TEXT NOT NULL,
        removed INTEGER NOT NULL,
        PRIMARY KEY (topic, id)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS counters (
        name  TEXT PRIMARY KEY NOT NULL,
        value INTEGER NOT NULL
    ) WITHOUT ROWID;
";

/// Onde uma mensagem salva está no bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRef {
    pub segment: String,
    pub offset: u64,
    pub len: u64,
    /// Quando o PUT do segmento foi confirmado, no relógio do nó que o gravou (informação, não para
    /// comparar com o relógio de outro nó). `0`: não se sabe (aviso antigo).
    pub saved_at_ms: u64,
}

/// O resultado de [`BackupDb::commit_segment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// Quantas mensagens do segmento ainda vivem.
    pub live: u64,
    /// As que morreram durante o envio.
    pub dead: Vec<String>,
}

/// Um segmento que este nó gravou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRow {
    pub key: String,
    pub topic: String,
    pub topic_id: String,
    pub total: u64,
    pub live: u64,
    pub bytes: u64,
    pub created_ms: u64,
}

/// Uma mudança de estado ainda fora do diário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirty {
    pub topic: String,
    pub id: String,
    pub removed: bool,
}

#[derive(Debug)]
pub struct BackupDb {
    conn: Mutex<Connection>,
}

impl BackupDb {
    pub fn open(dir: &Path) -> io::Result<Self> {
        let conn = Connection::open(dir.join(BACKUP_DB_FILE)).map_err(to_io)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(to_io)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(to_io)?;
        conn.execute_batch(SCHEMA).map_err(to_io)?;
        // Um `_backup.db` de antes de `saved_at` ganha a coluna.
        let has: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('saved') WHERE name = 'saved_at'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(to_io)?
            > 0;
        if !has {
            conn.execute_batch("ALTER TABLE saved ADD COLUMN saved_at INTEGER NOT NULL DEFAULT 0")
                .map_err(to_io)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- outbox ----

    /// Põe a mensagem na fila de envio. Repetir não faz nada.
    pub fn enqueue(&self, topic: &str, id: &str, now_ms: u64) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT OR IGNORE INTO outbox (topic, id, added_at) VALUES (?1, ?2, ?3)",
                params![topic, id, now_ms as i64],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// As mensagens da fila adicionadas até `before_ms` (inclusive), das mais antigas, no máximo
    /// `limit`. `before_ms = u64::MAX`: todas.
    pub fn outbox(&self, before_ms: u64, limit: usize) -> io::Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare_cached(
                "SELECT topic, id FROM outbox WHERE added_at <= ?1 ORDER BY topic, id LIMIT ?2",
            )
            .map_err(to_io)?;
        let rows = stmt
            .query_map(
                params![i64::try_from(before_ms).unwrap_or(i64::MAX), limit as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    pub fn outbox_len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// Tira da fila (a mensagem foi consumida antes de a janela fechar, ou deixou de ser deste
    /// nó). `true` se estava lá.
    pub fn unqueue(&self, topic: &str, id: &str) -> io::Result<bool> {
        let n = self
            .conn()
            .execute(
                "DELETE FROM outbox WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(to_io)?;
        Ok(n > 0)
    }

    // ---- segmentos e referências ----

    /// Registra um segmento que o PUT confirmou: o segmento, a referência de cada mensagem e a
    /// saída delas do `outbox`, numa transação só.
    ///
    /// A mensagem que já não estava no `outbox` morreu enquanto o segmento subia (ack ou `ttl`):
    /// ela não conta como viva e entra no diário como remoção, para a restauração não a
    /// ressuscitar. `segment.live` é recalculado aqui.
    pub fn commit_segment(
        &self,
        segment: &SegmentRow,
        entries: &[(String, u64, u64)],
    ) -> io::Result<Committed> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        let mut live = 0u64;
        let mut dead = Vec::new();
        for (id, offset, len) in entries {
            let queued = tx
                .execute(
                    "DELETE FROM outbox WHERE topic = ?1 AND id = ?2",
                    params![segment.topic, id],
                )
                .map_err(to_io)?;
            if queued == 0 {
                tx.execute(
                    "INSERT OR REPLACE INTO journal (topic, id, removed) VALUES (?1, ?2, 1)",
                    params![segment.topic, id],
                )
                .map_err(to_io)?;
                dead.push(id.clone());
                continue;
            }
            tx.execute(
                "INSERT OR REPLACE INTO saved (topic, id, segment, off, len, saved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    segment.topic,
                    id,
                    segment.key,
                    *offset as i64,
                    *len as i64,
                    segment.created_ms as i64
                ],
            )
            .map_err(to_io)?;
            live += 1;
        }
        tx.execute(
            "INSERT OR REPLACE INTO segments (key, topic, topic_id, total, live, bytes, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                segment.key,
                segment.topic,
                segment.topic_id,
                entries.len() as i64,
                live as i64,
                segment.bytes as i64,
                segment.created_ms as i64
            ],
        )
        .map_err(to_io)?;
        tx.commit().map_err(to_io)?;
        Ok(Committed { live, dead })
    }

    /// Registra um segmento que este nó passou a administrar (a restauração o leu do bucket).
    pub fn put_segment(&self, segment: &SegmentRow) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO segments (key, topic, topic_id, total, live, bytes, created_at)                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    segment.key,
                    segment.topic,
                    segment.topic_id,
                    segment.total as i64,
                    segment.live as i64,
                    segment.bytes as i64,
                    segment.created_ms as i64
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// Quantas mensagens têm referência no bucket.
    pub fn saved_len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM saved", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// A mensagem ainda espera no `outbox`?
    pub fn queued(&self, topic: &str, id: &str) -> io::Result<bool> {
        self.conn()
            .query_row(
                "SELECT 1 FROM outbox WHERE topic = ?1 AND id = ?2",
                params![topic, id],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(to_io)
    }

    /// Guarda a referência avisada pelo primário (a réplica e o herdeiro a usam para ler do bucket
    /// o que apagaram do disco).
    pub fn set_saved(&self, topic: &str, id: &str, saved: &SavedRef) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO saved (topic, id, segment, off, len, saved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    topic,
                    id,
                    saved.segment,
                    saved.offset as i64,
                    saved.len as i64,
                    saved.saved_at_ms as i64
                ],
            )
            .map_err(to_io)?;
        Ok(())
    }

    pub fn saved(&self, topic: &str, id: &str) -> io::Result<Option<SavedRef>> {
        self.conn()
            .query_row(
                "SELECT segment, off, len, saved_at FROM saved WHERE topic = ?1 AND id = ?2",
                params![topic, id],
                |row| {
                    Ok(SavedRef {
                        segment: row.get(0)?,
                        offset: row.get::<_, i64>(1)? as u64,
                        len: row.get::<_, i64>(2)? as u64,
                        saved_at_ms: row.get::<_, i64>(3)? as u64,
                    })
                },
            )
            .optional()
            .map_err(to_io)
    }

    /// As mensagens do tópico que já têm referência no bucket, com ela.
    pub fn saved_of_topic(&self, topic: &str) -> io::Result<Vec<(String, SavedRef)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id, segment, off, len, saved_at FROM saved WHERE topic = ?1")
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![topic], |row| {
                Ok((
                    row.get(0)?,
                    SavedRef {
                        segment: row.get(1)?,
                        offset: row.get::<_, i64>(2)? as u64,
                        len: row.get::<_, i64>(3)? as u64,
                        saved_at_ms: row.get::<_, i64>(4)? as u64,
                    },
                ))
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// A mensagem morreu (ack, `ttl`, dead-letter): sai do `outbox` e da lista de salvas, e o
    /// segmento dela perde uma viva. Devolve o segmento se todas as mensagens dele morreram (o
    /// primário o apaga do bucket) e se a mensagem estava salva. A remoção entra no diário.
    pub fn mark_dead(&self, topic: &str, id: &str) -> io::Result<Option<String>> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        let was_queued = tx
            .execute(
                "DELETE FROM outbox WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(to_io)?
            > 0;
        let segment: Option<String> = tx
            .query_row(
                "SELECT segment FROM saved WHERE topic = ?1 AND id = ?2",
                params![topic, id],
                |row| row.get(0),
            )
            .optional()
            .map_err(to_io)?;
        let mut empty = None;
        if let Some(segment) = segment {
            tx.execute(
                "DELETE FROM saved WHERE topic = ?1 AND id = ?2",
                params![topic, id],
            )
            .map_err(to_io)?;
            tx.execute(
                "UPDATE segments SET live = MAX(live - 1, 0) WHERE key = ?1",
                params![segment],
            )
            .map_err(to_io)?;
            let live: Option<i64> = tx
                .query_row(
                    "SELECT live FROM segments WHERE key = ?1",
                    params![segment],
                    |row| row.get(0),
                )
                .optional()
                .map_err(to_io)?;
            if live == Some(0) {
                empty = Some(segment);
            }
            tx.execute(
                "INSERT OR REPLACE INTO journal (topic, id, removed) VALUES (?1, ?2, 1)",
                params![topic, id],
            )
            .map_err(to_io)?;
        } else if was_queued {
            // Ainda esperava o envio: nunca saiu do nó, não há o que registrar.
            tx.execute(
                "DELETE FROM journal WHERE topic = ?1 AND id = ?2 AND removed = 0",
                params![topic, id],
            )
            .map_err(to_io)?;
        } else {
            // Nem salva nem na fila: a mensagem foi herdada de outro primário (que pode tê-la
            // salvado) ou o segmento dela está em voo. A remoção entra no diário por garantia:
            // sem ela a restauração ressuscitaria uma mensagem confirmada.
            tx.execute(
                "INSERT OR REPLACE INTO journal (topic, id, removed) VALUES (?1, ?2, 1)",
                params![topic, id],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)?;
        Ok(empty)
    }

    /// A mensagem morreu num nó que não é o dono (réplica): só esquece a referência e a fila.
    /// O diário e a contagem do segmento são do dono.
    pub fn forget(&self, topic: &str, id: &str) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        for table in ["outbox", "saved"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE topic = ?1 AND id = ?2"),
                params![topic, id],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    /// O segmento foi apagado do bucket: esquece a linha dele.
    pub fn drop_segment(&self, key: &str) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        tx.execute("DELETE FROM segments WHERE key = ?1", params![key])
            .map_err(to_io)?;
        tx.execute("DELETE FROM saved WHERE segment = ?1", params![key])
            .map_err(to_io)?;
        tx.commit().map_err(to_io)
    }

    /// Os segmentos que este nó gravou, do tópico.
    pub fn segments_of(&self, topic: &str) -> io::Result<Vec<SegmentRow>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT key, topic, topic_id, total, live, bytes, created_at FROM segments \
                 WHERE topic = ?1 ORDER BY key",
            )
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![topic], row_to_segment)
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    pub fn segment(&self, key: &str) -> io::Result<Option<SegmentRow>> {
        self.conn()
            .query_row(
                "SELECT key, topic, topic_id, total, live, bytes, created_at FROM segments \
                 WHERE key = ?1",
                params![key],
                row_to_segment,
            )
            .optional()
            .map_err(to_io)
    }

    /// Quantos segmentos e quantos bytes este nó mantém no bucket.
    pub fn segment_totals(&self) -> io::Result<(u64, u64)> {
        self.conn()
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(bytes), 0) FROM segments",
                [],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
            )
            .map_err(to_io)
    }

    // ---- diário ----

    /// A mensagem mudou de estado (entrega, invisibilidade): entra no próximo diário. Só vale
    /// para mensagem que já está salva num segmento; as pendentes vão com o estado atual.
    pub fn touch(&self, topic: &str, id: &str) -> io::Result<()> {
        self.conn()
            .execute(
                "INSERT INTO journal (topic, id, removed) \
                 SELECT ?1, ?2, 0 WHERE EXISTS \
                 (SELECT 1 FROM saved WHERE topic = ?1 AND id = ?2) \
                 ON CONFLICT(topic, id) DO NOTHING",
                params![topic, id],
            )
            .map_err(to_io)?;
        Ok(())
    }

    /// As mudanças ainda fora do diário.
    pub fn dirty(&self, limit: usize) -> io::Result<Vec<Dirty>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT topic, id, removed FROM journal ORDER BY topic, id LIMIT ?1")
            .map_err(to_io)?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Dirty {
                    topic: row.get(0)?,
                    id: row.get(1)?,
                    removed: row.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(to_io)?;
        rows.collect::<Result<_, _>>().map_err(to_io)
    }

    /// O diário foi gravado: tira as mudanças dele da lista (as que mudaram de novo no meio ficam,
    /// porque o `touch` não as duplica: o estado gravado já era o último lido).
    pub fn clear_dirty(&self, items: &[Dirty]) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        for item in items {
            tx.execute(
                "DELETE FROM journal WHERE topic = ?1 AND id = ?2",
                params![item.topic, item.id],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    pub fn dirty_len(&self) -> io::Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM journal", [], |row| row.get(0))
            .map_err(to_io)?;
        Ok(n as u64)
    }

    // ---- geral ----

    /// O próximo número de sequência (para nomear segmentos e diários sem repetir).
    pub fn next_seq(&self) -> io::Result<u64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO counters (name, value) VALUES ('seq', 1) \
             ON CONFLICT(name) DO UPDATE SET value = value + 1",
            [],
        )
        .map_err(to_io)?;
        let n: i64 = conn
            .query_row("SELECT value FROM counters WHERE name = 'seq'", [], |row| {
                row.get(0)
            })
            .map_err(to_io)?;
        Ok(n as u64)
    }

    /// O tópico foi apagado: esquece tudo dele.
    pub fn drop_topic(&self, topic: &str) -> io::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(to_io)?;
        for table in ["outbox", "saved", "segments", "journal"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE topic = ?1"),
                params![topic],
            )
            .map_err(to_io)?;
        }
        tx.commit().map_err(to_io)
    }

    /// Apaga tudo (o nó volta como nó novo).
    pub fn reset(&self) -> io::Result<()> {
        self.conn()
            .execute_batch(
                "DELETE FROM outbox; DELETE FROM saved; DELETE FROM segments; DELETE FROM journal;",
            )
            .map_err(to_io)
    }
}

fn row_to_segment(row: &rusqlite::Row<'_>) -> rusqlite::Result<SegmentRow> {
    Ok(SegmentRow {
        key: row.get(0)?,
        topic: row.get(1)?,
        topic_id: row.get(2)?,
        total: row.get::<_, i64>(3)? as u64,
        live: row.get::<_, i64>(4)? as u64,
        bytes: row.get::<_, i64>(5)? as u64,
        created_ms: row.get::<_, i64>(6)? as u64,
    })
}

#[cfg(test)]
mod tests;
