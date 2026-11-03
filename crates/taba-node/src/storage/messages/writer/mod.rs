//! A fila de escrita de cada banco de tópico (D192): as escritas quentes (publicar, entregar, confirmar e os pares
//! da tabela `expiry`) entram numa fila e a primeira chamada que espera vira a **líder**: leva tudo o que está na
//! fila, grava numa transação só e acorda as outras (commit em grupo). Quem chega enquanto ela grava espera a
//! próxima rodada, que leva o que se juntou nesse meio-tempo. Não há thread fixa: quem espera é quem grava.
//!
//! **Ler o que ainda não foi gravado:** cada `id` com escrita na fila tem a sua última forma num mapa (a linha
//! inteira, só os metadados, ou "removida"), e cada `id` com par da `expiry` na fila, o seu último par (ou
//! "apagado"). As leituras por `id` olham neles antes do banco. As páginas e a `expiry` mais próxima não esperam a
//! fila: leem o banco num instantâneo fixado com a fila parada ([`TopicDisk::read_pending`]) e aplicam por cima o
//! que está pendente. As outras leituras por faixa (contagens, idade, blocos) esperam a fila esvaziar.
//!
//! **Escrita desligada de quem pediu:** [`Written::detach`] entrega as escritas de um escopo a uma thread de fundo
//! (`taba-flush`) que espera o commit e registra a falha; quem pediu responde sem esperar. Só a entrega usa
//! (D193): perder o contador de uma entrega numa queda do processo devolve a mensagem com o contador de antes.
//!
//! **Escopo adiado:** dentro de [`MessageStore::deferred`](super::MessageStore::deferred) as escritas só entram
//! na fila e a espera fica para depois: o motor solta o lock do tópico antes de esperar o commit, e outras
//! chamadas do mesmo tópico entram no mesmo commit.
//!
//! **Leituras e checkpoint fora da conexão de escrita:** as leituras usam conexões só de leitura (o WAL deixa ler
//! em paralelo com a escrita), e o checkpoint roda numa thread de fundo com a sua própria conexão, chamada pela
//! líder a cada [`CHECKPOINT_BYTES`] gravados.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::ops::Bound;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::excluded::RECORD_FROM_ROW;
use super::{MessageMeta, Scope, TopicDisk, to_io};

/// Quanto se grava (estimado pelos corpos e por um custo fixo por operação) entre dois checkpoints de fundo.
const CHECKPOINT_BYTES: u64 = 32 * 1024 * 1024;
/// O custo fixo estimado de cada operação no WAL (linha, índices).
const OP_BYTES: u64 = 256;
/// Conexões só de leitura guardadas por tópico; as que sobram fecham ao voltar.
const READERS_KEPT: usize = 8;
/// O cache de páginas de cada conexão de leitura (forma negativa do `PRAGMA cache_size`, KiB): 8 MB.
const READER_CACHE_KIB: i64 = -8 * 1024;
/// Até quantas sequências atrás da última rodada a fila lembra as falhas, para quem ainda vai esperar por elas.
const FAILURES_KEPT: u64 = 1 << 20;

/// Uma escrita na fila.
#[derive(Debug)]
pub(super) enum Op {
    /// A mensagem inteira: a linha e o corpo.
    Put { meta: MessageMeta, body: Arc<[u8]> },
    /// Os campos que a entrega muda: contador, prazo e `receiptToken`.
    Delivery(MessageMeta),
    /// A linha e o corpo saem; com `excluded_at`, a exclusão vai para o log na mesma transação.
    Remove {
        id: String,
        excluded_at: Option<u64>,
    },
    /// Os pares da tabela `expiry`, em ordem: `true` grava, `false` apaga.
    Expiry(Vec<(bool, u64, String)>),
    /// Um pedaço da limpeza do log de excluídas: até `TRIM_CHUNK` linhas com instante antes de `before_ms`. Vai pela
    /// fila para entrar na vez dele entre as publicações: tomando a conexão direto, a limpeza a pegava de novo antes
    /// da escritora (o `Mutex` não é justo) e as publicações esperavam a limpeza inteira (PRB-090).
    TrimExcluded { before_ms: i64 },
}

