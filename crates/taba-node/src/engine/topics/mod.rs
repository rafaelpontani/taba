//! The node's topic registry (doc 02, section 3.3): every definition stays in memory and is
//! written to disk before taking effect. In `mode=development` the node is the whole cluster, so
//! the "register + commit" boils down to writing and applying here; in the cluster, this registry
//! becomes the side that receives the commit.
//!
//! The dead-letter queue has no definition of its own: it is derived from the source queue, and for
//! that reason it is neither stored nor kept, only built on read (doc 13, section 12.10).

mod list;
mod rules;

/// A volatile queue and what the idleness clock needs from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolatileQueue {
    pub name: String,
    pub id: String,
    pub created_ms: u64,
    pub idle_timeout_secs: u64,
}

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde_json::{Value, json};

pub use list::ListResult;
pub use rules::{CreateTopic, Kind, ListQuery, Patch, TopicRules, UpdateTopic};

use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id;
use crate::core::topic::{QueueSpec, Spec, TopicDefinition, dead_letter_id};
use crate::core::validation;
use crate::storage::topics::TopicStore;

/// A topic as the API shows it: the definition plus what is derived from it.
#[derive(Debug, Clone)]
pub struct TopicView {
    pub definition: TopicDefinition,
    /// `Some(queue)` when it is the dead-letter of `queue`.
    pub dead_letter_of: Option<String>,
    /// `maxMessageBytes`, `ttl` and `maxDeliveries` in force, with the default applied.
    effective: Option<[u64; 3]>,
}

impl TopicView {
    /// `maxMessageBytes` in force; `None` on a distributor.
    pub fn max_message_bytes(&self) -> Option<u64> {
        self.effective.map(|e| e[0])
    }

    /// `maxDeliveries` in force; `None` on a distributor.
    pub fn max_deliveries(&self) -> Option<u64> {
        self.effective.map(|e| e[2])
    }

    /// `ttl` in force, in seconds; `None` on a distributor.
    pub fn ttl_secs(&self) -> Option<u64> {
        self.effective.map(|e| e[1])
    }

    /// The API's `Topic` (doc 06a, "DTOs").
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

/// A change to the topic table, as the cluster propagates it to **all** nodes (doc 02, 3.3).
#[derive(Debug, Clone)]
pub enum TopicChange {
    Create(TopicDefinition),
    Update(TopicDefinition),
    Delete {
        name: String,
        id: String,
        /// Only the managing node of a volatile queue: the `id` the topic must have.
        expected_id: Option<String>,
    },
}

/// Who propagates a change to the other nodes before it takes effect (register + commit with
/// confirmation from all). At the end, the change has already been applied **on this node** too, through the commit.
pub trait TopicReplicator: Send + Sync + 'static {
    fn replicate(
        &self,
        change: TopicChange,
    ) -> Pin<Box<dyn Future<Output = Result<(), TabaError>> + Send + '_>>;
}

/// What changed in this node's topic table (the commit of a creation, change or removal).
#[derive(Debug, Clone)]
pub enum TopicEvent {
    Saved(TopicDefinition),
    Deleted { name: String, id: String },
}

/// Who follows the table's changes (the S3 backup). Never waits or fails.
pub type TopicHook = Arc<dyn Fn(&TopicEvent) + Send + Sync>;

pub struct Topics {
    rules: TopicRules,
    store: Arc<dyn TopicStore>,
    clock: Arc<dyn Clock>,
    map: RwLock<BTreeMap<String, TopicDefinition>>,
    /// One change at a time: validates, writes to disk and only then applies in memory.
    writer: tokio::sync::Mutex<()>,
    removed_hooks: Mutex<Vec<RemovedHook>>,
    event_hooks: Mutex<Vec<TopicHook>>,
    /// In the cluster: the propagation to all nodes.
    replicator: OnceLock<Arc<dyn TopicReplicator>>,
}

/// Called after a queue is removed, with its name and its dead-letter's.
pub type RemovedHook = Arc<dyn Fn(&[String]) + Send + Sync>;

fn field_error(code: ErrorCode, field: &str, message: &str) -> TabaError {
    TabaError::new(code, message).with_detail(field, code, message)
}

/// Ensures the field in `details` on an error of a validation that only brings the code.
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
        &format!("`{field}` does not apply to {what}."),
    )
}

