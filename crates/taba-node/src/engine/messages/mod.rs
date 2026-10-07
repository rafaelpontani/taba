//! Publicar e consumir mensagens (doc 06a, "Mensagens"; doc 13). Por enquanto as
//! mensagens vivem só em memória, numa fila ordenada por `id` (que é o instante da
//! publicação) por tópico; o disco e o SQLite entram depois (doc 02, seção 3.2).
//!
//! Entrega: `GET` pega as mensagens prontas em ordem de `id`, incrementa o
//! `deliveryCount`, troca o `receiptToken` e as deixa invisíveis por `visibilityTimeout`.
//! Passado o prazo a mensagem volta a ser entregável, e é por isso que ela não some se
//! ninguém a confirmar (*at-least-once*). O ack e o `PUT visibility` conferem o `receiptToken`
//! da última entrega. O `ttl` e o `maxDeliveries` (com a dead-letter) são aplicados por `settle`,
//! antes de cada operação e pela varredura de fundo. Ainda não há ordem FIFO, deduplicação,
//! nem a lista de prontos: cada `GET` varre a fila.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::Notify;

use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::content::{Body, ContentType, DEFAULT_CONTENT_TYPE};
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id::IdGenerator;
use crate::core::validation;
use crate::engine::topics::{TopicView, Topics};

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
    pub dead_letter_reason: Option<&'static str>,
}

struct Stored {
    body: Vec<u8>,
    content_type: ContentType,
    group_id: Option<String>,
    published_ms: u64,
    delivery_count: u32,
    /// Instante a partir do qual a mensagem é entregável. `0`: nunca foi entregue.
    invisible_until_ms: u64,
    receipt_token: Option<String>,
    /// Quando a mensagem entrou na dead-letter (e por quê). Dali em diante o `ttl` conta
    /// deste instante, e não do `id` (doc 13, seção 12.10).
    dead_lettered_ms: Option<u64>,
    dead_letter_reason: Option<&'static str>,
}

impl Stored {
    /// O instante de onde o `ttl` conta.
    fn age_base_ms(&self) -> u64 {
        self.dead_lettered_ms.unwrap_or(self.published_ms)
    }
}

#[derive(Default)]
struct Queue {
    /// Pela ordem do `id`: GUID v7 em minúsculas ordena pelo instante da publicação.
    messages: BTreeMap<String, Stored>,
}

impl Queue {
    /// Confere que a mensagem existe e que o token é o da última entrega. O vencimento por
    /// `ttl` e por `maxDeliveries` já foi aplicado por `settle`.
    fn check(&self, id: &str, receipt_token: &str) -> Result<(), TabaError> {
        let message = self.messages.get(id).ok_or_else(message_not_found)?;
        if message.receipt_token.as_deref() != Some(receipt_token) {
            return Err(TabaError::new(
                ErrorCode::MessageStaleReceipt,
                "O receiptToken não é o da última entrega desta mensagem.",
            ));
        }
        Ok(())
    }
}

/// O resultado de `PUT .../visibility`.
#[derive(Debug)]
pub struct VisibilityUpdated {
    pub id: String,
    pub invisible_until_ms: u64,
}

fn message_not_found() -> TabaError {
    TabaError::new(ErrorCode::MessageNotFound, "A mensagem não existe.")
}

/// `validation.required`, com cada campo que falta em `details`.
fn required(fields: &[&str]) -> TabaError {
    fields.iter().fold(
        TabaError::new(ErrorCode::ValidationRequired, "Faltam campos obrigatórios."),
        |error, field| {
            error.with_detail(*field, ErrorCode::ValidationRequired, "Campo obrigatório.")
        },
    )
}

