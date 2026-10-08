//! O registro de tokens de acesso do nó (doc 06a, seção 8.1.3; doc 02, seção 3.4): todo token
//! fica em memória (validar é uma consulta local, sem rede) e é gravado em disco antes de
//! valer. No cluster este registro passa a ser o lado que recebe o commit; num nó só, criar,
//! alterar e revogar se resumem a gravar e aplicar aqui.
//!
//! O segredo nunca é guardado: só o hash salgado (`core::token`).

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

/// `POST /tokens`, já lido do JSON.
#[derive(Debug, Clone, Default)]
pub struct CreateToken {
    pub name: String,
    pub permissions: Vec<String>,
    pub topic_prefixes: Vec<String>,
    pub expires_ms: Option<u64>,
}

/// `PATCH /tokens/{id}`: só o nome e o prazo; o resto é imutável.
#[derive(Debug, Clone, Default)]
pub struct UpdateToken {
    /// Campos imutáveis que vieram no corpo (`permissions`, `topicPrefixes`...).
    pub immutable_present: Vec<String>,
    pub name: Option<String>,
    pub expires_ms: Patch<u64>,
}

/// O token recém-criado: o registro e o segredo completo, que só aparece aqui.
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

/// Uma mudança na tabela de tokens, como o cluster a propaga a **todos** os nós (doc 02, 3.4).
#[derive(Debug, Clone)]
pub enum TokenChange {
    Create(TokenRecord),
    Update(TokenRecord),
    Revoke(String),
}

/// Quem propaga uma mudança aos outros nós antes de ela valer. Ao fim, a mudança já foi aplicada
/// **neste nó** também, pelo commit.
pub trait TokenReplicator: Send + Sync + 'static {
    fn replicate(
        &self,
        change: TokenChange,
    ) -> Pin<Box<dyn Future<Output = Result<(), TabaError>> + Send + '_>>;
}

pub struct Tokens {
    rules: TokenRules,
    store: Arc<dyn TokenStore>,
    clock: Arc<dyn Clock>,
    ids: IdGenerator,
    map: RwLock<HashMap<String, TokenRecord>>,
    /// Uma escrita por vez: grava no disco e só então aplica em memória.
    writer: tokio::sync::Mutex<()>,
    /// No cluster: a propagação a todos os nós.
    replicator: OnceLock<Arc<dyn TokenReplicator>>,
}

