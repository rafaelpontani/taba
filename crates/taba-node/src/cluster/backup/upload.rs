//! O envio ao bucket: o `outbox` vira segmentos (`.seg`), o estado mutável vira diário (`.jrn`), os
//! segmentos que morreram inteiros são apagados, e o `flush` força tudo (doc 23, "Backup em S3").
//!
//! Nada aqui roda no caminho da publicação: o commit de uma mensagem só inseriu uma linha no
//! `_backup.db`. Uma falha do S3 deixa tudo como está (o `outbox` guarda os IDs) e o envio tenta de
//! novo; só **depois do PUT confirmar** as linhas do lote saem do `outbox`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::{Backup, lock};
use crate::core::id;
use crate::storage::backup::journal::{self, Header, Record};
use crate::storage::backup::segment::{self, Footer, FooterRead, SegmentBuilder, TAIL_GUESS};
use crate::storage::backup::{SavedRef, SegmentRow, keys};
use crate::storage::messages::MessageMeta;

/// Quantas mensagens do `outbox` uma passada puxa de cada vez.
const PAGE: usize = 5_000;

/// Quanto o envio espera depois de uma falha do S3, antes de tentar de novo.
const RETRY_AFTER: Duration = Duration::from_secs(2);

/// O segmento que ainda está aberto para um tópico: mensagens que já passaram do `backup.delay` e
/// esperam o segmento fechar (por tamanho, quantidade ou idade).
struct OpenSegment {
    topic_id: String,
    items: Vec<(MessageMeta, Vec<u8>)>,
    bytes: u64,
    opened: Instant,
}

#[derive(Default)]
pub(crate) struct Uploader {
    open: HashMap<String, OpenSegment>,
    /// As mensagens que estão em segmento aberto: a próxima passada não as puxa de novo.
    in_open: HashSet<(String, String)>,
    /// O S3 falhou há pouco: espera até este instante.
    retry_at: Option<Instant>,
}

/// O que o `flush` fez (a resposta de `POST /node/backup/flush`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlushReport {
    /// Etapa 1: mensagens que saíram da memória para o disco.
    pub memory_moved: u64,
    /// Mensagens que continuaram em memória (erro de disco ou prazo).
    pub memory_remaining: u64,
    /// Etapa 2 e 3: segmentos enviados e mensagens neles.
    pub segments_uploaded: u64,
    pub messages_uploaded: u64,
    pub journals_uploaded: u64,
    /// O que ainda falta (zero quando completo).
    pub outbox_pending: u64,
    pub journal_pending: u64,
    /// O prazo (`backup.drain.timeout`) estourou antes de terminar.
    pub timed_out: bool,
    pub s3_reachable: bool,
}

impl FlushReport {
    /// O relatório como a API o mostra.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "memoryMoved": self.memory_moved,
            "memoryRemaining": self.memory_remaining,
            "segmentsUploaded": self.segments_uploaded,
            "messagesUploaded": self.messages_uploaded,
            "journalsUploaded": self.journals_uploaded,
            "outboxPending": self.outbox_pending,
            "journalPending": self.journal_pending,
            "timedOut": self.timed_out,
            "s3Reachable": self.s3_reachable,
            "complete": self.complete(),
        })
    }

    pub fn complete(&self) -> bool {
        !self.timed_out
            && self.memory_remaining == 0
            && self.outbox_pending == 0
            && self.journal_pending == 0
    }
}

