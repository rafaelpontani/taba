//! O índice derivado de um tópico (doc 13, seções 12.6 e 12.11; doc 14, seção 13.1): a lista
//! de prontos, a lista de invisíveis e os contadores por grupo. É **só um índice**: a fonte de
//! verdade são os metadados da mensagem, em memória ou no banco do tópico, e o índice é refeito
//! a partir deles quando o tópico é aberto (reinício ou novo dono).
//!
//! **Fila FIFO** (doc 17, seção 16.6). Um grupo é *livre* quando não tem mensagem em processamento
//! e tem prontas, e *ocupado* quando tem uma em voo. A lista de prontas passa a ter **uma entrada
//! por grupo livre**, a cabeça (a mais antiga do grupo), e a entrega tira a cabeça mais antiga
//! entre os grupos livres, sem faixas nem `normal-every`. Como a entregue deixa o grupo ocupado, o
//! mesmo lote nunca traz duas mensagens do mesmo grupo; o ack, o vencimento da invisibilidade, o
//! `ttl` e a dead-letter da cabeça liberam o grupo para a seguinte, na ordem do `id`.
//!
//! Sem I/O e sem relógio próprio (quem chama passa o `now`), para ser testado sozinho.
//!
//! **Entrega justa.** Cada grupo tem duas contagens, as **prontas** e as **em processamento**
//! (invisíveis). O tamanho é `em processamento + peso × prontas`, e a **faixa** é
//! `⌊log2(tamanho)⌋` (tamanho menor que 1 conta como 1). A entrega tira da menor faixa que tem
//! mensagem pronta; dentro dela alterna a mais antiga e a mais nova (efeito sanfona). A cada
//! `normal-every` entregas uma pega a mais antiga de todas, de qualquer grupo, para o grupo
//! grande nunca esperar sem limite. Como a faixa é recalculada a cada escolha, o "ID que mudou
//! de faixa" nunca fica velho na lista.

use std::collections::{BTreeSet, HashMap};

/// A chave de um grupo. Os grupos acima de `delivery.fair.max-groups` dividem um contador só.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GroupKey {
    /// O `groupId`; a mensagem sem grupo é o grupo de nome vazio.
    Named(String),
    Overflow,
}

#[derive(Debug, Clone, Copy)]
pub struct IndexRules {
    /// `delivery.fair.ready-weight`, de 0 a 1.
    pub ready_weight: f64,
    /// `delivery.fair.max-groups`; `None`, sem teto.
    pub max_groups: Option<usize>,
    /// `delivery.fair.normal-every`; `0` desliga a garantia.
    pub normal_every: u32,
    /// Fila FIFO: um grupo, uma mensagem em voo; a entrega justa não vale.
    pub fifo: bool,
}

#[derive(Default)]
struct Group {
    ready: BTreeSet<String>,
    processing: u64,
}

struct Entry {
    group: GroupKey,
    /// Fim da invisibilidade; só vale com `processing`.
    until_ms: u64,
    age_ms: u64,
    deliveries: u32,
    processing: bool,
}

pub struct TopicIndex {
    rules: IndexRules,
    entries: HashMap<String, Entry>,
    groups: HashMap<GroupKey, Group>,
    /// Os grupos com alguma pronta, por faixa.
    bands: BTreeSet<(u32, GroupKey)>,
    ready_all: BTreeSet<String>,
    /// As invisíveis, por prazo.
    invisible: BTreeSet<(u64, String)>,
    /// Todas, por idade (o `ttl` conta daqui).
    by_age: BTreeSet<(u64, String)>,
    picks: u64,
    newest_next: bool,
    /// Só na FIFO: a cabeça de cada grupo livre.
    free_heads: BTreeSet<String>,
    head_of: HashMap<GroupKey, String>,
}

/// `⌊log2(tamanho)⌋`, com tamanho menor que 1 contado como 1.
pub fn band(size: f64) -> u32 {
    let size = size.max(1.0);
    size.log2().floor() as u32
}

