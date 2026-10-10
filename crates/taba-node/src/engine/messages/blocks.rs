//! As mensagens por **bloco** do anel (doc 05, seção 7.3): o que o nó lista e entrega a quem
//! sincroniza (`GET /cluster/blocks/{id}/ids` e `/messages`), o que ele aplica quando é ele quem
//! sincroniza, e a limpeza do que deixou de ser dele.
//!
//! Um bloco é um conjunto de faixas de posição do anel (`Ring::block_ranges`). A posição de cada
//! mensagem está nos metadados (`MessageMeta::position`): em disco, a coluna `position` responde
//! pela faixa; em memória, a varredura do mapa do tópico.

use super::{ErrorCode, Messages, Place, Stored, TabaError, TopicState, lock, message_not_found};
use crate::storage::messages::MessageMeta;

/// Uma linha da listagem de IDs de um bloco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntry {
    pub topic: String,
    pub id: String,
    /// Só na fila FIFO: a mensagem confirmada que a deduplicação ainda lembra.
    pub delete_at_ms: Option<u64>,
}

/// Uma mensagem do bloco, com o corpo.
#[derive(Debug, Clone)]
pub struct BlockMessage {
    pub topic: String,
    pub meta: MessageMeta,
    pub body: Vec<u8>,
    /// Só na FIFO: é a linha curta de uma mensagem confirmada (sem corpo).
    pub delete_at_ms: Option<u64>,
}

/// O que `apply_load` fez com uma mensagem que veio de outro nó.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loaded {
    /// Gravada.
    Stored,
    /// O nó já a tinha (um evento chegou antes da carga, ou a carga se repetiu).
    Existing,
    /// Já passou do `ttl`: não vale copiar (doc 13, seção 12.10).
    Expired,
    /// O tópico ainda não existe neste nó.
    UnknownTopic,
}

fn in_ranges(ranges: &[(u64, u64)], position: u64) -> bool {
    ranges
        .iter()
        .any(|(low, high)| (*low..=*high).contains(&position))
}

impl Messages {
    /// Os tópicos que têm mensagens aqui, em memória ou em disco.
    fn message_topics(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .existing_states()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        if let Ok(on_disk) = self.store.topics_on_disk() {
            names.extend(on_disk);
        }
        names.sort();
        names.dedup();
        names.retain(|name| self.topics.get(name).is_ok());
        names
    }

