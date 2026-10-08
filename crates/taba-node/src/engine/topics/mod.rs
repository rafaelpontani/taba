//! O registro de tópicos do nó (doc 02, seção 3.3): toda definição fica em memória e é
//! gravada em disco antes de valer. Em `mode=development` o nó é o cluster inteiro, então
//! o "registro + commit" se resume a gravar e aplicar aqui; no cluster, este registro
//! passa a ser o lado que recebe o commit.
//!
//! A fila de dead-letter não tem definição própria: é derivada da fila de origem, e por
//! isso não é gravada nem guardada, só montada na leitura (doc 13, seção 12.10).

mod list;
mod rules;

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde_json::{Value, json};

pub use list::ListResult;
pub use rules::{CreateTopic, Kind, ListQuery, Patch, TopicRules, UpdateTopic};

use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id;
use crate::core::topic::{QueueSpec, Spec, TopicDefinition, dead_letter_id};
use crate::core::validation;
use crate::storage::topics::TopicStore;

/// Um tópico como a API o mostra: a definição mais o que se deriva dela.
#[derive(Debug, Clone)]
pub struct TopicView {
    pub definition: TopicDefinition,
    /// `Some(fila)` quando é a dead-letter de `fila`.
    pub dead_letter_of: Option<String>,
    /// `maxMessageBytes`, `ttl` e `maxDeliveries` em vigor, com o padrão aplicado.
    effective: Option<[u64; 3]>,
}

impl TopicView {
    /// `maxMessageBytes` em vigor; `None` num distribuidor.
    pub fn max_message_bytes(&self) -> Option<u64> {
        self.effective.map(|e| e[0])
    }

    /// `maxDeliveries` em vigor; `None` num distribuidor.
    pub fn max_deliveries(&self) -> Option<u64> {
        self.effective.map(|e| e[2])
    }

    /// `ttl` em vigor, em segundos; `None` num distribuidor.
    pub fn ttl_secs(&self) -> Option<u64> {
        self.effective.map(|e| e[1])
    }

    /// O `Topic` da API (doc 06a, "DTOs").
    pub fn to_json(&self) -> Value {
        let mut json = self.definition.to_json();
        if let (Some([bytes, ttl, deliveries]), Some(object)) =
            (self.effective, json.as_object_mut())
        {
            object.insert(
                "effective".into(),
                json!({ "maxMessageBytes": bytes, "ttl": ttl, "maxDeliveries": deliveries }),
            );
        }
        if let (Some(source), Some(object)) = (&self.dead_letter_of, json.as_object_mut()) {
            object.insert("deadLetter".into(), json!(true));
            object.insert("source".into(), json!(source));
        }
        json
    }
}

pub struct Topics {
    rules: TopicRules,
    store: Arc<dyn TopicStore>,
    clock: Arc<dyn Clock>,
    map: RwLock<BTreeMap<String, TopicDefinition>>,
    /// Uma alteração por vez: valida, grava em disco e só então aplica em memória.
    writer: tokio::sync::Mutex<()>,
    removed_hooks: Mutex<Vec<RemovedHook>>,
}

/// Chamada depois que uma fila é removida, com o nome dela e o da sua dead-letter.
pub type RemovedHook = Arc<dyn Fn(&[String]) + Send + Sync>;

fn field_error(code: ErrorCode, field: &str, message: &str) -> TabaError {
    TabaError::new(code, message).with_detail(field, code, message)
}

/// Garante o campo em `details` num erro de uma validação que só traz o código.
fn with_field(error: TabaError, field: &str) -> TabaError {
    if error.details.is_empty() {
        let (code, message) = (error.code, error.message.clone());
        error.with_detail(field, code, message)
    } else {
        error
    }
}

fn not_applicable(field: &str, what: &str) -> TabaError {
    field_error(
        ErrorCode::ValidationNotApplicable,
        field,
        &format!("`{field}` não vale para {what}."),
    )
}

fn out_of_range(field: &str, message: &str) -> TabaError {
    field_error(ErrorCode::ValidationOutOfRange, field, message)
}

/// Número dentro da faixa; negativo também é fora da faixa.
fn in_range(field: &str, value: i64, range: (u64, u64)) -> Result<u64, TabaError> {
    match u64::try_from(value) {
        Ok(v) => validation::in_range(field, v, range).map(|()| v),
        Err(_) => Err(out_of_range(
            field,
            &format!("{field} deve estar entre {} e {}.", range.0, range.1),
        )),
    }
}

