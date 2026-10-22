//! The propagation of **topic and token** definitions to all nodes (doc 02, sections 3.3 and 3.4;
//! doc 06c, "Topics and tokens"), and the `GET /cluster/topics` and `/cluster/tokens` streams that the new
//! node pulls after joining.
//!
//! Each definition has an **owner** on the ring: the first active node at the position of the hash of the topic name
//! or of the token `id`. The owner is the source of truth: only it decides a change, checks whether it fits
//! the current state (the name already exists, the topic does not exist...) and gives it an increasing **version**.
//! Every node keeps the full copy and reads locally; only the write goes through the owner.
//!
//! Whoever receives the client's call proposes the change to the owner (`DefinitionPropose`). The owner first writes
//! to an **heir** (the first other reachable owner of the position, so the change survives the owner's
//! fall), applies it to itself and sends it to all the other nodes (`DefinitionApply`). It answers after the nodes
//! that answer within `write.commit.timeout`; the owner and the heir queue the definition's state for all
//! peers in the channel's `definition` frame (D127), and the node that did not answer receives it when the channel
//! comes back, or is excluded if the queue overflows. Each node applies only a version that is not lower than
//! its own, so repeating or arriving out of order does not change the result. Removal leaves a tombstone with
//! the version, so a late change does not resurrect what was removed.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
use crate::storage::versions::{VersionRow, VersionStore};
use crate::telemetry::metrics::DEFINITION_CONFLICTS;

/// The default ceiling of lines in a stretch of stream (`cluster.sync.stream.max-items`, to be defined).
const STREAM_PAGE: usize = 500;
/// How long a tombstone is kept (if no node is pending for it).
const TOMBSTONE_TTL: Duration = Duration::from_secs(3600);

type Key = (DefinitionKind, String);
/// The SHA-256 of the content of a version of the definition.
type Fingerprint = [u8; 32];

/// What this node knows of a definition: the version it applied, whether it was removed and the fingerprint of the
/// content (so two copies of the same version are tie-broken the same on every node).
struct Entry {
    version: u64,
    deleted: bool,
    fingerprint: Fingerprint,
    at: SystemTime,
    /// The topic's `id` (a recreated topic has another one), also kept in its tombstone.
    topic_id: Option<String>,
}

/// What the definitions load brought (topic names and token IDs).
#[derive(Debug, Clone, Default)]
pub struct Pulled {
    pub topics: HashSet<String>,
    pub tokens: HashSet<String>,
}

pub struct Definitions {
    service: Arc<ClusterService>,
    topics: Arc<Topics>,
    tokens: Arc<Tokens>,
    /// Where the versions and the tombstones are kept on disk (`_node.db`).
    store: VersionStore,
    versions: Mutex<HashMap<Key, Entry>>,
    /// One change at a time on each definition (the owner's and the application on the nodes).
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

/// Erases the future's type (`tokio::spawn` needs `Send` and the calls between nodes are
/// recursive by nature).
fn boxed<T: Send + 'static>(
    future: impl Future<Output = T> + Send + 'static,
) -> Pin<Box<dyn Future<Output = T> + Send>> {
    Box::pin(future)
}

impl Definitions {
    /// `stored` are the rows of `definition_versions` of the disk, when the node resumes its definitions
    /// (otherwise empty, and the table is rebuilt as the definitions arrive): only those that still
    /// describe what `topics` and `tokens` hold, and the tombstones that did not expire, are kept.
    pub fn new(
        service: Arc<ClusterService>,
        topics: Arc<Topics>,
        tokens: Arc<Tokens>,
        store: VersionStore,
        stored: Vec<VersionRow>,
    ) -> Arc<Self> {
        let now = service.now();
        let (kept, dropped) = reconcile(stored, &topics, &tokens, now);
        for (kind, key) in dropped {
            if let Err(e) = store.delete(&kind, &key) {
                tracing::warn!(error = %e, "failed to delete a stale definition version");
            }
        }
        Arc::new(Self {
            service,
            topics,
            tokens,
            store,
            versions: Mutex::new(kept),
            locks: Mutex::new(HashMap::new()),
        })
    }

