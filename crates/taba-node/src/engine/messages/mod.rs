//! Publicar e consumir mensagens (doc 06a, "Mensagens"; doc 13).
//!
//! **Onde a mensagem mora.** Em memória, enquanto couber em `memory.limit`; o que passa do
//! limite (ou chega durante o drain) vai para o disco: o corpo em `${topic}/${id}.body` e o
//! resto numa linha do banco do tópico (doc 02, seção 3.2, `storage::messages`). Cada
//! mensagem está em **um só** lugar, e as operações (entrega, ack, invisibilidade, `ttl`,
//! dead-letter) funcionam igual nos dois: a entrega junta as duas origens em ordem de `id`.
//!
//! **Entrega.** O `GET` pega as mensagens prontas em ordem de `id`, incrementa o
//! `deliveryCount`, troca o `receiptToken` e as deixa invisíveis por `visibilityTimeout`.
//! Passado o prazo a mensagem volta a ser entregável (*at-least-once*). O ack e o `PUT
//! visibility` conferem o `receiptToken` da última entrega. O `ttl` e o `maxDeliveries` (com
//! a dead-letter) são aplicados por `settle`, antes de cada operação e pela varredura de fundo.
//!
//! Ainda não há ordem FIFO, deduplicação, lista de prontos nem de invisíveis: as mensagens em
//! memória são varridas a cada operação, e as em disco são achadas pelos índices do banco.
//! Os métodos são síncronos e bloqueiam no disco: quem chama de código assíncrono usa
//! `spawn_blocking`.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use tokio::sync::Notify;

use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::content::{Body, ContentType, DEFAULT_CONTENT_TYPE};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id::IdGenerator;
use crate::core::validation;
use crate::engine::topics::{TopicView, Topics};
use crate::storage;
use crate::storage::messages::{MessageMeta, MessageStore};

/// `waitTime` do `GET`: de 0 a 20 segundos (doc 12).
const WAIT_TIME_SECS: (u64, u64) = (0, 20);

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
    /// Numa fila: o `id` da mensagem criada.
    Queue { id: String },
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
}

/// Uma mensagem entregue, como o `Message` da API (doc 06a, "DTOs").
#[derive(Debug)]
pub struct Delivered {
    pub id: String,
    pub body: Body,
    pub content_type: String,
    pub group_id: Option<String>,
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

#[derive(Default)]
struct State {
    /// Por tópico, em ordem de `id`: o GUID v7 em minúsculas ordena pelo instante da
    /// publicação.
    memory: HashMap<String, BTreeMap<String, Stored>>,
    /// Bytes de corpo em memória, contra `memory.limit`.
    memory_bytes: u64,
    /// No drain o limite de memória efetivo é zero: toda mensagem nova vai para o disco.
    draining: bool,
}

impl State {
    fn insert(&mut self, topic: &str, stored: Stored) {
        self.memory_bytes += stored.body.len() as u64;
        let previous = self
            .memory
            .entry(topic.to_owned())
            .or_default()
            .insert(stored.meta.id.clone(), stored);
        if let Some(previous) = previous {
            self.memory_bytes -= previous.body.len() as u64;
        }
    }

    fn remove(&mut self, topic: &str, id: &str) -> Option<Stored> {
        let stored = self.memory.get_mut(topic)?.remove(id)?;
        self.memory_bytes -= stored.body.len() as u64;
        Some(stored)
    }

