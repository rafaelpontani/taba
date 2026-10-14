//! A restauração a partir do bucket (doc 23, "Backup em S3", seção 1.7).
//!
//! **Quem restaura.** O nó que sobe e **não acha ninguém vivo**: observa `nodes/*.json` por
//! `backup.dead-after`; se algum batimento estiver fresco, ele não está sozinho e entra pelo caminho
//! normal; se todos estiverem vencidos (ou o cluster foi parado de propósito), ele é o **sobrevivente
//! único**: começa um cluster de um nó só e restaura.
//!
//! **Dois restauradores ao mesmo tempo.** A trava `restore/lock.json`, com escrita condicional
//! (`If-None-Match` para criar), decide quem ganha. Quem perde espera e entra como nó normal. A trava
//! tem validade (`backup.restore.lock-ttl`) e o titular a renova. Num S3 sem escrita condicional a
//! trava não é segura: o nó avisa e segue (risco aceito).
//!
//! **O que volta.** As definições de tópico e os tokens; as mensagens dos segmentos com o estado do
//! último diário. As mensagens removidas pelo diário (ack, `ttl`, dead-letter) são descartadas.
//!
//! **Identidade.** Um nó morto que volta entra como nó novo, com id novo.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::definitions::{parse_token, parse_topic};
use super::heartbeat::Verdict;
use super::{Backup, lock};
use crate::core::id;
use crate::engine::messages::Loaded;
use crate::storage::backup::journal::{self, Record};
use crate::storage::backup::segment::{self, FooterRead, TAIL_GUESS};
use crate::storage::backup::{SavedRef, SegmentRow, StoreError, keys};
use crate::storage::messages::MessageMeta;

/// O que a decisão da partida manda fazer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupDecision {
    /// Ninguém vivo e a trava é deste nó: começa um cluster de um nó só e restaura.
    StartAlone,
    /// Há um cluster vivo (ou outro nó restaura): segue tentando entrar pelo caminho normal.
    KeepTrying,
}

/// O que a restauração devolveu.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreReport {
    pub topics: u64,
    pub tokens: u64,
    pub messages: u64,
    /// Descartadas porque o diário as removeu.
    pub removed: u64,
    /// Descartadas por `ttl` vencido.
    pub expired: u64,
}

/// O conteúdo de `restore/lock.json`.
#[derive(Debug, Serialize, Deserialize)]
struct LockBody {
    holder: String,
    #[serde(rename = "atMs")]
    at_ms: u64,
}

/// O resultado de tentar a trava.
enum Lock {
    /// Ganhou a trava.
    Held,
    /// Outro nó restaura.
    Busy,
    /// O S3 não respondeu.
    Unavailable,
}

impl Backup {
    /// A decisão da partida (doc 23, 1.7, exemplo C): chamada quando a listagem de nós pelo
    /// `cluster.entry-address` não encontra ninguém.
    pub async fn startup_decision(&self, first_node: bool) -> StartupDecision {
        let verdict = self.wait_for_verdict().await;
        let stopped = self.read_stop_list().await.is_some();
        match verdict {
            Verdict::Unavailable => {
                if first_node {
                    // Sem S3 o recurso fica desligado e o cluster funciona como hoje.
                    StartupDecision::StartAlone
                } else {
                    StartupDecision::KeepTrying
                }
            }
            Verdict::Alive(beats) => {
                tracing::info!(
                    nodes = ?beats.iter().map(|b| (&b.id, &b.addresses)).collect::<Vec<_>>(),
                    "backup: há nós batendo no bucket; este nó não está sozinho e segue tentando entrar"
                );
                StartupDecision::KeepTrying
            }
            Verdict::Empty if !first_node && !stopped => StartupDecision::KeepTrying,
            Verdict::Empty | Verdict::AllDead => match self.try_restore_lock().await {
                Lock::Held => {
                    tracing::warn!(
                        node_id = %self.node_id(),
                        stopped,
                        "backup: nenhum nó bate no bucket; este nó é o sobrevivente único e vai restaurar"
                    );
                    StartupDecision::StartAlone
                }
                Lock::Busy => {
                    tracing::info!("backup: outro nó restaura; este espera e entra como nó normal");
                    StartupDecision::KeepTrying
                }
                Lock::Unavailable if first_node => StartupDecision::StartAlone,
                Lock::Unavailable => StartupDecision::KeepTrying,
            },
        }
    }