fn out_of_range(field: &str, message: &str) -> TabaError {
    field_error(ErrorCode::ValidationOutOfRange, field, message)
}

/// A number within the range; negative is also outside the range.
fn in_range(field: &str, value: i64, range: (u64, u64)) -> Result<u64, TabaError> {
    match u64::try_from(value) {
        Ok(v) => validation::in_range(field, v, range).map(|()| v),
        Err(_) => Err(out_of_range(
            field,
            &format!("{field} must be between {} and {}.", range.0, range.1),
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
            event_hooks: Mutex::new(Vec::new()),
            replicator: OnceLock::new(),
        }
    }

    /// The definitions the node database holds, whether or not they are in memory (a cluster node does not load them
    /// on start: it takes them over only if it starts alone, doc 37, section 4.9).
    pub async fn stored(&self) -> io::Result<Vec<TopicDefinition>> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.load_all())
            .await
            .map_err(io::Error::other)?
    }

    /// Deletes every definition from the node database (the node joins a cluster and receives its definitions).
    pub async fn forget_stored(&self) {
        let store = self.store.clone();
        let result = tokio::task::spawn_blocking(move || {
            for definition in store.load_all()? {
                store.delete(&definition.name)?;
            }
            Ok::<(), io::Error>(())
        })
        .await;
        if !matches!(result, Ok(Ok(()))) {
            tracing::warn!("could not clear the stored topic definitions");
        }
    }

    /// Registers who needs to know when a topic disappears (the messages go with it).
    pub fn on_removed(&self, hook: RemovedHook) {
        self.removed_hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(hook);
    }

    /// Registers who follows each change to the table (creation, change, removal).
    pub fn on_event(&self, hook: TopicHook) {
        self.event_hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(hook);
    }

    fn emit(&self, event: &TopicEvent) {
        let hooks = self
            .event_hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for hook in hooks {
            hook(event);
        }
    }

    /// Attaches the propagation to the other nodes (`mode=production`). Only the first call counts.
    pub fn set_replicator(&self, replicator: Arc<dyn TopicReplicator>) {
        let _ = self.replicator.set(replicator);
    }

    /// A change's `register`, on the node that receives it: it can only be applied if the current state
    /// allows it (a name that already exists, a topic that does not exist, a different expected `id`).
    pub fn check_change(&self, change: &TopicChange) -> Result<(), TabaError> {
        let map = self.read();
        match change {
            TopicChange::Create(def) if map.contains_key(&def.name) => Err(TabaError::new(
                ErrorCode::TopicAlreadyExists,
                "A topic with this name already exists.",
            )),
            TopicChange::Update(def) if !map.contains_key(&def.name) => Err(Self::not_found()),
            TopicChange::Delete {
                name, expected_id, ..
            } => match map.get(name) {
                None => Err(Self::not_found()),
                Some(current) if expected_id.as_ref().is_some_and(|id| *id != current.id) => {
                    Err(TabaError::new(
                        ErrorCode::ClusterConflict,
                        "The current topic has another id: it was deleted and recreated.",
                    ))
                }
                Some(_) => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// The commit of a creation or change: writes and takes effect, without the write lock (whoever
    /// coordinates already holds it). Repeating is harmless.
    pub async fn apply_save(&self, definition: TopicDefinition) -> Result<(), TabaError> {
        let to_save = definition.clone();
        self.persist(move |store| store.save(&to_save)).await?;
        self.write()
            .insert(definition.name.clone(), definition.clone());
        self.emit(&TopicEvent::Saved(definition));
        Ok(())
    }

    /// The commit of a removal: deletes the definition and tells whoever keeps the topic's messages.
    /// Deleting what does not exist is no error.
    pub async fn apply_delete(&self, name: &str) -> Result<(), TabaError> {
        let Some(current) = self.read().get(name).cloned() else {
            return Ok(());
        };
        let to_delete = name.to_owned();
        self.persist(move |store| store.delete(&to_delete)).await?;
        self.write().remove(name);
        self.emit(&TopicEvent::Deleted {
            name: name.to_owned(),
            id: current.id.clone(),
        });
        let mut gone = vec![name.to_owned()];
        if current.queue().is_some() {
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

    /// The definitions with `id` from `from` (inclusive), in `id` order, at most
    /// `limit` plus one (the extra one says whether there are more): it is the `GET /cluster/topics` stream.
    pub fn definitions_from(
        &self,
        from: Option<&str>,
        limit: usize,
    ) -> (Vec<TopicDefinition>, bool) {
        let mut all: Vec<TopicDefinition> = self
            .read()
            .values()
            .filter(|d| from.is_none_or(|f| d.id.as_str() >= f))
            .cloned()
            .collect();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        let more = all.len() > limit;
        all.truncate(limit);
        (all, more)
    }

    /// What the new node receives from the topic stream: stores the definition it does not have yet.
    pub async fn load_remote(&self, definition: TopicDefinition) -> Result<(), TabaError> {
        let same = self
            .read()
            .get(&definition.name)
            .is_some_and(|d| d.id == definition.id);
        if same {
            return Ok(());
        }
        self.apply_save(definition).await
    }

    /// A topic's stored definition (the one the cluster replicates), without the dead-letter.
    pub fn definition(&self, name: &str) -> Option<TopicDefinition> {
        self.read().get(name).cloned()
    }

    /// The topic names (queues and distributors).
    pub fn names(&self) -> Vec<String> {
        self.read().keys().cloned().collect()
    }

    /// The volatile queues, for the idleness clock.
    pub fn volatile_queues(&self) -> Vec<VolatileQueue> {
        self.read()
            .values()
            .filter_map(|d| {
                let queue = d.queue()?;
                queue.volatile.then(|| VolatileQueue {
                    name: d.name.clone(),
                    id: d.id.clone(),
                    created_ms: d.created_ms,
                    idle_timeout_secs: queue
                        .idle_timeout
                        .unwrap_or(self.rules.idle_timeout_default),
                })
            })
            .collect()
    }

    /// `true` if a topic with this `id` exists.
    pub fn has_id(&self, id: &str) -> bool {
        self.read().values().any(|d| d.id == id)
    }

    pub fn rules(&self) -> &TopicRules {
        &self.rules
    }

    /// How many topics (queues and distributors) exist; dead-letters do not count.
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

    /// A queue's dead-letter: the values are the source's, read-only, and it
    /// is not FIFO (doc 13, section 12.10).
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

    /// The source queue, if `name` is the name of the dead-letter of a queue that exists.
    fn dead_letter_source(&self, name: &str) -> Option<TopicDefinition> {
        let source = name.strip_suffix(self.rules.dead_letter_suffix.as_str())?;
        self.read()
            .get(source)
            .filter(|d| d.queue().is_some())
            .cloned()
    }

    fn not_found() -> TabaError {
        TabaError::new(ErrorCode::TopicNotFound, "The topic does not exist.")
    }

    /// `404` if it does not exist, `409` if it is the dead-letter (read-only).
    fn writable(&self, name: &str) -> Result<TopicDefinition, TabaError> {
        if let Some(definition) = self.read().get(name) {
            return Ok(definition.clone());
        }
        if self.dead_letter_source(name).is_some() {
            return Err(TabaError::new(
                ErrorCode::TopicReadOnly,
                "The dead-letter is read-only: change the source queue.",
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
                tracing::error!(error = %e, "failed to write the topic definition");
                Err(TabaError::internal())
            }
            Err(e) => {
                tracing::error!(error = %e, "writing the topic definition failed");
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

    /// The queues a distributor with these prefixes reaches: those that have one of them at the
    /// start of the name, in name order. The dead-letter has no stored definition, so it
    /// never comes in here, as doc 13, section 12.10 dictates.
    pub fn queues_with_prefix(&self, prefixes: &[String]) -> Vec<TopicView> {
        self.read()
            .values()
            .filter(|d| d.queue().is_some() && prefixes.iter().any(|p| d.name.starts_with(p)))
            .map(|d| self.view(d))
            .collect()
    }

    pub async fn create(&self, request: CreateTopic) -> Result<TopicView, TabaError> {
        self.create_checked(request, false).await
    }

    /// Like `create`; with `enforce_volatile_limit` (the `autoCreate`), it refuses the volatile queue that
    /// would exceed `topics.volatile.max-count` (`422`). The count is local and done under the
    /// write lock (doc 15, section 14.4).
    pub async fn create_checked(
        &self,
        request: CreateTopic,
        enforce_volatile_limit: bool,
    ) -> Result<TopicView, TabaError> {
        let (spec, tags) = self.validate_create(&request)?;
        let _writer = self.writer.lock().await;
        if enforce_volatile_limit
            && matches!(&spec, Spec::Queue(queue) if queue.volatile)
            && let Some(max) = self.rules.volatile_max_count
            && self.volatile_queues().len() >= max as usize
        {
            return Err(TabaError::new(
                ErrorCode::TopicVolatileLimit,
                "The volatile queue ceiling (topics.volatile.max-count) was reached.",
            ));
        }
        if self.read().contains_key(&request.name) {
            return Err(TabaError::new(
                ErrorCode::TopicAlreadyExists,
                "A topic with this name already exists.",
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
        match self.replicator.get() {
            Some(replicator) => {
                replicator
                    .replicate(TopicChange::Create(definition.clone()))
                    .await?;
            }
            None => self.apply_save(definition.clone()).await?,
        }
        Ok(self.view(&definition))
    }

    pub async fn update(&self, name: &str, request: UpdateTopic) -> Result<TopicView, TabaError> {
        let _writer = self.writer.lock().await;
        let current = self.writable(name)?;
        let updated = self.apply_update(current, request)?;
        match self.replicator.get() {
            Some(replicator) => {
                replicator
                    .replicate(TopicChange::Update(updated.clone()))
                    .await?;
            }
            None => self.apply_save(updated.clone()).await?,
        }
        Ok(self.view(&updated))
    }

    /// Removes the topic. The messages go with it, when there are any. Always works,
    /// without checking pending messages (doc 02, section 3.3).
    pub async fn delete(&self, name: &str) -> Result<(), TabaError> {
        self.delete_checked(name, None).await
    }

    /// Like `delete`, but only deletes if the topic still has the expected `id`: a queue deleted and
    /// **recreated** along the way is not deleted by mistake (doc 15, section 14.5).
    pub async fn delete_expected(&self, name: &str, expected_id: &str) -> Result<(), TabaError> {
        self.delete_checked(name, Some(expected_id)).await
    }

    async fn delete_checked(&self, name: &str, expected_id: Option<&str>) -> Result<(), TabaError> {
        let _writer = self.writer.lock().await;
        let current = self.writable(name)?;
        if expected_id.is_some_and(|id| id != current.id) {
            return Err(Self::not_found());
        }
        let name = current.name;
        match self.replicator.get() {
            Some(replicator) => {
                replicator
                    .replicate(TopicChange::Delete {
                        name,
                        id: current.id,
                        expected_id: expected_id.map(str::to_owned),
                    })
                    .await
            }
            None => self.apply_delete(&name).await,
        }
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
                        return Err(not_applicable(field, "a distributor topic"));
                    }
                }
                let prefixes = request.prefixes.clone().unwrap_or_default();
                validation::prefixes(&prefixes)?;
                Spec::Distributor { prefixes }
            }
            Kind::Queue => {
                if request.prefixes.is_some() {
                    return Err(not_applicable("prefixes", "a queue"));
                }
                let volatile = request.volatile.unwrap_or(false);
                if request.idle_timeout.is_some() && !volatile {
                    return Err(not_applicable(
                        "idleTimeout",
                        "a queue that is not volatile",
                    ));
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

    /// `maxMessageBytes`: from 1 up to `message.hard-limit` (if there is one).
    fn max_message_bytes(&self, value: i64) -> Result<u64, TabaError> {
        let max = self.rules.hard_limit.unwrap_or(u64::MAX);
        let v = in_range("maxMessageBytes", value, (1, u64::MAX))?;
        if v > max {
            return Err(field_error(
                ErrorCode::ValidationMaxBytesAboveHardLimit,
                "maxMessageBytes",
                &format!("maxMessageBytes cannot exceed {max} (message.hard-limit)."),
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
                &format!("`{field}` cannot be changed."),
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
                        return Err(not_applicable(field, "a distributor topic"));
                    }
                }
                if let Some(new) = request.prefixes {
                    validation::prefixes(&new)?;
                    *prefixes = new;
                }
            }
            Spec::Queue(queue) => {
                if request.prefixes.is_some() {
                    return Err(not_applicable("prefixes", "a queue"));
                }
                if let Some(wanted) = request.idle_timeout {
                    if !queue.volatile {
                        return Err(not_applicable(
                            "idleTimeout",
                            "a queue that is not volatile",
                        ));
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