    fn drop_topic(&mut self, topic: &str) {
        if let Some(messages) = self.memory.remove(topic) {
            let bytes: u64 = messages.values().map(|s| s.body.len() as u64).sum();
            self.memory_bytes -= bytes;
        }
    }
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

pub struct Messages {
    topics: Arc<Topics>,
    clock: Arc<dyn Clock>,
    rules: MessageRules,
    ids: IdGenerator,
    store: Arc<MessageStore>,
    state: Mutex<State>,
    /// Acorda os `GET` em espera (long polling) quando chega mensagem ou o tópico some.
    notifiers: Mutex<HashMap<String, Arc<Notify>>>,
    /// Avisa o nó de que o disco falhou de um jeito que o derruba (estado `failed`).
    on_failure: OnceLock<FailureHook>,
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
        let messages = Arc::new(Self {
            topics: topics.clone(),
            clock,
            rules,
            ids: IdGenerator::new(),
            store,
            state: Mutex::new(State::default()),
            notifiers: Mutex::new(HashMap::new()),
            on_failure: OnceLock::new(),
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

    fn drop_topic(&self, name: &str) {
        lock(&self.state).drop_topic(name);
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

    /// O maior corpo que a rota de publicação lê: o `maxMessageBytes` da fila, ou o geral
    /// num distribuidor. Responde `404` se o tópico não existe antes de ler o corpo.
    pub fn publish_limit(&self, topic: &str) -> Result<u64, TabaError> {
        let view = self.topics.get(topic)?;
        Ok(view.max_message_bytes().unwrap_or(self.rules.max_bytes))
    }

    pub fn publish(&self, topic: &str, request: PublishRequest) -> Result<Published, TabaError> {
        let view = self.topics.get(topic)?;
        let prepared = Prepared::new(&request)?;
        if let Some(prefixes) = view.definition.prefixes() {
            if request.body.len() as u64 > self.rules.max_bytes {
                return Err(too_large(self.rules.max_bytes));
            }
            return self.publish_to_distributor(prefixes, &request, &prepared);
        }
        let limit = view.max_message_bytes().unwrap_or(self.rules.max_bytes);
        if request.body.len() as u64 > limit {
            return Err(too_large(limit));
        }
        self.store(&view, &request, &prepared)
            .map(|id| Published::Queue { id })
    }

    /// Valida os campos da publicação que dependem do tipo da fila e guarda a mensagem, em
    /// memória ou, se não couber, em disco (primeiro o corpo, depois a linha).
    fn store(
        &self,
        view: &TopicView,
        request: &PublishRequest,
        prepared: &Prepared,
    ) -> Result<String, TabaError> {
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
        let id = self.ids.new_at(now);
        let name = view.definition.name.clone();
        let meta = MessageMeta {
            id: id.clone(),
            content_type: prepared.content_type.as_str().to_owned(),
            group_id: request.group_id.clone(),
            published_ms: now,
            delivery_count: 0,
            invisible_until_ms: 0,
            receipt_token: None,
            dead_lettered_ms: None,
            dead_letter_reason: None,
        };
        let size = request.body.len() as u64;
        {
            let mut state = lock(&self.state);
            let fits = !state.draining
                && self
                    .rules
                    .memory_limit
                    .is_none_or(|limit| state.memory_bytes + size <= limit);
            if fits {
                state.insert(
                    &name,
                    Stored {
                        meta,
                        body: request.body.clone(),
                    },
                );
            } else {
                self.store
                    .put(&name, &meta, &request.body)
                    .map_err(|e| self.disk_error("falha ao gravar a mensagem em disco", &e))?;
            }
        }
        self.notifier(&name).notify_waiters();
        Ok(id)
    }

    /// Uma cópia independente, com `id` próprio, em cada fila que casa com um prefixo. Zero
    /// filas não é erro: a mensagem é descartada (doc 15, seção 14.2).
    fn publish_to_distributor(
        &self,
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
            .map(|view| DistributorDelivery {
                topic: view.definition.name.clone(),
                result: self.store(view, request, prepared),
            })
            .collect();
        Ok(Published::Distributor { deliveries })
    }

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
        state: &State,
        topic: &str,
        id: &str,
        receipt_token: &str,
    ) -> Result<(MessageMeta, Place), TabaError> {
        let found = match state.memory.get(topic).and_then(|m| m.get(id)) {
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
        let now = self.clock.now_millis();
        let mut state = lock(&self.state);
        self.settle(&mut state, &view, now);
        let (_, place) = self.locate(&state, &view.definition.name, id, receipt_token)?;
        self.remove_message(&mut state, &view.definition.name, id, place)
    }

    fn remove_message(
        &self,
        state: &mut State,
        topic: &str,
        id: &str,
        place: Place,
    ) -> Result<(), TabaError> {
        match place {
            Place::Memory => {
                state.remove(topic, id);
                Ok(())
            }
            Place::Disk => self
                .store
                .remove(topic, id)
                .map_err(|e| self.disk_error("falha ao remover a mensagem do disco", &e)),
        }
    }

    /// Grava a mensagem de volta onde ela está, depois de mudar os metadados de entrega.
    fn save_meta(
        &self,
        state: &mut State,
        topic: &str,
        meta: &MessageMeta,
        place: Place,
    ) -> Result<(), TabaError> {
        match place {
            Place::Memory => {
                if let Some(stored) = state
                    .memory
                    .get_mut(topic)
                    .and_then(|m| m.get_mut(&meta.id))
                {
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
        let mut state = lock(&self.state);
        self.settle(&mut state, &view, now);
        let (mut meta, place) = self.locate(&state, name, id, receipt_token)?;
        meta.invisible_until_ms = now + secs * 1000;
        self.save_meta(&mut state, name, &meta, place)?;
        // Liberar uma mensagem que já gastou as entregas a manda à dead-letter, em vez de
        // devolvê-la à fila (doc 13, seção 12.10).
        let exhausted = secs == 0 && self.exhausted(&view, meta.delivery_count);
        if exhausted {
            self.move_to_dead_letter(&mut state, &view, vec![id.to_owned()], now);
        }
        drop(state);
        if secs == 0 && !exhausted {
            self.notifier(name).notify_waiters();
        }
        Ok(VisibilityUpdated {
            id: id.to_owned(),
            invisible_until_ms: meta.invisible_until_ms,
        })
    }

    /// Entrega até `maxMessages` mensagens prontas. Com `waitTime` maior que zero e nada
    /// pronto, espera por mensagem nova (long polling) e devolve a primeira que chegar.
    pub async fn consume(
        self: &Arc<Self>,
        topic: &str,
        query: ConsumeQuery,
    ) -> Result<Vec<Delivered>, TabaError> {
        let view = self.topics.get(topic)?;
        if view.definition.queue().is_none() {
            return Err(wrong_type());
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
        loop {
            // Registra o interesse antes de olhar a fila, para não perder uma mensagem que
            // chegue entre o olhar e a espera.
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let view = self.topics.get(&name)?;
            let this = self.clone();
            let (delivered, next_visible_ms) = tokio::task::spawn_blocking(move || {
                this.take(&view, max_messages as usize, visibility)
            })
            .await
            .map_err(|_| TabaError::internal())??;
            // Em drain o `GET` em espera responde vazio na hora, para o cliente reconectar.
            if !delivered.is_empty()
                || tokio::time::Instant::now() >= deadline
                || self.is_draining()
            {
                return Ok(delivered);
            }
            // Dorme até chegar mensagem, o prazo acabar ou a próxima invisível vencer.
            let mut until = deadline;
            if let Some(at) = next_visible_ms {
                let in_ms = at.saturating_sub(self.clock.now_millis()).max(10);
                until = until.min(tokio::time::Instant::now() + Duration::from_millis(in_ms));
            }
            let _ = tokio::time::timeout_at(until, notified).await;
        }
    }

    /// `true` se a mensagem já gastou as entregas permitidas (`deliveryCount ≥ maxDeliveries`).
    /// A dead-letter não tem limite: não há dead-letter da dead-letter.
    fn exhausted(&self, view: &TopicView, delivery_count: u32) -> bool {
        view.dead_letter_of.is_none()
            && view
                .max_deliveries()
                .is_some_and(|max| u64::from(delivery_count) >= max)
    }

    /// Aplica o `ttl` e o limite de entregas ao que está guardado, no instante `now`:
    /// apaga o que venceu (em qualquer estado, inclusive invisível) e manda à dead-letter a
    /// mensagem cuja invisibilidade acabou depois de gastar as entregas. A dead-letter de uma
    /// fila é acertada junto, porque é lá que as mensagens aparecem (doc 13, seção 12.10, e
    /// doc 14, seção 13.6).
    fn settle(&self, state: &mut State, view: &TopicView, now: u64) {
        if let Some(source) = &view.dead_letter_of
            && let Ok(source_view) = self.topics.get(source)
        {
            self.settle_one(state, &source_view, now);
        }
        self.settle_one(state, view, now);
    }

    fn settle_one(&self, state: &mut State, view: &TopicView, now: u64) {
        let name = &view.definition.name;
        let ttl_ms = view.ttl_secs().unwrap_or(u64::MAX).saturating_mul(1000);
        let (mut expired, mut exhausted) = (Vec::new(), Vec::new());
        if let Some(messages) = state.memory.get(name) {
            for (id, stored) in messages {
                let meta = &stored.meta;
                if meta.age_base_ms().saturating_add(ttl_ms) <= now {
                    expired.push(id.clone());
                } else if meta.delivery_count > 0
                    && meta.invisible_until_ms <= now
                    && self.exhausted(view, meta.delivery_count)
                {
                    exhausted.push(id.clone());
                }
            }
        }
        match self.store.expired_ids(name, now, ttl_ms) {
            Ok(ids) => expired.extend(ids),
            Err(e) => tracing::error!(error = %e, topic = %name, "falha ao procurar vencidas"),
        }
        if !view.dead_letter_of.is_some()
            && let Some(max) = view.max_deliveries()
        {
            match self.store.exhausted_ids(name, now, max) {
                Ok(ids) => exhausted.extend(ids),
                Err(e) => tracing::error!(error = %e, topic = %name, "falha ao procurar esgotadas"),
            }
        }
        for id in expired {
            if state.remove(name, &id).is_none()
                && let Err(e) = self.store.remove(name, &id)
            {
                tracing::error!(error = %e, topic = %name, id = %id, "falha ao apagar vencida");
                continue;
            }
            tracing::info!(event = "taba.message.expired", topic = %name, id = %id);
        }
        // O que venceu já saiu: uma esgotada que também venceu não vai à dead-letter.
        exhausted.retain(|id| {
            state.memory.get(name).is_some_and(|m| m.contains_key(id))
                || self.store.get(name, id).ok().flatten().is_some()
        });
        if !exhausted.is_empty() {
            self.move_to_dead_letter(state, view, exhausted, now);
        }
    }

    /// Move as mensagens para a dead-letter da fila: o mesmo `id`, `deliveryCount` e prazo
    /// zerados, e `deadLetteredAt` e `deadLetterReason` preenchidos (doc 13, seção 12.10). A
    /// mensagem continua onde estava (memória ou disco); em disco o arquivo só é renomeado
    /// para a pasta da dead-letter.
    fn move_to_dead_letter(&self, state: &mut State, view: &TopicView, ids: Vec<String>, now: u64) {
        let source = &view.definition.name;
        let target = format!("{source}{}", self.topics.rules().dead_letter_suffix);
        let mut moved_any = false;
        for id in ids {
            let reset = |mut meta: MessageMeta| {
                tracing::info!(
                    event = "taba.message.dead_lettered",
                    topic = %source,
                    id = %id,
                    delivery_count = meta.delivery_count
                );
                meta.delivery_count = 0;
                meta.invisible_until_ms = 0;
                meta.receipt_token = None;
                meta.dead_lettered_ms = Some(now);
                meta.dead_letter_reason = Some("max-deliveries".to_owned());
                meta
            };
            if let Some(mut stored) = state.remove(source, &id) {
                stored.meta = reset(stored.meta);
                state.insert(&target, stored);
                moved_any = true;
                continue;
            }
            match self.store.get(source, &id) {
                Ok(Some(meta)) => {
                    let meta = reset(meta);
                    match self.store.move_to(source, &target, &meta) {
                        Ok(()) => moved_any = true,
                        Err(e) => tracing::error!(
                            error = %e, topic = %source, id = %id,
                            "falha ao mover para a dead-letter"
                        ),
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::error!(error = %e, topic = %source, "falha ao ler a mensagem"),
            }
        }
        if moved_any {
            self.notifier(&target).notify_waiters();
        }
    }

    /// Uma passada da varredura sobre todas as filas: apaga o que venceu por `ttl` e manda
    /// à dead-letter o que gastou as entregas. Cada nó faz a sua, sem propagar nada.
    pub fn sweep(&self) {
        let now = self.clock.now_millis();
        let mut names: Vec<String> = lock(&self.state).memory.keys().cloned().collect();
        match self.store.topics_on_disk() {
            Ok(on_disk) => names.extend(on_disk),
            Err(e) => tracing::error!(error = %e, "falha ao listar o disco na varredura"),
        }
        names.sort();
        names.dedup();
        for name in names {
            match self.topics.get(&name) {
                Ok(view) => self.settle(&mut lock(&self.state), &view, now),
                Err(_) => self.drop_topic(&name),
            }
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
        lock(&self.state).draining = true;
        for notify in lock(&self.notifiers).values() {
            notify.notify_waiters();
        }
    }

    /// Cancela o drain: o nó volta a gravar em memória (`DELETE /node/drain`).
    pub fn cancel_drain(&self) {
        lock(&self.state).draining = false;
    }

    pub fn is_draining(&self) -> bool {
        lock(&self.state).draining
    }

    /// Move a memória para o disco, uma mensagem por vez (o bloqueio não segura as outras
    /// chamadas), até acabar ou `stop` devolver `true` (cancelamento ou `drain.timeout`).
    /// Um tópico de cada vez, em ordem de nome: ainda não há taxa de leitura para ordenar
    /// como o overflow.
    pub fn drain_until(&self, stop: impl Fn() -> bool) -> DrainReport {
        let mut report = DrainReport {
            moved: 0,
            failed: 0,
            remaining: 0,
        };
        let mut plan: Vec<(String, Vec<String>)> = lock(&self.state)
            .memory
            .iter()
            .map(|(topic, messages)| (topic.clone(), messages.keys().cloned().collect()))
            .collect();
        plan.sort();
        'topics: for (topic, ids) in plan {
            for id in ids {
                if stop() {
                    break 'topics;
                }
                let mut state = lock(&self.state);
                let Some(stored) = state.remove(&topic, &id) else {
                    continue;
                };
                match self.store.put(&topic, &stored.meta, &stored.body) {
                    Ok(()) => report.moved += 1,
                    Err(e) => {
                        tracing::error!(error = %e, topic = %topic, id = %id, "falha no drain");
                        report.failed += 1;
                        state.insert(&topic, stored);
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
        lock(&self.state)
            .memory
            .values()
            .map(|m| m.len() as u64)
            .sum()
    }

    /// Armazenamento do nó (`NodeStatus.storage`): corpos e mensagens em memória, e
    /// mensagens em disco.
    pub fn storage_stats(&self) -> StorageStats {
        let (memory_bytes, memory_messages) = {
            let state = lock(&self.state);
            (
                state.memory_bytes,
                state.memory.values().map(|m| m.len() as u64).sum(),
            )
        };
        let disk_messages = self
            .store
            .topics_on_disk()
            .unwrap_or_default()
            .iter()
            .map(|topic| self.store.count(topic).unwrap_or(0))
            .sum();
        StorageStats {
            memory_bytes,
            memory_limit_bytes: self.rules.memory_limit,
            memory_messages,
            disk_messages,
        }
    }

    /// Bytes de corpo em memória e mensagens em disco de um tópico (para testes e métricas).
    pub fn usage(&self, topic: &str) -> (u64, u64) {
        let state = lock(&self.state);
        let memory = state
            .memory
            .get(topic)
            .map_or(0, |m| m.values().map(|s| s.body.len() as u64).sum());
        drop(state);
        (memory, self.store.count(topic).unwrap_or(0))
    }

    /// Pega as mensagens prontas, em ordem de `id`, juntando a memória e o disco. Devolve
    /// também o instante em que a próxima invisível volta a ser entregável, para o long polling
    /// saber até quando dormir.
    fn take(
        &self,
        view: &TopicView,
        max: usize,
        visibility_secs: u64,
    ) -> Result<(Vec<Delivered>, Option<u64>), TabaError> {
        let name = &view.definition.name;
        let now = self.clock.now_millis();
        let mut state = lock(&self.state);
        self.settle(&mut state, view, now);

        let mut candidates: Vec<(String, Place)> = Vec::new();
        let mut next_visible: Option<u64> = None;
        if let Some(messages) = state.memory.get(name) {
            for (id, stored) in messages {
                let until = stored.meta.invisible_until_ms;
                if until > now {
                    next_visible = Some(next_visible.map_or(until, |n| n.min(until)));
                } else if candidates.len() < max {
                    candidates.push((id.clone(), Place::Memory));
                }
            }
        }
        let on_disk = self
            .store
            .ready_ids(name, now, max)
            .map_err(|e| self.disk_error("falha ao ler as prontas do disco", &e))?;
        candidates.extend(on_disk.into_iter().map(|id| (id, Place::Disk)));
        if let Some(at) = self
            .store
            .next_invisible_until(name, now)
            .map_err(|e| self.disk_error("falha ao ler o próximo prazo do disco", &e))?
        {
            next_visible = Some(next_visible.map_or(at, |n| n.min(at)));
        }
        candidates.sort();
        candidates.truncate(max);

        let mut delivered = Vec::new();
        for (id, place) in candidates {
            let (mut meta, body) = match place {
                Place::Memory => {
                    let stored = &state.memory[name][&id];
                    (stored.meta.clone(), stored.body.clone())
                }
                Place::Disk => {
                    let Some(meta) = self
                        .store
                        .get(name, &id)
                        .map_err(|e| self.disk_error("falha ao ler a mensagem em disco", &e))?
                    else {
                        continue;
                    };
                    match self.store.body(name, &id) {
                        Ok(body) => (meta, body),
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            // Linha sem arquivo (uma queda no meio da gravação): perdida.
                            tracing::error!(topic = %name, id = %id, "corpo sumiu do disco");
                            let _ = self.store.remove(name, &id);
                            continue;
                        }
                        Err(e) => return Err(self.disk_error("falha ao ler o corpo em disco", &e)),
                    }
                }
            };
            meta.delivery_count += 1;
            meta.invisible_until_ms = now + visibility_secs * 1000;
            let receipt_token = self.ids.new_at(now);
            meta.receipt_token = Some(receipt_token.clone());
            self.save_meta(&mut state, name, &meta, place)?;
            let content_type = ContentType::parse(&meta.content_type).unwrap_or_else(|_| {
                ContentType::parse("application/octet-stream").expect("válido")
            });
            delivered.push(Delivered {
                id,
                body: Body::render(&content_type, &body),
                content_type: meta.content_type,
                group_id: meta.group_id,
                receipt_token,
                delivery_count: meta.delivery_count,
                published_ms: meta.published_ms,
                invisible_until_ms: meta.invisible_until_ms,
                dead_lettered_ms: meta.dead_lettered_ms,
                dead_letter_reason: meta.dead_letter_reason,
            });
        }
        Ok((delivered, next_visible))
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
