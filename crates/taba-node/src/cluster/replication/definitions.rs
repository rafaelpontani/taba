//! A propagação das definições de **tópicos e tokens** a todos os nós (doc 02, seções 3.3 e 3.4;
//! doc 06c, "Tópicos e tokens"), e os streams `GET /cluster/topics` e `/cluster/tokens` que o nó
//! novo puxa depois de entrar.
//!
//! Cada definição tem um **dono** no anel: o primeiro nó ativo na posição do hash do nome do tópico
//! ou do `id` do token. O dono é a fonte da verdade: só ele decide uma mudança, confere se ela cabe
//! no estado atual (o nome já existe, o tópico não existe...) e dá a ela uma **versão** crescente.
//! Todos os nós guardam a cópia completa e leem localmente; só a escrita passa pelo dono.
//!
//! Quem recebe a chamada do cliente propõe a mudança ao dono (`DefinitionPropose`). O dono grava
//! primeiro num **herdeiro** (o primeiro outro dono alcançável da posição, para a mudança sobreviver à queda do
//! dono), aplica em si e manda a todos os outros nós (`DefinitionApply`). Responde depois dos nós
//! que respondem dentro de `write.commit.timeout`; o dono e o herdeiro enfileiram o estado da definição para todos os
//! pares no quadro `definition` do canal (D127), e o nó que não respondeu o recebe quando o canal
//! volta, ou é excluído se a fila estoura. Cada nó aplica só uma versão que não seja menor que a
//! sua, então repetir ou chegar fora de ordem não muda o resultado. A remoção deixa uma lápide com
//! a versão, para uma mudança atrasada não ressuscitar o que foi removido.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::OwnedMutexGuard;
use tokio::task::JoinSet;

use crate::cluster::members::{Member, MemberState};
use crate::cluster::protocol::{
    DefinitionApply, DefinitionKind, DefinitionOperation, DefinitionProposal, Request, Response,
    StreamPage, StreamQuery, token_from_json, token_to_json,
};
use crate::cluster::service::ClusterService;
use crate::cluster::state::NodeState;
use crate::cluster::transport::CallError;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring;
use crate::core::token::TokenRecord;
use crate::core::topic::TopicDefinition;
use crate::engine::tokens::{TokenChange, TokenReplicator, Tokens};
use crate::engine::topics::{TopicChange, TopicReplicator, Topics};
use crate::telemetry::metrics::DEFINITION_CONFLICTS;

/// O teto padrão de linhas de um trecho de stream (`cluster.sync.stream.max-items`, a definir).
const STREAM_PAGE: usize = 500;
/// Quanto tempo uma lápide fica guardada (se nenhum nó está pendente para ela).
const TOMBSTONE_TTL: Duration = Duration::from_secs(3600);

type Key = (DefinitionKind, String);
/// O SHA-256 do conteúdo de uma versão da definição.
type Fingerprint = [u8; 32];