    /// Writes the entry's row to disk, off the async executor.
    async fn persist_row(&self, row: VersionRow, now: SystemTime) -> Result<(), TabaError> {
        let store = self.store.clone();
        let expired_before = unix_ms(now.checked_sub(TOMBSTONE_TTL).unwrap_or(UNIX_EPOCH));
        tokio::task::spawn_blocking(move || {
            store.save(&row)?;
            store.delete_tombstones_before(expired_before).map(|_| ())
        })
        .await
        .map_err(|_| TabaError::internal())?
        .map_err(|e| {
            tracing::error!(error = %e, "failed to write the definition version");
            TabaError::internal()
        })
    }

    /// Deletes the entry's row from disk.
    async fn unpersist(&self, kind: DefinitionKind, key: &str) {
        let (store, kind, key) = (self.store.clone(), kind.as_str(), key.to_owned());
        match tokio::task::spawn_blocking(move || store.delete(kind, &key)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "failed to delete the definition version"),
            Err(e) => tracing::warn!(error = %e, "failed to delete the definition version"),
        }
    }

    async fn lock_key(&self, key: &Key) -> OwnedMutexGuard<()> {
        let mutex = {
            let mut locks = lock(&self.locks);
            locks.retain(|_, m| Arc::strong_count(m) > 1);
            locks.entry(key.clone()).or_default().clone()
        };
        mutex.lock_owned().await
    }

    /// The version this node applied (`0` if it does not know the definition).
    fn version_of(&self, kind: DefinitionKind, key: &str) -> u64 {
        lock(&self.versions)
            .get(&(kind, key.to_owned()))
            .map_or(0, |e| e.version)
    }

    // ---- Who the owner is ----

    /// The definition's owners: the **active** nodes at the ring position, the first is the owner (a
    /// syncing node does not have the full copy yet). `503` if no active node occupies it.
    fn owners(&self, kind: DefinitionKind, key: &str) -> Result<Vec<Member>, TabaError> {
        let position = ring::definition_position(kind.as_str(), key);
        let owners: Vec<Member> = self
            .service
            .owners_of(position)?
            .into_iter()
            .filter(|m| m.state == MemberState::Active)
            .collect();
        if owners.is_empty() {
            return Err(unavailable("No active node owns this definition."));
        }
        Ok(owners)
    }

    // ---- Who proposes ----

    /// Takes the change to the owner and only returns when it already applies on this node too.
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
                // The owner waited for this node; repeating is harmless and guarantees "I read what I wrote".
                let key = (done.kind, done.key.clone());
                let _guard = self.lock_key(&key).await;
                self.apply_unlocked(&done).await?;
                Ok(())
            }
            Ok(_) => Err(TabaError::internal()),
            Err(CallError::Remote(error)) => Err(error),
            Err(other) => Err(unavailable(format!(
                "The definition's owner did not answer: {other}."
            ))),
        }
    }

    // ---- The owner ----

    /// Decides the change: checks it against the owner's state and gives the next version.
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

    /// Runs on the owner: decides, writes to the heir, applies to itself and spreads to the other nodes.
    async fn coordinate(&self, proposal: DefinitionProposal) -> Result<DefinitionApply, TabaError> {
        let key = (proposal.kind, proposal.key.clone());
        let _guard = self.lock_key(&key).await;
        let owners = self.owners(proposal.kind, &proposal.key)?;
        if !self.service.is_self(&owners[0]) {
            return Err(unavailable("This node is not the definition's owner."));
        }
        let decided = self.decide(&proposal)?;
        // The change must exist on another node before the owner answers: the first following
        // owner that confirms is its heir (if it falls, the change is not lost).
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
                "No other owner of the definition confirmed the change.",
            ));
        }
        self.apply_unlocked(&decided).await?;
        self.relay(decided.kind, &decided.key);
        let mut done = vec![owners[0].node_id.clone()];
        done.extend(heir.map(|m| m.node_id.clone()));
        self.spread(&decided, &done).await;
        Ok(decided)
    }

    /// Sends the change to the other nodes and waits for those that answer in `write.commit.timeout`; the
    /// others stay pending and the owner insists on them.
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
                        tracing::warn!(node_id = %member.node_id, code = %error.code, "definition refused by a node");
                        pending.remove(&member.node_id);
                    }
                    _ => {}
                }
            }
        })
        .await;
        set.detach_all();
    }

    /// The newest state this node has of a definition (or its tombstone).
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

    // ---- Who receives ----

    /// Does the change apply given what this node already has? A lower version loses. On the same version with
    /// different content (two decisions unaware of each other: the new attempt that reuses the
    /// version, or two nodes that thought they were owners), the one with the **lowest fingerprint** applies, the same choice on
    /// any node and in any order of arrival; the tie is counted and warned.
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
            "two copies of the definition with the same version and different content"
        );
        self.service.telemetry().registry.add(
            &DEFINITION_CONFLICTS,
            &[("taba.kind", change.kind.as_str())],
            1.0,
        );
        wins
    }

    /// Applies the change on this node if the version is not lower than the one it already has. Whoever calls holds the
    /// definition lock. Returns `false` when the change arrived too old.
    async fn apply_unlocked(&self, change: &DefinitionApply) -> Result<bool, TabaError> {
        self.apply_with(change, None).await
    }

    /// `apply_unlocked`, with the `topicId` a tombstone arrived with (when this node never held the topic).
    async fn apply_with(
        &self,
        change: &DefinitionApply,
        removed_topic_id: Option<String>,
    ) -> Result<bool, TabaError> {
        let key = (change.kind, change.key.clone());
        let fingerprint = fingerprint(change)?;
        if !self.wins_over_held(change, &fingerprint) {
            return Ok(false);
        }
        let mut topic_id = None;
        match (change.kind, change.deleted) {
            (DefinitionKind::Topic, false) => {
                let definition = TopicDefinition::from_json(&change.body).map_err(bad)?;
                if definition.name != change.key {
                    return Err(bad("The topic name is not the change's key."));
                }
                topic_id = Some(definition.id.clone());
                self.topics.apply_save(definition).await?;
            }
            (DefinitionKind::Topic, true) => {
                topic_id = self
                    .topics
                    .definition(&change.key)
                    .map(|d| d.id)
                    .or_else(|| {
                        lock(&self.versions)
                            .get(&key)
                            .and_then(|e| e.topic_id.clone())
                    })
                    .or(removed_topic_id);
                self.topics.apply_delete(&change.key).await?;
            }
            (DefinitionKind::Token, false) => {
                let record = token_from_json(&change.body)?;
                if record.id != change.key {
                    return Err(bad("The token id is not the change's key."));
                }
                self.tokens.apply_save(record).await?;
            }
            (DefinitionKind::Token, true) => self.tokens.apply_revoke(&change.key).await?,
        }
        let now = self.service.now();
        let entry = Entry {
            version: change.version,
            deleted: change.deleted,
            fingerprint,
            at: now,
            topic_id,
        };
        let row = row_of(&key, &entry);
        {
            let mut versions = lock(&self.versions);
            versions.insert(key.clone(), entry);
            versions.retain(|_, e| !expired(e, now));
        }
        self.persist_row(row, now).await?;
        Ok(true)
    }

    /// Queues for all peers the definition's current state, in the channel's `definition` frame: the node
    /// that did not answer in time (or was down) receives it when the channel comes back, without depending
    /// on whoever decided (D127). The owner and the heir do this.
    fn relay(&self, kind: DefinitionKind, key: &str) {
        let Some(change) = self.current(kind, key) else {
            return;
        };
        if let Some(replication) = self.service.replication() {
            replication.signals.announce_definition(change);
        }
    }

    /// A definition a peer said through the channel (`definition` frame): applies by the same rule as
    /// `definition-apply` (higher version applies; the same version with different content, the tie-break of
    /// D124). Whoever learns it here does not forward it.
    pub async fn learn(&self, change: DefinitionApply) {
        let key = (change.kind, change.key.clone());
        let _guard = self.lock_key(&key).await;
        if let Err(error) = self.apply_unlocked(&change).await {
            tracing::warn!(kind = change.kind.as_str(), key = %change.key, code = %error.code, "definition said by a peer refused");
        }
    }

    /// `POST /cluster/definitions/apply`.
    pub fn handle_apply(
        &self,
        change: DefinitionApply,
    ) -> Pin<Box<dyn Future<Output = Result<Response, TabaError>> + Send + '_>> {
        Box::pin(async move {
            if change.key.trim().is_empty() {
                return Err(bad("The definition key is required."));
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

    /// `POST /cluster/definitions/propose`: the owner decides the change.
    pub fn handle_propose(
        &self,
        proposal: DefinitionProposal,
    ) -> Pin<Box<dyn Future<Output = Result<Response, TabaError>> + Send + '_>> {
        Box::pin(async move {
            if proposal.key.trim().is_empty() {
                return Err(bad("The definition key is required."));
            }
            match self.service.node_state() {
                NodeState::Active => {}
                NodeState::Failed => {
                    return Err(TabaError::new(
                        ErrorCode::NodeFailed,
                        "The node is in failure and does not decide definitions until it is excluded.",
                    )
                    .with_retry_after(5));
                }
                _ => {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNodeSyncing,
                        "The node is still syncing and does not decide definitions.",
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

    /// `GET /cluster/topics`: each line is the definition plus the `version` this node applied.
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

    /// `GET /cluster/tokens`: each line is the record plus the `version` this node applied.
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

    /// `GET /cluster/definitions/tombstones`: the tombstones this node keeps (the removal and its
    /// version), so the syncing node knows the versions already used and does not recreate what was removed
    /// (D125). Each line carries `id` (`kind:key`, the order and the cursor), `kind`, `key`, `version` and, for a topic, its `topicId`.
    pub fn list_tombstones(&self, query: &StreamQuery) -> Response {
        let mut rows: Vec<(String, DefinitionKind, String, u64, Option<String>)> =
            lock(&self.versions)
                .iter()
                .filter(|(_, e)| e.deleted)
                .map(|((kind, key), e)| {
                    (
                        format!("{}:{key}", kind.as_str()),
                        *kind,
                        key.clone(),
                        e.version,
                        e.topic_id.clone(),
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
            .map(|(id, kind, key, version, topic_id)| {
                let mut item =
                    json!({"id": id, "kind": kind.as_str(), "key": key, "version": version});
                if let (Some(topic_id), Some(object)) = (topic_id, item.as_object_mut()) {
                    object.insert("topicId".into(), json!(topic_id));
                }
                item
            })
            .collect();
        Response::Stream(StreamPage::new(items, more))
    }

    /// Deletes the local definitions and tokens (the node comes back as a new node).
    pub async fn wipe(&self) {
        for name in self.topics.names() {
            let _ = self.topics.apply_delete(&name).await;
        }
        for id in self.tokens.ids() {
            let _ = self.tokens.apply_revoke(&id).await;
        }
        lock(&self.versions).clear();
        let store = self.store.clone();
        match tokio::task::spawn_blocking(move || store.clear()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "failed to clear the definition versions"),
            Err(e) => tracing::warn!(error = %e, "failed to clear the definition versions"),
        }
    }

    // ---- New node: pulling the tables ----

    /// Applies what a source's stream brought: it applies if the source's version is not lower than
    /// this node's (an event that already arrived and is newer beats the load).
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

    /// Pulls from a member the topic definitions and the tokens (doc 02, sections 3.3 and 3.4) and returns
    /// what came, for the final check to verify.
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
        for (kind, key, version, topic_id) in self.list_tombstones_of(source).await? {
            self.load_tombstone(kind, key, version, topic_id).await?;
        }
        Ok(pulled)
    }

    /// Stores the tombstone the source has, if this node does not know an equal or greater version of the
    /// definition; if it has a live one of a lower version, it is removed.
    async fn load_tombstone(
        &self,
        kind: DefinitionKind,
        key: String,
        version: u64,
        topic_id: Option<String>,
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
        self.apply_with(&change, topic_id).await.map(|_| ())
    }

    /// Final check (doc 05, section 7.3, step 5.5): repeats the topic and token listing and
    /// discards what the load brought and no longer shows up at the source (it was removed without the event
    /// reaching this node). It only touches what came from the load. Returns how many it deleted.
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
                self.unpersist(DefinitionKind::Topic, name).await;
                dropped += 1;
            }
        }
        for id in pulled.tokens.difference(&tokens) {
            if self.tokens.apply_revoke(id).await.is_ok() {
                lock(&self.versions).remove(&(DefinitionKind::Token, id.clone()));
                self.unpersist(DefinitionKind::Token, id).await;
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
    ) -> Result<Vec<(DefinitionKind, String, u64, Option<String>)>, TabaError> {
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
                    .map_err(|_| bad("The tombstone does not say the definition's type."))?;
                let key = item["key"]
                    .as_str()
                    .ok_or_else(|| bad("The tombstone does not say the definition's key."))?;
                out.push((
                    kind,
                    key.to_owned(),
                    version_of_item(item),
                    item["topicId"].as_str().map(str::to_owned),
                ));
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

/// The fingerprint of a change's content: the SHA-256 of the normalized body (read and written again by the
/// definition's type, so field order and omitted fields do not change the result). Removal
/// has a single fingerprint.
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
            return Err(bad("The topic name is not the proposal's key."));
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
                .ok_or_else(|| bad("missing `id` in the removal"))?
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
            return Err(bad("The token id is not the proposal's key."));
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

fn unix_ms(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A tombstone older than its time to live is forgotten (a live definition never expires).
fn expired(entry: &Entry, now: SystemTime) -> bool {
    entry.deleted
        && now
            .duration_since(entry.at)
            .is_ok_and(|age| age >= TOMBSTONE_TTL)
}

fn row_of(key: &Key, entry: &Entry) -> VersionRow {
    VersionRow {
        kind: key.0.as_str().to_owned(),
        key: key.1.clone(),
        version: entry.version,
        deleted: entry.deleted,
        fingerprint: entry.fingerprint,
        topic_id: entry.topic_id.clone(),
        at_ms: unix_ms(entry.at),
    }
}

/// What the node keeps of the versions stored on disk, given the definitions it has loaded: a live row
/// stays only if its definition is there (a topic with the same `id`; a recreated topic has another one,
/// so the old row does not describe it), a tombstone stays until it expires. Returns the entries and the
/// rows to delete from disk.
fn reconcile(
    stored: Vec<VersionRow>,
    topics: &Topics,
    tokens: &Tokens,
    now: SystemTime,
) -> (HashMap<Key, Entry>, Vec<(String, String)>) {
    let mut kept = HashMap::new();
    let mut dropped = Vec::new();
    for row in stored {
        let kind = match row.kind.as_str() {
            "topic" => DefinitionKind::Topic,
            "token" => DefinitionKind::Token,
            _ => {
                dropped.push((row.kind, row.key));
                continue;
            }
        };
        let entry = Entry {
            version: row.version,
            deleted: row.deleted,
            fingerprint: row.fingerprint,
            at: UNIX_EPOCH + Duration::from_millis(row.at_ms),
            topic_id: row.topic_id.clone(),
        };
        let describes_a_definition = match (kind, row.deleted) {
            // a tombstone of another life of the topic (recreated with another `id`) says nothing about it
            (DefinitionKind::Topic, true) => {
                !expired(&entry, now)
                    && !matches!(
                        (topics.definition(&row.key), &row.topic_id),
                        (Some(d), Some(id)) if d.id != *id
                    )
            }
            (DefinitionKind::Token, true) => !expired(&entry, now),
            (DefinitionKind::Topic, false) => topics
                .definition(&row.key)
                .is_some_and(|d| row.topic_id.as_ref().is_none_or(|id| *id == d.id)),
            (DefinitionKind::Token, false) => tokens.record(&row.key).is_some(),
        };
        if describes_a_definition {
            kept.insert((kind, row.key), entry);
        } else {
            dropped.push((row.kind, row.key));
        }
    }
    (kept, dropped)
}

#[cfg(test)]
mod tests;