    /// Os IDs que o nó guarda no bloco (as faixas de posição), de todos os tópicos, em ordem de ID,
    /// entre `from` e `to` (inclusivos), no máximo `limit`. Mensagens em memória e em disco.
    pub fn block_entries(
        &self,
        ranges: &[(u64, u64)],
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BlockEntry>, TabaError> {
        let mut out: Vec<BlockEntry> = Vec::new();
        for topic in self.message_topics() {
            let rows = self
                .store
                .block_rows(&topic, ranges, from, to, limit)
                .map_err(|e| self.disk_error("falha ao listar o bloco em disco", &e))?;
            out.extend(rows.into_iter().map(|row| BlockEntry {
                topic: topic.clone(),
                id: row.id,
                delete_at_ms: row.delete_at_ms,
            }));
            let state = self.state_of(&topic);
            let state = lock(&state);
            let lower = from.unwrap_or("");
            let mut taken = 0;
            for (id, stored) in state
                .memory
                .range::<str, _>((std::ops::Bound::Included(lower), std::ops::Bound::Unbounded))
            {
                if taken >= limit || to.is_some_and(|to| id.as_str() > to) {
                    break;
                }
                if in_ranges(ranges, stored.meta.position) {
                    out.push(BlockEntry {
                        topic: topic.clone(),
                        id: id.clone(),
                        delete_at_ms: None,
                    });
                    taken += 1;
                }
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.topic.cmp(&b.topic)));
        out.dedup_by(|a, b| a.id == b.id && a.topic == b.topic);
        out.truncate(limit);
        Ok(out)
    }

    /// Lê a mensagem que o nó guarda (memória ou disco), sem conferir se ela está no índice ou
    /// entre as réplicas: é a leitura de quem serve a sincronização.
    fn read_held(
        &self,
        topic: &str,
        id: &str,
    ) -> Result<Option<(MessageMeta, Vec<u8>)>, TabaError> {
        {
            let state = self.state_of(topic);
            let state = lock(&state);
            if let Some(stored) = state.memory.get(id) {
                return Ok(Some((stored.meta.clone(), stored.body.clone())));
            }
        }
        let Some(meta) = self
            .store
            .get(topic, id)
            .map_err(|e| self.disk_error("falha ao ler a mensagem em disco", &e))?
        else {
            return Ok(None);
        };
        if meta.content_type.is_empty() {
            // A linha curta da FIFO (sem corpo) só serve à deduplicação.
            return Ok(None);
        }
        match self.store.body(topic, id) {
            Ok(body) => Ok(Some((meta, body))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.disk_error("falha ao ler o corpo em disco", &e)),
        }
    }

    /// A mensagem que só o disco da vida anterior ainda tem (PRB-048): a recuperação do reinício a
    /// procura aqui depois de olhar o que o nó guarda hoje.
    pub fn held_previous_life(
        &self,
        topic: &str,
        id: &str,
    ) -> Result<Option<BlockMessage>, TabaError> {
        let Some(store) = lock(&self.previous_life).clone() else {
            return Ok(None);
        };
        let Some(meta) = store
            .get(topic, id)
            .map_err(|e| self.disk_error("falha ao ler a mensagem em disco", &e))?
        else {
            return Ok(None);
        };
        if meta.content_type.is_empty() {
            return Ok(None);
        }
        match store.body(topic, id) {
            Ok(body) => Ok(Some(BlockMessage {
                topic: topic.to_owned(),
                meta,
                body,
                delete_at_ms: None,
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.disk_error("falha ao ler o corpo em disco", &e)),
        }
    }

    /// A mensagem que o nó ainda guarda, em memória ou em disco: é o que a recuperação do nó que
    /// volta procura para cada perda da lista (doc 04, seção 6.6, passo 2). Só o conteúdo que
    /// existe de fato conta: a linha curta da FIFO e o corpo que sumiu do disco não valem.
    pub fn held_message(&self, topic: &str, id: &str) -> Result<Option<BlockMessage>, TabaError> {
        Ok(self.read_held(topic, id)?.map(|(meta, body)| BlockMessage {
            topic: topic.to_owned(),
            meta,
            body,
            delete_at_ms: None,
        }))
    }

    /// As mensagens do bloco na faixa de IDs `from`–`to`, com o corpo, até `max_items` ou até
    /// passar de `max_bytes` de corpo (a primeira sempre entra). O segundo valor diz se há mais.
    pub fn block_messages(
        &self,
        ranges: &[(u64, u64)],
        from: Option<&str>,
        to: Option<&str>,
        max_items: usize,
        max_bytes: u64,
    ) -> Result<(Vec<BlockMessage>, bool), TabaError> {
        let mut cursor = from.map(str::to_owned);
        loop {
            let entries =
                self.block_entries(ranges, cursor.as_deref(), to, max_items.saturating_add(1))?;
            let more_listed = entries.len() > max_items;
            let mut out = Vec::new();
            let mut bytes = 0u64;
            let mut stopped_early = false;
            let mut last_seen = None;
            for entry in entries.into_iter().take(max_items) {
                last_seen = Some(entry.id.clone());
                if let Some(delete_at_ms) = entry.delete_at_ms {
                    // A linha curta vai sem corpo, com o `deleteAt` (doc 17, seção 16.7).
                    if let Some(meta) = self
                        .store
                        .get(&entry.topic, &entry.id)
                        .map_err(|e| self.disk_error("falha ao ler a linha em disco", &e))?
                    {
                        out.push(BlockMessage {
                            topic: entry.topic,
                            meta,
                            body: Vec::new(),
                            delete_at_ms: Some(delete_at_ms),
                        });
                    }
                    continue;
                }
                // A mensagem pode ter sido confirmada entre a listagem e a leitura: simplesmente sai.
                let Some((meta, body)) = self.read_held(&entry.topic, &entry.id)? else {
                    continue;
                };
                if !out.is_empty() && bytes.saturating_add(body.len() as u64) > max_bytes {
                    stopped_early = true;
                    break;
                }
                bytes += body.len() as u64;
                out.push(BlockMessage {
                    topic: entry.topic,
                    meta,
                    body,
                    delete_at_ms: None,
                });
            }
            if out.is_empty() && more_listed {
                // Tudo o que a janela listou sumiu: segue para depois do último, sem devolver um
                // trecho vazio com "há mais" (o receptor ficaria no mesmo lugar).
                if let Some(last) = last_seen {
                    cursor = Some(format!("{last} "));
                    continue;
                }
            }
            return Ok((out, more_listed || stopped_early));
        }
    }

    /// Uma mensagem do bloco (`GET /cluster/blocks/{id}/messages/{messageId}`).
    pub fn block_message(
        &self,
        ranges: &[(u64, u64)],
        topic_hint: Option<&str>,
        id: &str,
    ) -> Result<BlockMessage, TabaError> {
        let topics = match topic_hint {
            Some(topic) => vec![topic.to_owned()],
            None => self.message_topics(),
        };
        for topic in topics {
            if let Some((meta, body)) = self.read_held(&topic, id)?
                && in_ranges(ranges, meta.position)
            {
                return Ok(BlockMessage {
                    topic,
                    meta,
                    body,
                    delete_at_ms: None,
                });
            }
        }
        Err(message_not_found())
    }

    /// Aplica a mensagem que veio de outro nó na sincronização: grava como cópia (o nó em
    /// sincronização não é dono de nada), a menos que já tenha vencido por `ttl` ou o nó já a
    /// tenha. Idempotente.
    pub fn apply_load(
        &self,
        topic: &str,
        meta: MessageMeta,
        body: Vec<u8>,
    ) -> Result<Loaded, TabaError> {
        let Ok(view) = self.topics.get(topic) else {
            return Ok(Loaded::UnknownTopic);
        };
        if view.definition.queue().is_none() {
            return Err(TabaError::new(
                ErrorCode::TopicWrongType,
                "Um tópico distribuidor não guarda mensagens.",
            ));
        }
        let now = self.clock.now_millis();
        let ttl_ms = view.ttl_secs().unwrap_or(u64::MAX).saturating_mul(1000);
        if meta.age_base_ms().saturating_add(ttl_ms) <= now {
            return Ok(Loaded::Expired);
        }
        if self.holds(topic, &meta.id) {
            return Ok(Loaded::Existing);
        }
        self.apply_publish(topic, meta, body)?;
        Ok(Loaded::Stored)
    }

    /// Aplica a linha curta de uma mensagem FIFO confirmada que veio de outro nó na sincronização
    /// (doc 17, seção 16.7): fica só para a deduplicação até o `deleteAt`. Se o nó já tem a
    /// mensagem, nada muda (o evento do ack, repetido, a trata). Idempotente.
    pub fn apply_tombstone(
        &self,
        topic: &str,
        mut meta: MessageMeta,
        delete_at_ms: u64,
    ) -> Result<Loaded, TabaError> {
        let Ok(view) = self.topics.get(topic) else {
            return Ok(Loaded::UnknownTopic);
        };
        let now = self.clock.now_millis();
        if delete_at_ms <= now {
            return Ok(Loaded::Expired);
        }
        meta.position = self.position_for(topic, &meta.id, meta.group_id.as_deref());
        let state = self.state_of(&view.definition.name);
        let mut state = lock(&state);
        if state.index.contains(&meta.id) || state.replicas.contains(&meta.id) {
            return Ok(Loaded::Existing);
        }
        if let (Some(group), Some(key)) = (&meta.group_id, &meta.deduplication_id) {
            state
                .dedup
                .entry((group.clone(), key.clone()))
                .or_insert((meta.id.clone(), meta.published_ms));
        }
        if let Some(group) = &meta.group_id {
            let last = state.group_ms.entry(group.clone()).or_default();
            *last = (*last).max(meta.published_ms);
        }
        self.store
            .tombstone(topic, &meta, delete_at_ms)
            .map_err(|e| self.disk_error("falha ao gravar a linha de deduplicação", &e))?;
        Ok(Loaded::Stored)
    }

    /// O nó guarda a mensagem (no índice ou entre as réplicas)?
    pub fn holds(&self, topic: &str, id: &str) -> bool {
        let state = self.state_of(topic);
        let state = lock(&state);
        state.index.contains(id) || state.replicas.contains(id)
    }

    /// Apaga as cópias que o nó guarda e de que deixou de ser responsável (doc 05, seção 7.3,
    /// passo 7): `responsible` diz, pela posição no anel, se o nó ainda é um dos nós do bloco. Só
    /// mexe nas cópias que não são do dono (as do dono, no índice, são sempre do bloco dele).
    /// Devolve quantas apagou.
    pub fn forget_foreign(&self, responsible: &dyn Fn(u64) -> bool) -> usize {
        let mut removed = 0;
        for (name, state) in self.existing_states() {
            let mut state = lock(&state);
            for id in state.replicas.ids() {
                let held = self.held_meta(&state, &name, &id);
                let Some(meta) = held else {
                    // A cópia sumiu do armazenamento: só sobrou o registro dela.
                    state.replicas.remove(&id);
                    continue;
                };
                if responsible(meta.position) {
                    continue;
                }
                let place = if state.memory.contains_key(&id) {
                    Place::Memory
                } else {
                    Place::Disk
                };
                match self.remove_message(&mut state, &name, &id, place) {
                    Ok(()) => removed += 1,
                    Err(_) => {
                        tracing::error!(topic = %name, id = %id, "falha ao apagar a cópia que deixou de ser do nó");
                    }
                }
            }
            self.ready_changed(&name, &mut state);
        }
        removed
    }

    /// Os metadados da cópia, onde quer que ela esteja.
    pub(super) fn held_meta(
        &self,
        state: &TopicState,
        topic: &str,
        id: &str,
    ) -> Option<MessageMeta> {
        match state.memory.get(id) {
            Some(Stored { meta, .. }) => Some(meta.clone()),
            None => self.store.get(topic, id).ok().flatten(),
        }
    }
}