    /// Uma tentativa de pegar a trava de restauração.
    async fn try_restore_lock(&self) -> Lock {
        let me = self.node_id();
        let body = |at_ms| {
            serde_json::to_vec(&LockBody {
                holder: me.clone(),
                at_ms,
            })
            .expect("a trava serializa")
        };
        match self
            .store
            .put_if_absent(keys::RESTORE_LOCK, body(self.clock.now_millis()))
            .await
        {
            Ok(true) => {
                self.note::<(), StoreError>("trava de restauração", &Ok(()));
                return Lock::Held;
            }
            Ok(false) => {}
            Err(StoreError::Unsupported(why)) => {
                tracing::warn!(
                    why,
                    "backup: o S3 não suporta escrita condicional; a trava de restauração não é segura (dois restauradores podem divergir)"
                );
                return Lock::Held;
            }
            Err(e) => {
                self.note::<(), _>("trava de restauração", &Err(e));
                return Lock::Unavailable;
            }
        }
        // Já existe: quem a tem? Sem relógio sincronizado, só se vê o ETag parado.
        let Ok(Some(object)) = self.store.get(keys::RESTORE_LOCK).await else {
            return Lock::Busy;
        };
        let held_by_me =
            serde_json::from_slice::<LockBody>(&object.body).is_ok_and(|l| l.holder == me);
        if held_by_me {
            return Lock::Held;
        }
        let expired = {
            let mut seen = lock(&self.lock_seen);
            match seen.as_ref() {
                Some((etag, since)) if *etag == object.etag => {
                    since.elapsed() >= self.rules.restore_lock_ttl
                }
                _ => {
                    *seen = Some((object.etag.clone(), Instant::now()));
                    false
                }
            }
        };
        if !expired {
            return Lock::Busy;
        }
        // O titular parou de renovar: toma a trava só se ela ainda é a que vimos.
        match self
            .store
            .put_if_match(
                keys::RESTORE_LOCK,
                body(self.clock.now_millis()),
                &object.etag,
            )
            .await
        {
            Ok(true) => {
                tracing::warn!("backup: a trava de restauração venceu; este nó a assumiu");
                Lock::Held
            }
            Ok(false) => Lock::Busy,
            Err(_) => Lock::Unavailable,
        }
    }