impl TopicIndex {
    pub fn new(rules: IndexRules) -> Self {
        Self {
            rules,
            entries: HashMap::new(),
            groups: HashMap::new(),
            bands: BTreeSet::new(),
            ready_all: BTreeSet::new(),
            invisible: BTreeSet::new(),
            by_age: BTreeSet::new(),
            picks: 0,
            newest_next: false,
            free_heads: BTreeSet::new(),
            head_of: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    #[cfg(test)]
    /// Prontas e em processamento do tópico todo.
    pub fn counts(&self) -> (usize, usize) {
        (self.ready_all.len(), self.invisible.len())
    }

    #[cfg(test)]
    /// Prontas e em processamento de um grupo.
    pub fn group_counts(&self, key: &GroupKey) -> Option<(usize, u64)> {
        self.groups.get(key).map(|g| (g.ready.len(), g.processing))
    }

    #[cfg(test)]
    /// A faixa atual do grupo (`None` se não existe).
    pub fn group_band(&self, key: &GroupKey) -> Option<u32> {
        self.groups.get(key).map(|g| self.band_of(g))
    }

    #[cfg(test)]
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// A chave com que o `groupId` é contado: os grupos novos acima do teto vão para o contador
    /// compartilhado.
    pub fn key_for(&self, group: Option<&str>) -> GroupKey {
        let named = GroupKey::Named(group.unwrap_or_default().to_owned());
        // Na FIFO o grupo serializa a entrega: dois grupos num contador só se atrapalhariam.
        let Some(max) = self.rules.max_groups.filter(|_| !self.rules.fifo) else {
            return named;
        };
        let tracked =
            self.groups.len() - usize::from(self.groups.contains_key(&GroupKey::Overflow));
        if self.groups.contains_key(&named) || tracked < max {
            named
        } else {
            GroupKey::Overflow
        }
    }

    #[cfg(test)]
    pub fn entry_group(&self, id: &str) -> Option<&GroupKey> {
        self.entries.get(id).map(|e| &e.group)
    }

    pub fn deliveries(&self, id: &str) -> Option<u32> {
        self.entries.get(id).map(|e| e.deliveries)
    }

    fn band_of(&self, group: &Group) -> u32 {
        band(group.processing as f64 + self.rules.ready_weight * group.ready.len() as f64)
    }

    /// Aplica `change` ao grupo mantendo a posição dele na tabela de faixas e apagando o grupo
    /// que ficou sem nada.
    fn mutate(&mut self, key: &GroupKey, change: impl FnOnce(&mut Group)) {
        let mut group = self.groups.remove(key).unwrap_or_default();
        if !group.ready.is_empty() {
            self.bands.remove(&(self.band_of(&group), key.clone()));
        }
        change(&mut group);
        if !group.ready.is_empty() {
            self.bands.insert((self.band_of(&group), key.clone()));
        }
        if self.rules.fifo {
            self.refresh_head(key, &group);
        }
        if !(group.ready.is_empty() && group.processing == 0) {
            self.groups.insert(key.clone(), group);
        }
    }

    /// Mantém a lista de grupos livres: a cabeça do grupo só está nela se o grupo não tem
    /// mensagem em processamento.
    fn refresh_head(&mut self, key: &GroupKey, group: &Group) {
        if let Some(old) = self.head_of.remove(key) {
            self.free_heads.remove(&old);
        }
        if group.processing == 0
            && let Some(head) = group.ready.first()
        {
            self.free_heads.insert(head.clone());
            self.head_of.insert(key.clone(), head.clone());
        }
    }

    fn make_ready(&mut self, id: &str, key: &GroupKey) {
        self.ready_all.insert(id.to_owned());
        self.mutate(key, |g| {
            g.ready.insert(id.to_owned());
        });
    }

    fn make_processing(&mut self, id: &str, key: &GroupKey, until_ms: u64) {
        self.invisible.insert((until_ms, id.to_owned()));
        self.mutate(key, |g| g.processing += 1);
    }

    /// Passa a mensagem a conhecida do índice: pronta, ou em processamento se `until_ms` ainda
    /// não chegou. Repetir o mesmo `id` troca o que havia.
    pub fn insert(
        &mut self,
        id: &str,
        group: Option<&str>,
        age_ms: u64,
        until_ms: u64,
        deliveries: u32,
        now_ms: u64,
    ) {
        self.remove(id);
        let key = self.key_for(group);
        let processing = until_ms > now_ms;
        self.entries.insert(
            id.to_owned(),
            Entry {
                group: key.clone(),
                until_ms,
                age_ms,
                deliveries,
                processing,
            },
        );
        self.by_age.insert((age_ms, id.to_owned()));
        if processing {
            self.make_processing(id, &key, until_ms);
        } else {
            self.make_ready(id, &key);
        }
    }

    /// Tira a mensagem do índice (ack, `ttl`, dead-letter). `false` se não estava.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(entry) = self.entries.remove(id) else {
            return false;
        };
        self.by_age.remove(&(entry.age_ms, id.to_owned()));
        if entry.processing {
            self.invisible.remove(&(entry.until_ms, id.to_owned()));
            self.mutate(&entry.group, |g| g.processing -= 1);
        } else {
            self.ready_all.remove(id);
            self.mutate(&entry.group, |g| {
                g.ready.remove(id);
            });
        }
        true
    }