/// O que uma leitura por `id` vê de uma escrita que ainda está na fila.
#[derive(Debug, Clone)]
pub(super) enum Pending {
    Row {
        meta: MessageMeta,
        body: Arc<[u8]>,
    },
    /// Só os metadados mudaram: o corpo é o que está no banco.
    Meta(MessageMeta),
    Gone,
}

#[derive(Debug, Default)]
pub(super) struct Queue {
    ops: Vec<(u64, Op)>,
    /// A última sequência dada.
    next: u64,
    /// A última sequência que já passou por uma rodada (gravada ou falhada).
    done: u64,
    /// Uma líder está gravando.
    writing: bool,
    /// As falhas das últimas rodadas: `(sequência, erro)`.
    failures: VecDeque<(u64, io::Error)>,
    /// A última forma de cada `id` com escrita na fila: `(sequência, forma)`, em ordem de `id` (as páginas leem por
    /// faixa).
    overlay: BTreeMap<String, (u64, Pending)>,
    /// O último par da `expiry` de cada `id` com par na fila: `(sequência, prazo)`, `None` quando foi apagado.
    expiry: BTreeMap<String, (u64, Option<u64>)>,
    /// As escritas desligadas ([`Written::detach`]) que a thread de fundo ainda vai esperar.
    detached: Vec<u64>,
    /// O que se gravou desde o último checkpoint.
    since_checkpoint: u64,
}

/// O que está na fila numa faixa de `id`, copiado no instante em que a leitura fixou o instantâneo do banco.
#[derive(Debug, Default)]
pub(super) struct PendingView {
    pub(super) rows: BTreeMap<String, Pending>,
    /// O par da `expiry` de cada `id` com par na fila: `Some(prazo)` gravado, `None` apagado.
    pub(super) expiry: BTreeMap<String, Option<u64>>,
    /// Com a cópia limitada ([`TopicDisk::read_pending`]), o último `id` copiado quando a fila tinha mais adiante: a
    /// cópia só vale até ele (inclusive), e quem lê não passa dele.
    pub(super) edge: Option<String>,
}

impl PendingView {
    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.expiry.is_empty()
    }
}

impl Queue {
    fn failure_of(&self, seqs: &[u64]) -> Option<io::Error> {
        self.failures
            .iter()
            .find(|(seq, _)| seqs.contains(seq))
            .map(|(_, e)| copy_error(e))
    }
}

/// Uma cópia do erro que mantém o código do SQLite (`storage::is_fatal` olha nele).
fn copy_error(e: &io::Error) -> io::Error {
    match e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rusqlite::Error>())
    {
        Some(rusqlite::Error::SqliteFailure(code, message)) => {
            to_io(rusqlite::Error::SqliteFailure(*code, message.clone()))
        }
        _ => io::Error::new(e.kind(), e.to_string()),
    }
}

/// Os primeiros `n` `id`s de `map` na faixa, no sentido da leitura.
fn first_in<'a, V>(
    map: &'a BTreeMap<String, V>,
    range: (Bound<&str>, Bound<&str>),
    desc: bool,
    n: usize,
) -> Vec<(&'a String, &'a V)> {
    let entries = map.range::<str, _>(range);
    if desc {
        entries.rev().take(n).collect()
    } else {
        entries.take(n).collect()
    }
}

/// As escritas na fila de um banco acima das quais [`Written::detach_unless_full`] espera o commit em vez de soltá-lo.
const DETACH_MAX_QUEUED: usize = 8192;

/// As sequências de escrita de um escopo adiado, por banco.
type Tickets = Vec<(Arc<TopicDisk>, Vec<u64>)>;

thread_local! {
    /// As escritas feitas dentro de um escopo adiado desta thread, por banco: `None` fora de um escopo.
    static DEFERRED: RefCell<Option<Tickets>> = const { RefCell::new(None) };
}

