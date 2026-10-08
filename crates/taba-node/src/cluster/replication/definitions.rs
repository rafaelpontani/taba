//! A propagação das definições de **tópicos e tokens** a todos os nós (doc 02, seções 3.3 e 3.4;
//! doc 06c, "Tópicos e tokens"): registro + commit com a confirmação de **todos** os membros, sem
//! quórum reduzido (doc 05, seção 7.2), e os streams `GET /cluster/topics` e `/cluster/tokens` que
//! o nó novo puxa depois de entrar.
//!
//! Do lado de quem coordena (`Definitions` como [`TopicReplicator`] e [`TokenReplicator`]): o
//! registro vai a todos; falhando algum, rollback nos que confirmaram e erro ao cliente; fechando,
//! o commit vai a todos e insiste dentro de `write.commit.timeout` (esgotado, o erro vai ao cliente
//! e a tentativa segue em segundo plano). Do lado de quem recebe: confere, deixa pendente, e só no
//! commit a definição passa a valer e é gravada em disco.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde_json::{Value, json};
use tokio::task::JoinSet;

use crate::cluster::members::Member;
use crate::cluster::protocol::{
    Ack, OperationRef, Request, Response, Step, StreamPage, StreamQuery, TokenOperation,
    TokenRegisterRequest, TopicOperation, TopicRegisterRequest, token_from_json, token_to_json,
};
use crate::cluster::service::ClusterService;
use crate::cluster::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::token::TokenRecord;
use crate::core::topic::TopicDefinition;
use crate::engine::tokens::{TokenChange, TokenReplicator, Tokens};
use crate::engine::topics::{TopicChange, TopicReplicator, Topics};

const FINISHED_MEMORY: usize = 1024;
/// O teto padrão de linhas de um trecho de stream (`cluster.sync.stream.max-items`, a definir).
const STREAM_PAGE: usize = 500;

#[derive(Clone)]
enum Change {
    Topic(TopicChange),
    Token(TokenChange),
}

struct Pending {
    key: String,
    change: Change,
    at: SystemTime,
}

#[derive(Default)]
struct State {
    ops: HashMap<String, Pending>,
    by_key: HashMap<String, String>,
    finished: VecDeque<(String, bool)>,
}

/// O que a carga de definições trouxe (nomes dos tópicos e IDs dos tokens).
#[derive(Debug, Clone, Default)]
pub struct Pulled {
    pub topics: HashSet<String>,
    pub tokens: HashSet<String>,
}

pub struct Definitions {
    service: Arc<ClusterService>,
    topics: Arc<Topics>,
    tokens: Arc<Tokens>,
    state: Mutex<State>,
}

fn conflict(message: &str) -> TabaError {
    TabaError::new(ErrorCode::ClusterConflict, message)
}

fn ack(operation_id: &str) -> Ack {
    Ack {
        operation_id: operation_id.to_owned(),
        accepted: true,
    }
}

