//! Publicar e consumir mensagens (doc 06a, "Mensagens"; doc 13).
//!
//! **Onde a mensagem mora.** Em memória, enquanto couber em `memory.limit`; o que passa do
//! limite (ou chega durante o drain) vai para o disco: o corpo em `${topic}/${id}.body` e o
//! resto numa linha do banco do tópico (doc 02, seção 3.2, `storage::messages`). Cada
//! mensagem está em **um só** lugar, e as operações (entrega, ack, invisibilidade, `ttl`,
//! dead-letter) funcionam igual nos dois. Quando a memória enche, a divisão entre os tópicos
//! segue a taxa de leitura de cada um, com uma reserva por tópico (`rates`, `reserve_memory`).
//!
//! **Um dono por tópico.** Cada tópico tem o seu estado atrás de uma trava própria: uma
//! operação de um tópico (inclusive a de disco) nunca espera a de outro. É o escritor único por
//! tópico do doc 02 e do doc 18 (seção 17.4), como trava em vez de task com fila de comandos.
//! A única operação que olha dois tópicos de uma vez é a ida à dead-letter, sempre da fila de
//! origem para a dead-letter; o rebaixamento de memória só usa `try_lock` nos outros.
//!
//! **Índice.** Cada tópico mantém um índice derivado (`index`): a lista de prontos, a de
//! invisíveis, a idade (para o `ttl`) e os contadores por grupo da entrega justa (doc 13,
//! seção 12.11). Ele é refeito dos metadados (memória e linhas do banco) quando o tópico é
//! aberto, então a perda dele nunca perde mensagem.
//!
//! **Entrega.** O `GET` pega as mensagens prontas pelo índice (a justiça entre grupos e o efeito
//! sanfona), incrementa o `deliveryCount`, troca o `receiptToken` e as deixa invisíveis por
//! `visibilityTimeout`. Passado o prazo a mensagem volta a ser entregável (*at-least-once*). O
//! ack e o `PUT visibility` conferem o `receiptToken` da última entrega. O `ttl` e o
//! `maxDeliveries` (com a dead-letter) são aplicados por `settle`, antes de cada operação e
//! pela varredura de fundo. Os `GET` em espera (long polling) são atendidos na ordem de chegada.
//!
//! **Fila FIFO** (doc 17). A entrega serializa por grupo (`index`): uma mensagem em voo por grupo,
//! na ordem do `id`. A publicação é deduplicada por `(grupo, deduplicationId)` dentro de
//! `fifo.dedup.window`, e o ack não apaga a linha: deixa uma linha curta com `deleteAt` para a
//! deduplicação valer pela janela inteira.
//!
//! Os métodos são síncronos e bloqueiam no disco:
//! quem chama de código assíncrono usa `spawn_blocking`.

mod index;
mod rates;
pub mod stats;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::content::{Body, ContentType, DEFAULT_CONTENT_TYPE};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id::IdGenerator;
use crate::core::validation;
use crate::engine::topics::{CreateTopic, Kind, TopicView, Topics};
use crate::storage;
use crate::storage::messages::{MessageMeta, MessageStore};
use index::{GroupKey, IndexRules, TopicIndex};
use rates::Rates;
use sha2::{Digest, Sha256};
use stats::{
    DistributorCounters, GroupSnapshot, NodeCounters, NodeObservation, Percentiles, StatsRules,
    TopicCounters, TopicSnapshot,
};

/// `waitTime` do `GET`: de 0 a 20 segundos (doc 12).
const WAIT_TIME_SECS: (u64, u64) = (0, 20);

/// Quantos `GET` em espera, no máximo, um evento acorda de uma vez.
const MAX_WAKE: usize = 16;

/// Acima disto, a publicação na FIFO poda do mapa de deduplicação o que já saiu da janela.
const DEDUP_PRUNE_ABOVE: usize = 10_000;

/// O SHA-256 dos bytes do corpo, em hexadecimal (a chave de deduplicação padrão, doc 17).
fn body_hash(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Os valores de configuração de que o consumo e a publicação dependem (doc 10).
#[derive(Debug, Clone)]
pub struct MessageRules {
    /// `message.max-bytes`: vale num distribuidor e nas filas sem `maxMessageBytes`.
    pub max_bytes: u64,
    /// `delivery.max-messages.default` e `.max` (sem teto, se não houver `.max`).
    pub max_messages_default: u64,
    pub max_messages_max: Option<u64>,
    /// `delivery.visibility-timeout.default` e `.max`, em segundos.
    pub visibility_default_secs: u64,
    pub visibility_max_secs: Option<u64>,
    /// `topics.distributor.max-targets`.
    pub distributor_max_targets: Option<u64>,
    /// `retention.sweep.interval`: de quanto em quanto tempo a varredura apaga o vencido.
    pub sweep_interval: Duration,
    /// `memory.limit`: bytes de corpo em memória; sem ele, nada vai para o disco por overflow.
    pub memory_limit: Option<u64>,
    /// `memory.rate.window`, em segundos: a janela da taxa de leitura.
    pub rate_window_secs: u64,
    /// `memory.topic.reserve`: mensagens em memória garantidas a cada tópico.
    pub topic_reserve: u64,
    /// `delivery.fair.ready-weight`, `.max-groups` e `.normal-every` (doc 13, seção 12.11).
    pub fair_ready_weight: f64,
    pub fair_max_groups: Option<usize>,
    pub fair_normal_every: u32,
    /// `fifo.dedup.window`, em milissegundos.
    pub fifo_dedup_window_ms: u64,
}

impl MessageRules {
    pub fn from_config(config: &Config) -> Self {
        Self {
            max_bytes: config.messages.max_bytes,
            max_messages_default: u64::from(config.delivery.max_messages_default),
            max_messages_max: config.delivery.max_messages_max.map(u64::from),
            visibility_default_secs: config.delivery.visibility_timeout_default.as_secs(),
            visibility_max_secs: config.delivery.visibility_timeout_max.map(|d| d.as_secs()),
            distributor_max_targets: config.topics.distributor_max_targets.map(u64::from),
            sweep_interval: config.retention.sweep_interval,
            memory_limit: config.storage.memory_limit,
            rate_window_secs: config.storage.rate_window.as_secs().max(1),
            topic_reserve: config.storage.topic_reserve,
            fair_ready_weight: config.delivery.fair_ready_weight,
            fair_max_groups: config.delivery.fair_max_groups.map(|n| n as usize),
            fair_normal_every: config.delivery.fair_normal_every,
            fifo_dedup_window_ms: u64::try_from(config.fifo.dedup_window.as_millis())
                .unwrap_or(u64::MAX),
        }
    }

    fn index_rules(&self, fifo: bool) -> IndexRules {
        IndexRules {
            ready_weight: self.fair_ready_weight,
            max_groups: self.fair_max_groups,
            normal_every: self.fair_normal_every,
            fifo,
        }
    }
}

/// `POST /topics/{topic}/messages`: o corpo são os bytes da mensagem; o resto vem do
/// `Content-Type` e da query.
#[derive(Debug, Clone, Default)]
pub struct PublishRequest {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub group_id: Option<String>,
    pub deduplication_id: Option<String>,
}

/// O resultado de uma publicação.
#[derive(Debug)]
pub enum Published {
    /// Numa fila: o `id` da mensagem criada (ou, numa duplicata, o da original). `duplicate` só
    /// existe na fila FIFO.
    Queue { id: String, duplicate: Option<bool> },
    /// Num distribuidor: uma cópia por fila que casou.
    Distributor {
        deliveries: Vec<DistributorDelivery>,
    },
}

#[derive(Debug)]
pub struct DistributorDelivery {
    pub topic: String,
    /// O `id` da cópia, ou o erro daquela fila.
    pub result: Result<String, TabaError>,
}

/// Os parâmetros de query de `GET /topics/{topic}/messages`, como vieram, para o `engine`
/// validar.
#[derive(Debug, Clone, Default)]
pub struct ConsumeQuery {
    pub max_messages: Option<String>,
    pub visibility_timeout: Option<String>,
    pub wait_time: Option<String>,
    /// `autoCreate`: cria a fila se ela não existir (doc 15, seção 14.4). Os quatro campos
    /// seguintes só valem na criação.
    pub auto_create: Option<String>,
    pub volatile: Option<String>,
    pub idle_timeout: Option<String>,
    pub fifo: Option<String>,
    /// `tag.<chave>=<valor>`, sem o prefixo `tag.`.
    pub tags: BTreeMap<String, String>,
    /// O chamador tem a permissão `create` (sem autenticação, sempre): o `autoCreate` que
    /// criaria a fila sem ela é `403`.
    pub can_create: bool,
}

/// Uma mensagem entregue, como o `Message` da API (doc 06a, "DTOs").
#[derive(Debug)]
pub struct Delivered {
    pub id: String,
    pub body: Body,
    pub content_type: String,
    pub group_id: Option<String>,
    /// Só na fila FIFO: a chave de deduplicação (a informada, ou o hash do corpo).
    pub deduplication_id: Option<String>,
    pub receipt_token: String,
    pub delivery_count: u32,
    pub published_ms: u64,
    pub invisible_until_ms: u64,
    /// Só na dead-letter: quando a mensagem entrou nela e por quê.
    pub dead_lettered_ms: Option<u64>,
    pub dead_letter_reason: Option<String>,
}

/// O resultado de `PUT .../visibility`.
#[derive(Debug)]
pub struct VisibilityUpdated {
    pub id: String,
    pub invisible_until_ms: u64,
}

/// O que o drain fez (doc 04, seção 6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    /// Mensagens que saíram da memória para o disco.
    pub moved: u64,
    /// Mensagens que ficaram em memória por erro de disco.
    pub failed: u64,
    /// Mensagens ainda em memória ao terminar (as `failed` e as que a parada antecipada deixou).
    pub remaining: u64,
}