/// As escritas de um escopo adiado que ainda podem estar na fila: [`wait`](Self::wait) espera o commit delas.
/// Largado sem esperar, espera do mesmo jeito (e só registra um erro).
#[derive(Debug, Default)]
#[must_use = "as escritas só estão garantidas depois de wait()"]
pub struct Written(Tickets);

impl Written {
    pub fn wait(mut self) -> io::Result<()> {
        let mut first_error = None;
        for (disk, seqs) in std::mem::take(&mut self.0) {
            if let Err(e) = disk.wait(&seqs) {
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Não espera: uma thread de fundo espera o commit e registra a falha (ver o topo do módulo).
    pub fn detach(mut self) {
        for (disk, seqs) in std::mem::take(&mut self.0) {
            disk.flush_later(seqs);
        }
    }

    /// Como [`detach`](Self::detach), mas espera o commit no banco cuja fila passou de [`DETACH_MAX_QUEUED`] escritas:
    /// quem escreve mais rápido do que o disco grava espera a vez, e a fila (com os corpos) não cresce sem limite.
    pub fn detach_unless_full(mut self) -> io::Result<()> {
        let mut first_error = None;
        for (disk, seqs) in std::mem::take(&mut self.0) {
            if disk.queue().ops.len() > DETACH_MAX_QUEUED {
                if let Err(e) = disk.wait(&seqs) {
                    first_error.get_or_insert(e);
                }
            } else {
                disk.flush_later(seqs);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for Written {
    fn drop(&mut self) {
        for (disk, seqs) in std::mem::take(&mut self.0) {
            if let Err(e) = disk.wait(&seqs) {
                tracing::error!(error = %e, "failed to write a deferred batch of the topic");
            }
        }
    }
}

/// Roda `work` num escopo adiado (ver o topo do módulo). Dentro de outro escopo, `work` só roda: as escritas ficam
/// com o de fora.
pub(super) fn deferred<R>(work: impl FnOnce() -> R) -> (R, Written) {
    if DEFERRED.with(|d| d.borrow().is_some()) {
        return (work(), Written::default());
    }
    /// Fecha o escopo mesmo se `work` entrar em pânico (e espera o que ficou na fila).
    struct Close;
    impl Drop for Close {
        fn drop(&mut self) {
            if let Some(left) = DEFERRED.with(|d| d.borrow_mut().take()) {
                drop(Written(left));
            }
        }
    }
    DEFERRED.with(|d| *d.borrow_mut() = Some(Vec::new()));
    let close = Close;
    let result = work();
    let written = Written(DEFERRED.with(|d| d.borrow_mut().take()).unwrap_or_default());
    drop(close);
    (result, written)
}

impl TopicDisk {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Põe a escrita na fila e, fora de um escopo adiado, espera o commit dela.
    pub(super) fn submit(self: &Arc<Self>, op: Op) -> io::Result<()> {
        let seq = self.enqueue(op);
        let deferred = DEFERRED.with(|d| {
            let mut d = d.borrow_mut();
            let Some(list) = d.as_mut() else {
                return false;
            };
            match list.iter_mut().find(|(disk, _)| Arc::ptr_eq(disk, self)) {
                Some((_, seqs)) => seqs.push(seq),
                None => list.push((self.clone(), vec![seq])),
            }
            true
        });
        if deferred { Ok(()) } else { self.wait(&[seq]) }
    }

    fn enqueue(&self, op: Op) -> u64 {
        let mut queue = self.queue();
        queue.next += 1;
        let seq = queue.next;
        let pending = match &op {
            Op::Put { meta, body } => Some((
                meta.id.clone(),
                Pending::Row {
                    meta: meta.clone(),
                    body: body.clone(),
                },
            )),
            Op::Delivery(meta) => {
                let pending = match queue.overlay.get(&meta.id) {
                    Some((_, Pending::Row { body, .. })) => Pending::Row {
                        meta: meta.clone(),
                        body: body.clone(),
                    },
                    // A mudança de uma linha que já saiu não muda nada.
                    Some((_, Pending::Gone)) => Pending::Gone,
                    _ => Pending::Meta(meta.clone()),
                };
                Some((meta.id.clone(), pending))
            }
            Op::Remove { id, .. } => Some((id.clone(), Pending::Gone)),
            Op::Expiry(pairs) => {
                for (put, until, id) in pairs {
                    if *put {
                        queue.expiry.insert(id.clone(), (seq, Some(*until)));
                    } else if queue
                        .expiry
                        .get(id)
                        .is_none_or(|(_, last)| *last == Some(*until))
                    {
                        // Apagar um par antigo depois de gravar o novo não apaga o novo.
                        queue.expiry.insert(id.clone(), (seq, None));
                    }
                }
                None
            }
            Op::TrimExcluded { .. } => None,
        };
        if let Some((id, pending)) = pending {
            queue.overlay.insert(id, (seq, pending));
        }
        queue.ops.push((seq, op));
        seq
    }

    /// A forma de `id` que ainda está na fila, se houver.
    pub(super) fn pending(&self, id: &str) -> Option<Pending> {
        let queue = self.queue();
        if queue.overlay.is_empty() {
            return None;
        }
        queue.overlay.get(id).map(|(_, pending)| pending.clone())
    }

    /// O par da `expiry` de `id` que ainda está na fila: `Some(Some(prazo))` gravado, `Some(None)` apagado.
    pub(super) fn pending_expiry(&self, id: &str) -> Option<Option<u64>> {
        let queue = self.queue();
        queue.expiry.get(id).map(|(_, until)| *until)
    }

    /// Lê com uma conexão só de leitura sem esperar a fila: com a fila parada, fixa o instantâneo do banco e copia o
    /// que está pendente na faixa `(low, high)` de `id`. O banco nunca está à frente da cópia para um `id` que está
    /// nela (nada entra na fila entre as duas), e `work` aplica a cópia por cima do que lê.
    ///
    /// Com `cap`, copia só os primeiros `cap` `id`s de cada mapa no sentido da leitura (do `low` para cima, ou do
    /// `high` para baixo com `desc`), e [`PendingView::edge`] diz até onde a cópia vale: uma página lê poucas linhas,
    /// e copiar a fila inteira a cada página era o maior custo do consumo com o backlog em disco.
    pub(super) fn read_pending<R>(
        &self,
        (low, high): (Bound<&str>, Bound<&str>),
        desc: bool,
        cap: Option<usize>,
        work: impl FnOnce(&Connection, &PendingView) -> rusqlite::Result<R>,
    ) -> io::Result<R> {
        self.read(|conn| {
            let queue = self.queue();
            if queue.overlay.is_empty() && queue.expiry.is_empty() {
                drop(queue);
                return work(conn, &PendingView::default());
            }
            conn.execute_batch("BEGIN")?;
            /// Fecha a transação de leitura em qualquer saída.
            struct End<'c>(&'c Connection);
            impl Drop for End<'_> {
                fn drop(&mut self) {
                    let _ = self.0.execute_batch("COMMIT");
                }
            }
            let _end = End(conn);
            // A primeira leitura da transação fixa o instantâneo do WAL.
            conn.query_row("SELECT 1 FROM sqlite_master LIMIT 1", [], |_| Ok(()))
                .optional()?;
            let range = (low, high);
            let take = cap.map_or(usize::MAX, |cap| cap.max(1));
            let rows = first_in(&queue.overlay, range, desc, take.saturating_add(1));
            let expiry = first_in(&queue.expiry, range, desc, take.saturating_add(1));
            // A borda é o último `id` copiado do mapa que se esgota antes, no sentido da leitura.
            let edge = [
                (rows.len() > take).then(|| rows[take - 1].0),
                (expiry.len() > take).then(|| expiry[take - 1].0),
            ]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .reduce(|a, b| if desc == (a > b) { a } else { b });
            let within =
                |id: &str| edge.is_none_or(|edge| if desc { id >= edge } else { id <= edge });
            let view = PendingView {
                rows: rows
                    .into_iter()
                    .filter(|(id, _)| within(id))
                    .map(|(id, (_, pending))| (id.clone(), pending.clone()))
                    .collect(),
                expiry: expiry
                    .into_iter()
                    .filter(|(id, _)| within(id))
                    .map(|(id, (_, until))| (id.clone(), *until))
                    .collect(),
                edge: edge.map(str::to_owned),
            };
            drop(queue);
            work(conn, &view)
        })
    }

    /// Entrega as escritas `seqs` à thread de fundo que espera o commit delas ([`Written::detach`]).
    fn flush_later(self: &Arc<Self>, seqs: Vec<u64>) {
        self.queue().detached.extend(seqs);
        if self.flushing.swap(true, Ordering::AcqRel) {
            return;
        }
        let disk = self.clone();
        let spawned = std::thread::Builder::new()
            .name("taba-flush".into())
            .spawn(move || disk.flush_detached());
        if spawned.is_err() {
            // Sem thread, quem pediu espera.
            self.flushing.store(false, Ordering::Release);
            let seqs = std::mem::take(&mut self.queue().detached);
            if let Err(e) = self.wait(&seqs) {
                tracing::error!(error = %e, "failed to write a delivery to the topic database");
            }
        }
    }

    fn flush_detached(self: &Arc<Self>) {
        loop {
            let seqs = std::mem::take(&mut self.queue().detached);
            if seqs.is_empty() {
                self.flushing.store(false, Ordering::Release);
                // Quem desligou escritas entre a cópia e a marca não achou a thread: ela continua.
                if self.queue().detached.is_empty() || self.flushing.swap(true, Ordering::AcqRel) {
                    return;
                }
                continue;
            }
            if let Err(e) = self.wait(&seqs) {
                tracing::error!(error = %e, "failed to write a delivery to the topic database");
            }
        }
    }

    /// Espera tudo o que está na fila até agora chegar ao banco (gravado ou falhado). Para as leituras por faixa e
    /// para as escritas que não passam pela fila.
    pub(super) fn flush(self: &Arc<Self>) {
        let last = {
            let queue = self.queue();
            if queue.ops.is_empty() && !queue.writing {
                return;
            }
            queue.next
        };
        let _ = self.wait_until(last, &[]);
    }

    /// Espera as escritas `seqs` chegarem ao banco; o erro é o da primeira que falhou.
    pub(super) fn wait(self: &Arc<Self>, seqs: &[u64]) -> io::Result<()> {
        match seqs.iter().max() {
            Some(&last) => self.wait_until(last, seqs),
            None => Ok(()),
        }
    }

    fn wait_until(self: &Arc<Self>, last: u64, seqs: &[u64]) -> io::Result<()> {
        let mut queue = self.queue();
        loop {
            if queue.done >= last {
                return queue.failure_of(seqs).map_or(Ok(()), Err);
            }
            if queue.writing {
                queue = self.turn.wait(queue).unwrap_or_else(|e| e.into_inner());
                continue;
            }
            queue.writing = true;
            let ops = std::mem::take(&mut queue.ops);
            drop(queue);
            let round = Round {
                disk: self,
                seqs: ops.iter().map(|(seq, _)| *seq).collect(),
            };
            let outcome = self.write(ops);
            queue = round.finish(outcome);
        }
    }

    /// Grava uma rodada numa transação. Devolve a última sequência, as falhas e quanto foi gravado.
    fn write(&self, ops: Vec<(u64, Op)>) -> Outcome {
        let last = ops.last().map_or(0, |(seq, _)| *seq);
        let mut outcome = Outcome {
            last,
            failures: Vec::new(),
            bytes: 0,
            legacy: Vec::new(),
        };
        let mut conn = self.conn();
        let mut open = conn.execute_batch("BEGIN").is_ok();
        // As sequências gravadas desde o último BEGIN: se a transação cair, falham todas.
        let mut in_tx: Vec<u64> = Vec::new();
        for (seq, op) in ops {
            outcome.bytes += OP_BYTES
                + match &op {
                    Op::Put { body, .. } => body.len() as u64,
                    _ => 0,
                };
            if let Op::Remove { id, .. } = &op
                && self.legacy_files
            {
                outcome.legacy.push(id.clone());
            }
            match self.apply(&mut conn, op) {
                Ok(()) => in_tx.push(seq),
                Err(e) => {
                    // Um erro de disco pode desfazer a transação inteira: o que estava nela também falhou.
                    if open && conn.is_autocommit() {
                        for done in in_tx.drain(..) {
                            outcome.failures.push((done, copy_error(&e)));
                        }
                        open = conn.execute_batch("BEGIN").is_ok();
                    }
                    outcome.failures.push((seq, e));
                }
            }
            if !open {
                in_tx.clear();
            }
        }
        if open && let Err(e) = conn.execute_batch("COMMIT") {
            let e = to_io(e);
            let _ = conn.execute_batch("ROLLBACK");
            for seq in in_tx {
                outcome.failures.push((seq, copy_error(&e)));
            }
            outcome.legacy.clear();
        }
        outcome
    }

    fn apply(&self, conn: &mut Connection, op: Op) -> io::Result<()> {
        match op {
            Op::Put { meta, body } => self.insert_row(conn, &meta, &body),
            Op::Delivery(meta) => conn
                .prepare_cached(
                    "UPDATE messages SET delivery_count = ?2, invisible_until = ?3, \
                     receipt_token = ?4 WHERE id = ?1",
                )
                .and_then(|mut stmt| {
                    stmt.execute(params![
                        meta.id,
                        meta.delivery_count,
                        meta.invisible_until_ms as i64,
                        meta.receipt_token
                    ])
                })
                .map(|_| ())
                .map_err(to_io),
            Op::Remove { id, excluded_at } => {
                let tx = Scope::begin(conn).map_err(to_io)?;
                if let Some(at_ms) = excluded_at {
                    tx.prepare_cached(RECORD_FROM_ROW)
                        .and_then(|mut stmt| stmt.execute(params![id, at_ms as i64]))
                        .map_err(to_io)?;
                }
                tx.prepare_cached("DELETE FROM messages WHERE id = ?1")
                    .and_then(|mut stmt| stmt.execute(params![id]))
                    .map_err(to_io)?;
                self.drop_body(&tx, &id)?;
                tx.commit().map_err(to_io)
            }
            Op::Expiry(ops) => {
                if ops.iter().any(|(put, _, _)| *put) {
                    self.may_hold_expiry.store(true, Ordering::SeqCst);
                }
                let tx = Scope::begin(conn).map_err(to_io)?;
                for (put, until, id) in ops {
                    let sql = if put {
                        "INSERT OR REPLACE INTO expiry (invisible_until, id) VALUES (?1, ?2)"
                    } else {
                        "DELETE FROM expiry WHERE invisible_until = ?1 AND id = ?2"
                    };
                    tx.prepare_cached(sql)
                        .and_then(|mut stmt| stmt.execute(params![until as i64, id]))
                        .map_err(to_io)?;
                }
                tx.commit().map_err(to_io)
            }
            Op::TrimExcluded { before_ms } => conn
                .prepare_cached(
                    "DELETE FROM excluded WHERE id IN \
                     (SELECT id FROM excluded WHERE at_ms < ?1 LIMIT ?2)",
                )
                .and_then(|mut stmt| stmt.execute(params![before_ms, super::excluded::TRIM_CHUNK]))
                .map(|_| ())
                .map_err(to_io),
        }
    }

    /// Lê com uma conexão só de leitura (a do conjunto do tópico, ou uma nova).
    pub(super) fn read<R>(
        &self,
        work: impl FnOnce(&Connection) -> rusqlite::Result<R>,
    ) -> io::Result<R> {
        let taken = self.readers.lock().unwrap_or_else(|e| e.into_inner()).pop();
        let conn = match taken {
            Some(conn) => conn,
            None => self.open_reader()?,
        };
        let result = work(&conn).map_err(to_io);
        let mut readers = self.readers.lock().unwrap_or_else(|e| e.into_inner());
        if readers.len() < READERS_KEPT {
            readers.push(conn);
        }
        result
    }

    /// Aberta para leitura e escrita, mas com `query_only`: se ela for a última conexão do banco a fechar, ainda
    /// consegue copiar o WAL e apagá-lo (uma `SQLITE_OPEN_READ_ONLY` deixaria o WAL e o `-shm` na pasta).
    fn open_reader(&self) -> io::Result<Connection> {
        let conn = Connection::open_with_flags(
            self.dir.join(super::TOPIC_DB_FILE),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(to_io)?;
        conn.pragma_update(None, "query_only", true)
            .map_err(to_io)?;
        conn.pragma_update(None, "cache_size", READER_CACHE_KIB)
            .map_err(to_io)?;
        Ok(conn)
    }

    /// Copia o WAL para o arquivo do banco numa thread de fundo, sem bloquear quem grava (`PASSIVE`).
    fn checkpoint_later(self: &Arc<Self>) {
        if self.checkpointing.swap(true, Ordering::AcqRel) {
            return;
        }
        let disk = self.clone();
        let spawned = std::thread::Builder::new()
            .name("taba-checkpoint".into())
            .spawn(move || {
                if !disk.dropped.load(Ordering::Acquire) {
                    let done = Connection::open(disk.dir.join(super::TOPIC_DB_FILE))
                        .and_then(|conn| conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)"));
                    if let Err(e) = done {
                        tracing::warn!(error = %e, "failed to checkpoint the topic database");
                    }
                }
                disk.checkpointing.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.checkpointing.store(false, Ordering::Release);
        }
    }
}

/// O resultado de uma rodada.
struct Outcome {
    last: u64,
    failures: Vec<(u64, io::Error)>,
    bytes: u64,
    /// Os arquivos de corpo do layout antigo a apagar depois do commit.
    legacy: Vec<String>,
}

/// A rodada de uma líder: ao terminar, libera a vez e acorda quem espera. Se a gravação entrar em pânico, as
/// escritas que ela levou contam como falhadas.
struct Round<'a> {
    disk: &'a Arc<TopicDisk>,
    seqs: Vec<u64>,
}

impl<'a> Round<'a> {
    fn finish(mut self, outcome: Outcome) -> MutexGuard<'a, Queue> {
        let disk = self.disk;
        self.seqs.clear();
        drop(self);
        for id in &outcome.legacy {
            if let Err(e) = disk.drop_legacy_file(id) {
                tracing::warn!(error = %e, "failed to remove a legacy body file");
            }
        }
        let mut queue = disk.queue();
        let Outcome {
            last,
            failures,
            bytes,
            ..
        } = outcome;
        queue.done = queue.done.max(last);
        queue.failures.extend(failures);
        let done = queue.done;
        while queue
            .failures
            .front()
            .is_some_and(|(seq, _)| *seq + FAILURES_KEPT < done)
        {
            queue.failures.pop_front();
        }
        queue.overlay.retain(|_, (seq, _)| *seq > done);
        queue.expiry.retain(|_, (seq, _)| *seq > done);
        queue.since_checkpoint += bytes;
        if queue.since_checkpoint >= CHECKPOINT_BYTES {
            queue.since_checkpoint = 0;
            disk.checkpoint_later();
        }
        queue.writing = false;
        disk.turn.notify_all();
        queue
    }
}

impl Drop for Round<'_> {
    fn drop(&mut self) {
        if self.seqs.is_empty() {
            return;
        }
        let mut queue = self.disk.queue();
        for &seq in &self.seqs {
            queue
                .failures
                .push_back((seq, io::Error::other("the write of the batch panicked")));
            queue.done = queue.done.max(seq);
        }
        let done = queue.done;
        queue.overlay.retain(|_, (seq, _)| *seq > done);
        queue.expiry.retain(|_, (seq, _)| *seq > done);
        queue.writing = false;
        self.disk.turn.notify_all();
    }
}

#[cfg(test)]
mod tests;