impl Definitions {
    pub fn new(
        service: Arc<ClusterService>,
        topics: Arc<Topics>,
        tokens: Arc<Tokens>,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            topics,
            tokens,
            state: Mutex::new(State::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- Quem recebe ----

    fn purge(&self, state: &mut State) {
        let now = self.service.now();
        let ttl = self.service.rules().pending_ttl;
        let expired: Vec<String> = state
            .ops
            .iter()
            .filter(|(_, p)| now.duration_since(p.at).is_ok_and(|age| age >= ttl))
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(op) = state.ops.remove(&id) {
                state.by_key.remove(&op.key);
            }
        }
    }

    fn register(&self, operation_id: &str, key: String, change: Change) -> Result<Ack, TabaError> {
        let mut state = self.lock();
        self.purge(&mut state);
        if state.ops.contains_key(operation_id)
            || state.finished.iter().any(|(id, _)| id == operation_id)
        {
            return Ok(ack(operation_id));
        }
        if state.by_key.contains_key(&key) {
            return Err(conflict("Há outra operação pendente para este recurso."));
        }
        state.by_key.insert(key.clone(), operation_id.to_owned());
        state.ops.insert(
            operation_id.to_owned(),
            Pending {
                key,
                change,
                at: self.service.now(),
            },
        );
        Ok(ack(operation_id))
    }

    async fn commit(&self, operation_id: &str) -> Result<Ack, TabaError> {
        let pending = {
            let mut state = self.lock();
            self.purge(&mut state);
            match state.ops.remove(operation_id) {
                Some(op) => {
                    state.by_key.remove(&op.key);
                    op
                }
                None => {
                    return match state.finished.iter().find(|(id, _)| id == operation_id) {
                        Some((_, true)) => Ok(ack(operation_id)),
                        Some((_, false)) => Err(conflict("A operação foi cancelada (rollback).")),
                        None => Err(TabaError::new(
                            ErrorCode::ClusterOperationUnknown,
                            "O nó não conhece essa operação (nunca chegou ou já expirou).",
                        )),
                    };
                }
            }
        };
        let result = match pending.change {
            Change::Topic(TopicChange::Create(def) | TopicChange::Update(def)) => {
                self.topics.apply_save(def).await
            }
            Change::Topic(TopicChange::Delete { name, .. }) => {
                self.topics.apply_delete(&name).await
            }
            Change::Token(TokenChange::Create(record) | TokenChange::Update(record)) => {
                self.tokens.apply_save(record).await
            }
            Change::Token(TokenChange::Revoke(id)) => self.tokens.apply_revoke(&id).await,
        };
        self.remember(operation_id, true);
        result.map(|()| ack(operation_id))
    }

    fn remember(&self, operation_id: &str, committed: bool) {
        let mut state = self.lock();
        state.finished.retain(|(id, _)| id != operation_id);
        state
            .finished
            .push_back((operation_id.to_owned(), committed));
        while state.finished.len() > FINISHED_MEMORY {
            state.finished.pop_front();
        }
    }

    fn rollback(&self, operation_id: &str) -> Result<Ack, TabaError> {
        let mut state = self.lock();
        self.purge(&mut state);
        if let Some(op) = state.ops.remove(operation_id) {
            state.by_key.remove(&op.key);
            drop(state);
            self.remember(operation_id, false);
            return Ok(ack(operation_id));
        }
        match state.finished.iter().find(|(id, _)| id == operation_id) {
            Some((_, false)) => Ok(ack(operation_id)),
            Some((_, true)) => Err(conflict(
                "A operação já foi efetivada: não há rollback depois do commit.",
            )),
            None => Err(TabaError::new(
                ErrorCode::ClusterOperationUnknown,
                "O nó não conhece essa operação (nunca chegou ou já expirou).",
            )),
        }
    }

    /// `POST /cluster/topics/register|commit|rollback`.
    pub async fn handle_topic(
        &self,
        step: Step<TopicRegisterRequest>,
    ) -> Result<Response, TabaError> {
        match step {
            Step::Register(request) => {
                let change = topic_change(&request)?;
                self.topics.check_change(&change)?;
                let key = match &change {
                    TopicChange::Create(d) | TopicChange::Update(d) => d.name.clone(),
                    TopicChange::Delete { name, .. } => name.clone(),
                };
                self.register(
                    &request.operation_id,
                    format!("topic:{key}"),
                    Change::Topic(change),
                )
            }
            Step::Commit(OperationRef { operation_id }) => self.commit(&operation_id).await,
            Step::Rollback(OperationRef { operation_id }) => self.rollback(&operation_id),
        }
        .map(Response::Ack)
    }

    /// `POST /cluster/tokens/register|commit|rollback`.
    pub async fn handle_token(
        &self,
        step: Step<TokenRegisterRequest>,
    ) -> Result<Response, TabaError> {
        match step {
            Step::Register(request) => {
                let change = match request.operation {
                    TokenOperation::Create => TokenChange::Create(token_from_json(&request.token)?),
                    TokenOperation::Update => TokenChange::Update(token_from_json(&request.token)?),
                    TokenOperation::Revoke => TokenChange::Revoke(
                        request
                            .token
                            .get("id")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                TabaError::new(
                                    ErrorCode::ValidationRequired,
                                    "`token.id` é obrigatório.",
                                )
                            })?
                            .to_owned(),
                    ),
                };
                self.tokens.check_change(&change)?;
                let key = match &change {
                    TokenChange::Create(r) | TokenChange::Update(r) => r.id.clone(),
                    TokenChange::Revoke(id) => id.clone(),
                };
                self.register(
                    &request.operation_id,
                    format!("token:{key}"),
                    Change::Token(change),
                )
            }
            Step::Commit(OperationRef { operation_id }) => self.commit(&operation_id).await,
            Step::Rollback(OperationRef { operation_id }) => self.rollback(&operation_id),
        }
        .map(Response::Ack)
    }

    fn page(query: &StreamQuery) -> usize {
        query
            .limit
            .map_or(STREAM_PAGE, |n| (n as usize).clamp(2, STREAM_PAGE))
    }

    /// `GET /cluster/topics`.
    pub fn list_topics(&self, query: &StreamQuery) -> Response {
        let (definitions, more) = self
            .topics
            .definitions_from(query.from.as_deref(), Self::page(query));
        Response::Stream(StreamPage::new(
            definitions.iter().map(TopicDefinition::to_json).collect(),
            more,
        ))
    }

    /// `GET /cluster/tokens`.
    pub fn list_tokens(&self, query: &StreamQuery) -> Response {
        let (records, more) = self
            .tokens
            .records_from(query.from.as_deref(), Self::page(query));
        Response::Stream(StreamPage::new(
            records.iter().map(token_to_json).collect(),
            more,
        ))
    }

    // ---- Quem coordena ----

    /// Registro em todos os membros e commit em todos (doc 05, seção 7.2): sem quórum reduzido.
    async fn all_nodes(
        &self,
        register: Request,
        commit: Request,
        rollback: Request,
    ) -> Result<(), TabaError> {
        let service = &self.service;
        let members: Vec<Member> =
            service
                .membership()
                .table()
                .map(|t| t.members)
                .ok_or_else(|| {
                    TabaError::new(
                        ErrorCode::ClusterNodeSyncing,
                        "O nó ainda não tem a tabela de membros.",
                    )
                    .with_retry_after(5)
                })?;

        // Fase 1.
        let mut set = JoinSet::new();
        for member in members.iter().cloned() {
            let (service, request) = (service.clone(), register.clone());
            set.spawn(boxed(async move {
                let result = service.call_member_retrying(&member, request).await;
                (member, result)
            }));
        }
        let (mut confirmed, mut failure): (Vec<Member>, Option<CallError>) = (Vec::new(), None);
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((member, Ok(_))) => confirmed.push(member),
                Ok((_, Err(error))) => {
                    // O erro de um nó vale mais que o de rede: é o motivo de verdade.
                    if failure
                        .as_ref()
                        .is_none_or(|f| f.is_network() && !error.is_network())
                    {
                        failure = Some(error);
                    }
                }
                Err(_) => {}
            }
        }
        if let Some(error) = failure {
            let mut set = JoinSet::new();
            for member in confirmed {
                let (service, request) = (service.clone(), rollback.clone());
                set.spawn(boxed(async move {
                    let _ = service.call_member_retrying(&member, request).await;
                }));
            }
            while set.join_next().await.is_some() {}
            return Err(match error {
                CallError::Remote(e) if e.status() < 500 => e,
                _ => TabaError::new(
                    ErrorCode::ClusterRegistrationFailed,
                    "A fase de registro não foi confirmada por todos os nós; foi cancelada.",
                )
                .with_retry_after(5),
            });
        }

        // Fase 2: a decisão está tomada; insiste até o teto.
        let mut set = JoinSet::new();
        for member in members {
            let (service, request) = (service.clone(), commit.clone());
            set.spawn(boxed(async move {
                loop {
                    match service.call_member(&member, request.clone()).await {
                        Ok(_) => return,
                        Err(CallError::Remote(e)) if e.status() < 500 => {
                            tracing::warn!(node_id = %member.node_id, code = %e.code, "commit de definição recusado");
                            return;
                        }
                        Err(_) => {}
                    }
                    if service.cancel_token().is_cancelled()
                        || !service.membership().is_member(&member.node_id)
                    {
                        return;
                    }
                    tokio::select! {
                        () = service.cancel_token().cancelled() => return,
                        () = tokio::time::sleep(service.rules().retry_backoff) => {}
                    }
                }
            }));
        }
        let all = tokio::time::timeout(service.rules().commit_timeout, async {
            while set.join_next().await.is_some() {}
        })
        .await
        .is_ok();
        if all {
            Ok(())
        } else {
            set.detach_all();
            Err(TabaError::new(
                ErrorCode::ClusterCommitTimeout,
                "A definição foi decidida e o commit segue em segundo plano em algum nó.",
            )
            .with_retry_after(5))
        }
    }

    /// Apaga as definições e os tokens locais (o nó volta como nó novo).
    pub async fn wipe(&self) {
        for name in self.topics.names() {
            let _ = self.topics.apply_delete(&name).await;
        }
        for id in self.tokens.ids() {
            let _ = self.tokens.apply_revoke(&id).await;
        }
        *self.lock() = State::default();
    }

    fn references(&self) -> (String, String) {
        (
            self.service.new_operation_id(),
            self.service.node_id().to_owned(),
        )
    }

    // ---- Nó novo: puxar as tabelas ----

    /// Puxa de um membro as definições de tópico e os tokens (doc 02, seções 3.3 e 3.4) e devolve
    /// o que veio, para a verificação final conferir.
    pub async fn pull(&self, source: &Member) -> Result<Pulled, TabaError> {
        let mut pulled = Pulled::default();
        for definition in self.list_topics_of(source).await? {
            pulled.topics.insert(definition.name.clone());
            self.topics.load_remote(definition).await?;
        }
        for record in self.list_tokens_of(source).await? {
            pulled.tokens.insert(record.id.clone());
            self.tokens.load_remote(record).await?;
        }
        Ok(pulled)
    }

    /// Verificação final (doc 05, seção 7.3, passo 5.5): repete a listagem de tópicos e de tokens e
    /// descarta o que a carga trouxe e já não aparece na origem (foi removido sem que o evento
    /// chegasse a este nó). Só mexe no que veio da carga. Devolve quantos apagou.
    pub async fn verify(&self, source: &Member, pulled: &Pulled) -> Result<usize, TabaError> {
        let topics: HashSet<String> = self
            .list_topics_of(source)
            .await?
            .into_iter()
            .map(|d| d.name)
            .collect();
        let tokens: HashSet<String> = self
            .list_tokens_of(source)
            .await?
            .into_iter()
            .map(|r| r.id)
            .collect();
        let mut dropped = 0;
        for name in pulled.topics.difference(&topics) {
            if self.topics.apply_delete(name).await.is_ok() {
                dropped += 1;
            }
        }
        for id in pulled.tokens.difference(&tokens) {
            if self.tokens.apply_revoke(id).await.is_ok() {
                dropped += 1;
            }
        }
        Ok(dropped)
    }

    async fn list_topics_of(&self, source: &Member) -> Result<Vec<TopicDefinition>, TabaError> {
        let mut out = Vec::new();
        let mut from: Option<String> = None;
        loop {
            let request = Request::ListTopics(StreamQuery {
                from: from.clone(),
                to: None,
                limit: None,
            });
            let Response::Stream(page) = self.call(source, request).await? else {
                return Err(TabaError::internal());
            };
            for item in &page.items {
                out.push(
                    TopicDefinition::from_json(item)
                        .map_err(|e| TabaError::new(ErrorCode::ValidationFailed, e))?,
                );
            }
            if !page.end.has_more || page.end.last == from {
                break;
            }
            from = page.end.last;
        }
        Ok(out)
    }

    async fn list_tokens_of(&self, source: &Member) -> Result<Vec<TokenRecord>, TabaError> {
        let mut out = Vec::new();
        let mut from: Option<String> = None;
        loop {
            let request = Request::ListTokens(StreamQuery {
                from: from.clone(),
                to: None,
                limit: None,
            });
            let Response::Stream(page) = self.call(source, request).await? else {
                return Err(TabaError::internal());
            };
            for item in &page.items {
                out.push(token_from_json(item)?);
            }
            if !page.end.has_more || page.end.last == from {
                break;
            }
            from = page.end.last;
        }
        Ok(out)
    }

    async fn call(&self, member: &Member, request: Request) -> Result<Response, TabaError> {
        self.service
            .call_member_retrying(member, request)
            .await
            .map_err(|e| match e {
                CallError::Remote(error) => error,
                other => TabaError::new(ErrorCode::ClusterNodeSyncing, other.to_string())
                    .with_retry_after(5),
            })
    }
}