/// O que `NodeStatus.storage` mostra do disco e da memória (doc 06b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageStats {
    pub memory_bytes: u64,
    /// `memory.limit`; `None` sem limite.
    pub memory_limit_bytes: Option<u64>,
    pub memory_messages: u64,
    pub disk_messages: u64,
}

/// Uma mensagem em memória: os metadados são os mesmos da linha do banco.
struct Stored {
    meta: MessageMeta,
    body: Vec<u8>,
}

/// Onde uma mensagem está.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Memory,
    Disk,
}

/// O estado de um tópico, atrás da trava do tópico.
struct TopicState {
    /// Em ordem de `id`: o GUID v7 em minúsculas ordena pelo instante da publicação.
    memory: BTreeMap<String, Stored>,
    /// Tudo o que o tópico tem (memória e disco), derivado dos metadados.
    index: TopicIndex,
    rates: Rates,
    /// Só na fila FIFO: `(grupo, deduplicationId)` → `(id da original, instante do id)`. Vale até
    /// a janela passar, inclusive depois do ack.
    dedup: HashMap<(String, String), (String, u64)>,
    /// O `maxDeliveries` com que as prontas foram conferidas por último: se o `PATCH` o mudar,
    /// as que já passam do novo limite vão à dead-letter.
    checked_max_deliveries: Option<Option<u64>>,
    /// O que o tópico conta desde que o nó o abriu (doc 11).
    stats: TopicCounters,
}

fn message_not_found() -> TabaError {
    TabaError::new(ErrorCode::MessageNotFound, "A mensagem não existe.")
}

fn stale_receipt() -> TabaError {
    TabaError::new(
        ErrorCode::MessageStaleReceipt,
        "O receiptToken não é o da última entrega desta mensagem.",
    )
}

/// Chamado quando um erro de disco derruba o nó (disco cheio, banco corrompido).
pub type FailureHook = Arc<dyn Fn() + Send + Sync>;

/// `validation.required`, com cada campo que falta em `details`.
fn required(fields: &[&str]) -> TabaError {
    fields.iter().fold(
        TabaError::new(ErrorCode::ValidationRequired, "Faltam campos obrigatórios."),
        |error, field| {
            error.with_detail(*field, ErrorCode::ValidationRequired, "Campo obrigatório.")
        },
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `autoCreate=true` na query? Valor que não é `true` nem `false` é `400`.
fn wants_auto_create(query: &ConsumeQuery) -> Result<bool, TabaError> {
    Ok(optional_bool("autoCreate", query.auto_create.as_deref())?.unwrap_or(false))
}

fn query_failed(field: &str, message: &str) -> TabaError {
    TabaError::new(
        ErrorCode::ValidationFailed,
        "A requisição tem campos inválidos.",
    )
    .with_detail(field, ErrorCode::ValidationFailed, message)
}

fn optional_bool(field: &str, text: Option<&str>) -> Result<Option<bool>, TabaError> {
    match text {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(_) => Err(query_failed(field, "Use true ou false.")),
    }
}

fn optional_int(field: &str, text: Option<&str>) -> Result<Option<i64>, TabaError> {
    match text {
        None => Ok(None),
        Some(t) => t
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| query_failed(field, "Esperado um número inteiro.")),
    }
}

fn wrong_type() -> TabaError {
    TabaError::new(
        ErrorCode::TopicWrongType,
        "Um tópico distribuidor não é consumido.",
    )
}

fn too_large(limit: u64) -> TabaError {
    TabaError::new(
        ErrorCode::MessageTooLarge,
        format!("O corpo passa do limite de {limit} bytes do tópico."),
    )
}

/// Um inteiro de query dentro da faixa: texto que não é inteiro é `validation.failed`, e
/// inteiro fora da faixa (inclusive negativo) é `validation.out-of-range`.
fn query_number(field: &str, text: &str, range: (u64, u64)) -> Result<u64, TabaError> {
    let Ok(value) = text.trim().parse::<i64>() else {
        let message = "Esperado um número inteiro.";
        return Err(TabaError::new(
            ErrorCode::ValidationFailed,
            "A requisição tem campos inválidos.",
        )
        .with_detail(field, ErrorCode::ValidationFailed, message));
    };
    validation::in_range_signed(field, value, range)
}

/// O que um `take` devolve.
struct Taken {
    delivered: Vec<Delivered>,
    /// Quando a próxima invisível volta a ser entregável, para o long polling saber até quando
    /// dormir.
    next_visible_ms: Option<u64>,
    /// Ainda há mensagem pronta: o próximo ouvinte em espera deve acordar.
    more_ready: bool,
}

pub struct Messages {
    topics: Arc<Topics>,
    clock: Arc<dyn Clock>,
    rules: MessageRules,
    ids: IdGenerator,
    store: Arc<MessageStore>,
    /// O estado de cada tópico, criado (e o índice refeito do disco) no primeiro uso.
    states: RwLock<HashMap<String, Arc<Mutex<TopicState>>>>,
    /// Bytes de corpo em memória, de todos os tópicos, contra `memory.limit`.
    memory_bytes: AtomicU64,
    /// No drain o limite de memória efetivo é zero: toda mensagem nova vai para o disco.
    draining: AtomicBool,
    /// Acorda os `GET` em espera (long polling) quando chega mensagem ou o tópico some.
    notifiers: Mutex<HashMap<String, Arc<Notify>>>,
    /// Avisa o nó de que o disco falhou de um jeito que o derruba (estado `failed`).
    on_failure: OnceLock<FailureHook>,
    /// O instante do último `GET` de cada fila, por `id` do tópico (o nome pode ser reusado por
    /// uma fila recriada). Vale para o relógio de ociosidade das filas voláteis.
    last_get: Mutex<HashMap<String, u64>>,
    /// A partida do nó, em milissegundos: o relógio de ociosidade nunca conta o tempo parado.
    started_ms: u64,
    /// O que o nó todo conta (doc 11): lido por `GET /metrics`.
    counters: NodeCounters,
    /// O que cada distribuidor conta, por nome.
    distributors: Mutex<HashMap<String, DistributorCounters>>,
}

/// Fecha a espera de um `GET` em long polling: se o chamador desistiu antes do fim (a conexão
/// caiu e o futuro foi descartado), conta como `disconnected`.
struct LongPoll<'a> {
    counters: &'a NodeCounters,
    /// `Some(true)`: terminou entregando; `Some(false)`: terminou vazio; `None`: foi descartado.
    delivered: Option<bool>,
}

impl<'a> LongPoll<'a> {
    fn start(counters: &'a NodeCounters) -> Self {
        counters.longpoll_waiting.fetch_add(1, Ordering::Relaxed);
        Self {
            counters,
            delivered: None,
        }
    }
}

impl Drop for LongPoll<'_> {
    fn drop(&mut self) {
        self.counters
            .longpoll_waiting
            .fetch_sub(1, Ordering::Relaxed);
        let counter = match self.delivered {
            Some(true) => &self.counters.longpoll_delivered,
            Some(false) => &self.counters.longpoll_empty,
            None => &self.counters.longpoll_disconnected,
        };
        NodeCounters::add(counter, 1);
    }
}

impl Messages {
    /// Registra quem é avisado de um erro de disco fatal. Vale só a primeira chamada.
    pub fn on_failure(&self, hook: FailureHook) {
        let _ = self.on_failure.set(hook);
    }

    /// Erro de disco: vira `500`, e o detalhe vai para o log. Disco cheio ou banco corrompido
    /// também avisam o nó, que passa a `failed` (doc 05, seção 7.3).
    fn disk_error(&self, what: &str, e: &io::Error) -> TabaError {
        tracing::error!(error = %e, "{what}");
        NodeCounters::add(&self.counters.disk_errors, 1);
        if storage::is_fatal(e) {
            self.fail();
        }
        TabaError::internal()
    }

    fn fail(&self) {
        if let Some(hook) = self.on_failure.get() {
            hook();
        }
    }