impl Topics {
    pub fn new(
        rules: TopicRules,
        store: Arc<dyn TopicStore>,
        clock: Arc<dyn Clock>,
        definitions: Vec<TopicDefinition>,
    ) -> Self {
        let map = definitions
            .into_iter()
            .map(|d| (d.name.clone(), d))
            .collect();
        Self {
            rules,
            store,
            clock,
            map: RwLock::new(map),
            writer: tokio::sync::Mutex::new(()),
            removed_hooks: Mutex::new(Vec::new()),
        }
    }

    /// Abre o registro. Com `load`, retoma as definições gravadas em disco.
    pub async fn open(
        rules: TopicRules,
        store: Arc<dyn TopicStore>,
        clock: Arc<dyn Clock>,
        load: bool,
    ) -> io::Result<Self> {
        let definitions = if load {
            let store = store.clone();
            tokio::task::spawn_blocking(move || store.load_all())
                .await
                .map_err(io::Error::other)??
        } else {
            Vec::new()
        };
        Ok(Self::new(rules, store, clock, definitions))
    }

    /// Registra quem precisa saber quando um tópico some (as mensagens saem junto).
    pub fn on_removed(&self, hook: RemovedHook) {
        self.removed_hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(hook);
    }

    pub fn rules(&self) -> &TopicRules {
        &self.rules
    }