    /// Roda a restauração em segundo plano (o nó já está ativo, sozinho no cluster) e solta a
    /// trava ao fim.
    pub fn spawn_restore(self: &std::sync::Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            me.restoring.store(true, Ordering::SeqCst);
            // Renova a trava enquanto restaura.
            let renew = me.cancel.child_token();
            let renewer = {
                let (me, renew) = (me.clone(), renew.clone());
                tokio::spawn(async move {
                    let every = (me.rules.restore_lock_ttl / 3).max(Duration::from_millis(50));
                    loop {
                        tokio::select! {
                            () = renew.cancelled() => return,
                            () = tokio::time::sleep(every) => {}
                        }
                        let body = serde_json::to_vec(&LockBody {
                            holder: me.node_id(),
                            at_ms: me.clock.now_millis(),
                        })
                        .expect("a trava serializa");
                        let _ = me.store.put(keys::RESTORE_LOCK, body).await;
                    }
                })
            };
            let report = me.restore().await;
            tracing::info!(?report, "backup: restauração concluída");
            renew.cancel();
            let _ = renewer.await;
            me.restoring.store(false, Ordering::SeqCst);
            // O cluster voltou a funcionar: solta a trava, apaga a lista da parada e os batimentos
            // de nós mortos que ninguém reclama.
            let _ = me.store.delete(keys::RESTORE_LOCK).await;
            let _ = me.store.delete(keys::STOP_LIST).await;
            me.cleanup_nodes().await;
            me.adopt_all();
        });
    }

    /// Lê o bucket e devolve ao nó o que ele guarda: definições, tokens e mensagens.
    pub async fn restore(&self) -> RestoreReport {
        let mut report = RestoreReport::default();

        // 1. Definições de tópico.
        let listed = self.store.list(keys::TOPICS_PREFIX).await;
        self.note("LIST de topics/", &listed);
        let mut restored = Vec::new();
        for object in listed.unwrap_or_default() {
            let Ok(Some(body)) = self.store.get(&object.key).await else {
                continue;
            };
            let Some(definition) = parse_topic(&body.body) else {
                continue;
            };
            if self.topics.definition(&definition.name).is_some() {
                continue;
            }
            match self.topics.apply_save(definition.clone()).await {
                Ok(()) => {
                    report.topics += 1;
                    restored.push(definition);
                }
                Err(e) => {
                    tracing::error!(topic = %definition.name, error = %e.message, "backup: não foi possível restaurar o tópico")
                }
            }
        }

        // 2. Tokens (os vencidos ficam de fora).
        let listed = self.store.list(keys::TOKENS_PREFIX).await;
        self.note("LIST de tokens/", &listed);
        let now = self.clock.now_millis();
        for object in listed.unwrap_or_default() {
            let Ok(Some(body)) = self.store.get(&object.key).await else {
                continue;
            };
            let Some(record) = parse_token(&body.body) else {
                continue;
            };
            if record.expired_at(now) || self.tokens.record(&record.id).is_some() {
                continue;
            }
            if self.tokens.apply_save(record).await.is_ok() {
                report.tokens += 1;
            }
        }

        // 3. Mensagens, tópico por tópico (a dead-letter de cada fila também).
        for definition in restored {
            if definition.queue().is_none() {
                continue;
            }
            let mut names = vec![definition.name.clone()];
            names.push(format!(
                "{}{}",
                definition.name,
                self.topics.rules().dead_letter_suffix
            ));
            for name in names {
                self.restore_topic_messages(&name, &mut report).await;
            }
        }
        report
    }

    async fn restore_topic_messages(&self, name: &str, report: &mut RestoreReport) {
        let Ok(view) = self.topics.get(name) else {
            return;
        };
        let topic_id = view.definition.id.clone();
        let segments = self
            .store
            .list(&keys::segments_prefix(name, &topic_id))
            .await;
        self.note("LIST de segmentos", &segments);
        let mut alive: BTreeMap<String, (MessageMeta, Vec<u8>, SavedRef)> = BTreeMap::new();
        let mut sizes: HashMap<String, (u64, u64)> = HashMap::new();
        for object in segments.unwrap_or_default() {
            let Ok(Some(whole)) = self.store.get(&object.key).await else {
                continue;
            };
            let Ok((footer, messages)) = segment::decode_all(&whole.body) else {
                tracing::error!(key = %object.key, "backup: segmento ilegível; ignorado");
                continue;
            };
            sizes.insert(
                object.key.clone(),
                (footer.entries.len() as u64, object.size),
            );
            for (entry, (meta, body)) in footer.entries.iter().zip(messages) {
                alive.insert(
                    meta.id.clone(),
                    (
                        meta.into(),
                        body,
                        SavedRef {
                            segment: object.key.clone(),
                            offset: entry.offset,
                            len: entry.len,
                            saved_at_ms: self.clock.now_millis(),
                        },
                    ),
                );
            }
        }
        // O diário, do mais antigo ao mais novo: o último estado de cada mensagem vence.
        let journals = self
            .store
            .list(&keys::journals_prefix(name, &topic_id))
            .await;
        self.note("LIST de diários", &journals);
        for object in journals.unwrap_or_default() {
            let Ok(Some(body)) = self.store.get(&object.key).await else {
                continue;
            };
            let Ok((_, records)) = journal::decode(&body.body) else {
                tracing::error!(key = %object.key, "backup: diário ilegível; ignorado");
                continue;
            };
            apply_records(&mut alive, &records, &mut report.removed);
        }
        // Quantas vivas cada segmento tem, para o nó saber apagá-lo quando todas morrerem.
        let mut live: HashMap<String, u64> = HashMap::new();
        for (id, (meta, body, saved)) in alive {
            let _ = self.db.set_saved(name, &id, &saved);
            match self.messages.apply_load(name, meta, body) {
                Ok(Loaded::Stored | Loaded::Existing) => {
                    report.messages += 1;
                    *live.entry(saved.segment).or_default() += 1;
                }
                Ok(Loaded::Expired) => {
                    report.expired += 1;
                    let _ = self.db.forget(name, &id);
                }
                Ok(_) => {
                    let _ = self.db.forget(name, &id);
                }
                Err(e) => {
                    tracing::error!(topic = %name, id = %id, error = %e.message, "backup: não foi possível restaurar a mensagem");
                    let _ = self.db.forget(name, &id);
                }
            }
        }
        for (key, (total, bytes)) in sizes {
            let remaining = live.get(&key).copied().unwrap_or(0);
            let _ = self.db.put_segment(&SegmentRow {
                key: key.clone(),
                topic: name.to_owned(),
                topic_id: topic_id.clone(),
                total,
                live: remaining,
                bytes,
                created_ms: self.clock.now_millis(),
            });
            if remaining == 0 {
                lock(&self.runtime).dead_segments.push(key);
            }
        }
    }

    /// Se algum membro saiu da tabela desde a última volta (excluído por morte), procura no
    /// bucket as mensagens dos blocos que este nó passou a guardar e que ele não tem: as que só
    /// existiam nos nós mortos (doc 23, seção 1.7, exemplo B).
    pub(crate) async fn check_membership_loss(&self) {
        if !self.topology.is_active() || self.restoring.load(Ordering::SeqCst) {
            return;
        }
        let now: std::collections::BTreeSet<String> = self.topology.members().into_iter().collect();
        let lost = {
            let mut last = lock(&self.last_members);
            let lost = last.iter().any(|id| !now.contains(id));
            *last = now;
            lost
        };
        if lost {
            let recovered = self.recover_missing().await;
            if recovered > 0 {
                tracing::warn!(
                    recovered,
                    "backup: mensagens que só existiam em nós mortos voltaram do S3"
                );
            }
        }
    }

    /// Traz do bucket as mensagens de que este nó é responsável (primário ou réplica) e que ele não
    /// tem. Lê só o rodapé de cada segmento e, do que falta, a entrada por `Range`; o diário diz o
    /// que já foi confirmado. Devolve quantas voltaram.
    pub async fn recover_missing(&self) -> u64 {
        let me = self.node_id();
        let mut recovered = 0;
        let mut names = self.topics.names();
        let suffix = self.topics.rules().dead_letter_suffix.clone();
        let dead_letters: Vec<String> = names
            .iter()
            .filter(|n| {
                self.topics
                    .definition(n)
                    .is_some_and(|d| d.queue().is_some())
            })
            .map(|n| format!("{n}{suffix}"))
            .collect();
        names.extend(dead_letters);
        for name in names {
            let Ok(view) = self.topics.get(&name) else {
                continue;
            };
            if view.definition.queue().is_none() {
                continue;
            }
            let topic_id = view.definition.id.clone();
            let listed = self
                .store
                .list(&keys::segments_prefix(&name, &topic_id))
                .await;
            self.note("LIST de segmentos", &listed);
            let Ok(segments) = listed else {
                continue;
            };
            // O que o diário já removeu.
            let mut removed = std::collections::HashSet::new();
            let mut states: HashMap<String, Record> = HashMap::new();
            if let Ok(journals) = self
                .store
                .list(&keys::journals_prefix(&name, &topic_id))
                .await
            {
                for object in journals {
                    let Ok(Some(body)) = self.store.get(&object.key).await else {
                        continue;
                    };
                    let Ok((_, records)) = journal::decode(&body.body) else {
                        continue;
                    };
                    for record in records {
                        if record.removed {
                            removed.insert(record.id.clone());
                            states.remove(&record.id);
                        } else {
                            states.insert(record.id.clone(), record);
                        }
                    }
                }
            }
            for object in segments {
                let Some(footer) = self.footer_of(&object.key).await else {
                    continue;
                };
                for entry in &footer.entries {
                    if removed.contains(&entry.id) || self.messages.holds(&name, &entry.id) {
                        continue;
                    }
                    let Some(raw) = self
                        .store
                        .get_range(&object.key, entry.offset, entry.len)
                        .await
                        .ok()
                        .flatten()
                    else {
                        continue;
                    };
                    let Ok((meta, body)) = segment::decode_entry(&raw) else {
                        continue;
                    };
                    let mut meta: MessageMeta = meta.into();
                    if !self.topology.owners(meta.position).contains(&me) {
                        continue;
                    }
                    if let Some(record) = states.get(&entry.id) {
                        record.apply_to(&mut meta);
                    }
                    let saved = SavedRef {
                        segment: object.key.clone(),
                        offset: entry.offset,
                        len: entry.len,
                        saved_at_ms: self.clock.now_millis(),
                    };
                    let _ = self.db.set_saved(&name, &entry.id, &saved);
                    if matches!(
                        self.messages.apply_load(&name, meta, body),
                        Ok(Loaded::Stored)
                    ) {
                        recovered += 1;
                    }
                }
            }
        }
        recovered
    }

    /// Apaga de `nodes/` os arquivos de ids que não pertencem ao cluster **e** cujo batimento está
    /// vencido. Um arquivo fresco nunca é apagado: pode ser um nó vivo em outra partição.
    pub async fn cleanup_nodes(&self) {
        if !self.topology.is_active() || self.restoring.load(Ordering::SeqCst) {
            return;
        }
        self.observe().await;
        let members = self.topology.members();
        for (id, _, liveness) in self.observed() {
            if liveness == super::Liveness::Dead && !members.contains(&id) {
                let deleted = self.store.delete(&keys::node(&id)).await;
                self.note("DELETE do batimento morto", &deleted);
            }
        }
    }

    /// A mensagem `id` do tópico, como o bucket a tem (segmentos mais o último estado do diário);
    /// `None` se ela não está no bucket ou foi removida pelo diário.
    pub async fn find_message(
        &self,
        topic: &str,
        message_id: &str,
    ) -> Option<(MessageMeta, Vec<u8>)> {
        let view = self.topics.get(topic).ok()?;
        let topic_id = view.definition.id;
        let segments = self
            .store
            .list(&keys::segments_prefix(topic, &topic_id))
            .await
            .ok()?;
        let mut found: Option<(MessageMeta, Vec<u8>)> = None;
        for object in segments {
            // O segmento só pode ter a mensagem se o primeiro `id` dele é menor ou igual.
            if keys::first_id_of(&object.key).is_some_and(|first| first > message_id) {
                continue;
            }
            let Some(tail) = self
                .store
                .get_suffix(&object.key, TAIL_GUESS)
                .await
                .ok()
                .flatten()
            else {
                continue;
            };
            let footer = match segment::read_footer(&tail).ok()? {
                FooterRead::Done(footer) => footer,
                FooterRead::NeedMore(n) => {
                    let tail = self.store.get_suffix(&object.key, n).await.ok().flatten()?;
                    match segment::read_footer(&tail).ok()? {
                        FooterRead::Done(footer) => footer,
                        FooterRead::NeedMore(_) => continue,
                    }
                }
            };
            let Some(entry) = footer.entries.iter().find(|e| e.id == message_id) else {
                continue;
            };
            let raw = self
                .store
                .get_range(&object.key, entry.offset, entry.len)
                .await
                .ok()
                .flatten()?;
            let (meta, body) = segment::decode_entry(&raw).ok()?;
            found = Some((meta.into(), body));
            break;
        }
        let (mut meta, body) = found?;
        // O estado e a remoção vêm do diário.
        let journals = self
            .store
            .list(&keys::journals_prefix(topic, &topic_id))
            .await
            .ok()?;
        for object in journals {
            let Some(content) = self.store.get(&object.key).await.ok().flatten() else {
                continue;
            };
            let Ok((_, records)) = journal::decode(&content.body) else {
                continue;
            };
            for record in records.iter().filter(|r| r.id == message_id) {
                if record.removed {
                    return None;
                }
                record.apply_to(&mut meta);
            }
        }
        let _ = id::unix_millis_of(message_id);
        Some((meta, body))
    }
}

