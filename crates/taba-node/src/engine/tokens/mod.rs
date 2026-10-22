//! The node's access-token registry (doc 06a, section 8.1.3; doc 02, section 3.4): every token
//! stays in memory (validating is a local query, without the network) and is written to disk before
//! taking effect. In the cluster this registry becomes the side that receives the commit; on a single
//! node, creating, changing and revoking boil down to writing and applying here.
//!
//! The secret is never stored: only the salted hash (`core::token`).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::{Deserialize, Serialize};

use super::topics::Patch;
use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id::IdGenerator;
use crate::core::pagination;
use crate::core::token::{self, Permission, TokenRecord};
use crate::core::validation;
use crate::storage::tokens::TokenStore;

/// The `id` of the token `auth.dev-token` creates; the presented token is `tb_dev.<secret>`.
pub const DEV_TOKEN_ID: &str = "dev";

#[derive(Debug, Clone)]
pub struct TokenRules {
    pub default_limit: u32,
    pub max_limit: u32,
}

impl TokenRules {
    pub fn from_config(config: &Config) -> Self {
        Self {
            default_limit: config.pagination.default_limit,
            max_limit: config.pagination.max_limit,
        }
    }
}

/// `POST /tokens`, already read from the JSON.
#[derive(Debug, Clone, Default)]
pub struct CreateToken {
    pub name: String,
    pub permissions: Vec<String>,
    pub topic_prefixes: Vec<String>,
    pub expires_ms: Option<u64>,
}

/// `PATCH /tokens/{id}`: only the name and the deadline; the rest is immutable.
#[derive(Debug, Clone, Default)]
pub struct UpdateToken {
    /// Immutable fields that came in the body (`permissions`, `topicPrefixes`...).
    pub immutable_present: Vec<String>,
    pub name: Option<String>,
    pub expires_ms: Patch<u64>,
}

/// The newly created token: the record and the full secret, which only appears here.
pub struct Created {
    pub record: TokenRecord,
    pub token: String,
}

impl std::fmt::Debug for Created {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Created")
            .field("record", &self.record)
            .field("token", &"<oculto>")
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    pub limit: Option<String>,
    pub cursor: Option<String>,
    pub sort: Option<String>,
    pub name: Option<String>,
}

pub struct ListResult {
    pub items: Vec<TokenRecord>,
    pub next_cursor: Option<String>,
}

/// A change to the token table, as the cluster propagates it to **all** nodes (doc 02, 3.4).
#[derive(Debug, Clone)]
pub enum TokenChange {
    Create(TokenRecord),
    Update(TokenRecord),
    Revoke(String),
}

/// Who propagates a change to the other nodes before it takes effect. At the end, the change has already been applied
/// **on this node** too, through the commit.
pub trait TokenReplicator: Send + Sync + 'static {
    fn replicate(
        &self,
        change: TokenChange,
    ) -> Pin<Box<dyn Future<Output = Result<(), TabaError>> + Send + '_>>;
}

/// What changed in this node's token table (the commit of a creation, change or revocation).
#[derive(Debug, Clone)]
pub enum TokenEvent {
    Saved(TokenRecord),
    Revoked(String),
}

/// Who follows the table's changes (the S3 backup). Never waits or fails.
pub type TokenHook = Arc<dyn Fn(&TokenEvent) + Send + Sync>;

pub struct Tokens {
    rules: TokenRules,
    store: Arc<dyn TokenStore>,
    clock: Arc<dyn Clock>,
    ids: IdGenerator,
    map: RwLock<HashMap<String, TokenRecord>>,
    /// One write at a time: writes to disk and only then applies in memory.
    writer: tokio::sync::Mutex<()>,
    /// In the cluster: the propagation to all nodes.
    replicator: OnceLock<Arc<dyn TokenReplicator>>,
    event_hooks: std::sync::Mutex<Vec<TokenHook>>,
}