impl Backup {
    /// Uma passada do envio. Sem `force`, só sobem as mensagens que passaram de `backup.delay` e os
    /// segmentos que fecharam; com `force` (flush, drain), sobe tudo agora. Devolve quantos segmentos
    /// subiram.
    pub(crate) async fn upload_pass(&self, force: bool) -> u64 {
        let mut up = self.uploader.lock().await;
        if !force && up.retry_at.is_some_and(|at| Instant::now() < at) {
            return 0;
        }
        if !self.topology.is_active() {
            return 0;
        }
        up.retry_at = None;
        let mut uploaded = 0;
        loop {
            let before = if force {
                u64::MAX
            } else {
                self.clock
                    .now_millis()
                    .saturating_sub(self.rules.delay.as_millis() as u64)
            };
            let rows = match self.db.outbox(before, PAGE + up.in_open.len()) {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::error!(error = %e, "backup: não foi possível ler o outbox");
                    return uploaded;
                }
            };
            let fresh: Vec<(String, String)> = rows
                .into_iter()
                .filter(|row| !up.in_open.contains(row))
                .collect();
            let pulled = fresh.len();
            for (topic, id) in fresh {
                self.admit(&mut up, topic.clone(), id).await;
                // Chegou a um limite de tamanho ou de quantidade: fecha já, sem esperar a página.
                if up.open.get(&topic).is_some_and(|open| {
                    open.items.len() >= self.rules.segment_max_messages
                        || open.bytes >= self.rules.segment_max_bytes
                }) {
                    uploaded += self.close_topic(&mut up, &topic).await;
                    if up.retry_at.is_some() {
                        break;
                    }
                }
            }
            uploaded += self.close_ready(&mut up, force).await;
            // Sem `force` uma página basta (a próxima passada continua); com `force` repete até o
            // outbox esvaziar ou o S3 falhar.
            if !force || pulled == 0 || up.retry_at.is_some() {
                break;
            }
        }
        uploaded
    }

    /// Põe a mensagem no segmento aberto do tópico. Se ela já não está no nó (ack, `ttl`), sai do
    /// `outbox`.
    async fn admit(&self, up: &mut Uploader, topic: String, id: String) {
        let Ok(view) = self.topics.get(&topic) else {
            let _ = self.db.unqueue(&topic, &id);
            return;
        };
        let messages = self.messages.clone();
        let (t, i) = (topic.clone(), id.clone());
        let loaded = tokio::task::spawn_blocking(move || messages.backup_load(&t, &i))
            .await
            .ok()
            .flatten();
        let Some((meta, body)) = loaded else {
            let _ = self.db.unqueue(&topic, &id);
            return;
        };
        let open = up.open.entry(topic.clone()).or_insert_with(|| OpenSegment {
            topic_id: view.definition.id.clone(),
            items: Vec::new(),
            bytes: 0,
            opened: Instant::now(),
        });
        // O tópico foi apagado e recriado com o mesmo nome: o segmento antigo fecha antes.
        if open.topic_id != view.definition.id {
            return;
        }
        open.bytes += body.len() as u64;
        open.items.push((meta, body));
        up.in_open.insert((topic, id));
    }

    /// Fecha e envia os segmentos que bateram num limite (ou todos, com `force`).
    async fn close_ready(&self, up: &mut Uploader, force: bool) -> u64 {
        let ready: Vec<String> = up
            .open
            .iter()
            .filter(|(_, open)| {
                force
                    || open.items.len() >= self.rules.segment_max_messages
                    || open.bytes >= self.rules.segment_max_bytes
                    || open.opened.elapsed() >= self.rules.segment_max_age
            })
            .map(|(topic, _)| topic.clone())
            .collect();
        let mut uploaded = 0;
        for topic in ready {
            uploaded += self.close_topic(up, &topic).await;
            if up.retry_at.is_some() {
                break;
            }
        }
        uploaded
    }

    /// Fecha e envia o segmento aberto do tópico. Devolve `1` se um segmento subiu. Se o S3
    /// falhar, as linhas continuam no `outbox` e a próxima passada tenta de novo.
    async fn close_topic(&self, up: &mut Uploader, topic: &str) -> u64 {
        let Some(open) = up.open.remove(topic) else {
            return 0;
        };
        for (meta, _) in &open.items {
            up.in_open.remove(&(topic.to_owned(), meta.id.clone()));
        }
        match self.send_segment(topic, open).await {
            Ok(true) => 1,
            Ok(false) => 0,
            Err(()) => {
                up.retry_at = Some(Instant::now() + RETRY_AFTER);
                0
            }
        }
    }

    /// Monta o segmento, envia e, **só depois do PUT confirmar**, registra as referências e tira as
    /// mensagens do `outbox`. `Ok(false)`: nada a enviar.
    async fn send_segment(&self, topic: &str, open: OpenSegment) -> Result<bool, ()> {
        // Só entra o que ainda espera no outbox: o que morreu enquanto esperava não vai ao bucket.
        let items: Vec<_> = open
            .items
            .into_iter()
            .filter(|(meta, _)| self.db.queued(topic, &meta.id).unwrap_or(false))
            .collect();
        if items.is_empty() {
            return Ok(false);
        }
        let seq = self.db.next_seq().map_err(|_| ())?;
        let writer = self.node_id();
        let mut builder = SegmentBuilder::new(topic, &open.topic_id, &writer);
        let mut entries = Vec::with_capacity(items.len());
        for (meta, body) in &items {
            let entry = builder.push(meta, body);
            entries.push((entry.id, entry.offset, entry.len));
        }
        let key = keys::segment(
            topic,
            &open.topic_id,
            builder.first_id().unwrap_or_default(),
            &writer,
            seq,
        );
        let bytes = builder.finish();
        let size = bytes.len() as u64;
        let put = self.store.put(&key, bytes).await;
        self.note("PUT do segmento", &put);
        if put.is_err() {
            return Err(());
        }
        let row = SegmentRow {
            key: key.clone(),
            topic: topic.to_owned(),
            topic_id: open.topic_id.clone(),
            total: entries.len() as u64,
            live: 0,
            bytes: size,
            created_ms: self.clock.now_millis(),
        };
        let committed = match self.db.commit_segment(&row, &entries) {
            Ok(committed) => committed,
            Err(e) => {
                tracing::error!(error = %e, "backup: não foi possível registrar o segmento");
                return Err(());
            }
        };
        self.counters
            .segments_uploaded
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .messages_uploaded
            .fetch_add(committed.live, Ordering::Relaxed);
        self.counters
            .bytes_uploaded
            .fetch_add(size, Ordering::Relaxed);
        if committed.live == 0 {
            // Todas morreram durante o envio: o segmento nasce morto.
            lock(&self.runtime).dead_segments.push(key.clone());
        }
        let saved: Vec<(String, SavedRef)> = entries
            .iter()
            .filter(|(id, ..)| !committed.dead.contains(id))
            .map(|(id, offset, len)| {
                (
                    id.clone(),
                    SavedRef {
                        segment: key.clone(),
                        offset: *offset,
                        len: *len,
                        saved_at_ms: row.created_ms,
                    },
                )
            })
            .collect();
        self.announce_saved(topic, &items, &saved);
        Ok(true)
    }

    /// Avisa as réplicas de quais mensagens já estão no bucket, em lote (`BackupSaved`). Sem
    /// cluster, não há a quem avisar.
    fn announce_saved(
        &self,
        topic: &str,
        items: &[(MessageMeta, Vec<u8>)],
        saved: &[(String, SavedRef)],
    ) {
        let positions: HashMap<&str, u64> = items
            .iter()
            .map(|(meta, _)| (meta.id.as_str(), meta.position))
            .collect();
        let mut by_node: HashMap<String, Vec<(String, SavedRef)>> = HashMap::new();
        let me = self.node_id();
        for (id, saved) in saved {
            let Some(position) = positions.get(id.as_str()) else {
                continue;
            };
            for owner in self.topology.owners(*position) {
                if owner != me {
                    by_node
                        .entry(owner)
                        .or_default()
                        .push((id.clone(), saved.clone()));
                }
            }
        }
        for (node, list) in by_node {
            self.topology.send_saved(&node, topic, list);
        }
    }

    /// Apaga do bucket os segmentos em que todas as mensagens morreram.
    pub(crate) async fn purge_dead_segments(&self) {
        let pending: Vec<String> = std::mem::take(&mut lock(&self.runtime).dead_segments);
        let mut failed = Vec::new();
        for key in pending {
            let result = self.store.delete(&key).await;
            self.note("DELETE do segmento", &result);
            match result {
                Ok(()) => {
                    let _ = self.db.drop_segment(&key);
                    self.counters
                        .segments_deleted
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => failed.push(key),
            }
        }
        if !failed.is_empty() {
            lock(&self.runtime).dead_segments.extend(failed);
        }
    }

    /// O diário: só o **último estado** de cada mensagem alterada desde o último, por tópico. Um
    /// ack entra como remoção.
    pub(crate) async fn journal_pass(&self) -> u64 {
        if !self.topology.is_active() {
            return 0;
        }
        let dirty = match self.db.dirty(PAGE) {
            Ok(dirty) if !dirty.is_empty() => dirty,
            _ => return 0,
        };
        let mut by_topic: HashMap<String, Vec<Record>> = HashMap::new();
        let mut done = Vec::with_capacity(dirty.len());
        for item in dirty {
            let record = if item.removed {
                Some(Record::removal(&item.id))
            } else {
                match self.messages.message_info(&item.topic, &item.id) {
                    Ok(Some(info)) => Some(Record::state(&info.meta)),
                    // Já não está no nó: o ack virá como remoção.
                    _ => None,
                }
            };
            if let Some(record) = record {
                by_topic.entry(item.topic.clone()).or_default().push(record);
            }
            done.push(item);
        }
        let mut written = 0;
        let mut ok_topics: HashSet<String> = HashSet::new();
        for (topic, records) in by_topic {
            let Ok(view) = self.topics.get(&topic) else {
                ok_topics.insert(topic);
                continue;
            };
            let Ok(seq) = self.db.next_seq() else {
                continue;
            };
            let writer = self.node_id();
            let header = Header {
                topic: topic.clone(),
                topic_id: view.definition.id.clone(),
                writer: writer.clone(),
                at_ms: self.clock.now_millis(),
            };
            let key = keys::journal(&topic, &view.definition.id, header.at_ms, &writer, seq);
            let put = self
                .store
                .put(&key, journal::encode(&header, &records))
                .await;
            self.note("PUT do diário", &put);
            if put.is_ok() {
                ok_topics.insert(topic);
                written += 1;
                self.counters
                    .journals_uploaded
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        // Só sai da lista o que foi para o bucket: o resto tenta na próxima.
        let sent: Vec<_> = done
            .into_iter()
            .filter(|item| ok_topics.contains(&item.topic))
            .collect();
        let _ = self.db.clear_dirty(&sent);
        written
    }

    /// `POST /node/backup/flush` (doc 23, seção 1.11): em três etapas em sequência, sem desregistrar
    /// nem parar o nó.
    ///
    /// 1. memória → disco, pelo mesmo caminho do drain (o limite de memória cai a zero);
    /// 2. disco → S3: os segmentos e o diário pendentes, com prioridade;
    /// 3. `outbox` → S3: as mensagens que ainda não saíram.
    ///
    /// Termina quando as três acabam ou quando `backup.drain.timeout` estoura.
    pub async fn flush(&self) -> FlushReport {
        let deadline = Instant::now() + self.rules.drain_timeout;
        let mut report = FlushReport::default();

        // Etapa 1.
        let was_draining = self.messages.is_draining();
        if !was_draining {
            self.messages.begin_drain();
        }
        let messages = self.messages.clone();
        let moved = tokio::task::spawn_blocking(move || {
            messages.drain_until(|| Instant::now() >= deadline)
        })
        .await;
        // Um drain de verdade que começou no meio do flush segue com o limite de memória em zero.
        if !was_draining && self.shared.drain_state() == crate::cluster::state::DrainState::Active {
            self.messages.cancel_drain();
        }
        if let Ok(moved) = moved {
            report.memory_moved = moved.moved;
            report.memory_remaining = moved.remaining;
        }

        // Etapas 2 e 3.
        self.upload_everything(deadline, &mut report).await;
        report
    }

    /// O envio do drain (a memória já está em disco): tudo o que há vai ao S3, com prioridade,
    /// dentro de `backup.drain.timeout`.
    pub async fn drain_upload(&self) -> FlushReport {
        let deadline = Instant::now() + self.rules.drain_timeout;
        let mut report = FlushReport::default();
        self.upload_everything(deadline, &mut report).await;
        report
    }

    /// Etapas 2 e 3 do flush: os segmentos, o diário e o `outbox`, até esvaziar ou o prazo.
    async fn upload_everything(&self, deadline: Instant, report: &mut FlushReport) {
        let before = self.counters_snapshot();
        loop {
            self.upload_pass(true).await;
            self.journal_pass().await;
            self.purge_dead_segments().await;
            self.flush_definitions().await;
            let status = self.status();
            if status.outbox_pending == 0
                && status.journal_pending == 0
                && status.definitions_pending == 0
            {
                break;
            }
            if Instant::now() >= deadline {
                report.timed_out = true;
                break;
            }
            // O S3 falhou: espera um pouco e tenta de novo, até o prazo.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let after = self.counters_snapshot();
        report.segments_uploaded = after.0 - before.0;
        report.messages_uploaded = after.1 - before.1;
        report.journals_uploaded = after.2 - before.2;
        let status = self.status();
        report.outbox_pending = status.outbox_pending;
        report.journal_pending = status.journal_pending;
        report.s3_reachable = status.s3_ok;
    }

    fn counters_snapshot(&self) -> (u64, u64, u64) {
        (
            self.counters.segments_uploaded.load(Ordering::Relaxed),
            self.counters.messages_uploaded.load(Ordering::Relaxed),
            self.counters.journals_uploaded.load(Ordering::Relaxed),
        )
    }

    /// Lê do bucket o corpo de uma mensagem salva, por `Range` (a evicção o tirou do disco).
    pub(crate) async fn read_saved(&self, saved: &SavedRef) -> Option<Vec<u8>> {
        let raw = self
            .store
            .get_range(&saved.segment, saved.offset, saved.len)
            .await;
        self.note("GET do segmento", &raw);
        let raw = raw.ok().flatten()?;
        segment::decode_entry(&raw).ok().map(|(_, body)| body)
    }
}

impl Backup {
    /// O rodapé de um segmento do bucket, lido pelo fim (sem baixar o segmento).
    pub(crate) async fn footer_of(&self, key: &str) -> Option<Footer> {
        let tail = self
            .store
            .get_suffix(key, TAIL_GUESS)
            .await
            .ok()
            .flatten()?;
        match segment::read_footer(&tail).ok()? {
            FooterRead::Done(footer) => Some(footer),
            FooterRead::NeedMore(n) => {
                let tail = self.store.get_suffix(key, n).await.ok().flatten()?;
                match segment::read_footer(&tail).ok()? {
                    FooterRead::Done(footer) => Some(footer),
                    FooterRead::NeedMore(_) => None,
                }
            }
        }
    }

    /// Apaga os segmentos em que todas as mensagens já passaram do `ttl` do tópico: quem os gravou
    /// pode ter parado de ser o primário (ou ter morrido), e ninguém mais os contaria como mortos.
    /// Só o gestor do tópico varre, e a dead-letter fica de fora (o `ttl` dela conta da entrada
    /// nela, não da publicação).
    pub(crate) async fn sweep_expired_segments(&self) {
        if !self.topology.is_active() || self.restoring.load(Ordering::SeqCst) {
            return;
        }
        let now = self.clock.now_millis();
        let slack = 2 * self.rules.meta_interval.as_millis() as u64;
        for name in self.topics.names() {
            if !self.topology.manages_definition("topic", &name) {
                continue;
            }
            let Ok(view) = self.topics.get(&name) else {
                continue;
            };
            let Some(ttl_secs) = view.ttl_secs() else {
                continue;
            };
            let ttl_ms = ttl_secs.saturating_mul(1000);
            let listed = self
                .store
                .list(&keys::segments_prefix(&name, &view.definition.id))
                .await;
            self.note("LIST de segmentos", &listed);
            for object in listed.unwrap_or_default().into_iter().take(50) {
                let Some(footer) = self.footer_of(&object.key).await else {
                    continue;
                };
                let newest = footer
                    .entries
                    .iter()
                    .filter_map(|e| id::unix_millis_of(&e.id))
                    .max();
                if newest.is_some_and(|at| at.saturating_add(ttl_ms).saturating_add(slack) < now) {
                    let deleted = self.store.delete(&object.key).await;
                    self.note("DELETE do segmento vencido", &deleted);
                    if deleted.is_ok() {
                        let _ = self.db.drop_segment(&object.key);
                        self.counters
                            .segments_deleted
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}