    /// Muda o fim da invisibilidade (entrega, `PUT visibility`). Com `until_ms` no futuro a
    /// mensagem fica em processamento; com `until_ms ≤ now` volta a pronta.
    pub fn set_until(&mut self, id: &str, until_ms: u64, now_ms: u64) {
        let Some(entry) = self.entries.get(id) else {
            return;
        };
        let (key, was_processing, old_until) =
            (entry.group.clone(), entry.processing, entry.until_ms);
        let now_processing = until_ms > now_ms;
        if was_processing {
            self.invisible.remove(&(old_until, id.to_owned()));
        }
        match (was_processing, now_processing) {
            (false, true) => {
                self.ready_all.remove(id);
                self.mutate(&key, |g| {
                    g.ready.remove(id);
                });
                self.make_processing(id, &key, until_ms);
            }
            (true, true) => {
                self.invisible.insert((until_ms, id.to_owned()));
            }
            (true, false) => {
                self.mutate(&key, |g| g.processing -= 1);
                self.make_ready(id, &key);
            }
            (false, false) => {}
        }
        if let Some(entry) = self.entries.get_mut(id) {
            entry.processing = now_processing;
            entry.until_ms = if now_processing { until_ms } else { 0 };
        }
    }

    /// Registra a entrega: mais uma no contador e o novo fim da invisibilidade.
    pub fn deliver(&mut self, id: &str, until_ms: u64, now_ms: u64, deliveries: u32) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.deliveries = deliveries;
        }
        self.set_until(id, until_ms, now_ms);
    }

    /// Devolve a prontas as mensagens cuja invisibilidade venceu, e quais foram. Não se
    /// propaga nada: é só o índice acompanhando os metadados.
    pub fn release_due(&mut self, now_ms: u64) -> Vec<String> {
        let mut released = Vec::new();
        while let Some((until, id)) = self.invisible.first().cloned() {
            if until > now_ms {
                break;
            }
            self.set_until(&id, 0, now_ms);
            released.push(id);
        }
        released
    }

    /// As mensagens cujo `ttl` venceu: `idade + ttl ≤ agora`.
    pub fn expired(&self, now_ms: u64, ttl_ms: u64) -> Vec<String> {
        let Some(limit) = now_ms.checked_sub(ttl_ms) else {
            return Vec::new();
        };
        self.by_age
            .iter()
            .take_while(|(age, _)| *age <= limit)
            .map(|(_, id)| id.clone())
            .collect()
    }

    /// Quando a próxima invisível volta a ser entregável.
    pub fn next_visible(&self) -> Option<u64> {
        self.invisible.first().map(|(until, _)| *until)
    }

    pub fn has_ready(&self) -> bool {
        if self.rules.fifo {
            return !self.free_heads.is_empty();
        }
        !self.ready_all.is_empty()
    }

    /// O próximo ID a entregar (sem tirá-lo da lista de prontas; quem entrega chama
    /// [`TopicIndex::deliver`]). `None` se não há pronta.
    pub fn pick(&mut self) -> Option<String> {
        if self.rules.fifo {
            return self.free_heads.first().cloned();
        }
        let every = u64::from(self.rules.normal_every);
        let normal = every > 0 && (self.picks + 1).is_multiple_of(every);
        self.picks += 1;
        if normal {
            return self.ready_all.first().cloned();
        }
        let lowest = self.bands.first()?.0;
        let in_band = self
            .bands
            .range((lowest, GroupKey::Named(String::new()))..)
            .take_while(|(b, _)| *b == lowest)
            .filter_map(|(_, key)| self.groups.get(key));
        let newest = self.newest_next;
        self.newest_next = !self.newest_next;
        if newest {
            in_band.filter_map(|g| g.ready.last()).max().cloned()
        } else {
            in_band.filter_map(|g| g.ready.first()).min().cloned()
        }
    }

    /// A mensagem está em processamento (invisível, já entregue e sem ack)?
    pub fn is_processing(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|e| e.processing)
    }

    /// As prontas que já foram entregues alguma vez, com o número de entregas: as candidatas à
    /// dead-letter quando o `maxDeliveries` as alcança.
    pub fn ready_delivered(&self) -> Vec<(String, u32)> {
        self.entries
            .iter()
            .filter(|(_, e)| !e.processing && e.deliveries > 0)
            .map(|(id, e)| (id.clone(), e.deliveries))
            .collect()
    }

    /// Os IDs prontos, em ordem de `id` (para os testes).
    #[cfg(test)]
    pub fn ready_ids(&self) -> Vec<String> {
        self.ready_all.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests;