    /// Quantos tópicos (filas e distribuidores) existem; as dead-letters não contam.
    pub fn count(&self) -> usize {
        self.read().len()
    }

    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<String, TopicDefinition>> {
        self.map.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, TopicDefinition>> {
        self.map.write().unwrap_or_else(|e| e.into_inner())
    }

    fn view(&self, definition: &TopicDefinition) -> TopicView {
        let effective = definition.queue().map(|q| {
            [
                q.max_message_bytes
                    .unwrap_or(self.rules.max_message_bytes_default),
                q.ttl.unwrap_or(self.rules.ttl_default),
                q.max_deliveries
                    .unwrap_or(self.rules.max_deliveries_default),
            ]
        });
        TopicView {
            definition: definition.clone(),
            dead_letter_of: None,
            effective,
        }
    }

    /// A dead-letter de uma fila: os valores são os da origem, somente leitura, e ela
    /// não é FIFO (doc 13, seção 12.10).
    fn dead_letter_view(&self, source: &TopicDefinition) -> TopicView {
        let mut view = self.view(source);
        view.definition.name = format!("{}{}", source.name, self.rules.dead_letter_suffix);
        view.definition.id = dead_letter_id(&source.id);
        if let Spec::Queue(q) = &mut view.definition.spec {
            q.fifo = false;
        }
        view.dead_letter_of = Some(source.name.clone());
        view
    }

    /// A fila de origem, se `name` é o nome da dead-letter de uma fila que existe.
    fn dead_letter_source(&self, name: &str) -> Option<TopicDefinition> {
        let source = name.strip_suffix(self.rules.dead_letter_suffix.as_str())?;
        self.read()
            .get(source)
            .filter(|d| d.queue().is_some())
            .cloned()
    }

    fn not_found() -> TabaError {
        TabaError::new(ErrorCode::TopicNotFound, "O tópico não existe.")
    }

    /// `404` se não existe, `409` se é a dead-letter (somente leitura).
    fn writable(&self, name: &str) -> Result<TopicDefinition, TabaError> {
        if let Some(definition) = self.read().get(name) {
            return Ok(definition.clone());
        }
        if self.dead_letter_source(name).is_some() {
            return Err(TabaError::new(
                ErrorCode::TopicReadOnly,
                "A dead-letter é somente leitura: altere a fila de origem.",
            ));
        }
        Err(Self::not_found())
    }

    async fn persist<F>(&self, op: F) -> Result<(), TabaError>
    where
        F: FnOnce(&dyn TopicStore) -> io::Result<()> + Send + 'static,
    {
        let store = self.store.clone();
        match tokio::task::spawn_blocking(move || op(store.as_ref())).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                tracing::error!(error = %e, "falha ao gravar a definição do tópico");
                Err(TabaError::internal())
            }
            Err(e) => {
                tracing::error!(error = %e, "a gravação da definição do tópico falhou");
                Err(TabaError::internal())
            }
        }
    }

    pub fn get(&self, name: &str) -> Result<TopicView, TabaError> {
        if let Some(definition) = self.read().get(name) {
            return Ok(self.view(definition));
        }
        self.dead_letter_source(name)
            .map(|source| self.dead_letter_view(&source))
            .ok_or_else(Self::not_found)
    }

    /// As filas que um distribuidor com estes prefixos alcança: as que têm um deles no
    /// começo do nome, em ordem de nome. A dead-letter não tem definição guardada, então
    /// nunca entra aqui, como manda o doc 13, seção 12.10.
    pub fn queues_with_prefix(&self, prefixes: &[String]) -> Vec<TopicView> {
        self.read()
            .values()
            .filter(|d| d.queue().is_some() && prefixes.iter().any(|p| d.name.starts_with(p)))
            .map(|d| self.view(d))
            .collect()
    }

    pub async fn create(&self, request: CreateTopic) -> Result<TopicView, TabaError> {
        let (spec, tags) = self.validate_create(&request)?;
        let _writer = self.writer.lock().await;
        if self.read().contains_key(&request.name) {
            return Err(TabaError::new(
                ErrorCode::TopicAlreadyExists,
                "Já existe um tópico com este nome.",
            ));
        }
        let now = self.clock.now_millis();
        let definition = TopicDefinition {
            id: id::new_at(now),
            name: request.name,
            created_ms: now,
            tags,
            spec,
        };
        let to_save = definition.clone();
        self.persist(move |store| store.save(&to_save)).await?;
        self.write()
            .insert(definition.name.clone(), definition.clone());
        Ok(self.view(&definition))
    }

    pub async fn update(&self, name: &str, request: UpdateTopic) -> Result<TopicView, TabaError> {
        let _writer = self.writer.lock().await;
        let current = self.writable(name)?;
        let updated = self.apply_update(current, request)?;
        let to_save = updated.clone();
        self.persist(move |store| store.save(&to_save)).await?;
        self.write().insert(updated.name.clone(), updated.clone());
        Ok(self.view(&updated))
    }

    /// Remove o tópico. As mensagens saem junto, quando existirem. Sempre funciona,
    /// sem checar mensagens pendentes (doc 02, seção 3.3).
    pub async fn delete(&self, name: &str) -> Result<(), TabaError> {
        let _writer = self.writer.lock().await;
        let current = self.writable(name)?;
        let is_queue = current.queue().is_some();
        let name = current.name;
        let to_delete = name.clone();
        self.persist(move |store| store.delete(&to_delete)).await?;
        self.write().remove(&name);
        let mut gone = vec![name.clone()];
        if is_queue {
            gone.push(format!("{name}{}", self.rules.dead_letter_suffix));
        }
        let hooks = self
            .removed_hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for hook in hooks {
            hook(&gone);
        }
        Ok(())
    }

    fn validate_create(
        &self,
        request: &CreateTopic,
    ) -> Result<(Spec, BTreeMap<String, String>), TabaError> {
        validation::topic_name(&request.name, &self.rules.dead_letter_suffix)
            .map_err(|e| with_field(e, "name"))?;
        let tags = request.tags.clone().unwrap_or_default();
        validation::tags(&tags)?;

        let spec = match request.kind {
            Kind::Distributor => {
                for (field, present) in [
                    ("volatile", request.volatile.is_some()),
                    ("idleTimeout", request.idle_timeout.is_some()),
                    ("fifo", request.fifo.is_some()),
                    ("maxMessageBytes", request.max_message_bytes.is_some()),
                    ("ttl", request.ttl.is_some()),
                    ("maxDeliveries", request.max_deliveries.is_some()),
                ] {
                    if present {
                        return Err(not_applicable(field, "um tópico distribuidor"));
                    }
                }
                let prefixes = request.prefixes.clone().unwrap_or_default();
                validation::prefixes(&prefixes)?;
                Spec::Distributor { prefixes }
            }
            Kind::Queue => {
                if request.prefixes.is_some() {
                    return Err(not_applicable("prefixes", "uma fila"));
                }
                let volatile = request.volatile.unwrap_or(false);
                if request.idle_timeout.is_some() && !volatile {
                    return Err(not_applicable("idleTimeout", "uma fila que não é volátil"));
                }
                let idle_timeout = if volatile {
                    let wanted = request
                        .idle_timeout
                        .unwrap_or(self.rules.idle_timeout_default as i64);
                    Some(in_range(
                        "idleTimeout",
                        wanted,
                        validation::IDLE_TIMEOUT_SECS,
                    )?)
                } else {
                    None
                };
                let fifo = request.fifo.unwrap_or(false);
                if fifo && self.rules.fifo_needs_replication {
                    return Err(TabaError::new(
                        ErrorCode::TopicFifoNeedsReplication,
                        "A fila FIFO exige replication.factor de pelo menos 2.",
                    ));
                }
                Spec::Queue(QueueSpec {
                    volatile,
                    idle_timeout,
                    fifo,
                    max_message_bytes: request
                        .max_message_bytes
                        .map(|v| self.max_message_bytes(v))
                        .transpose()?,
                    ttl: request
                        .ttl
                        .map(|v| in_range("ttl", v, validation::TTL_SECS))
                        .transpose()?,
                    max_deliveries: request
                        .max_deliveries
                        .map(|v| in_range("maxDeliveries", v, validation::MAX_DELIVERIES))
                        .transpose()?,
                })
            }
        };
        Ok((spec, tags))
    }

    /// `maxMessageBytes`: de 1 até `message.hard-limit` (se houver).
    fn max_message_bytes(&self, value: i64) -> Result<u64, TabaError> {
        let max = self.rules.hard_limit.unwrap_or(u64::MAX);
        let v = in_range("maxMessageBytes", value, (1, u64::MAX))?;
        if v > max {
            return Err(field_error(
                ErrorCode::ValidationMaxBytesAboveHardLimit,
                "maxMessageBytes",
                &format!("maxMessageBytes não pode passar de {max} (message.hard-limit)."),
            ));
        }
        Ok(v)
    }

    fn apply_update(
        &self,
        mut topic: TopicDefinition,
        request: UpdateTopic,
    ) -> Result<TopicDefinition, TabaError> {
        if let Some(field) = request.immutable_present.first() {
            return Err(field_error(
                ErrorCode::ValidationImmutableField,
                field,
                &format!("`{field}` não pode ser alterado."),
            ));
        }
        if let Some(tags) = request.tags {
            validation::tags(&tags)?;
            topic.tags = tags;
        }
        match &mut topic.spec {
            Spec::Distributor { prefixes } => {
                for (field, present) in [
                    ("idleTimeout", request.idle_timeout.is_some()),
                    (
                        "maxMessageBytes",
                        request.max_message_bytes != Patch::Absent,
                    ),
                    ("ttl", request.ttl != Patch::Absent),
                    ("maxDeliveries", request.max_deliveries != Patch::Absent),
                ] {
                    if present {
                        return Err(not_applicable(field, "um tópico distribuidor"));
                    }
                }
                if let Some(new) = request.prefixes {
                    validation::prefixes(&new)?;
                    *prefixes = new;
                }
            }
            Spec::Queue(queue) => {
                if request.prefixes.is_some() {
                    return Err(not_applicable("prefixes", "uma fila"));
                }
                if let Some(wanted) = request.idle_timeout {
                    if !queue.volatile {
                        return Err(not_applicable("idleTimeout", "uma fila que não é volátil"));
                    }
                    queue.idle_timeout = Some(in_range(
                        "idleTimeout",
                        wanted,
                        validation::IDLE_TIMEOUT_SECS,
                    )?);
                }
                match request.max_message_bytes {
                    Patch::Absent => {}
                    Patch::Null => queue.max_message_bytes = None,
                    Patch::Value(v) => queue.max_message_bytes = Some(self.max_message_bytes(v)?),
                }
                match request.ttl {
                    Patch::Absent => {}
                    Patch::Null => queue.ttl = None,
                    Patch::Value(v) => {
                        queue.ttl = Some(in_range("ttl", v, validation::TTL_SECS)?);
                    }
                }
                match request.max_deliveries {
                    Patch::Absent => {}
                    Patch::Null => queue.max_deliveries = None,
                    Patch::Value(v) => {
                        queue.max_deliveries =
                            Some(in_range("maxDeliveries", v, validation::MAX_DELIVERIES)?);
                    }
                }
            }
        }
        Ok(topic)
    }
}

#[cfg(test)]
mod tests;