/// Aplica os registros de um diário às mensagens lidas dos segmentos.
fn apply_records(
    alive: &mut BTreeMap<String, (MessageMeta, Vec<u8>, SavedRef)>,
    records: &[Record],
    removed: &mut u64,
) {
    for record in records {
        if record.removed {
            if alive.remove(&record.id).is_some() {
                *removed += 1;
            }
        } else if let Some((meta, ..)) = alive.get_mut(&record.id) {
            record.apply_to(meta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::backup::journal::Record;

    fn entry(id: &str) -> (MessageMeta, Vec<u8>, SavedRef) {
        (
            MessageMeta {
                id: id.into(),
                content_type: "text/plain".into(),
                group_id: None,
                deduplication_id: None,
                published_ms: 1,
                delivery_count: 0,
                invisible_until_ms: 0,
                receipt_token: None,
                dead_lettered_ms: None,
                dead_letter_reason: None,
                position: 0,
            },
            b"x".to_vec(),
            SavedRef {
                segment: "s".into(),
                offset: 0,
                len: 1,
                saved_at_ms: 0,
            },
        )
    }

    /// UC: UC-BKP-026
    #[test]
    fn the_last_journal_state_wins_and_removals_drop_the_message() {
        let mut alive = BTreeMap::new();
        alive.insert("a".to_owned(), entry("a"));
        alive.insert("b".to_owned(), entry("b"));
        let mut state = entry("a").0;
        state.delivery_count = 4;
        state.receipt_token = Some("tok".into());
        let mut removed = 0;
        apply_records(
            &mut alive,
            &[
                Record::state(&state),
                Record::removal("b"),
                Record::removal("zzz"),
            ],
            &mut removed,
        );
        assert_eq!(removed, 1, "só conta a que estava nos segmentos");
        assert!(!alive.contains_key("b"));
        assert_eq!(alive["a"].0.delivery_count, 4);
        assert_eq!(alive["a"].0.receipt_token.as_deref(), Some("tok"));
    }
}