    /// `resume`: retoma as mensagens que estão em disco (desenvolvimento com
    /// `development.resume-on-start`). Sem ele, o que sobrou em disco de uma execução
    /// anterior é apagado: o nó começa vazio. Pastas de tópicos que não existem mais também
    /// são apagadas, para um tópico recriado com o mesmo nome não ressuscitar mensagens.
    pub fn new(
        topics: Arc<Topics>,
        clock: Arc<dyn Clock>,
        rules: MessageRules,
        store: Arc<MessageStore>,
        resume: bool,
    ) -> Arc<Self> {
        match store.topics_on_disk() {
            Ok(on_disk) => {
                for name in on_disk {
                    if !resume || topics.get(&name).is_err() {
                        tracing::info!(topic = %name, "apagando mensagens antigas do disco");
                        if let Err(e) = store.drop_topic(&name) {
                            tracing::warn!(topic = %name, error = %e, "não foi possível apagar");
                        }
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "não foi possível listar as mensagens do disco"),
        }
        let started_ms = clock.now_millis();
        let messages = Arc::new(Self {
            topics: topics.clone(),
            clock,
            rules,
            ids: IdGenerator::new(),
            store,
            states: RwLock::new(HashMap::new()),
            memory_bytes: AtomicU64::new(0),
            draining: AtomicBool::new(false),
            notifiers: Mutex::new(HashMap::new()),
            on_failure: OnceLock::new(),
            last_get: Mutex::new(HashMap::new()),
            started_ms,
            counters: NodeCounters::new(),
            distributors: Mutex::new(HashMap::new()),
        });
        // As mensagens saem junto com o tópico (doc 02, seção 3.3).
        let weak = Arc::downgrade(&messages);
        topics.on_removed(Arc::new(move |names| {
            if let Some(messages) = weak.upgrade() {
                for name in names {
                    messages.drop_topic(name);
                }
            }
        }));
        messages
    }

    // ---- Estado dos tópicos ----

    /// O estado do tópico. No primeiro uso o índice é refeito das linhas do banco do tópico
    /// (reinício ou tópico retomado do disco).
    fn state_of(&self, name: &str) -> Arc<Mutex<TopicState>> {
        if let Some(state) = self
            .states
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
        {
            return state.clone();
        }
        let mut states = self.states.write().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = states.get(name) {
            return state.clone();
        }
        let now = self.clock.now_millis();
        let fifo = self
            .topics
            .get(name)
            .is_ok_and(|view| view.definition.queue().is_some_and(|q| q.fifo));
        let mut index = TopicIndex::new(self.rules.index_rules(fifo));
        let mut dedup = HashMap::new();
        match self.store.scan(name) {
            Ok(rows) => {
                for row in rows {
                    if fifo
                        && row
                            .published_ms
                            .saturating_add(self.rules.fifo_dedup_window_ms)
                            > now
                        && let (Some(group), Some(key)) = (&row.group_id, &row.deduplication_id)
                    {
                        dedup.insert(
                            (group.clone(), key.clone()),
                            (row.id.clone(), row.published_ms),
                        );
                    }
                    // A linha curta do ack só serve à deduplicação.
                    if row.delete_at_ms.is_some() {
                        continue;
                    }
                    index.insert(
                        &row.id,
                        row.group_id.as_deref(),
                        row.age_ms,
                        row.invisible_until_ms,
                        row.delivery_count,
                        now,
                    );
                }
            }
            Err(e) => {
                self.disk_error("falha ao ler o índice do tópico em disco", &e);
            }
        }
        let state = Arc::new(Mutex::new(TopicState {
            memory: BTreeMap::new(),
            index,
            rates: Rates::default(),
            dedup,
            checked_max_deliveries: None,
            stats: TopicCounters::new(),
        }));
        states.insert(name.to_owned(), state.clone());
        state
    }

    fn existing_states(&self) -> Vec<(String, Arc<Mutex<TopicState>>)> {
        self.states
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(name, state)| (name.clone(), state.clone()))
            .collect()
    }

    fn drop_topic(&self, name: &str) {
        let removed = self
            .states
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        if let Some(state) = removed {
            let bytes: u64 = lock(&state)
                .memory
                .values()
                .map(|s| s.body.len() as u64)
                .sum();
            self.memory_bytes.fetch_sub(bytes, Ordering::SeqCst);
        }
        if let Err(e) = self.store.drop_topic(name) {
            tracing::warn!(topic = %name, error = %e, "não foi possível apagar o disco do tópico");
        }
        if let Some(notify) = lock(&self.notifiers).get(name) {
            notify.notify_waiters();
        }
    }

    fn notifier(&self, name: &str) -> Arc<Notify> {
        lock(&self.notifiers)
            .entry(name.to_owned())
            .or_default()
            .clone()
    }

    /// Acorda até `count` `GET` em espera, na ordem em que chegaram (cada `notify_one` leva o
    /// primeiro da fila de ouvintes, doc 13, seção 12.8).
    fn wake(&self, name: &str, count: usize) {
        let notify = self.notifier(name);
        for _ in 0..count.clamp(1, MAX_WAKE) {
            notify.notify_one();
        }
    }

    // ---- Memória ----

    fn add_memory(&self, size: u64) {
        self.memory_bytes.fetch_add(size, Ordering::SeqCst);
    }

    fn sub_memory(&self, size: u64) {
        self.memory_bytes.fetch_sub(size, Ordering::SeqCst);
    }

    /// Reserva `size` bytes sem passar do limite. `false` se não cabe.
    fn try_reserve(&self, size: u64, limit: u64) -> bool {
        self.memory_bytes
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
                used.checked_add(size).filter(|total| *total <= limit)
            })
            .is_ok()
    }

    /// Tira a mensagem da memória do tópico, devolvendo os bytes ao total.
    fn take_from_memory(&self, state: &mut TopicState, id: &str) -> Option<Stored> {
        let stored = state.memory.remove(id)?;
        self.sub_memory(stored.body.len() as u64);
        Some(stored)
    }

    fn put_in_memory(&self, state: &mut TopicState, stored: Stored) {
        self.add_memory(stored.body.len() as u64);
        if let Some(previous) = state.memory.insert(stored.meta.id.clone(), stored) {
            self.sub_memory(previous.body.len() as u64);
        }
    }

    /// Decide se a mensagem nova de `size` bytes fica em memória (e já reserva o espaço).
    ///
    /// Com a memória cheia (doc 02, seção 3.2): o tópico dentro da sua **reserva** abre espaço
    /// rebaixando as mensagens que os outros guardam acima da reserva deles, começando pelo de
    /// menor taxa de leitura; o tópico que já passou da reserva só rebaixa os de taxa **menor**
    /// que a dele. Não achando espaço, a mensagem vai para o disco.
    fn reserve_memory(&self, state: &TopicState, name: &str, size: u64, now: u64) -> bool {
        if self.draining.load(Ordering::SeqCst) {
            return false;
        }
        let Some(limit) = self.rules.memory_limit else {
            self.add_memory(size);
            return true;
        };
        if self.try_reserve(size, limit) {
            return true;
        }
        let within_reserve = (state.memory.len() as u64) < self.rules.topic_reserve;
        let below_rate = (!within_reserve).then(|| self.consume_rate(state, now));
        let used = self.memory_bytes.load(Ordering::SeqCst);
        let need = (used + size).saturating_sub(limit);
        self.demote(name, need, below_rate, now);
        self.try_reserve(size, limit)
    }

    fn consume_rate(&self, state: &TopicState, now: u64) -> f64 {
        state
            .rates
            .consumed
            .per_second(now, self.rules.rate_window_secs)
    }

    /// Rebaixa para o disco ao menos `need` bytes de outros tópicos, acima da reserva deles. Só
    /// usa `try_lock`: um tópico ocupado é pulado, o que evita travar um no outro.
    fn demote(&self, exclude: &str, need: u64, below_rate: Option<f64>, now: u64) {
        let mut victims: Vec<(f64, String, Arc<Mutex<TopicState>>)> = Vec::new();
        for (name, state) in self.existing_states() {
            if name == exclude {
                continue;
            }
            let Ok(guard) = state.try_lock() else {
                continue;
            };
            if (guard.memory.len() as u64) <= self.rules.topic_reserve {
                continue;
            }
            let rate = self.consume_rate(&guard, now);
            if below_rate.is_some_and(|mine| rate >= mine) {
                continue;
            }
            drop(guard);
            victims.push((rate, name, state));
        }
        victims.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let mut freed = 0;
        for (_, name, state) in victims {
            let Ok(mut guard) = state.try_lock() else {
                continue;
            };
            while freed < need && (guard.memory.len() as u64) > self.rules.topic_reserve {
                let Some(id) = Self::demotion_candidate(&guard) else {
                    break;
                };
                let Some(stored) = self.take_from_memory(&mut guard, &id) else {
                    break;
                };
                match self.store.put(&name, &stored.meta, &stored.body) {
                    Ok(()) => {
                        freed += stored.body.len() as u64;
                        NodeCounters::add(&self.counters.overflow, 1);
                    }
                    Err(e) => {
                        self.disk_error("falha ao rebaixar a mensagem para o disco", &e);
                        self.put_in_memory(&mut guard, stored);
                        break;
                    }
                }
            }
            if freed >= need {
                return;
            }
        }
    }

    /// Quem sai da memória primeiro: o que está em processamento (só volta a ser lido depois do
    /// prazo) e, depois, o **meio** da lista de prontas, o último que o efeito sanfona
    /// alcançaria. As pontas ficam: são as próximas a entregar.
    fn demotion_candidate(state: &TopicState) -> Option<String> {
        state
            .memory
            .keys()
            .find(|id| state.index.is_processing(id))
            .or_else(|| state.memory.keys().nth(state.memory.len() / 2))
            .cloned()
    }

    // ---- Publicação ----

    /// O maior corpo que a rota de publicação lê: o `maxMessageBytes` da fila, ou o geral
    /// num distribuidor. Responde `404` se o tópico não existe antes de ler o corpo.
    pub fn publish_limit(&self, topic: &str) -> Result<u64, TabaError> {
        let view = self.topics.get(topic)?;
        Ok(view.max_message_bytes().unwrap_or(self.rules.max_bytes))
    }

    pub fn publish(&self, topic: &str, request: PublishRequest) -> Result<Published, TabaError> {
        let started = Instant::now();
        let view = self.topics.get(topic)?;
        let result = self.publish_checked(&view, &request, started);
        if result.is_err() {
            self.count_publish_error(&view);
        }
        result
    }

    fn publish_checked(
        &self,
        view: &TopicView,
        request: &PublishRequest,
        started: Instant,
    ) -> Result<Published, TabaError> {
        let prepared = Prepared::new(request)?;
        if let Some(prefixes) = view.definition.prefixes() {
            if request.body.len() as u64 > self.rules.max_bytes {
                return Err(too_large(self.rules.max_bytes));
            }
            return self.publish_to_distributor(
                &view.definition.name,
                prefixes,
                request,
                &prepared,
            );
        }
        let limit = view.max_message_bytes().unwrap_or(self.rules.max_bytes);
        if request.body.len() as u64 > limit {
            return Err(too_large(limit));
        }
        self.store(view, request, &prepared, started)
            .map(|(id, duplicate)| Published::Queue { id, duplicate })
    }

    /// Uma publicação que voltou erro ao cliente (validação, tamanho, disco). O distribuidor
    /// não entra: ele conta publicações aceitas e cópias.
    fn count_publish_error(&self, view: &TopicView) {
        if view.definition.queue().is_some() {
            lock(&self.state_of(&view.definition.name))
                .stats
                .publish_errors += 1;
        }
    }

    /// Valida os campos da publicação que dependem do tipo da fila e guarda a mensagem, em
    /// memória ou, se não couber, em disco (primeiro o corpo, depois a linha).
    fn store(
        &self,
        view: &TopicView,
        request: &PublishRequest,
        prepared: &Prepared,
        started: Instant,
    ) -> Result<(String, Option<bool>), TabaError> {
        let fifo = view.definition.queue().is_some_and(|q| q.fifo);
        if fifo && request.group_id.is_none() {
            return Err(TabaError::new(
                ErrorCode::ValidationGroupIdRequired,
                "A fila FIFO exige `groupId`.",
            )
            .with_detail(
                "groupId",
                ErrorCode::ValidationGroupIdRequired,
                "Obrigatório numa fila FIFO.",
            ));
        }
        if !fifo && request.deduplication_id.is_some() {
            return Err(TabaError::new(
                ErrorCode::ValidationNotApplicable,
                "`deduplicationId` só vale numa fila FIFO.",
            )
            .with_detail(
                "deduplicationId",
                ErrorCode::ValidationNotApplicable,
                "Só vale numa fila FIFO.",
            ));
        }
        let now = self.clock.now_millis();
        // Na FIFO toda mensagem tem chave de deduplicação: a informada, ou o hash do corpo.
        let dedup_key = fifo.then(|| {
            (
                request.group_id.clone().unwrap_or_default(),
                request
                    .deduplication_id
                    .clone()
                    .unwrap_or_else(|| body_hash(&request.body)),
            )
        });
        let id = self.ids.new_at(now);
        let name = view.definition.name.clone();
        let meta = MessageMeta {
            id: id.clone(),
            content_type: prepared.content_type.as_str().to_owned(),
            group_id: request.group_id.clone(),
            deduplication_id: dedup_key.as_ref().map(|(_, key)| key.clone()),
            published_ms: now,
            delivery_count: 0,
            invisible_until_ms: 0,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
        };
        let size = request.body.len() as u64;
        {
            let state = self.state_of(&name);
            let mut state = lock(&state);
            // O primário do grupo (aqui, este nó) confere a duplicata antes de criar qualquer coisa.
            if let Some(key) = &dedup_key {
                if let Some((original, at)) = state.dedup.get(key)
                    && at.saturating_add(self.rules.fifo_dedup_window_ms) > now
                {
                    let original = original.clone();
                    state.stats.deduplicated += 1;
                    NodeCounters::add(&self.counters.deduplicated, 1);
                    tracing::debug!(
                        event.name = "taba.message.deduplicated",
                        messaging.destination.name = %name,
                        group_id = %key.0,
                        deduplication_id = %key.1,
                        message.id = %original,
                        "publicação descartada por deduplicação"
                    );
                    return Ok((original, Some(true)));
                }
                if state.dedup.len() > DEDUP_PRUNE_ABOVE {
                    let window = self.rules.fifo_dedup_window_ms;
                    state
                        .dedup
                        .retain(|_, (_, at)| at.saturating_add(window) > now);
                }
            }
            if self.reserve_memory(&state, &name, size, now) {
                state.memory.insert(
                    id.clone(),
                    Stored {
                        meta,
                        body: request.body.clone(),
                    },
                );
            } else {
                self.store
                    .put(&name, &meta, &request.body)
                    .map_err(|e| self.disk_error("falha ao gravar a mensagem em disco", &e))?;
                // No drain toda mensagem vai ao disco: isso não é falta de memória.
                if !self.draining.load(Ordering::SeqCst) {
                    NodeCounters::add(&self.counters.overflow, 1);
                }
            }
            state
                .index
                .insert(&id, request.group_id.as_deref(), now, 0, 0, now);
            if let Some(key) = dedup_key {
                state.dedup.insert(key, (id.clone(), now));
            }
            state
                .rates
                .published
                .record(now, 1, self.rules.rate_window_secs);
            state.stats.published += 1;
            state.stats.published_bytes += size;
            state.stats.last_published_ms = Some(now);
            state
                .stats
                .publish_latency
                .record(started.elapsed().as_secs_f64());
        }
        self.wake(&name, 1);
        Ok((id, fifo.then_some(false)))
    }

    /// Uma cópia independente, com `id` próprio, em cada fila que casa com um prefixo. Zero
    /// filas não é erro: a mensagem é descartada (doc 15, seção 14.2).
    fn publish_to_distributor(
        &self,
        name: &str,
        prefixes: &[String],
        request: &PublishRequest,
        prepared: &Prepared,
    ) -> Result<Published, TabaError> {
        let targets = self.topics.queues_with_prefix(prefixes);
        if let Some(max) = self.rules.distributor_max_targets
            && targets.len() as u64 > max
        {
            return Err(TabaError::new(
                ErrorCode::DistributorTooManyTargets,
                format!("A publicação casa com mais de {max} filas."),
            ));
        }
        let deliveries = targets
            .iter()
            .map(|view| {
                // A chave de deduplicação só vale nas filas FIFO: nas comuns a cópia a ignora.
                let is_fifo = view.definition.queue().is_some_and(|q| q.fifo);
                let copy;
                let request = if is_fifo || request.deduplication_id.is_none() {
                    request
                } else {
                    copy = PublishRequest {
                        deduplication_id: None,
                        ..request.clone()
                    };
                    &copy
                };
                DistributorDelivery {
                    topic: view.definition.name.clone(),
                    result: self
                        .store(view, request, prepared, Instant::now())
                        .map(|(id, _)| id),
                }
            })
            .collect::<Vec<_>>();
        let failed = deliveries.iter().filter(|d| d.result.is_err()).count() as u64;
        {
            let mut distributors = lock(&self.distributors);
            let counters = distributors.entry(name.to_owned()).or_default();
            counters.received += 1;
            counters.copies += deliveries.len() as u64 - failed;
            counters.copy_errors += failed;
        }
        if deliveries.is_empty() {
            tracing::info!(
                event.name = "taba.distributor.no_target",
                messaging.destination.name = %name,
                "publicação sem nenhuma fila que case: a mensagem foi descartada"
            );
        } else if failed > 0 {
            tracing::warn!(
                event.name = "taba.distributor.partial",
                messaging.destination.name = %name,
                failed,
                copies = deliveries.len(),
                "ao menos uma cópia da publicação falhou"
            );
        }
        Ok(Published::Distributor { deliveries })
    }

    // ---- Ack e invisibilidade ----

    /// A fila de um tópico que aceita ack e `PUT visibility`: `404` se não existe, `409` se
    /// é um distribuidor.
    fn queue_view(&self, topic: &str) -> Result<TopicView, TabaError> {
        let view = self.topics.get(topic)?;
        if view.definition.queue().is_none() {
            return Err(wrong_type());
        }
        Ok(view)
    }

    /// Acha a mensagem, na memória ou no disco, e confere o `receiptToken`: um token que não
    /// é o da última entrega é `409`.
    fn locate(
        &self,
        state: &TopicState,
        topic: &str,
        id: &str,
        receipt_token: &str,
    ) -> Result<(MessageMeta, Place), TabaError> {
        if !state.index.contains(id) {
            return Err(message_not_found());
        }
        let found = match state.memory.get(id) {
            Some(stored) => Some((stored.meta.clone(), Place::Memory)),
            None => self
                .store
                .get(topic, id)
                .map_err(|e| self.disk_error("falha ao ler a mensagem em disco", &e))?
                .map(|meta| (meta, Place::Disk)),
        };
        let (meta, place) = found.ok_or_else(message_not_found)?;
        if meta.receipt_token.as_deref() != Some(receipt_token) {
            return Err(stale_receipt());
        }
        Ok((meta, place))
    }

    /// Confirma o consumo e remove a mensagem (doc 13, seção 12.4). O `receiptToken` é o da
    /// **última** entrega; um token antigo é `409` e nada muda. Passado o prazo de
    /// invisibilidade, mas antes de uma nova entrega, o token ainda vale. Em disco remove a
    /// linha e depois o arquivo.
    pub fn ack(&self, topic: &str, id: &str, receipt_token: Option<&str>) -> Result<(), TabaError> {
        let view = self.queue_view(topic)?;
        let receipt_token = receipt_token.ok_or_else(|| required(&["receiptToken"]))?;
        let started = Instant::now();
        let now = self.clock.now_millis();
        self.settle(&view, now);
        let name = &view.definition.name;
        let state = self.state_of(name);
        let mut state = lock(&state);
        let (meta, place) = self.locate(&state, name, id, receipt_token)?;
        if view.definition.queue().is_some_and(|q| q.fifo) {
            self.fifo_ack(&mut state, name, meta, place, now)?;
        } else {
            self.remove_message(&mut state, name, id, place)?;
        }
        state.stats.acked += 1;
        state.stats.last_consumed_ms = Some(now);
        state
            .rates
            .acked
            .record(now, 1, self.rules.rate_window_secs);
        state
            .stats
            .consume_latency
            .record(started.elapsed().as_secs_f64());
        Ok(())
    }

    /// O ack da fila FIFO não apaga a linha: grava `deleteAt` (o instante do id mais a janela de
    /// deduplicação) e apaga só o corpo e os metadados de entrega, para a deduplicação valer pela
    /// janela inteira (doc 17, seção 16.7). Se `deleteAt` já passou, apaga na hora, como na fila
    /// comum.
    fn fifo_ack(
        &self,
        state: &mut TopicState,
        topic: &str,
        meta: MessageMeta,
        place: Place,
        now: u64,
    ) -> Result<(), TabaError> {
        let delete_at = meta
            .published_ms
            .saturating_add(self.rules.fifo_dedup_window_ms);
        if delete_at <= now {
            return self.remove_message(state, topic, &meta.id, place);
        }
        self.store
            .tombstone(topic, &meta, delete_at)
            .map_err(|e| self.disk_error("falha ao gravar o ack da fila FIFO", &e))?;
        self.take_from_memory(state, &meta.id);
        state.index.remove(&meta.id);
        Ok(())
    }

    /// Tira a mensagem de onde ela está e do índice.
    fn remove_message(
        &self,
        state: &mut TopicState,
        topic: &str,
        id: &str,
        place: Place,
    ) -> Result<(), TabaError> {
        match place {
            Place::Memory => {
                self.take_from_memory(state, id);
            }
            Place::Disk => self
                .store
                .remove(topic, id)
                .map_err(|e| self.disk_error("falha ao remover a mensagem do disco", &e))?,
        }
        state.index.remove(id);
        Ok(())
    }

    /// Grava a mensagem de volta onde ela está, depois de mudar os metadados de entrega.
    fn save_meta(
        &self,
        state: &mut TopicState,
        topic: &str,
        meta: &MessageMeta,
        place: Place,
    ) -> Result<(), TabaError> {
        match place {
            Place::Memory => {
                if let Some(stored) = state.memory.get_mut(&meta.id) {
                    stored.meta = meta.clone();
                }
                Ok(())
            }
            Place::Disk => self
                .store
                .update_delivery(topic, meta)
                .map_err(|e| self.disk_error("falha ao gravar a entrega em disco", &e)),
        }
    }

    /// Define a invisibilidade a partir de agora (doc 06a, `PUT .../visibility`): substitui o
    /// prazo, não soma, e não mexe em `deliveryCount` nem no `receiptToken`. `0` libera a
    /// mensagem na hora.
    pub fn set_visibility(
        &self,
        topic: &str,
        id: &str,
        receipt_token: Option<&str>,
        visibility_timeout: Option<i64>,
    ) -> Result<VisibilityUpdated, TabaError> {
        let view = self.queue_view(topic)?;
        let mut missing = Vec::new();
        if receipt_token.is_none() {
            missing.push("receiptToken");
        }
        if visibility_timeout.is_none() {
            missing.push("visibilityTimeout");
        }
        let (Some(receipt_token), Some(visibility_timeout)) = (receipt_token, visibility_timeout)
        else {
            return Err(required(&missing));
        };
        let secs = validation::in_range_signed(
            "visibilityTimeout",
            visibility_timeout,
            (0, self.rules.visibility_max_secs.unwrap_or(u64::MAX)),
        )?;
        let name = &view.definition.name;
        let now = self.clock.now_millis();
        self.settle(&view, now);
        let mut exhausted = false;
        let meta = {
            let state = self.state_of(name);
            let mut state = lock(&state);
            let (mut meta, place) = self.locate(&state, name, id, receipt_token)?;
            meta.invisible_until_ms = now + secs * 1000;
            self.save_meta(&mut state, name, &meta, place)?;
            state.index.set_until(id, meta.invisible_until_ms, now);
            // Liberar uma mensagem que já gastou as entregas a manda à dead-letter, em vez de
            // devolvê-la à fila (doc 13, seção 12.10).
            if secs == 0 && self.exhausted(&view, meta.delivery_count) {
                exhausted = true;
                self.move_to_dead_letter(&mut state, &view, vec![id.to_owned()], now);
            }
            meta
        };
        if secs == 0 && !exhausted {
            self.wake(name, 1);
        }
        Ok(VisibilityUpdated {
            id: id.to_owned(),
            invisible_until_ms: meta.invisible_until_ms,
        })
    }

    // ---- Consumo ----

    /// Entrega até `maxMessages` mensagens prontas. Com `waitTime` maior que zero e nada
    /// pronto, espera por mensagem nova (long polling) e devolve a primeira que chegar.
    pub async fn consume(
        self: &Arc<Self>,
        topic: &str,
        query: ConsumeQuery,
    ) -> Result<Vec<Delivered>, TabaError> {
        let view = match self.topics.get(topic) {
            Err(e) if e.code == ErrorCode::TopicNotFound && wants_auto_create(&query)? => {
                self.auto_create(topic, &query).await?
            }
            other => other?,
        };
        if view.definition.queue().is_none() {
            return Err(wrong_type());
        }
        // Qualquer `GET` conta como consumidor, no começo da chamada, com ou sem mensagem. O da
        // dead-letter não conta (doc 15, seção 14.3).
        if view.dead_letter_of.is_none() {
            lock(&self.last_get).insert(view.definition.id.clone(), self.clock.now_millis());
        }
        let max_messages = match &query.max_messages {
            Some(text) => query_number(
                "maxMessages",
                text,
                (1, self.rules.max_messages_max.unwrap_or(u64::MAX)),
            )?,
            None => self.rules.max_messages_default,
        };
        let visibility = match &query.visibility_timeout {
            Some(text) => query_number(
                "visibilityTimeout",
                text,
                (0, self.rules.visibility_max_secs.unwrap_or(u64::MAX)),
            )?,
            None => self.rules.visibility_default_secs,
        };
        let wait = match &query.wait_time {
            Some(text) => query_number("waitTime", text, WAIT_TIME_SECS)?,
            None => 0,
        };

        let name = view.definition.name.clone();
        let notify = self.notifier(&name);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(wait);
        // Só a chamada que chega a dormir é um long polling para as métricas.
        let mut polling: Option<LongPoll<'_>> = None;
        loop {
            // Registra o interesse antes de olhar a fila, para não perder uma mensagem que
            // chegue entre o olhar e a espera. A ordem do registro é a ordem de atendimento.
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let view = self.topics.get(&name)?;
            let this = self.clone();
            let taken = tokio::task::spawn_blocking(move || {
                this.take(&view, max_messages as usize, visibility)
            })
            .await
            .map_err(|_| TabaError::internal())??;
            // Sobrou mensagem pronta: o próximo ouvinte em espera também pode ser atendido.
            if !taken.delivered.is_empty() && taken.more_ready {
                notify.notify_one();
            }
            // Em drain o `GET` em espera responde vazio na hora, para o cliente reconectar.
            if !taken.delivered.is_empty()
                || tokio::time::Instant::now() >= deadline
                || self.is_draining()
            {
                if let Some(poll) = polling.as_mut() {
                    poll.delivered = Some(!taken.delivered.is_empty());
                }
                return Ok(taken.delivered);
            }
            polling.get_or_insert_with(|| LongPoll::start(&self.counters));
            // Dorme até chegar mensagem, o prazo acabar ou a próxima invisível vencer.
            let mut until = deadline;
            if let Some(at) = taken.next_visible_ms {
                let in_ms = at.saturating_sub(self.clock.now_millis()).max(10);
                until = until.min(tokio::time::Instant::now() + Duration::from_millis(in_ms));
            }
            let _ = tokio::time::timeout_at(until, notified).await;
        }
    }

    /// O `autoCreate`: cria a fila (volátil por padrão) e já a devolve. Quem perde a disputa pela
    /// criação trata como "já existe" e segue como um consumo normal.
    async fn auto_create(&self, topic: &str, query: &ConsumeQuery) -> Result<TopicView, TabaError> {
        if !query.can_create {
            return Err(TabaError::new(
                ErrorCode::AuthForbidden,
                "Criar a fila pelo autoCreate exige a permissão create.",
            ));
        }
        let request = CreateTopic {
            name: topic.to_owned(),
            kind: Kind::Queue,
            tags: (!query.tags.is_empty()).then(|| query.tags.clone()),
            prefixes: None,
            volatile: Some(optional_bool("volatile", query.volatile.as_deref())?.unwrap_or(true)),
            idle_timeout: optional_int("idleTimeout", query.idle_timeout.as_deref())?,
            fifo: optional_bool("fifo", query.fifo.as_deref())?,
            max_message_bytes: None,
            ttl: None,
            max_deliveries: None,
        };
        match self.topics.create_checked(request, true).await {
            Ok(view) => Ok(view),
            Err(e) if e.code == ErrorCode::TopicAlreadyExists => self.topics.get(topic),
            Err(e) => Err(e),
        }
    }

    /// A tarefa que exclui as filas voláteis ociosas, até `cancel`.
    pub async fn run_idle_sweeper(self: Arc<Self>, cancel: tokio_util::sync::CancellationToken) {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => { self.expire_idle().await; }
            }
        }
    }

    /// Exclui as filas voláteis sem consumidor há `idleTimeout`, com as mensagens e a
    /// dead-letter. O prazo é `max(criação, partida, último GET) + idleTimeout`: a partida entra
    /// para um nó que retomou do disco não excluir a fila pelo tempo em que esteve parado.
    /// Devolve os nomes excluídos.
    pub async fn expire_idle(&self) -> Vec<String> {
        let now = self.clock.now_millis();
        let queues = self.topics.volatile_queues();
        let live: HashSet<String> = queues.iter().map(|q| q.id.clone()).collect();
        let mut deleted = Vec::new();
        for queue in queues {
            let last = lock(&self.last_get)
                .get(&queue.id)
                .copied()
                .unwrap_or(0)
                .max(queue.created_ms)
                .max(self.started_ms);
            if now < last.saturating_add(queue.idle_timeout_secs.saturating_mul(1000)) {
                continue;
            }
            // Só apaga o `id` que se verificou: uma fila recriada no meio do caminho fica.
            match self.topics.delete_expected(&queue.name, &queue.id).await {
                Ok(()) => {
                    tracing::info!(
                        event.name = "taba.volatile.deleted",
                        messaging.destination.name = %queue.name,
                        topic.id = %queue.id,
                        "fila volátil excluída por ociosidade"
                    );
                    deleted.push(queue.name);
                }
                Err(e) if e.code == ErrorCode::TopicNotFound => {}
                Err(e) => tracing::warn!(
                    topic = %queue.name,
                    error = %e,
                    "não foi possível excluir a fila ociosa"
                ),
            }
        }
        lock(&self.last_get).retain(|id, _| live.contains(id));
        deleted
    }

    // ---- Retenção ----

    /// `true` se a mensagem já gastou as entregas permitidas (`deliveryCount ≥ maxDeliveries`).
    /// A dead-letter não tem limite: não há dead-letter da dead-letter.
    fn exhausted(&self, view: &TopicView, delivery_count: u32) -> bool {
        view.dead_letter_of.is_none()
            && view
                .max_deliveries()
                .is_some_and(|max| u64::from(delivery_count) >= max)
    }

    /// Aplica o `ttl` e o limite de entregas ao que está guardado, no instante `now`:
    /// apaga o que venceu (em qualquer estado, inclusive invisível), devolve a prontas o que
    /// teve a invisibilidade vencida e manda à dead-letter a mensagem que a teve vencida depois
    /// de gastar as entregas. A dead-letter de uma fila é acertada junto, porque é lá que as
    /// mensagens aparecem (doc 13, seção 12.10, e doc 14, seção 13.6). Cada tópico é acertado
    /// com a trava dele, uma de cada vez.
    fn settle(&self, view: &TopicView, now: u64) {
        if let Some(source) = &view.dead_letter_of
            && let Ok(source_view) = self.topics.get(source)
        {
            self.settle_one(&source_view, now);
        }
        self.settle_one(view, now);
    }

    fn settle_one(&self, view: &TopicView, now: u64) {
        let name = &view.definition.name;
        let state = self.state_of(name);
        let mut state = lock(&state);
        let ttl_ms = view.ttl_secs().unwrap_or(u64::MAX).saturating_mul(1000);
        for id in state.index.expired(now, ttl_ms) {
            let place = if state.memory.contains_key(&id) {
                Place::Memory
            } else {
                Place::Disk
            };
            match self.remove_message(&mut state, name, &id, place) {
                Ok(()) => {
                    state.stats.expired += 1;
                    NodeCounters::add(&self.counters.expired, 1);
                    tracing::info!(
                        event.name = "taba.message.expired",
                        messaging.destination.name = %name,
                        message.id = %id,
                        "mensagem descartada por ttl"
                    );
                }
                Err(_) => tracing::error!(topic = %name, id = %id, "falha ao apagar vencida"),
            }
        }
        // A invisibilidade que venceu devolve a mensagem a pronta; a que já tinha gastado as
        // entregas vai à dead-letter em vez de voltar à fila.
        let released = state.index.release_due(now);
        let mut exhausted: Vec<String> = released
            .iter()
            .filter(|id| {
                state
                    .index
                    .deliveries(id)
                    .is_some_and(|n| n > 0 && self.exhausted(view, n))
            })
            .cloned()
            .collect();
        // Primeira vez que o tópico é visto, ou o `maxDeliveries` mudou por `PATCH`: as prontas
        // que já passam do limite também saem.
        let max = view.max_deliveries();
        if state.checked_max_deliveries != Some(max) {
            state.checked_max_deliveries = Some(max);
            for (id, deliveries) in state.index.ready_delivered() {
                if self.exhausted(view, deliveries) && !exhausted.contains(&id) {
                    exhausted.push(id);
                }
            }
        }
        let back_to_ready =
            released.len() - released.iter().filter(|id| exhausted.contains(id)).count();
        if !exhausted.is_empty() {
            self.move_to_dead_letter(&mut state, view, exhausted, now);
        }
        drop(state);
        if back_to_ready > 0 {
            self.wake(name, back_to_ready);
        }
    }

    /// Move as mensagens para a dead-letter da fila: o mesmo `id`, `deliveryCount` e prazo
    /// zerados, e `deadLetteredAt` e `deadLetterReason` preenchidos (doc 13, seção 12.10). A
    /// mensagem continua onde estava (memória ou disco); em disco o arquivo só é renomeado
    /// para a pasta da dead-letter. A ordem das travas é sempre fila de origem, depois
    /// dead-letter.
    fn move_to_dead_letter(
        &self,
        source_state: &mut TopicState,
        view: &TopicView,
        ids: Vec<String>,
        now: u64,
    ) {
        let source = &view.definition.name;
        let target = format!("{source}{}", self.topics.rules().dead_letter_suffix);
        let target_state = self.state_of(&target);
        let mut target_state = lock(&target_state);
        let mut moved = 0;
        let fifo = view.definition.queue().is_some_and(|q| q.fifo);
        for id in ids {
            let original: Option<MessageMeta>;
            let reset = |mut meta: MessageMeta| {
                meta.delivery_count = 0;
                meta.invisible_until_ms = 0;
                meta.receipt_token = None;
                meta.dead_lettered_ms = Some(now);
                meta.dead_letter_reason = Some("max-deliveries".to_owned());
                // A dead-letter é uma fila comum: não deduplica.
                meta.deduplication_id = None;
                meta
            };
            let group;
            if let Some(mut stored) = self.take_from_memory(source_state, &id) {
                original = Some(stored.meta.clone());
                stored.meta = reset(stored.meta);
                group = stored.meta.group_id.clone();
                self.put_in_memory(&mut target_state, stored);
            } else {
                match self.store.get(source, &id) {
                    Ok(Some(meta)) => {
                        original = Some(meta.clone());
                        let meta = reset(meta);
                        group = meta.group_id.clone();
                        if let Err(e) = self.store.move_to(source, &target, &meta) {
                            tracing::error!(
                                error = %e, topic = %source, id = %id,
                                "falha ao mover para a dead-letter"
                            );
                            continue;
                        }
                    }
                    Ok(None) => {
                        source_state.index.remove(&id);
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, topic = %source, "falha ao ler a mensagem");
                        continue;
                    }
                }
            }
            source_state.index.remove(&id);
            source_state.stats.dead_lettered += 1;
            NodeCounters::add(&self.counters.dead_lettered, 1);
            tracing::warn!(
                event.name = "taba.message.dead_lettered",
                messaging.destination.name = %source,
                message.id = %id,
                delivery_count = original.as_ref().map_or(0, |m| m.delivery_count),
                "mensagem enviada à dead-letter por atingir maxDeliveries"
            );
            // Na FIFO a deduplicação segue valendo depois da saída da mensagem (doc 17, 16.7).
            if fifo
                && let Some(original) = original
                && let delete_at = original
                    .published_ms
                    .saturating_add(self.rules.fifo_dedup_window_ms)
                && delete_at > now
                && let Err(e) = self.store.tombstone(source, &original, delete_at)
            {
                tracing::error!(error = %e, topic = %source, id = %id, "falha ao gravar a linha de deduplicação");
            }
            target_state
                .index
                .insert(&id, group.as_deref(), now, 0, 0, now);
            moved += 1;
        }
        drop(target_state);
        if moved > 0 {
            self.wake(&target, moved);
        }
    }

    /// Uma passada da varredura sobre todas as filas: apaga o que venceu por `ttl` e manda
    /// à dead-letter o que gastou as entregas. Cada nó faz a sua, sem propagar nada.
    pub fn sweep(&self) {
        let now = self.clock.now_millis();
        let mut names: Vec<String> = self
            .existing_states()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        match self.store.topics_on_disk() {
            Ok(on_disk) => names.extend(on_disk),
            Err(e) => tracing::error!(error = %e, "falha ao listar o disco na varredura"),
        }
        names.sort();
        names.dedup();
        for name in names {
            match self.topics.get(&name) {
                Ok(view) => {
                    self.settle(&view, now);
                    if view.definition.queue().is_some_and(|q| q.fifo) {
                        self.sweep_dedup(&name, now);
                    }
                }
                Err(_) => self.drop_topic(&name),
            }
        }
    }

    /// Na fila FIFO: apaga as chaves de deduplicação que saíram da janela e as linhas curtas com
    /// `deleteAt` vencido, pelo índice, sem propagar nada (doc 17, seção 16.7).
    fn sweep_dedup(&self, name: &str, now: u64) {
        let window = self.rules.fifo_dedup_window_ms;
        lock(&self.state_of(name))
            .dedup
            .retain(|_, (_, at)| at.saturating_add(window) > now);
        if let Err(e) = self.store.sweep_tombstones(name, now) {
            tracing::error!(error = %e, topic = %name, "falha ao varrer as linhas de deduplicação");
        }
    }

    /// A tarefa da varredura, até `cancel`. O intervalo é `retention.sweep.interval`, mas
    /// nunca passa de 1 s: a mesma passada leva à dead-letter a mensagem cuja invisibilidade
    /// venceu, e quem consome a dead-letter não deve esperar o intervalo inteiro.
    pub async fn run_sweeper(self: Arc<Self>, cancel: tokio_util::sync::CancellationToken) {
        let period = self.rules.sweep_interval.min(Duration::from_secs(1));
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => {
                    let this = self.clone();
                    let _ = tokio::task::spawn_blocking(move || this.sweep()).await;
                }
            }
        }
    }

    // ---- Drain ----

    /// O drain local, que no desenvolvimento serve de *save* (doc 04, seção 6.10): o limite
    /// de memória cai a zero, então toda mensagem nova vai direto para o disco, e as que
    /// estão em memória são gravadas com os metadados (`deliveryCount`, `invisibleUntil`,
    /// `receiptToken`, `groupId`, `contentType`) e saem da memória. Erro de disco numa
    /// mensagem não interrompe o resto: ela fica em memória e entra no relatório.
    pub fn drain(&self) -> DrainReport {
        self.begin_drain();
        self.drain_until(|| false)
    }

    /// Entra em drain: o limite de memória efetivo cai a zero (toda mensagem nova vai para o
    /// disco) e os `GET` em espera são acordados, para responderem vazios e o cliente
    /// reconectar (doc 04, seção 6.10, passo 1).
    pub fn begin_drain(&self) {
        self.draining.store(true, Ordering::SeqCst);
        for notify in lock(&self.notifiers).values() {
            notify.notify_waiters();
        }
    }

    /// Cancela o drain: o nó volta a gravar em memória (`DELETE /node/drain`).
    pub fn cancel_drain(&self) {
        self.draining.store(false, Ordering::SeqCst);
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Move a memória para o disco, uma mensagem por vez (a trava do tópico é solta entre elas),
    /// até acabar ou `stop` devolver `true` (cancelamento ou `drain.timeout`). Os tópicos saem
    /// na ordem do overflow: primeiro o de **menor taxa de leitura**, e, empatando, por nome.
    pub fn drain_until(&self, stop: impl Fn() -> bool) -> DrainReport {
        let mut report = DrainReport {
            moved: 0,
            failed: 0,
            remaining: 0,
        };
        let now = self.clock.now_millis();
        let mut plan: Vec<(f64, String, Vec<String>)> = self
            .existing_states()
            .into_iter()
            .map(|(name, state)| {
                let state = lock(&state);
                (
                    self.consume_rate(&state, now),
                    name,
                    state.memory.keys().cloned().collect(),
                )
            })
            .collect();
        plan.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        'topics: for (_, topic, ids) in plan {
            let state = self.state_of(&topic);
            for id in ids {
                if stop() {
                    break 'topics;
                }
                let mut state = lock(&state);
                let Some(stored) = self.take_from_memory(&mut state, &id) else {
                    continue;
                };
                match self.store.put(&topic, &stored.meta, &stored.body) {
                    Ok(()) => report.moved += 1,
                    Err(e) => {
                        tracing::error!(error = %e, topic = %topic, id = %id, "falha no drain");
                        report.failed += 1;
                        self.put_in_memory(&mut state, stored);
                        if storage::is_fatal(&e) {
                            self.fail();
                        }
                    }
                }
            }
        }
        report.remaining = self.pending();
        report
    }

    /// Mensagens que ainda estão em memória (`pendingMessages` do `DrainStatus`).
    pub fn pending(&self) -> u64 {
        self.existing_states()
            .iter()
            .map(|(_, state)| lock(state).memory.len() as u64)
            .sum()
    }

    /// Armazenamento do nó (`NodeStatus.storage`): corpos e mensagens em memória, e
    /// mensagens em disco.
    pub fn storage_stats(&self) -> StorageStats {
        let disk_messages = self
            .store
            .topics_on_disk()
            .unwrap_or_default()
            .iter()
            .map(|topic| self.store.count(topic).unwrap_or(0))
            .sum();
        StorageStats {
            memory_bytes: self.memory_bytes.load(Ordering::SeqCst),
            memory_limit_bytes: self.rules.memory_limit,
            memory_messages: self.pending(),
            disk_messages,
        }
    }

    /// Bytes de corpo em memória e mensagens em disco de um tópico (para testes e métricas).
    pub fn usage(&self, topic: &str) -> (u64, u64) {
        let state = self.state_of(topic);
        let memory = lock(&state)
            .memory
            .values()
            .map(|s| s.body.len() as u64)
            .sum();
        (memory, self.store.count(topic).unwrap_or(0))
    }

    /// A taxa de publicação e a de leitura do tópico, em mensagens por segundo na janela
    /// `memory.rate.window` (doc 11).
    pub fn rates(&self, topic: &str) -> (f64, f64) {
        let state = self.state_of(topic);
        let state = lock(&state);
        let now = self.clock.now_millis();
        (
            state
                .rates
                .published
                .per_second(now, self.rules.rate_window_secs),
            state
                .rates
                .consumed
                .per_second(now, self.rules.rate_window_secs),
        )
    }

    // ---- Estatística (doc 11) ----

    /// O retrato de uma fila para `GET /metrics/topics*`. Acerta antes o `ttl` e as invisibilidades
    /// vencidas, para a contagem de prontas e em processamento ser a de agora. `404` se o tópico
    /// não existe e `409` num distribuidor (que tem [`Messages::distributor_counters`]).
    pub fn topic_snapshot(
        &self,
        topic: &str,
        rules: &StatsRules,
    ) -> Result<TopicSnapshot, TabaError> {
        let view = self.queue_view(topic)?;
        let now = self.clock.now_millis();
        self.settle(&view, now);
        let name = &view.definition.name;
        let window = self.rules.rate_window_secs;
        let mut snapshot = {
            let handle = self.state_of(name);
            let state = lock(&handle);
            let fifo = state.index.is_fifo();
            let totals = state.index.group_totals();
            let memory_messages = state.memory.len() as u64;
            TopicSnapshot {
                counters: state.stats.clone(),
                publish_rate: state.rates.published.per_second(now, window),
                consume_rate: state.rates.consumed.per_second(now, window),
                ack_rate: state.rates.acked.per_second(now, window),
                visible: state.index.ready_count() as u64,
                invisible: state.index.processing_count() as u64,
                oldest_ms: state.index.oldest_age_ms(),
                memory_messages,
                disk_messages: (state.index.message_count() as u64).saturating_sub(memory_messages),
                memory_bytes: state.memory.values().map(|s| s.body.len() as u64).sum(),
                disk_bytes: 0,
                groups: totals.groups,
                largest_group: totals.largest,
                busy_groups: if fifo { totals.busy } else { 0 },
                stalled_group: rules
                    .group_stalled_deliveries
                    .filter(|_| fifo)
                    .and_then(|over| state.index.stalled_group(over)),
                publish_latency: Percentiles::of(&state.stats.publish_latency),
                consume_latency: Percentiles::of(&state.stats.consume_latency),
            }
        };
        // Os bytes em disco vêm do tamanho dos arquivos, fora da trava do tópico.
        if snapshot.disk_messages > 0 {
            snapshot.disk_bytes = self.store.body_bytes(name).unwrap_or(0);
        }
        Ok(snapshot)
    }

    /// Os grupos de uma fila, com as contagens e a faixa de hoje (sem ordem). Um grupo sem
    /// mensagens não existe no índice, então não aparece.
    pub fn group_snapshots(&self, topic: &str) -> Result<Vec<GroupSnapshot>, TabaError> {
        let view = self.queue_view(topic)?;
        self.settle(&view, self.clock.now_millis());
        let handle = self.state_of(&view.definition.name);
        let state = lock(&handle);
        Ok(state
            .index
            .group_infos()
            .into_iter()
            .map(|info| {
                let (group_id, overflow) = match info.key {
                    GroupKey::Named(name) => (Some(name), false),
                    GroupKey::Overflow => (None, true),
                };
                GroupSnapshot {
                    group_id,
                    overflow,
                    visible: info.ready,
                    invisible: info.processing,
                    band: info.band,
                    oldest_ms: info.oldest_age_ms,
                }
            })
            .collect())
    }

    /// O que o distribuidor contou desde a partida do nó.
    pub fn distributor_counters(&self, name: &str) -> DistributorCounters {
        lock(&self.distributors)
            .get(name)
            .copied()
            .unwrap_or_default()
    }

    /// O que `GET /metrics` mostra do nó: memória, disco, prontas, grupos ocupados e os
    /// contadores. Conta pelos índices dos tópicos abertos (todo tópico com mensagem em disco é
    /// aberto na primeira varredura depois da partida).
    pub fn observe_node(&self) -> NodeObservation {
        let (mut ready, mut busy_groups, mut memory_messages, mut disk_messages) = (0, 0, 0, 0);
        for (_, state) in self.existing_states() {
            let state = lock(&state);
            ready += state.index.ready_count() as u64;
            if state.index.is_fifo() {
                busy_groups += state.index.group_totals().busy;
            }
            let in_memory = state.memory.len() as u64;
            memory_messages += in_memory;
            disk_messages += (state.index.message_count() as u64).saturating_sub(in_memory);
        }
        let c = &self.counters;
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        NodeObservation {
            memory_bytes: self.memory_bytes.load(Ordering::SeqCst),
            memory_limit_bytes: self.rules.memory_limit,
            memory_messages,
            disk_messages,
            ready,
            busy_groups,
            overflow: load(&c.overflow),
            reads_memory: load(&c.reads_memory),
            reads_disk: load(&c.reads_disk),
            disk_errors: load(&c.disk_errors),
            redeliveries: load(&c.redeliveries),
            dead_lettered: load(&c.dead_lettered),
            expired: load(&c.expired),
            deduplicated: load(&c.deduplicated),
            longpoll_waiting: c.longpoll_waiting.load(Ordering::Relaxed),
            longpoll_delivered: load(&c.longpoll_delivered),
            longpoll_empty: load(&c.longpoll_empty),
            longpoll_disconnected: load(&c.longpoll_disconnected),
            fair_wait: c.fair_wait(),
        }
    }

    /// O espaço livre no volume de `storage.dir`, em bytes; `None` se o sistema não disse.
    pub fn disk_free_bytes(&self) -> Option<u64> {
        self.store.free_bytes().ok()
    }

    /// A partida do nó, em milissegundos: onde a conta de ociosidade de um tópico sem atividade
    /// começa quando ele é mais velho que o nó.
    pub fn started_ms(&self) -> u64 {
        self.started_ms
    }

    // ---- Entrega ----

    /// Pega as mensagens prontas pelo índice (a justiça entre grupos e o efeito sanfona),
    /// juntando memória e disco.
    fn take(&self, view: &TopicView, max: usize, visibility_secs: u64) -> Result<Taken, TabaError> {
        let started = Instant::now();
        let name = &view.definition.name;
        let now = self.clock.now_millis();
        self.settle(view, now);
        let state = self.state_of(name);
        let mut state = lock(&state);
        let until = now + visibility_secs * 1000;

        let mut delivered = Vec::new();
        while delivered.len() < max {
            let Some(id) = state.index.pick() else {
                break;
            };
            let place = if state.memory.contains_key(&id) {
                Place::Memory
            } else {
                Place::Disk
            };
            let (mut meta, body) = match place {
                Place::Memory => {
                    let stored = &state.memory[&id];
                    (stored.meta.clone(), stored.body.clone())
                }
                Place::Disk => {
                    let Some(meta) = self
                        .store
                        .get(name, &id)
                        .map_err(|e| self.disk_error("falha ao ler a mensagem em disco", &e))?
                    else {
                        state.index.remove(&id);
                        continue;
                    };
                    match self.store.body(name, &id) {
                        Ok(body) => (meta, body),
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            // Linha sem arquivo (uma queda no meio da gravação): perdida.
                            tracing::error!(topic = %name, id = %id, "corpo sumiu do disco");
                            let _ = self.store.remove(name, &id);
                            state.index.remove(&id);
                            continue;
                        }
                        Err(e) => return Err(self.disk_error("falha ao ler o corpo em disco", &e)),
                    }
                }
            };
            match place {
                Place::Memory => NodeCounters::add(&self.counters.reads_memory, 1),
                Place::Disk => NodeCounters::add(&self.counters.reads_disk, 1),
            }
            if meta.delivery_count == 0 {
                // A espera até a primeira entrega, pela faixa do grupo na hora de entregar.
                let band = state.index.band_of_entry(&id).unwrap_or(0);
                let waited_ms = now.saturating_sub(meta.age_base_ms());
                self.counters.observe_wait(band, waited_ms as f64 / 1000.0);
            } else {
                state.stats.redelivered += 1;
                NodeCounters::add(&self.counters.redeliveries, 1);
            }
            meta.delivery_count += 1;
            meta.invisible_until_ms = until;
            let receipt_token = self.ids.new_at(now);
            meta.receipt_token = Some(receipt_token.clone());
            self.save_meta(&mut state, name, &meta, place)?;
            // Dentro do laço a mensagem fica em processamento ao menos até o instante seguinte,
            // para o mesmo `GET` não a escolher de novo (com `visibilityTimeout=0`).
            state
                .index
                .deliver(&id, until.max(now + 1), now, meta.delivery_count);
            let content_type = ContentType::parse(&meta.content_type).unwrap_or_else(|_| {
                ContentType::parse("application/octet-stream").expect("válido")
            });
            delivered.push(Delivered {
                id,
                body: Body::render(&content_type, &body),
                content_type: meta.content_type,
                group_id: meta.group_id,
                deduplication_id: meta.deduplication_id,
                receipt_token,
                delivery_count: meta.delivery_count,
                published_ms: meta.published_ms,
                invisible_until_ms: meta.invisible_until_ms,
                dead_lettered_ms: meta.dead_lettered_ms,
                dead_letter_reason: meta.dead_letter_reason,
            });
        }
        if until <= now {
            for message in &delivered {
                state.index.set_until(&message.id, until, now);
            }
        }
        state
            .rates
            .consumed
            .record(now, delivered.len() as u64, self.rules.rate_window_secs);
        if !delivered.is_empty() {
            state.stats.consumed += delivered.len() as u64;
            state.stats.last_consumed_ms = Some(now);
            state
                .stats
                .consume_latency
                .record(started.elapsed().as_secs_f64());
        }
        Ok(Taken {
            delivered,
            next_visible_ms: state.index.next_visible(),
            more_ready: state.index.has_ready(),
        })
    }
}

/// O que se valida na publicação antes de saber o tipo do tópico.
struct Prepared {
    content_type: ContentType,
}

impl Prepared {
    fn new(request: &PublishRequest) -> Result<Self, TabaError> {
        let content_type = ContentType::parse(
            request
                .content_type
                .as_deref()
                .unwrap_or(DEFAULT_CONTENT_TYPE),
        )?;
        content_type.validate_body(&request.body)?;
        if let Some(group) = &request.group_id {
            validation::message_key("groupId", group)?;
        }
        if let Some(key) = &request.deduplication_id {
            validation::message_key("deduplicationId", key)?;
        }
        Ok(Self { content_type })
    }
}

#[cfg(test)]
mod tests;