fn not_found() -> TabaError {
    TabaError::new(ErrorCode::TokenNotFound, "O token não existe.")
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
        }
    }

    /// Abre o registro. Com `load`, retoma os tokens gravados em disco (só em desenvolvimento
    /// com `development.resume-on-start`, como as definições de tópico).
    pub async fn open(
        rules: TokenRules,
        store: Arc<dyn TokenStore>,
        clock: Arc<dyn Clock>,
        load: bool,
    ) -> io::Result<Self> {
        let tokens = if load {
            let store = store.clone();
            tokio::task::spawn_blocking(move || store.load_all())
                .await
                .map_err(io::Error::other)??
        } else {
            Vec::new()
        };
        Ok(Self::new(rules, store, clock, tokens))
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
            .map_err(|e| disk_error("falha ao gravar o token", &e))
    }

    /// Liga a propagação aos outros nós (`mode=production`). Vale só a primeira chamada.
    pub fn set_replicator(&self, replicator: Arc<dyn TokenReplicator>) {
        let _ = self.replicator.set(replicator);
    }

    /// O `register` de uma mudança no nó que a recebe.
    pub fn check_change(&self, change: &TokenChange) -> Result<(), TabaError> {
        let map = self.read();
        match change {
            TokenChange::Create(record) if map.contains_key(&record.id) => Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "Já existe um token com este id.",
            )),
            TokenChange::Update(record) if !map.contains_key(&record.id) => Err(not_found()),
            TokenChange::Revoke(id) if !map.contains_key(id) => Err(not_found()),
            _ => Ok(()),
        }
    }

    /// O commit de uma criação ou alteração: grava e passa a valer.
    pub async fn apply_save(&self, record: TokenRecord) -> Result<(), TabaError> {
        self.save(record.clone()).await?;
        self.write().insert(record.id.clone(), record);
        Ok(())
    }

    /// O commit de uma revogação. Revogar o que não existe não é erro.
    pub async fn apply_revoke(&self, id: &str) -> Result<(), TabaError> {
        let store = self.store.clone();
        let owned = id.to_owned();
        tokio::task::spawn_blocking(move || store.delete(&owned))
            .await
            .map_err(|_| TabaError::internal())?
            .map_err(|e| disk_error("falha ao apagar o token", &e))?;
        self.write().remove(id);
        Ok(())
    }

    /// Os tokens com `id` a partir de `from` (inclusive), em ordem de `id`: o stream
    /// `GET /cluster/tokens`. A segunda parte diz se há mais.
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

    /// O que o nó novo recebe do stream de tokens.
    pub async fn load_remote(&self, record: TokenRecord) -> Result<(), TabaError> {
        if self.read().get(&record.id) == Some(&record) {
            return Ok(());
        }
        self.apply_save(record).await
    }

    /// Os `id` dos tokens.
    pub fn ids(&self) -> Vec<String> {
        self.read().keys().cloned().collect()
    }

    /// Quantos tokens o nó conhece (`taba.auth.tokens`).
    pub fn count(&self) -> usize {
        self.read().len()
    }

    /// Valida o token apresentado: formato, `id` conhecido, ainda dentro do prazo e segredo
    /// certo. É uma consulta local em memória. Qualquer falha é `None`, sem dizer qual.
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
            "A requisição tem campos inválidos.",
        );
        if request.permissions.is_empty() {
            err = err.with_detail(
                "permissions",
                ErrorCode::ValidationFailed,
                "Informe ao menos uma de publish, consume, create ou admin.",
            );
        }
        for (i, text) in request.permissions.iter().enumerate() {
            match Permission::parse(text) {
                Some(p) => permissions.push(p),
                None => {
                    err = err.with_detail(
                        format!("permissions[{i}]"),
                        ErrorCode::ValidationFailed,
                        "Use publish, consume, create ou admin.",
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
        let now = self.clock.now_millis();
        if let Some(at) = request.expires_ms
            && at <= now
        {
            return Err(field_error(
                ErrorCode::ValidationOutOfRange,
                "expiresAt",
                "expiresAt deve estar no futuro.",
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
        // Grava antes de valer: uma falha de disco não deixa token visível.
        match self.replicator.get() {
            Some(replicator) => {
                replicator
                    .replicate(TokenChange::Create(new.record.clone()))
                    .await?;
            }
            None => self.apply_save(new.record.clone()).await?,
        }
        tracing::info!(token.id = %new.record.id, "token criado");
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
                &format!("`{field}` não pode ser alterado; crie outro token e revogue este."),
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
                "expiresAt deve estar no futuro.",
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
        tracing::info!(token.id = %record.id, "token alterado");
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

    /// A varredura local dos vencidos: cada nó os ignora e os apaga por conta própria, sem
    /// propagar nada (doc 06a). Devolve quantos apagou.
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
                    tracing::info!(token.id = %id, "token vencido apagado");
                }
                Ok(Err(e)) => {
                    tracing::warn!(token.id = %id, error = %e, "não foi possível apagar o token vencido")
                }
                Err(_) => {}
            }
        }
        removed
    }

    /// `GET /tokens`: paginada por chave, ordenada por `id` (padrão) ou `createdAt`, com o
    /// filtro `name` por trecho. Os vencidos não aparecem.
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
                "O cursor não vale para este `sort` e filtro.",
            )
            .with_detail(
                "cursor",
                ErrorCode::ValidationInvalidCursor,
                "Cursor gerado com outro `sort` ou filtro.",
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
    /// `id` ou `createdAt`, com `-` na frente para decrescente. `name` não vale: é alterável,
    /// e a paginação por chave não se sustentaria.
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
                "A requisição tem campos inválidos.",
            )
            .with_detail(
                "query.sort",
                ErrorCode::ValidationFailed,
                "Use id ou createdAt, com `-` na frente para decrescente.",
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