pub struct Messages {
    topics: Arc<Topics>,
    clock: Arc<dyn Clock>,
    rules: MessageRules,
    ids: IdGenerator,
    queues: Mutex<HashMap<String, Queue>>,
    /// Acorda os `GET` em espera (long polling) quando chega mensagem ou o tópico some.
    notifiers: Mutex<HashMap<String, Arc<Notify>>>,
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

impl Messages {
    pub fn new(topics: Arc<Topics>, clock: Arc<dyn Clock>, rules: MessageRules) -> Arc<Self> {
        let messages = Arc::new(Self {
            topics: topics.clone(),
            clock,
            rules,
            ids: IdGenerator::new(),
            queues: Mutex::new(HashMap::new()),
            notifiers: Mutex::new(HashMap::new()),
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
        lock(&self.queues).remove(name);
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

    /// Valida os campos da publicação que dependem do tipo da fila e guarda a mensagem.
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
        lock(&self.queues)
            .entry(name.clone())
            .or_default()
            .messages
            .insert(
                id.clone(),
                Stored {
                    body: request.body.clone(),
                    content_type: prepared.content_type.clone(),
                    group_id: request.group_id.clone(),
                    published_ms: now,
                    delivery_count: 0,
                    invisible_until_ms: 0,
                    receipt_token: None,
                    dead_lettered_ms: None,
                    dead_letter_reason: None,
                },
            );
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

    /// Confirma o consumo e remove a mensagem (doc 13, seção 12.4). O `receiptToken` é o da
    /// **última** entrega; um token antigo é `409` e nada muda. Passado o prazo de
    /// invisibilidade, mas antes de uma nova entrega, o token ainda vale.
    pub fn ack(&self, topic: &str, id: &str, receipt_token: Option<&str>) -> Result<(), TabaError> {
        let view = self.queue_view(topic)?;
        let receipt_token = receipt_token.ok_or_else(|| required(&["receiptToken"]))?;
        let now = self.clock.now_millis();
        let mut queues = lock(&self.queues);
        self.settle(&mut queues, &view, now);
        let queue = queues
            .get_mut(&view.definition.name)
            .ok_or_else(message_not_found)?;
        queue.check(id, receipt_token)?;
        queue.messages.remove(id);
        Ok(())
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
        let now = self.clock.now_millis();
        let mut queues = lock(&self.queues);
        self.settle(&mut queues, &view, now);
        let queue = queues
            .get_mut(&view.definition.name)
            .ok_or_else(message_not_found)?;
        queue.check(id, receipt_token)?;
        let message = queue.messages.get_mut(id).ok_or_else(message_not_found)?;
        message.invisible_until_ms = now + secs * 1000;
        let invisible_until_ms = message.invisible_until_ms;
        // Liberar uma mensagem que já gastou as entregas a manda à dead-letter, em vez de
        // devolvê-la à fila (doc 13, seção 12.10).
        let exhausted = secs == 0 && self.exhausted(&view, message.delivery_count);
        if exhausted {
            self.move_to_dead_letter(&mut queues, &view, vec![id.to_owned()], now);
        }
        drop(queues);
        if secs == 0 && !exhausted {
            self.notifier(&view.definition.name).notify_waiters();
        }
        Ok(VisibilityUpdated {
            id: id.to_owned(),
            invisible_until_ms,
        })
    }

    /// Entrega até `maxMessages` mensagens prontas. Com `waitTime` maior que zero e nada
    /// pronto, espera por mensagem nova (long polling) e devolve a primeira que chegar.
    pub async fn consume(
        &self,
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
            let (delivered, next_visible_ms) = self.take(&view, max_messages as usize, visibility);
            if !delivered.is_empty() || tokio::time::Instant::now() >= deadline {
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
    fn settle(&self, queues: &mut HashMap<String, Queue>, view: &TopicView, now: u64) {
        if let Some(source) = &view.dead_letter_of
            && let Ok(source_view) = self.topics.get(source)
        {
            self.settle_one(queues, &source_view, now);
        }
        self.settle_one(queues, view, now);
    }

    fn settle_one(&self, queues: &mut HashMap<String, Queue>, view: &TopicView, now: u64) {
        let name = &view.definition.name;
        let ttl_ms = view.ttl_secs().unwrap_or(u64::MAX).saturating_mul(1000);
        let Some(queue) = queues.get_mut(name) else {
            return;
        };
        let (mut expired, mut exhausted) = (Vec::new(), Vec::new());
        for (id, message) in &queue.messages {
            if message.age_base_ms().saturating_add(ttl_ms) <= now {
                expired.push(id.clone());
            } else if message.delivery_count > 0
                && message.invisible_until_ms <= now
                && self.exhausted(view, message.delivery_count)
            {
                exhausted.push(id.clone());
            }
        }
        for id in expired {
            queue.messages.remove(&id);
            tracing::info!(event = "taba.message.expired", topic = %name, id = %id);
        }
        if !exhausted.is_empty() {
            self.move_to_dead_letter(queues, view, exhausted, now);
        }
    }

    /// Move as mensagens para a dead-letter da fila: o mesmo `id`, `deliveryCount` e prazo
    /// zerados, e `deadLetteredAt` e `deadLetterReason` preenchidos (doc 13, seção 12.10).
    fn move_to_dead_letter(
        &self,
        queues: &mut HashMap<String, Queue>,
        view: &TopicView,
        ids: Vec<String>,
        now: u64,
    ) {
        let source = &view.definition.name;
        let target_name = format!("{source}{}", self.topics.rules().dead_letter_suffix);
        let Some(queue) = queues.get_mut(source) else {
            return;
        };
        let moved: Vec<(String, Stored)> = ids
            .into_iter()
            .filter_map(|id| queue.messages.remove(&id).map(|message| (id, message)))
            .collect();
        if moved.is_empty() {
            return;
        }
        let target = queues.entry(target_name.clone()).or_default();
        for (id, mut message) in moved {
            tracing::info!(
                event = "taba.message.dead_lettered",
                topic = %source,
                id = %id,
                delivery_count = message.delivery_count
            );
            message.delivery_count = 0;
            message.invisible_until_ms = 0;
            message.receipt_token = None;
            message.dead_lettered_ms = Some(now);
            message.dead_letter_reason = Some("max-deliveries");
            target.messages.insert(id, message);
        }
        self.notifier(&target_name).notify_waiters();
    }

    /// Uma passada da varredura sobre todas as filas: apaga o que venceu por `ttl` e manda
    /// à dead-letter o que gastou as entregas. Cada nó faz a sua, sem propagar nada.
    pub fn sweep(&self) {
        let now = self.clock.now_millis();
        let names: Vec<String> = lock(&self.queues).keys().cloned().collect();
        for name in names {
            match self.topics.get(&name) {
                Ok(view) => self.settle(&mut lock(&self.queues), &view, now),
                Err(_) => {
                    lock(&self.queues).remove(&name);
                }
            }
        }
    }

    /// A tarefa da varredura, até `cancel`. O intervalo é `retention.sweep.interval`, mas
    /// nunca passa de 1 s: a mesma passada leva à dead-letter a mensagem cuja invisibilidade
    /// venceu, e quem consome a dead-letter não deve esperar o intervalo inteiro.
    pub async fn run_sweeper(&self, cancel: tokio_util::sync::CancellationToken) {
        let period = self.rules.sweep_interval.min(Duration::from_secs(1));
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => self.sweep(),
            }
        }
    }

    /// Pega as mensagens prontas, em ordem de `id`. Devolve também o instante em que a
    /// próxima invisível volta a ser entregável, para o long polling saber até quando dormir.
    fn take(
        &self,
        view: &TopicView,
        max: usize,
        visibility_secs: u64,
    ) -> (Vec<Delivered>, Option<u64>) {
        let now = self.clock.now_millis();
        let mut queues = lock(&self.queues);
        self.settle(&mut queues, view, now);
        let Some(queue) = queues.get_mut(&view.definition.name) else {
            return (Vec::new(), None);
        };
        let (mut delivered, mut next_visible) = (Vec::new(), None);
        for (id, message) in &mut queue.messages {
            if delivered.len() >= max {
                break;
            }
            if message.invisible_until_ms > now {
                next_visible = Some(next_visible.map_or(message.invisible_until_ms, |n: u64| {
                    n.min(message.invisible_until_ms)
                }));
                continue;
            }
            message.delivery_count += 1;
            message.invisible_until_ms = now + visibility_secs * 1000;
            let receipt_token = self.ids.new_at(now);
            message.receipt_token = Some(receipt_token.clone());
            delivered.push(Delivered {
                id: id.clone(),
                body: Body::render(&message.content_type, &message.body),
                content_type: message.content_type.as_str().to_owned(),
                group_id: message.group_id.clone(),
                receipt_token,
                delivery_count: message.delivery_count,
                published_ms: message.published_ms,
                invisible_until_ms: message.invisible_until_ms,
                dead_lettered_ms: message.dead_lettered_ms,
                dead_letter_reason: message.dead_letter_reason,
            });
        }
        (delivered, next_visible)
    }
}

fn too_large(limit: u64) -> TabaError {
    TabaError::new(
        ErrorCode::MessageTooLarge,
        format!("O corpo passa do limite de {limit} bytes do tópico."),
    )
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