fn not_found() -> TabaError {
    TabaError::new(ErrorCode::TokenNotFound, "The token does not exist.")
}

fn field_error(code: ErrorCode, field: &str, message: &str) -> TabaError {
    TabaError::new(code, message).with_detail(field, code, message)
}

fn disk_error(what: &str, e: &io::Error) -> TabaError {
    tracing::error!(error = %e, "{what}");
    TabaError::internal()
}

impl Tokens {
    pub fn new(
        rules: TokenRules,
        store: Arc<dyn TokenStore>,
        clock: Arc<dyn Clock>,
        tokens: Vec<TokenRecord>,
    ) -> Self {
        Self {
            rules,
            store,
            clock,
            ids: IdGenerator::new(),
            map: RwLock::new(tokens.into_iter().map(|t| (t.id.clone(), t)).collect()),
            writer: tokio::sync::Mutex::new(()),
            replicator: OnceLock::new(),
            event_hooks: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// The tokens the node database holds, whether or not they are in memory (see `Topics::stored`).
    pub async fn stored(&self) -> io::Result<Vec<TokenRecord>> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.load_all())
            .await
            .map_err(io::Error::other)?
    }

    /// Deletes every token from the node database (the node joins a cluster and receives its tokens).
    pub async fn forget_stored(&self) {
        let store = self.store.clone();
        let result = tokio::task::spawn_blocking(move || {
            for record in store.load_all()? {
                store.delete(&record.id)?;
            }
            Ok::<(), io::Error>(())
        })
        .await;
        if !matches!(result, Ok(Ok(()))) {
            tracing::warn!("could not clear the stored tokens");
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, HashMap<String, TokenRecord>> {
        self.map.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, HashMap<String, TokenRecord>> {
        self.map.write().unwrap_or_else(|e| e.into_inner())
    }

    async fn save(&self, record: TokenRecord) -> Result<(), TabaError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.save(&record))
            .await
            .map_err(|_| TabaError::internal())?
            .map_err(|e| disk_error("failed to write the token", &e))
    }

    /// Attaches the propagation to the other nodes (`mode=production`). Only the first call counts.
    pub fn set_replicator(&self, replicator: Arc<dyn TokenReplicator>) {
        let _ = self.replicator.set(replicator);
    }

    /// Registers who follows each change to the table (creation, change, revocation).
    pub fn on_event(&self, hook: TokenHook) {
        self.event_hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(hook);
    }

    fn emit(&self, event: &TokenEvent) {
        let hooks = self
            .event_hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for hook in hooks {
            hook(event);
        }
    }

    /// A change's `register` on the node that receives it.
    pub fn check_change(&self, change: &TokenChange) -> Result<(), TabaError> {
        let map = self.read();
        match change {
            TokenChange::Create(record) if map.contains_key(&record.id) => Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "A token with this id already exists.",
            )),
            TokenChange::Update(record) if !map.contains_key(&record.id) => Err(not_found()),
            TokenChange::Revoke(id) if !map.contains_key(id) => Err(not_found()),
            _ => Ok(()),
        }
    }

    /// The commit of a creation or change: writes and takes effect.
    pub async fn apply_save(&self, record: TokenRecord) -> Result<(), TabaError> {
        self.save(record.clone()).await?;
        self.write().insert(record.id.clone(), record.clone());
        self.emit(&TokenEvent::Saved(record));
        Ok(())
    }

    /// The `auth.dev-token` (doc 37, section 4.16): creates the `admin` work token `dev` with that secret if it does not
    /// exist, and replaces it if the secret changed. Local only: the profile has a single node.
    pub async fn ensure_dev(&self, secret: &str) -> Result<(), TabaError> {
        let current = self.record(DEV_TOKEN_ID);
        if current.as_ref().is_some_and(|r| r.matches_secret(secret)) {
            return Ok(());
        }
        let _writer = self.writer.lock().await;
        let record = token::with_secret(
            DEV_TOKEN_ID.to_owned(),
            "dev-token".to_owned(),
            vec![Permission::Publish, Permission::Consume, Permission::Admin],
            self.clock.now_millis(),
            secret,
        );
        self.apply_save(record).await?;
        tracing::info!(
            token.id = DEV_TOKEN_ID,
            "dev token created: for development only"
        );
        Ok(())
    }

    /// The commit of a revocation. Revoking what does not exist is no error.
    pub async fn apply_revoke(&self, id: &str) -> Result<(), TabaError> {
        let store = self.store.clone();
        let owned = id.to_owned();
        tokio::task::spawn_blocking(move || store.delete(&owned))
            .await
            .map_err(|_| TabaError::internal())?
            .map_err(|e| disk_error("failed to delete the token", &e))?;
        self.write().remove(id);
        self.emit(&TokenEvent::Revoked(id.to_owned()));
        Ok(())
    }

    /// The tokens with `id` from `from` (inclusive), in `id` order: the
    /// `GET /cluster/tokens` stream. The second part says whether there are more.
    pub fn records_from(&self, from: Option<&str>, limit: usize) -> (Vec<TokenRecord>, bool) {
        let mut all: Vec<TokenRecord> = self
            .read()
            .values()
            .filter(|t| from.is_none_or(|f| t.id.as_str() >= f))
            .cloned()
            .collect();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        let more = all.len() > limit;
        all.truncate(limit);
        (all, more)
    }

    /// What the new node receives from the token stream.
    pub async fn load_remote(&self, record: TokenRecord) -> Result<(), TabaError> {
        if self.read().get(&record.id) == Some(&record) {
            return Ok(());
        }
        self.apply_save(record).await
    }

    /// A token's stored record, expired or not (the sweep deletes the expired ones).
    pub fn record(&self, id: &str) -> Option<TokenRecord> {
        self.read().get(id).cloned()
    }

    /// The tokens' `id`s.
    pub fn ids(&self) -> Vec<String> {
        self.read().keys().cloned().collect()
    }

    /// How many tokens the node knows (`taba.auth.tokens`).
    pub fn count(&self) -> usize {
        self.read().len()
    }

    /// Validates the presented token: format, known `id`, still within the deadline and the right
    /// secret. It is a local in-memory query. Any failure is `None`, without saying which.
    pub fn authenticate(&self, presented: &str) -> Option<TokenRecord> {
        let (id, secret) = token::parse(presented)?;
        let record = self.read().get(id).cloned()?;
        if record.expired_at(self.clock.now_millis()) || !record.matches_secret(secret) {
            return None;
        }
        Some(record)
    }

    pub async fn create(&self, request: CreateToken) -> Result<Created, TabaError> {
        validation::token_name(&request.name)?;
        let mut permissions = Vec::new();
        let mut err = TabaError::new(
            ErrorCode::ValidationFailed,
            "The request has invalid fields.",
        );
        if request.permissions.is_empty() {
            err = err.with_detail(
                "permissions",
                ErrorCode::ValidationFailed,
                "Provide at least one of publish, consume, create, manage or admin.",
            );
        }
        for (i, text) in request.permissions.iter().enumerate() {
            match Permission::parse(text) {
                Some(p) => permissions.push(p),
                None => {
                    err = err.with_detail(
                        format!("permissions[{i}]"),
                        ErrorCode::ValidationFailed,
                        "Use publish, consume, create, manage or admin.",
                    );
                }
            }
        }
        if !err.details.is_empty() {
            return Err(err);
        }
        permissions.sort();
        permissions.dedup();
        validation::prefixes_of("topicPrefixes", &request.topic_prefixes)?;
        // `admin` acts on the cluster, not on a topic: a prefix would only suggest a limit it does not have.
        if permissions.contains(&Permission::Admin) && !request.topic_prefixes.is_empty() {
            return Err(field_error(
                ErrorCode::ValidationFailed,
                "topicPrefixes",
                "A token with admin reaches every topic; use manage to limit it to prefixes.",
            ));
        }
        let now = self.clock.now_millis();
        if let Some(at) = request.expires_ms
            && at <= now
        {
            return Err(field_error(
                ErrorCode::ValidationOutOfRange,
                "expiresAt",
                "expiresAt must be in the future.",
            ));
        }

        let _writer = self.writer.lock().await;
        let new = token::generate(
            self.ids.new_at(now),
            request.name,
            permissions,
            request.topic_prefixes,
            now,
            request.expires_ms,
        );
        // Writes before taking effect: a disk failure leaves no visible token.
        match self.replicator.get() {
            Some(replicator) => {
                replicator
                    .replicate(TokenChange::Create(new.record.clone()))
                    .await?;
            }
            None => self.apply_save(new.record.clone()).await?,
        }
        tracing::info!(token.id = %new.record.id, "token created");
        Ok(Created {
            record: new.record,
            token: new.token,
        })
    }

    pub fn get(&self, id: &str) -> Result<TokenRecord, TabaError> {
        let now = self.clock.now_millis();
        self.read()
            .get(id)
            .filter(|t| !t.expired_at(now))
            .cloned()
            .ok_or_else(not_found)
    }

    pub async fn update(&self, id: &str, request: UpdateToken) -> Result<TokenRecord, TabaError> {
        if let Some(field) = request.immutable_present.first() {
            return Err(field_error(
                ErrorCode::ValidationImmutableField,
                field,
                &format!("`{field}` cannot be changed; create another token and revoke this one."),
            ));
        }
        if let Some(name) = &request.name {
            validation::token_name(name)?;
        }
        let now = self.clock.now_millis();
        if let Patch::Value(at) = request.expires_ms
            && at <= now
        {
            return Err(field_error(
                ErrorCode::ValidationOutOfRange,
                "expiresAt",
                "expiresAt must be in the future.",
            ));
        }
        let _writer = self.writer.lock().await;
        let mut record = self.get(id)?;
        if let Some(name) = request.name {
            record.name = name;
        }
        match request.expires_ms {
            Patch::Absent => {}
            Patch::Null => record.expires_ms = None,
            Patch::Value(at) => record.expires_ms = Some(at),
        }
        match self.replicator.get() {
            Some(replicator) => {
                replicator
                    .replicate(TokenChange::Update(record.clone()))
                    .await?;
            }
            None => self.apply_save(record.clone()).await?,
        }
        tracing::info!(token.id = %record.id, "token changed");
        Ok(record)
    }

    /// Revoga: a partir daqui o token responde `401`.
    pub async fn revoke(&self, id: &str) -> Result<(), TabaError> {
        let _writer = self.writer.lock().await;
        self.get(id)?;
        match self.replicator.get() {
            Some(replicator) => {
                replicator
                    .replicate(TokenChange::Revoke(id.to_owned()))
                    .await?;
            }
            None => self.apply_revoke(id).await?,
        }
        tracing::info!(token.id = %id, "token revogado");
        Ok(())
    }

    /// The local sweep of the expired: each node ignores them and deletes them on its own, without
    /// propagating anything (doc 06a). Returns how many it deleted.
    pub async fn sweep(&self) -> usize {
        let now = self.clock.now_millis();
        let expired: Vec<String> = self
            .read()
            .values()
            .filter(|t| t.expired_at(now))
            .map(|t| t.id.clone())
            .collect();
        if expired.is_empty() {
            return 0;
        }
        let _writer = self.writer.lock().await;
        let mut removed = 0;
        for id in expired {
            let store = self.store.clone();
            let owned = id.clone();
            let result = tokio::task::spawn_blocking(move || store.delete(&owned)).await;
            match result {
                Ok(Ok(())) => {
                    self.write().remove(&id);
                    removed += 1;
                    tracing::info!(token.id = %id, "expired token deleted");
                }
                Ok(Err(e)) => {
                    tracing::warn!(token.id = %id, error = %e, "could not delete the expired token")
                }
                Err(_) => {}
            }
        }
        removed
    }

    /// `GET /tokens`: paginated by key, ordered by `id` (default) or `createdAt`, with the
    /// `name` filter by fragment. The expired ones do not show up.
    pub fn list(&self, query: ListQuery) -> Result<ListResult, TabaError> {
        let limit = pagination::parse_limit(
            query.limit.as_deref(),
            self.rules.default_limit,
            self.rules.max_limit,
        )?;
        let sort = Sort::parse(query.sort.as_deref())?;
        let cursor = query
            .cursor
            .as_deref()
            .map(pagination::decode_cursor::<Cursor>)
            .transpose()?;
        if let Some(c) = &cursor
            && (c.s != sort.canonical() || c.f != query.name)
        {
            return Err(TabaError::new(
                ErrorCode::ValidationInvalidCursor,
                "The cursor does not apply to this `sort` and filter.",
            )
            .with_detail(
                "cursor",
                ErrorCode::ValidationInvalidCursor,
                "Cursor generated with another `sort` or filter.",
            ));
        }

        let now = self.clock.now_millis();
        let mut items: Vec<(Key, TokenRecord)> = self
            .read()
            .values()
            .filter(|t| !t.expired_at(now))
            .filter(|t| {
                query
                    .name
                    .as_ref()
                    .is_none_or(|f| t.name.contains(f.as_str()))
            })
            .map(|t| {
                (
                    Key {
                        i: t.id.clone(),
                        c: t.created_ms,
                    },
                    t.clone(),
                )
            })
            .collect();
        items.sort_by(|a, b| sort.compare(&a.0, &b.0));
        if let Some(c) = &cursor {
            items.retain(|(key, _)| sort.compare(key, &c.k) == Ordering::Greater);
        }
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = match (has_more, items.last()) {
            (true, Some((key, _))) => Some(pagination::encode_cursor(&Cursor {
                k: key.clone(),
                s: sort.canonical(),
                f: query.name.clone(),
            })),
            _ => None,
        };
        Ok(ListResult {
            items: items.into_iter().map(|(_, t)| t).collect(),
            next_cursor,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Key {
    i: String,
    c: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    k: Key,
    s: String,
    f: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct Sort {
    by_created: bool,
    descending: bool,
}

impl Sort {
    /// `id` or `createdAt`, with `-` in front for descending. `name` does not apply: it is changeable,
    /// and key pagination could not hold up.
    fn parse(raw: Option<&str>) -> Result<Self, TabaError> {
        let raw = raw.unwrap_or("id");
        let (descending, name) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        match name {
            "id" => Ok(Self {
                by_created: false,
                descending,
            }),
            "createdAt" => Ok(Self {
                by_created: true,
                descending,
            }),
            _ => Err(TabaError::new(
                ErrorCode::ValidationFailed,
                "The request has invalid fields.",
            )
            .with_detail(
                "query.sort",
                ErrorCode::ValidationFailed,
                "Use id or createdAt, with `-` in front for descending.",
            )),
        }
    }

    fn canonical(&self) -> String {
        format!(
            "{}{}",
            if self.descending { "-" } else { "" },
            if self.by_created { "createdAt" } else { "id" }
        )
    }

    fn compare(&self, a: &Key, b: &Key) -> Ordering {
        let primary = if self.by_created {
            a.c.cmp(&b.c)
        } else {
            a.i.cmp(&b.i)
        };
        let primary = if self.descending {
            primary.reverse()
        } else {
            primary
        };
        primary.then_with(|| a.i.cmp(&b.i))
    }
}

#[cfg(test)]
mod tests;