/// Apaga o tipo do futuro (o `tokio::spawn` precisa de `Send` e as chamadas entre nós são
/// recursivas por natureza).
fn boxed<T: Send + 'static>(
    future: impl Future<Output = T> + Send + 'static,
) -> Pin<Box<dyn Future<Output = T> + Send>> {
    Box::pin(future)
}

fn topic_change(request: &TopicRegisterRequest) -> Result<TopicChange, TabaError> {
    let bad = |what: String| TabaError::new(ErrorCode::ValidationFailed, what);
    match request.operation {
        TopicOperation::Create => Ok(TopicChange::Create(
            TopicDefinition::from_json(&request.topic).map_err(bad)?,
        )),
        TopicOperation::Update => Ok(TopicChange::Update(
            TopicDefinition::from_json(&request.topic).map_err(bad)?,
        )),
        TopicOperation::Delete => {
            let text = |key: &str| {
                request
                    .topic
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| bad(format!("falta `{key}` na remoção")))
            };
            Ok(TopicChange::Delete {
                name: text("name")?,
                id: text("id")?,
                expected_id: request.expected_id.clone(),
            })
        }
    }
}

impl TopicReplicator for Definitions {
    fn replicate(
        &self,
        change: TopicChange,
    ) -> Pin<Box<dyn Future<Output = Result<(), TabaError>> + Send + '_>> {
        Box::pin(async move {
            let (operation_id, coordinator_id) = self.references();
            let (operation, topic, expected_id) = match &change {
                TopicChange::Create(d) => (TopicOperation::Create, d.to_json(), None),
                TopicChange::Update(d) => (TopicOperation::Update, d.to_json(), None),
                TopicChange::Delete {
                    name,
                    id,
                    expected_id,
                } => (
                    TopicOperation::Delete,
                    json!({ "name": name, "id": id }),
                    expected_id.clone(),
                ),
            };
            let reference = OperationRef {
                operation_id: operation_id.clone(),
            };
            self.all_nodes(
                Request::Topic(Step::Register(TopicRegisterRequest {
                    operation_id,
                    coordinator_id,
                    operation,
                    topic,
                    expected_id,
                })),
                Request::Topic(Step::Commit(reference.clone())),
                Request::Topic(Step::Rollback(reference)),
            )
            .await
        })
    }
}

impl TokenReplicator for Definitions {
    fn replicate(
        &self,
        change: TokenChange,
    ) -> Pin<Box<dyn Future<Output = Result<(), TabaError>> + Send + '_>> {
        Box::pin(async move {
            let (operation_id, coordinator_id) = self.references();
            let (operation, token) = match &change {
                TokenChange::Create(r) => (TokenOperation::Create, token_to_json(r)),
                TokenChange::Update(r) => (TokenOperation::Update, token_to_json(r)),
                TokenChange::Revoke(id) => (TokenOperation::Revoke, json!({ "id": id })),
            };
            let reference = OperationRef {
                operation_id: operation_id.clone(),
            };
            self.all_nodes(
                Request::Token(Step::Register(TokenRegisterRequest {
                    operation_id,
                    coordinator_id,
                    operation,
                    token,
                })),
                Request::Token(Step::Commit(reference.clone())),
                Request::Token(Step::Rollback(reference)),
            )
            .await
        })
    }
}