/// O que este nó sabe de uma definição: a versão que aplicou, se ela foi removida e a impressão do
/// conteúdo (para duas cópias de mesma versão serem desempatadas igual em todo nó).
struct Entry {
    version: u64,
    deleted: bool,
    fingerprint: Fingerprint,
    at: SystemTime,
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
    versions: Mutex<HashMap<Key, Entry>>,
    /// Uma mudança por vez em cada definição (a do dono e a aplicação nos nós).
    locks: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn unavailable(message: impl Into<String>) -> TabaError {
    TabaError::new(ErrorCode::ClusterQuorumUnavailable, message.into()).with_retry_after(5)
}

fn bad(what: impl Into<String>) -> TabaError {
    TabaError::new(ErrorCode::ValidationFailed, what.into())
}

/// Apaga o tipo do futuro (o `tokio::spawn` precisa de `Send` e as chamadas entre nós são
/// recursivas por natureza).
fn boxed<T: Send + 'static>(
    future: impl Future<Output = T> + Send + 'static,
) -> Pin<Box<dyn Future<Output = T> + Send>> {
    Box::pin(future)
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
            versions: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
        })
    }

    async fn lock_key(&self, key: &Key) -> OwnedMutexGuard<()> {
        let mutex = {
            let mut locks = lock(&self.locks);
            locks.retain(|_, m| Arc::strong_count(m) > 1);
            locks.entry(key.clone()).or_default().clone()
        };
        mutex.lock_owned().await
    }

    /// A versão que este nó aplicou (`0` se não conhece a definição).
    fn version_of(&self, kind: DefinitionKind, key: &str) -> u64 {
        lock(&self.versions)
            .get(&(kind, key.to_owned()))
            .map_or(0, |e| e.version)
    }

    // ---- Quem é o dono ----

    /// Os donos da definição: os nós **ativos** da posição no anel, o primeiro é o dono (um
    /// nó que sincroniza ainda não tem a cópia completa). `503` se nenhum nó ativo a ocupa.
    fn owners(&self, kind: DefinitionKind, key: &str) -> Result<Vec<Member>, TabaError> {
        let position = ring::definition_position(kind.as_str(), key);
        let owners: Vec<Member> = self
            .service
            .owners_of(position)?
            .into_iter()
            .filter(|m| m.state == MemberState::Active)
            .collect();
        if owners.is_empty() {
            return Err(unavailable("Nenhum nó ativo é dono desta definição."));
        }
        Ok(owners)
    }

    // ---- Quem propõe ----

    /// Leva a mudança ao dono e só volta quando ela já vale neste nó também.
    async fn propose(&self, proposal: DefinitionProposal) -> Result<(), TabaError> {
        let owners = self.owners(proposal.kind, &proposal.key)?;
        let owner = &owners[0];
        if self.service.is_self(owner) {
            self.coordinate(proposal).await?;
            return Ok(());
        }
        match self
            .service
            .call_member(owner, Request::DefinitionPropose(proposal))
            .await
        {
            Ok(Response::Definition(done)) => {
                // O dono esperou este nó; repetir é inofensivo e garante o "li o que escrevi".
                let key = (done.kind, done.key.clone());
                let _guard = self.lock_key(&key).await;
                self.apply_unlocked(&done).await?;
                Ok(())
            }
            Ok(_) => Err(TabaError::internal()),
            Err(CallError::Remote(error)) => Err(error),
            Err(other) => Err(unavailable(format!(
                "O dono da definição não respondeu: {other}."
            ))),
        }
    }

    // ---- O dono ----

    /// Decide a mudança: confere com o estado do dono e dá a próxima versão.
    fn decide(&self, proposal: &DefinitionProposal) -> Result<DefinitionApply, TabaError> {
        match proposal.kind {
            DefinitionKind::Topic => self.topics.check_change(&topic_change(proposal)?)?,
            DefinitionKind::Token => self.tokens.check_change(&token_change(proposal)?)?,
        }
        let deleted = proposal.operation == DefinitionOperation::Delete;
        Ok(DefinitionApply {
            kind: proposal.kind,
            key: proposal.key.clone(),
            version: self.version_of(proposal.kind, &proposal.key) + 1,
            deleted,
            body: if deleted {
                Value::Null
            } else {
                proposal.body.clone()
            },
            relay: false,
        })
    }

    /// Roda no dono: decide, grava no herdeiro, aplica em si e espalha aos demais nós.
    async fn coordinate(&self, proposal: DefinitionProposal) -> Result<DefinitionApply, TabaError> {
        let key = (proposal.kind, proposal.key.clone());
        let _guard = self.lock_key(&key).await;
        let owners = self.owners(proposal.kind, &proposal.key)?;
        if !self.service.is_self(&owners[0]) {
            return Err(unavailable("Este nó não é o dono da definição."));
        }
        let decided = self.decide(&proposal)?;
        // A mudança precisa existir em outro nó antes de o dono responder: o primeiro dono
        // seguinte que confirma é o herdeiro dela (se ele cair, a mudança não se perde).
        let mut heir: Option<&Member> = None;
        for candidate in &owners[1..] {
            let for_heir = DefinitionApply {
                relay: true,
                ..decided.clone()
            };
            match self
                .service
                .call_member(candidate, Request::DefinitionApply(for_heir))
                .await
            {
                Ok(_) => {
                    heir = Some(candidate);
                    break;
                }
                Err(CallError::Remote(error)) if error.status() < 500 => return Err(error),
                Err(_) => {}
            }
        }
        if owners.len() > 1 && heir.is_none() {
            return Err(unavailable(
                "Nenhum outro dono da definição confirmou a mudança.",
            ));
        }
        self.apply_unlocked(&decided).await?;
        self.relay(decided.kind, &decided.key);
        let mut done = vec![owners[0].node_id.clone()];
        done.extend(heir.map(|m| m.node_id.clone()));
        self.spread(&decided, &done).await;
        Ok(decided)
    }

    /// Manda a mudança aos demais nós e espera os que respondem em `write.commit.timeout`; os
    /// outros ficam pendentes e o dono insiste neles.
    async fn spread(&self, decided: &DefinitionApply, done: &[String]) {
        let Some(table) = self.service.membership().table() else {
            return;
        };
        let skip: HashSet<&str> = done.iter().map(String::as_str).collect();
        let mut pending: HashMap<String, Member> = table
            .members
            .iter()
            .filter(|m| !skip.contains(m.node_id.as_str()))
            .map(|m| (m.node_id.clone(), m.clone()))
            .collect();
        let mut set = JoinSet::new();
        for member in pending.values().cloned() {
            let (service, change) = (self.service.clone(), decided.clone());
            set.spawn(boxed(async move {
                let result = service
                    .call_member(&member, Request::DefinitionApply(change))
                    .await;
                (member, result)
            }));
        }
        let _ = tokio::time::timeout(self.service.rules().commit_timeout, async {
            while let Some(joined) = set.join_next().await {
                match joined {
                    Ok((member, Ok(_))) => {
                        pending.remove(&member.node_id);
                    }
                    Ok((member, Err(CallError::Remote(error)))) if error.status() < 500 => {
                        tracing::warn!(node_id = %member.node_id, code = %error.code, "definição recusada por um nó");
                        pending.remove(&member.node_id);
                    }
                    _ => {}
                }
            }
        })
        .await;
        set.detach_all();
    }

    /// O estado mais novo que este nó tem de uma definição (ou a lápide dela).
    fn current(&self, kind: DefinitionKind, key: &str) -> Option<DefinitionApply> {
        let (version, deleted) = lock(&self.versions)
            .get(&(kind, key.to_owned()))
            .map(|e| (e.version, e.deleted))?;
        let body = if deleted {
            Value::Null
        } else {
            match kind {
                DefinitionKind::Topic => self.topics.definition(key)?.to_json(),
                DefinitionKind::Token => token_to_json(&self.tokens.record(key)?),
            }
        };
        Some(DefinitionApply {
            kind,
            key: key.to_owned(),
            version,
            deleted,
            body,
            relay: false,
        })
    }

    // ---- Quem recebe ----

    /// A mudança vale diante do que este nó já tem? Perde a de versão menor. Na mesma versão com
    /// conteúdo diferente (duas decisões sem saber uma da outra: a nova tentativa que reusa a
    /// versão, ou dois nós que se achavam donos), vale a de **menor impressão**, a mesma escolha em
    /// qualquer nó e em qualquer ordem de chegada; o empate é contado e avisado.
    fn wins_over_held(&self, change: &DefinitionApply, fingerprint: &Fingerprint) -> bool {
        let held = lock(&self.versions)
            .get(&(change.kind, change.key.clone()))
            .map(|e| (e.version, e.fingerprint));
        let Some((version, held_fingerprint)) = held else {
            return true;
        };
        if version != change.version {
            return version < change.version;
        }
        if held_fingerprint == *fingerprint {
            return true;
        }
        let wins = fingerprint < &held_fingerprint;
        tracing::warn!(
            kind = change.kind.as_str(),
            key = %change.key,
            version,
            replaced = wins,
            "duas cópias da definição com a mesma versão e conteúdo diferente"
        );
        self.service.telemetry().registry.add(
            &DEFINITION_CONFLICTS,
            &[("taba.kind", change.kind.as_str())],
            1.0,
        );
        wins
    }

    /// Aplica a mudança neste nó se a versão não é menor que a que ele já tem. Quem chama segura a
    /// trava da definição. Devolve `false` quando a mudança chegou velha demais.
    async fn apply_unlocked(&self, change: &DefinitionApply) -> Result<bool, TabaError> {
        let key = (change.kind, change.key.clone());
        let fingerprint = fingerprint(change)?;
        if !self.wins_over_held(change, &fingerprint) {
            return Ok(false);
        }
        match (change.kind, change.deleted) {
            (DefinitionKind::Topic, false) => {
                let definition = TopicDefinition::from_json(&change.body).map_err(bad)?;
                if definition.name != change.key {
                    return Err(bad("O nome do tópico não é a chave da mudança."));
                }
                self.topics.apply_save(definition).await?;
            }
            (DefinitionKind::Topic, true) => self.topics.apply_delete(&change.key).await?,
            (DefinitionKind::Token, false) => {
                let record = token_from_json(&change.body)?;
                if record.id != change.key {
                    return Err(bad("O id do token não é a chave da mudança."));
                }
                self.tokens.apply_save(record).await?;
            }
            (DefinitionKind::Token, true) => self.tokens.apply_revoke(&change.key).await?,
        }
        let now = self.service.now();
        let mut versions = lock(&self.versions);
        versions.insert(
            key,
            Entry {
                version: change.version,
                deleted: change.deleted,
                fingerprint,
                at: now,
            },
        );
        versions.retain(|_, e| {
            !(e.deleted
                && now
                    .duration_since(e.at)
                    .is_ok_and(|age| age >= TOMBSTONE_TTL))
        });
        Ok(true)
    }

    /// Enfileira para todos os pares o estado atual da definição, no quadro `definition` do canal: o nó
    /// que não respondeu a tempo (ou estava fora do ar) o recebe quando o canal volta, sem depender
    /// de quem decidiu (D127). O dono e o herdeiro fazem isso.
    fn relay(&self, kind: DefinitionKind, key: &str) {
        let Some(change) = self.current(kind, key) else {
            return;
        };
        if let Some(replication) = self.service.replication() {
            replication.signals.announce_definition(change);
        }
    }

    /// Uma definição que um par disse pelo canal (quadro `definition`): aplica pela mesma regra do
    /// `definition-apply` (versão maior vale; a mesma versão com conteúdo diferente, o desempate do
    /// D124). Quem aprende por aqui não repassa.
    pub async fn learn(&self, change: DefinitionApply) {
        let key = (change.kind, change.key.clone());
        let _guard = self.lock_key(&key).await;
        if let Err(error) = self.apply_unlocked(&change).await {
            tracing::warn!(kind = change.kind.as_str(), key = %change.key, code = %error.code, "definição dita por um par recusada");
        }
    }

    /// `POST /cluster/definitions/apply`.
    pub fn handle_apply(
        &self,
        change: DefinitionApply,
    ) -> Pin<Box<dyn Future<Output = Result<Response, TabaError>> + Send + '_>> {
        Box::pin(async move {
            if change.key.trim().is_empty() {
                return Err(bad("A chave da definição é obrigatória."));
            }
            let key = (change.kind, change.key.clone());
            let _guard = self.lock_key(&key).await;
            let applied = self.apply_unlocked(&change).await?;
            if change.relay && applied {
                self.relay(change.kind, &change.key);
            }
            Ok(Response::Definition(change))
        })
    }

    /// `POST /cluster/definitions/propose`: o dono decide a mudança.
    pub fn handle_propose(
        &self,
        proposal: DefinitionProposal,
    ) -> Pin<Box<dyn Future<Output = Result<Response, TabaError>> + Send + '_>> {
        Box::pin(async move {
            if proposal.key.trim().is_empty() {
                return Err(bad("A chave da definição é obrigatória."));
            }
            match self.service.node_state() {
                NodeState::Active => {}
                NodeState::Failed => {
                    return Err(TabaError::new(
                        ErrorCode::NodeFailed,
                        "O nó está em falha e não decide definições até ser excluído.",
                    )
                    .with_retry_after(5));
                }
                _ => {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNodeSyncing,
                        "O nó ainda está em sincronização e não decide definições.",
                    )
                    .with_retry_after(5));
                }
            }
            self.coordinate(proposal).await.map(Response::Definition)
        })
    }

    // ---- Streams ----

    fn page(query: &StreamQuery) -> usize {
        query
            .limit
            .map_or(STREAM_PAGE, |n| (n as usize).clamp(2, STREAM_PAGE))
    }

    /// `GET /cluster/topics`: cada linha é a definição mais a `version` que este nó aplicou.
    pub fn list_topics(&self, query: &StreamQuery) -> Response {
        let (definitions, more) = self
            .topics
            .definitions_from(query.from.as_deref(), Self::page(query));
        let items = definitions
            .iter()
            .map(|d| with_version(d.to_json(), self.version_of(DefinitionKind::Topic, &d.name)))
            .collect();
        Response::Stream(StreamPage::new(items, more))
    }

    /// `GET /cluster/tokens`: cada linha é o registro mais a `version` que este nó aplicou.
    pub fn list_tokens(&self, query: &StreamQuery) -> Response {
        let (records, more) = self
            .tokens
            .records_from(query.from.as_deref(), Self::page(query));
        let items = records
            .iter()
            .map(|r| {
                with_version(
                    token_to_json(r),
                    self.version_of(DefinitionKind::Token, &r.id),
                )
            })
            .collect();
        Response::Stream(StreamPage::new(items, more))
    }

    /// `GET /cluster/definitions/tombstones`: as lápides que este nó guarda (a remoção e a versão
    /// dela), para o nó que sincroniza saber as versões já usadas e não recriar o que foi removido
    /// (D125). Cada linha leva `id` (`tipo:chave`, a ordem e o cursor), `kind`, `key` e `version`.
    pub fn list_tombstones(&self, query: &StreamQuery) -> Response {
        let mut rows: Vec<(String, DefinitionKind, String, u64)> = lock(&self.versions)
            .iter()
            .filter(|(_, e)| e.deleted)
            .map(|((kind, key), e)| {
                (
                    format!("{}:{key}", kind.as_str()),
                    *kind,
                    key.clone(),
                    e.version,
                )
            })
            .filter(|(id, ..)| query.from.as_deref().is_none_or(|from| id.as_str() >= from))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let page = Self::page(query);
        let more = rows.len() > page;
        rows.truncate(page);
        let items = rows
            .into_iter()
            .map(|(id, kind, key, version)| {
                json!({"id": id, "kind": kind.as_str(), "key": key, "version": version})
            })
            .collect();
        Response::Stream(StreamPage::new(items, more))
    }

    /// Apaga as definições e os tokens locais (o nó volta como nó novo).
    pub async fn wipe(&self) {
        for name in self.topics.names() {
            let _ = self.topics.apply_delete(&name).await;
        }
        for id in self.tokens.ids() {
            let _ = self.tokens.apply_revoke(&id).await;
        }
        lock(&self.versions).clear();
    }

    // ---- Nó novo: puxar as tabelas ----

    /// Aplica o que o stream de uma origem trouxe: vale se a versão da origem não é menor que a
    /// deste nó (um evento que já chegou e é mais novo vence a carga).
    async fn load(&self, change: DefinitionApply) -> Result<(), TabaError> {
        let key = (change.kind, change.key.clone());
        let _guard = self.lock_key(&key).await;
        let fingerprint = fingerprint(&change)?;
        let same = lock(&self.versions).get(&key).is_some_and(|e| {
            e.version == change.version && !e.deleted && e.fingerprint == fingerprint
        });
        if same {
            return Ok(());
        }
        self.apply_unlocked(&change).await.map(|_| ())
    }

    /// Puxa de um membro as definições de tópico e os tokens (doc 02, seções 3.3 e 3.4) e devolve
    /// o que veio, para a verificação final conferir.
    pub async fn pull(&self, source: &Member) -> Result<Pulled, TabaError> {
        let mut pulled = Pulled::default();
        for (definition, version) in self.list_topics_of(source).await? {
            pulled.topics.insert(definition.name.clone());
            self.load(DefinitionApply {
                kind: DefinitionKind::Topic,
                key: definition.name.clone(),
                version,
                deleted: false,
                body: definition.to_json(),
                relay: false,
            })
            .await?;
        }
        for (record, version) in self.list_tokens_of(source).await? {
            pulled.tokens.insert(record.id.clone());
            self.load(DefinitionApply {
                kind: DefinitionKind::Token,
                key: record.id.clone(),
                version,
                deleted: false,
                body: token_to_json(&record),
                relay: false,
            })
            .await?;
        }
        for (kind, key, version) in self.list_tombstones_of(source).await? {
            self.load_tombstone(kind, key, version).await?;
        }
        Ok(pulled)
    }

    /// Guarda a lápide que a origem tem, se este nó não conhece uma versão igual ou maior da
    /// definição; se tem uma viva de versão menor, ela é removida.
    async fn load_tombstone(
        &self,
        kind: DefinitionKind,
        key: String,
        version: u64,
    ) -> Result<(), TabaError> {
        let tag = (kind, key.clone());
        let _guard = self.lock_key(&tag).await;
        if lock(&self.versions)
            .get(&tag)
            .is_some_and(|e| e.version >= version)
        {
            return Ok(());
        }
        let change = DefinitionApply {
            kind,
            key,
            version,
            deleted: true,
            body: Value::Null,
            relay: false,
        };
        self.apply_unlocked(&change).await.map(|_| ())
    }

    /// Verificação final (doc 05, seção 7.3, passo 5.5): repete a listagem de tópicos e de tokens e
    /// descarta o que a carga trouxe e já não aparece na origem (foi removido sem que o evento
    /// chegasse a este nó). Só mexe no que veio da carga. Devolve quantos apagou.
    pub async fn verify(&self, source: &Member, pulled: &Pulled) -> Result<usize, TabaError> {
        let topics: HashSet<String> = self
            .list_topics_of(source)
            .await?
            .into_iter()
            .map(|(d, _)| d.name)
            .collect();
        let tokens: HashSet<String> = self
            .list_tokens_of(source)
            .await?
            .into_iter()
            .map(|(r, _)| r.id)
            .collect();
        let mut dropped = 0;
        for name in pulled.topics.difference(&topics) {
            if self.topics.apply_delete(name).await.is_ok() {
                lock(&self.versions).remove(&(DefinitionKind::Topic, name.clone()));
                dropped += 1;
            }
        }
        for id in pulled.tokens.difference(&tokens) {
            if self.tokens.apply_revoke(id).await.is_ok() {
                lock(&self.versions).remove(&(DefinitionKind::Token, id.clone()));
                dropped += 1;
            }
        }
        Ok(dropped)
    }

    async fn list_topics_of(
        &self,
        source: &Member,
    ) -> Result<Vec<(TopicDefinition, u64)>, TabaError> {
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
                out.push((
                    TopicDefinition::from_json(item).map_err(bad)?,
                    version_of_item(item),
                ));
            }
            if !page.end.has_more || page.end.last == from {
                break;
            }
            from = page.end.last;
        }
        Ok(out)
    }

    async fn list_tokens_of(&self, source: &Member) -> Result<Vec<(TokenRecord, u64)>, TabaError> {
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
                out.push((token_from_json(item)?, version_of_item(item)));
            }
            if !page.end.has_more || page.end.last == from {
                break;
            }
            from = page.end.last;
        }
        Ok(out)
    }

    async fn list_tombstones_of(
        &self,
        source: &Member,
    ) -> Result<Vec<(DefinitionKind, String, u64)>, TabaError> {
        let mut out = Vec::new();
        let mut from: Option<String> = None;
        loop {
            let request = Request::ListTombstones(StreamQuery {
                from: from.clone(),
                to: None,
                limit: None,
            });
            let Response::Stream(page) = self.call(source, request).await? else {
                return Err(TabaError::internal());
            };
            for item in &page.items {
                let kind = serde_json::from_value::<DefinitionKind>(item["kind"].clone())
                    .map_err(|_| bad("A lápide não diz o tipo da definição."))?;
                let key = item["key"]
                    .as_str()
                    .ok_or_else(|| bad("A lápide não diz a chave da definição."))?;
                out.push((kind, key.to_owned(), version_of_item(item)));
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

/// A impressão do conteúdo de uma mudança: o SHA-256 do corpo normalizado (lido e escrito de novo pelo
/// tipo da definição, para a ordem dos campos e os campos omitidos não mudarem o resultado). A
/// remoção tem uma impressão só.
fn fingerprint(change: &DefinitionApply) -> Result<Fingerprint, TabaError> {
    let text = if change.deleted {
        b"deleted".to_vec()
    } else {
        let body = match change.kind {
            DefinitionKind::Topic => TopicDefinition::from_json(&change.body)
                .map_err(bad)?
                .to_json(),
            DefinitionKind::Token => token_to_json(&token_from_json(&change.body)?),
        };
        serde_json::to_vec(&body).map_err(|_| TabaError::internal())?
    };
    Ok(Sha256::digest(text).into())
}

fn with_version(mut item: Value, version: u64) -> Value {
    if let Some(object) = item.as_object_mut() {
        object.insert("version".into(), json!(version));
    }
    item
}

fn version_of_item(item: &Value) -> u64 {
    item.get("version").and_then(Value::as_u64).unwrap_or(0)
}

fn topic_change(proposal: &DefinitionProposal) -> Result<TopicChange, TabaError> {
    let definition = || {
        let definition = TopicDefinition::from_json(&proposal.body).map_err(bad)?;
        if definition.name != proposal.key {
            return Err(bad("O nome do tópico não é a chave da proposta."));
        }
        Ok(definition)
    };
    match proposal.operation {
        DefinitionOperation::Create => Ok(TopicChange::Create(definition()?)),
        DefinitionOperation::Update => Ok(TopicChange::Update(definition()?)),
        DefinitionOperation::Delete => {
            let id = proposal
                .body
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("falta `id` na remoção"))?
                .to_owned();
            Ok(TopicChange::Delete {
                name: proposal.key.clone(),
                id,
                expected_id: proposal.expected_id.clone(),
            })
        }
    }
}

fn token_change(proposal: &DefinitionProposal) -> Result<TokenChange, TabaError> {
    let record = || {
        let record = token_from_json(&proposal.body)?;
        if record.id != proposal.key {
            return Err(bad("O id do token não é a chave da proposta."));
        }
        Ok(record)
    };
    match proposal.operation {
        DefinitionOperation::Create => Ok(TokenChange::Create(record()?)),
        DefinitionOperation::Update => Ok(TokenChange::Update(record()?)),
        DefinitionOperation::Delete => Ok(TokenChange::Revoke(proposal.key.clone())),
    }
}

impl TopicReplicator for Definitions {
    fn replicate(
        &self,
        change: TopicChange,
    ) -> Pin<Box<dyn Future<Output = Result<(), TabaError>> + Send + '_>> {
        Box::pin(async move {
            let (operation, key, body, expected_id) = match change {
                TopicChange::Create(d) => (
                    DefinitionOperation::Create,
                    d.name.clone(),
                    d.to_json(),
                    None,
                ),
                TopicChange::Update(d) => (
                    DefinitionOperation::Update,
                    d.name.clone(),
                    d.to_json(),
                    None,
                ),
                TopicChange::Delete {
                    name,
                    id,
                    expected_id,
                } => (
                    DefinitionOperation::Delete,
                    name.clone(),
                    json!({ "name": name, "id": id }),
                    expected_id,
                ),
            };
            self.propose(DefinitionProposal {
                kind: DefinitionKind::Topic,
                operation,
                key,
                body,
                expected_id,
            })
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
            let (operation, key, body) = match change {
                TokenChange::Create(r) => {
                    (DefinitionOperation::Create, r.id.clone(), token_to_json(&r))
                }
                TokenChange::Update(r) => {
                    (DefinitionOperation::Update, r.id.clone(), token_to_json(&r))
                }
                TokenChange::Revoke(id) => {
                    (DefinitionOperation::Delete, id.clone(), json!({ "id": id }))
                }
            };
            self.propose(DefinitionProposal {
                kind: DefinitionKind::Token,
                operation,
                key,
                body,
                expected_id: None,
            })
            .await
        })
    }
}
