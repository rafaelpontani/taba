//! A sincronização de blocos de um nó que entra (doc 05, seção 7.3).
//!
//! O nó novo já consta na tabela de membros e recebe o protocolo normal do cluster, mas não pode
//! processar os eventos: guarda cada um no banco de eventos (`_sync/sync.db`) e confirma o
//! registro depois de gravar. Enquanto isso ele carrega, em ordem:
//!
//! 1. **tópicos e tokens**, puxados de um membro;
//! 2. **blocos**: para cada bloco de que é responsável, a lista de IDs de todas as outras origens
//!    (união, por *merge* ordenado), as mensagens em faixas balanceadas entre elas, e o que faltou
//!    em outra origem, na rota de ID único e, por fim, na relistagem (o que ainda consta e não tem
//!    conteúdo em nenhum nó é uma **perda**);
//! 3. **eventos**, por cima da carga: sempre o mais antigo, aplicado e apagado, até esvaziar;
//! 4. **verificação final**: repete as listagens e descarta o que a carga trouxe e já não aparece;
//! 5. **troca para o modo normal** (*cutover*): quando o banco esvazia, sob a mesma trava que a
//!    recepção usa para decidir o destino de cada evento, o nó passa a processar em tempo real.
//!
//! Só então o nó fica pronto e passa a ser dono das mensagens dos blocos dele. Este módulo é
//! também o **lado que serve** a sincronização (`source`) e a limpeza do que o nó deixou de ter de
//! guardar (passo 7).

mod source;

pub use source::BlockSource;

use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::members::{Member, MemberState};
use super::protocol::{Ack, MessageRecord, Request, Response, Step, StreamQuery};
use super::replication::{Definitions, Pulled, Replica};
use super::service::ClusterService;
use super::state::NodeState;
use super::transport::CallError;
use crate::config::Config;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring::{BlockId, Ring};
use crate::core::time;
use crate::engine::messages::{Loaded, Messages};
use crate::storage::losses::{Loss, LossStore};
use crate::storage::synclog::{StoredEvent, SyncLog};
use crate::telemetry::metrics::{
    SYNC_BLOCKS_ACTIVE, SYNC_BYTES, SYNC_DURATION, SYNC_FAILURES, SYNC_IDS_LISTED, SYNC_LOG_BYTES,
    SYNC_LOG_EVENTS, SYNC_MESSAGES_PULLED, SYNC_PHASE,
};

/// Os parâmetros `cluster.sync.*` (doc 10) com os padrões escolhidos pela implementação.
#[derive(Debug, Clone)]
pub struct SyncRules {
    /// `cluster.sync.stream.max-items`.
    pub max_items: usize,
    /// `cluster.sync.stream.max-bytes`.
    pub max_bytes: u64,
    /// `cluster.sync.range.size`: IDs por faixa pedida a uma origem.
    pub range_size: usize,
    /// `cluster.sync.log.max-bytes`.
    pub log_max_bytes: u64,
    /// `cluster.sync.timeout`: quanto tempo se tenta de novo uma origem que não serve.
    pub timeout: Duration,
    pub storage_dir: PathBuf,
}

impl SyncRules {
    pub fn from_config(config: &Config) -> Self {
        let sync = &config.cluster.sync;
        Self {
            max_items: sync.stream_max_items.map_or(500, |n| (n as usize).max(2)),
            max_bytes: sync.stream_max_bytes.unwrap_or(4 * 1024 * 1024),
            range_size: sync.range_size.map_or(200, |n| (n as usize).max(1)),
            log_max_bytes: sync.log_max_bytes.unwrap_or(1024 * 1024 * 1024),
            timeout: sync.timeout.unwrap_or(Duration::from_secs(60)),
            storage_dir: config.storage.dir.clone(),
        }
    }
}

/// Por que a sincronização não terminou.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("nenhum nó de origem atendeu: {0}")]
    NoSource(String),
    #[error("falha de disco: {0}")]
    Disk(String),
    #[error("o banco de eventos passou de cluster.sync.log.max-bytes")]
    LogOverflow,
    #[error("o nó parou durante a sincronização")]
    Cancelled,
}

impl SyncError {
    fn reason(&self) -> &'static str {
        match self {
            SyncError::NoSource(_) => "no-source",
            SyncError::Disk(_) => "disk",
            SyncError::LogOverflow => "log-overflow",
            SyncError::Cancelled => "cancelled",
        }
    }
}

fn disk(e: &io::Error) -> SyncError {
    SyncError::Disk(e.to_string())
}

const PHASES: [&str; 5] = ["topics", "blocks", "replay", "verification", "ready"];

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Uma chamada que o nó em sincronização guarda no banco de eventos em vez de processar.
fn is_event(request: &Request) -> bool {
    matches!(
        request,
        Request::Topic(_)
            | Request::Token(_)
            | Request::Write { .. }
            | Request::Visibility { .. }
            | Request::Ack { .. }
    )
}

fn step_operation<R>(step: &Step<R>, register: impl Fn(&R) -> &str) -> String {
    match step {
        Step::Register(body) => register(body).to_owned(),
        Step::Commit(op) | Step::Rollback(op) => op.operation_id.clone(),
    }
}

fn operation_of(request: &Request) -> String {
    match request {
        Request::Topic(step) => step_operation(step, |r| &r.operation_id),
        Request::Token(step) => step_operation(step, |r| &r.operation_id),
        Request::Write { step, .. } => step_operation(step, |r| &r.operation_id),
        Request::Visibility { step, .. } => step_operation(step, |r| &r.operation_id),
        Request::Ack { step, .. } => step_operation(step, |r| &r.operation_id),
        _ => String::new(),
    }
}

/// Uma linha da listagem de IDs de um bloco.
#[derive(Debug, Clone)]
struct IdLine {
    id: String,
    topic: String,
    delete_at_ms: Option<u64>,
}

/// Um ID da união das listas, com as origens que o têm (índices na lista de origens).
#[derive(Debug, Clone)]
struct Merged {
    line: IdLine,
    holders: Vec<usize>,
}

struct Cursor {
    last: Option<String>,
    buf: VecDeque<IdLine>,
    more: bool,
    dead: bool,
}

/// O *merge* ordenado das listas de IDs de várias origens, sem materializá-las: cada origem tem
/// um trecho em memória e o seguinte só é pedido quando o anterior acaba.
struct MergedIds<'a> {
    syncer: &'a Syncer,
    block: String,
    sources: &'a [Member],
    cursors: Vec<Cursor>,
}

impl<'a> MergedIds<'a> {
    fn new(syncer: &'a Syncer, block: String, sources: &'a [Member]) -> Self {
        Self {
            syncer,
            block,
            sources,
            cursors: sources
                .iter()
                .map(|_| Cursor {
                    last: None,
                    buf: VecDeque::new(),
                    more: true,
                    dead: false,
                })
                .collect(),
        }
    }

    /// Alguma origem deixou de responder: a união está incompleta.
    fn degraded(&self) -> bool {
        self.cursors.iter().any(|c| c.dead)
    }

    async fn fill(&mut self, index: usize) -> Result<(), SyncError> {
        let request = Request::BlockIds {
            block: self.block.clone(),
            query: StreamQuery {
                from: self.cursors[index].last.clone(),
                to: None,
                limit: Some(u32::try_from(self.syncer.rules().max_items).unwrap_or(u32::MAX)),
            },
        };
        let reply = self.syncer.call_source(&self.sources[index], request).await;
        let cursor = &mut self.cursors[index];
        let page = match reply {
            Ok(Response::Stream(page)) => page,
            Ok(_) | Err(_) => {
                tracing::warn!(
                    block = %self.block,
                    peer = %self.sources[index].node_id,
                    "a origem não listou os IDs do bloco"
                );
                cursor.dead = true;
                cursor.more = false;
                return Ok(());
            }
        };
        for item in &page.items {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            // A retomada devolve de novo a última linha lida: é ignorada.
            if cursor.last.as_deref() == Some(id) {
                continue;
            }
            cursor.buf.push_back(IdLine {
                id: id.to_owned(),
                topic: item
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                delete_at_ms: item
                    .get("deleteAt")
                    .and_then(|v| v.as_str())
                    .and_then(time::parse_ms),
            });
        }
        cursor.more = page.end.has_more && page.end.last != cursor.last && !page.items.is_empty();
        cursor.last = page.end.last;
        Ok(())
    }

    /// Até `count` IDs da união, em ordem. Vazio quando acabou.
    async fn next_batch(&mut self, count: usize) -> Result<Vec<Merged>, SyncError> {
        let mut out = Vec::new();
        while out.len() < count {
            for index in 0..self.cursors.len() {
                if self.cursors[index].buf.is_empty()
                    && self.cursors[index].more
                    && !self.cursors[index].dead
                {
                    self.fill(index).await?;
                }
            }
            if self.cursors.iter().all(|c| c.dead) {
                return Err(SyncError::NoSource(format!(
                    "nenhuma origem listou o bloco {}",
                    self.block
                )));
            }
            let Some(min) = self
                .cursors
                .iter()
                .filter_map(|c| c.buf.front())
                .map(|l| (l.id.clone(), l.topic.clone()))
                .min()
            else {
                break;
            };
            let mut holders = Vec::new();
            let mut line = None;
            for (index, cursor) in self.cursors.iter_mut().enumerate() {
                if cursor
                    .buf
                    .front()
                    .is_some_and(|l| l.id == min.0 && l.topic == min.1)
                {
                    line = cursor.buf.pop_front();
                    holders.push(index);
                }
            }
            if let Some(line) = line {
                out.push(Merged { line, holders });
            }
        }
        Ok(out)
    }
}

pub struct Syncer {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    definitions: Arc<Definitions>,
    replica: Arc<Replica>,
    losses: LossStore,
    rules: SyncRules,
    /// `Some` enquanto o nó grava os eventos no banco (modo de sincronização). A trava também
    /// decide, a cada evento, entre o banco e o processamento normal (*cutover*).
    recording: Mutex<Option<Arc<SyncLog>>>,
    overflow: AtomicBool,
    pulled: Mutex<Pulled>,
    round_robin: AtomicUsize,
    /// A versão da tabela para a qual a limpeza já rodou.
    cleaned_version: Mutex<Option<u64>>,
    /// Quando as definições foram puxadas de novo por causa de um tópico desconhecido.
    refreshed_at: Mutex<Option<Instant>>,
    /// Gancho dos testes: espera o semáforo ao entrar na fase.
    pause: Mutex<Option<(&'static str, Arc<tokio::sync::Semaphore>)>>,
}

impl Syncer {
    pub fn new(
        service: Arc<ClusterService>,
        messages: Arc<Messages>,
        definitions: Arc<Definitions>,
        replica: Arc<Replica>,
        losses: LossStore,
        rules: SyncRules,
    ) -> Arc<Self> {
        Arc::new(Self {
            service,
            messages,
            definitions,
            replica,
            losses,
            rules,
            recording: Mutex::new(None),
            overflow: AtomicBool::new(false),
            pulled: Mutex::new(Pulled::default()),
            round_robin: AtomicUsize::new(0),
            cleaned_version: Mutex::new(None),
            refreshed_at: Mutex::new(None),
            pause: Mutex::new(None),
        })
    }

    fn rules(&self) -> &SyncRules {
        &self.rules
    }

    /// O nó está gravando os eventos no banco?
    pub fn is_recording(&self) -> bool {
        lock(&self.recording).is_some()
    }

    /// Abre o banco de eventos e passa a gravar. Vem antes de o nó instalar a tabela de membros:
    /// a partir daí ele recebe o protocolo normal.
    pub fn begin(&self) -> io::Result<()> {
        let log = SyncLog::create(&self.rules.storage_dir)?;
        self.overflow.store(false, Ordering::SeqCst);
        *lock(&self.recording) = Some(Arc::new(log));
        Ok(())
    }

    /// Larga o banco de eventos sem processá-lo (o nó volta a ser novo).
    pub fn abandon(&self) {
        if let Some(log) = lock(&self.recording).take() {
            let _ = log.discard();
        }
    }

    // ---- Receber eventos durante a sincronização ----

    /// Guarda o evento no banco em vez de processá-lo e devolve a confirmação do registro. `None`
    /// quando o nó processa normalmente (não é um evento, ou a sincronização já terminou).
    pub fn intercept(&self, request: &Request) -> Result<Option<Response>, TabaError> {
        if !is_event(request) {
            return Ok(None);
        }
        let guard = lock(&self.recording);
        let Some(log) = guard.as_ref() else {
            return Ok(None);
        };
        let call = request.to_http();
        log.append(&call.method, &call.path, &call.headers, &call.body)
            .map_err(|e| {
                tracing::error!(error = %e, "falha ao gravar o evento no banco de eventos");
                TabaError::internal()
            })?;
        let registry = &self.service.telemetry().registry;
        registry.add(&SYNC_LOG_EVENTS, &[], 1.0);
        if let Ok(size) = log.size_bytes() {
            registry.set(&SYNC_LOG_BYTES, &[], size as f64);
            if size > self.rules.log_max_bytes {
                self.overflow.store(true, Ordering::SeqCst);
            }
        }
        Ok(Some(Response::Ack(Ack {
            operation_id: operation_of(request),
            accepted: true,
        })))
    }

    // ---- Sincronizar ----

    /// Só para os testes: a sincronização para ao entrar em `phase` e segue quando o semáforo
    /// ganha uma licença. Serve para o teste agir (publicar, confirmar, mudar a tabela) com o nó
    /// em sincronização.
    #[doc(hidden)]
    pub fn pause_at(&self, phase: &'static str, gate: Arc<tokio::sync::Semaphore>) {
        *lock(&self.pause) = Some((phase, gate));
    }

    async fn phase(&self, current: &str) {
        let registry = &self.service.telemetry().registry;
        for phase in PHASES {
            registry.set(
                &SYNC_PHASE,
                &[("taba.phase", phase)],
                if phase == current { 1.0 } else { 0.0 },
            );
        }
        let gate = lock(&self.pause)
            .as_ref()
            .filter(|(phase, _)| *phase == current)
            .map(|(_, gate)| gate.clone());
        if let Some(gate) = gate
            && let Ok(permit) = gate.acquire().await
        {
            permit.forget();
        }
    }

    fn log(&self) -> Result<Arc<SyncLog>, SyncError> {
        lock(&self.recording)
            .clone()
            .ok_or_else(|| SyncError::Disk("o banco de eventos não foi aberto".into()))
    }

    fn check(&self) -> Result<(), SyncError> {
        if self.service.cancel_token().is_cancelled() {
            return Err(SyncError::Cancelled);
        }
        if self.overflow.load(Ordering::SeqCst) {
            return Err(SyncError::LogOverflow);
        }
        Ok(())
    }

    /// Carrega tudo o que o nó precisa e troca para o modo normal. `sources` são os membros
    /// de onde puxar as definições (o que coordenou o ingresso primeiro).
    pub async fn run(&self, sources: &[Member]) -> Result<(), SyncError> {
        let result = self.run_phases(sources).await;
        if let Err(e) = &result {
            self.service.telemetry().registry.add(
                &SYNC_FAILURES,
                &[("taba.reason", e.reason())],
                1.0,
            );
        }
        result
    }

    async fn run_phases(&self, sources: &[Member]) -> Result<(), SyncError> {
        let log = self.log()?;
        self.phase("topics").await;
        let pulled = self.pull_definitions(sources).await?;
        *lock(&self.pulled) = pulled.clone();

        self.phase("blocks").await;
        let ring = self.service.membership().ring();
        let me = self.service.node_id();
        let factor = self.service.rules().replication_factor.max(1) as usize;
        let blocks = ring.blocks_of(&me, factor);
        for block in &blocks {
            self.check()?;
            self.sync_block(&ring, *block, &log).await?;
        }

        self.phase("replay").await;
        self.replay(&log, false).await?;

        self.phase("verification").await;
        self.verify(&ring, &blocks, &log, &pulled, sources).await?;

        self.phase("ready").await;
        self.replay(&log, true).await?;
        log.discard().map_err(|e| disk(&e))?;
        self.service
            .telemetry()
            .registry
            .set(&SYNC_LOG_BYTES, &[], 0.0);
        Ok(())
    }

    /// Puxa as definições de um membro; tenta os outros e repete até `cluster.sync.timeout`.
    async fn pull_definitions(&self, sources: &[Member]) -> Result<Pulled, SyncError> {
        let deadline = Instant::now() + self.rules.timeout;
        loop {
            self.check()?;
            let mut last = String::from("sem origem");
            for source in sources {
                match self.definitions.pull(source).await {
                    Ok(pulled) => return Ok(pulled),
                    Err(e) => last = format!("{}: {}", source.node_id, e.message),
                }
            }
            if Instant::now() >= deadline {
                return Err(SyncError::NoSource(last));
            }
            self.nap().await;
        }
    }

    async fn nap(&self) {
        tokio::select! {
            () = self.service.cancel_token().cancelled() => {}
            () = tokio::time::sleep(self.service.rules().retry_backoff) => {}
        }
    }

    /// Chama uma origem. Falha de rede, `5xx` e `409` (a tabela de lá ainda é outra) repetem até
    /// `cluster.sync.timeout`; o resto é a resposta da origem.
    async fn call_source(&self, source: &Member, request: Request) -> Result<Response, TabaError> {
        let deadline = Instant::now() + self.rules.timeout;
        loop {
            if self.service.cancel_token().is_cancelled() {
                return Err(TabaError::new(ErrorCode::ClusterNodeSyncing, "o nó parou"));
            }
            let failure = match self
                .service
                .call_peer(source, request.clone(), self.service.rules().call_timeout)
                .await
            {
                Ok(reply) => return Ok(reply),
                Err(CallError::Remote(e))
                    if e.status() < 500 && e.code != ErrorCode::ClusterConflict =>
                {
                    return Err(e);
                }
                Err(CallError::Remote(e)) => e,
                Err(other) => TabaError::new(ErrorCode::ClusterNodeSyncing, other.to_string()),
            };
            if Instant::now() >= deadline {
                return Err(failure);
            }
            self.nap().await;
        }
    }

    /// As origens de um bloco: os outros nós responsáveis por ele.
    fn sources_of(&self, ring: &Ring, block: BlockId) -> Vec<Member> {
        let me = self.service.node_id();
        let factor = self.service.rules().replication_factor.max(1) as usize;
        let Some(table) = self.service.membership().table() else {
            return Vec::new();
        };
        ring.block_owners(block, factor)
            .unwrap_or_default()
            .into_iter()
            .filter(|owner| *owner != me)
            .filter_map(|owner| table.member(owner).cloned())
            .collect()
    }

    async fn sync_block(
        &self,
        ring: &Ring,
        block: BlockId,
        log: &SyncLog,
    ) -> Result<(), SyncError> {
        let sources = self.sources_of(ring, block);
        if sources.is_empty() {
            return Ok(());
        }
        let registry = &self.service.telemetry().registry;
        let started = Instant::now();
        registry.add(&SYNC_BLOCKS_ACTIVE, &[], 1.0);
        let result = self.load_block(block, &sources, log).await;
        registry.add(&SYNC_BLOCKS_ACTIVE, &[], -1.0);
        registry.observe(&SYNC_DURATION, &[], started.elapsed().as_secs_f64());
        if result.is_ok() {
            tracing::info!(block = %block, sources = sources.len(), "bloco sincronizado");
        }
        result
    }

    async fn load_block(
        &self,
        block: BlockId,
        sources: &[Member],
        log: &SyncLog,
    ) -> Result<(), SyncError> {
        let text = block.to_string();
        let mut ids = MergedIds::new(self, text.clone(), sources);
        loop {
            self.check()?;
            let batch = ids.next_batch(self.rules.range_size).await?;
            if batch.is_empty() {
                return Ok(());
            }
            self.service
                .telemetry()
                .registry
                .add(&SYNC_IDS_LISTED, &[], batch.len() as f64);
            let missing = self.pull_range(&text, sources, &batch, log).await?;
            self.settle_missing(&text, sources, missing).await?;
        }
    }

    /// Pede as mensagens de uma faixa de IDs a uma origem, e o que faltou, às outras.
    async fn pull_range(
        &self,
        block: &str,
        sources: &[Member],
        batch: &[Merged],
        log: &SyncLog,
    ) -> Result<Vec<Merged>, SyncError> {
        let wanted: Vec<&Merged> = batch
            .iter()
            .filter(|m| m.line.delete_at_ms.is_none())
            .collect();
        let (Some(first), Some(last)) = (wanted.first(), wanted.last()) else {
            return Ok(Vec::new());
        };
        // A origem da faixa é a que tem mais dos IDs; empatando, as origens se revezam.
        let turn = self.round_robin.fetch_add(1, Ordering::Relaxed);
        let score = |index: usize| wanted.iter().filter(|m| m.holders.contains(&index)).count();
        let best = (0..sources.len()).map(score).max().unwrap_or(0);
        let tied: Vec<usize> = (0..sources.len()).filter(|i| score(*i) == best).collect();
        let chosen = tied[turn % tied.len()];

        let mut got: HashSet<String> = HashSet::new();
        self.stream_range(
            block,
            &sources[chosen],
            &first.line.id,
            &last.line.id,
            log,
            &mut got,
        )
        .await;

        let missing_now = |got: &HashSet<String>| -> Vec<&Merged> {
            wanted
                .iter()
                .copied()
                .filter(|m| !got.contains(&m.line.id))
                .collect()
        };
        // O que faltou na faixa é buscado em outra origem que o tenha.
        for index in (0..sources.len()).filter(|i| *i != chosen) {
            let missing = missing_now(&got);
            let from_here: Vec<&&Merged> = missing
                .iter()
                .filter(|m| m.holders.contains(&index))
                .collect();
            let (Some(low), Some(high)) = (from_here.first(), from_here.last()) else {
                continue;
            };
            let (low, high) = (low.line.id.clone(), high.line.id.clone());
            self.stream_range(block, &sources[index], &low, &high, log, &mut got)
                .await;
        }
        // O último recurso: a rota de ID único.
        let still: Vec<Merged> = missing_now(&got).into_iter().cloned().collect();
        let mut gone = Vec::new();
        for entry in still {
            let mut found = false;
            for holder in &entry.holders {
                let request = Request::BlockMessage {
                    block: block.to_owned(),
                    id: entry.line.id.clone(),
                };
                if let Ok(Response::Record(record)) =
                    self.call_source(&sources[*holder], request).await
                {
                    let mut fresh = HashSet::new();
                    self.apply_record(block, &sources[*holder], record, log, &mut fresh)
                        .await;
                    found = !fresh.is_empty();
                    if found {
                        got.insert(entry.line.id.clone());
                        break;
                    }
                }
            }
            if !found {
                gone.push(entry);
            }
        }
        Ok(gone)
    }

    /// Lê o stream de mensagens `from`–`to` de uma origem, de trecho em trecho, e aplica cada uma.
    /// Uma falha da origem só deixa de somar ao `got`: o que faltar é buscado em outro lugar.
    async fn stream_range(
        &self,
        block: &str,
        source: &Member,
        from: &str,
        to: &str,
        log: &SyncLog,
        got: &mut HashSet<String>,
    ) {
        let mut cursor = Some(from.to_owned());
        loop {
            let request = Request::BlockMessages {
                block: block.to_owned(),
                query: StreamQuery {
                    from: cursor.clone(),
                    to: Some(to.to_owned()),
                    limit: Some(u32::try_from(self.rules.max_items).unwrap_or(u32::MAX)),
                },
            };
            let Ok(Response::Stream(page)) = self.call_source(source, request).await else {
                return;
            };
            for item in &page.items {
                match serde_json::from_value::<MessageRecord>(item.clone()) {
                    Ok(record) => self.apply_record(block, source, record, log, got).await,
                    Err(e) => tracing::warn!(error = %e, "mensagem ilegível na sincronização"),
                }
            }
            if !page.end.has_more || page.end.last == cursor || page.items.is_empty() {
                return;
            }
            cursor = page.end.last;
        }
    }

    /// Grava uma mensagem da carga. Uma de tópico ainda desconhecido faz o nó puxar as definições
    /// de novo (o tópico foi criado depois da primeira carga).
    async fn apply_record(
        &self,
        block: &str,
        source: &Member,
        record: MessageRecord,
        log: &SyncLog,
        got: &mut HashSet<String>,
    ) {
        let (topic, id) = (record.topic.clone(), record.id.clone());
        let size = record.body.len() as f64;
        let mut outcome = self.load(&record).await;
        if matches!(outcome, Ok(Loaded::UnknownTopic)) {
            self.refresh_definitions(source).await;
            outcome = self.load(&record).await;
        }
        let registry = &self.service.telemetry().registry;
        match outcome {
            Ok(Loaded::Stored | Loaded::Existing) => {
                registry.add(
                    &SYNC_MESSAGES_PULLED,
                    &[("taba.peer", &source.node_id)],
                    1.0,
                );
                registry.add(&SYNC_BYTES, &[("taba.peer", &source.node_id)], size);
                if let Err(e) = log.mark_loaded(block, &[(topic, id.clone())]) {
                    tracing::error!(error = %e, "falha ao anotar a mensagem carregada");
                }
                got.insert(id);
            }
            // Venceu por ttl, ou o tópico foi removido: não há o que guardar nem o que cobrar.
            Ok(Loaded::Expired | Loaded::UnknownTopic) => {
                got.insert(id);
            }
            Err(e) => {
                tracing::error!(topic = %topic, id = %id, error = %e.message, "falha ao gravar a mensagem carregada");
            }
        }
    }

    async fn load(&self, record: &MessageRecord) -> Result<Loaded, TabaError> {
        let (messages, topic) = (self.messages.clone(), record.topic.clone());
        let (meta, body) = (record.to_meta(), record.body.clone());
        tokio::task::spawn_blocking(move || messages.apply_load(&topic, meta, body))
            .await
            .map_err(|_| TabaError::internal())?
    }

    async fn refresh_definitions(&self, source: &Member) {
        {
            let mut at = lock(&self.refreshed_at);
            if at.is_some_and(|t| t.elapsed() < Duration::from_millis(200)) {
                return;
            }
            *at = Some(Instant::now());
        }
        match self.definitions.pull(source).await {
            Ok(pulled) => {
                let mut all = lock(&self.pulled);
                all.topics.extend(pulled.topics);
                all.tokens.extend(pulled.tokens);
            }
            Err(e) => {
                tracing::warn!(error = %e.message, "não foi possível puxar as definições de novo")
            }
        }
    }

    /// As mensagens listadas que nenhuma origem entregou: a relistagem diz se foram confirmadas
    /// (somem das listas, e o caso é descartado em silêncio) ou se são uma perda.
    async fn settle_missing(
        &self,
        block: &str,
        sources: &[Member],
        missing: Vec<Merged>,
    ) -> Result<(), SyncError> {
        for entry in missing {
            let mut listed = false;
            let mut consulted = Vec::new();
            for source in sources {
                let request = Request::BlockIds {
                    block: block.to_owned(),
                    query: StreamQuery {
                        from: Some(entry.line.id.clone()),
                        to: Some(entry.line.id.clone()),
                        limit: Some(2),
                    },
                };
                consulted.push(source.node_id.clone());
                if let Ok(Response::Stream(page)) = self.call_source(source, request).await
                    && page.items.iter().any(|item| {
                        item.get("id").and_then(|v| v.as_str()) == Some(entry.line.id.as_str())
                    })
                {
                    listed = true;
                }
            }
            if !listed {
                continue;
            }
            tracing::error!(
                event.name = "taba.message.lost",
                messaging.destination.name = %entry.line.topic,
                message.id = %entry.line.id,
                block,
                nodes = ?consulted,
                "mensagem perdida: consta nas listas e nenhum nó tem o conteúdo"
            );
            let loss = Loss {
                topic: entry.line.topic.clone(),
                id: entry.line.id.clone(),
                block: block.to_owned(),
                detected_ms: self.service.clock().now_millis(),
            };
            if let Err(e) = self.losses.record(&loss) {
                tracing::error!(error = %e, "falha ao registrar a perda");
            }
        }
        Ok(())
    }

    // ---- Eventos ----

    /// Processa os eventos do banco, sempre o mais antigo, até esvaziar. Com `cutover`, quando o
    /// banco esvazia o nó passa ao modo normal sob a mesma trava da recepção.
    async fn replay(&self, log: &SyncLog, cutover: bool) -> Result<(), SyncError> {
        loop {
            self.check()?;
            let next = {
                let mut guard = lock(&self.recording);
                let next = log.oldest().map_err(|e| disk(&e))?;
                if next.is_none() && cutover {
                    *guard = None;
                }
                next
            };
            let Some(event) = next else {
                return Ok(());
            };
            self.apply_event(&event).await;
            log.delete(event.seq).map_err(|e| disk(&e))?;
            self.service
                .telemetry()
                .registry
                .add(&SYNC_LOG_EVENTS, &[], -1.0);
        }
    }

    async fn apply_event(&self, event: &StoredEvent) {
        let request = match Request::from_http(
            &event.method,
            &event.path,
            &event.headers,
            &event.body,
        ) {
            Ok(request) => request,
            Err(e) => {
                tracing::warn!(path = %event.path, error = %e.message, "evento ilegível descartado");
                return;
            }
        };
        // O evento vence a carga e é idempotente: um erro (a mensagem já não está aqui, a
        // definição já existe) não muda nada.
        let result = match request {
            Request::Topic(step) => self.definitions.handle_topic(step).await.map(|_| ()),
            Request::Token(step) => self.definitions.handle_token(step).await.map(|_| ()),
            other => self.replica.replay(other).await,
        };
        if let Err(e) = result {
            tracing::debug!(path = %event.path, code = %e.code, "evento da sincronização sem efeito");
        }
    }

    // ---- Verificação final ----

    /// Repete as listagens só para o que o nó pretende manter e descarta o que a carga trouxe e já
    /// não aparece em nenhuma origem (doc 05, seção 7.3, passo 5.5).
    async fn verify(
        &self,
        ring: &Ring,
        blocks: &[BlockId],
        log: &SyncLog,
        pulled: &Pulled,
        sources: &[Member],
    ) -> Result<(), SyncError> {
        let all = lock(&self.pulled).clone();
        let pulled = Pulled {
            topics: pulled.topics.union(&all.topics).cloned().collect(),
            tokens: pulled.tokens.union(&all.tokens).cloned().collect(),
        };
        for source in sources {
            if let Ok(dropped) = self.definitions.verify(source, &pulled).await {
                if dropped > 0 {
                    tracing::info!(dropped, "definições removidas na verificação final");
                }
                break;
            }
        }
        for block in blocks {
            self.check()?;
            let text = block.to_string();
            let loaded = log.loaded_in(&text).map_err(|e| disk(&e))?;
            if loaded.is_empty() {
                continue;
            }
            let block_sources = self.sources_of(ring, *block);
            if block_sources.is_empty() {
                continue;
            }
            let mut ids = MergedIds::new(self, text.clone(), &block_sources);
            let mut listed: HashSet<String> = HashSet::new();
            loop {
                let batch = ids.next_batch(self.rules.max_items).await?;
                if batch.is_empty() {
                    break;
                }
                listed.extend(batch.into_iter().map(|m| m.line.id));
            }
            if ids.degraded() {
                // Uma origem não respondeu: a união está incompleta e não prova que algo sumiu.
                tracing::warn!(block = %text, "verificação do bloco adiada: uma origem não respondeu");
                continue;
            }
            for (topic, id) in loaded {
                if listed.contains(&id) {
                    continue;
                }
                let messages = self.messages.clone();
                let (t, i) = (topic.clone(), id.clone());
                let _ = tokio::task::spawn_blocking(move || messages.apply_ack(&t, &i)).await;
                tracing::info!(topic = %topic, id = %id, "mensagem confirmada durante a carga descartada na verificação final");
            }
        }
        Ok(())
    }

    // ---- Limpeza dos blocos que o nó perdeu ----

    /// A limpeza dos nós que perderam o bloco (doc 05, seção 7.3, passo 7): só com todo o cluster
    /// íntegro (nenhum nó em sincronização, nenhuma mudança de membros pendente), e uma vez por
    /// versão da tabela.
    pub async fn clean_once(&self) {
        if self.service.node_state() != NodeState::Active {
            return;
        }
        let Some(table) = self.service.membership().table() else {
            return;
        };
        if table.members.iter().any(|m| m.state != MemberState::Active)
            || self.service.membership().has_pending(self.service.now())
        {
            return;
        }
        if *lock(&self.cleaned_version) == Some(table.version) {
            return;
        }
        let ring = self.service.membership().ring();
        let me = self.service.node_id();
        let factor = self.service.rules().replication_factor.max(1) as usize;
        let messages = self.messages.clone();
        let removed = tokio::task::spawn_blocking(move || {
            messages
                .forget_foreign(&|position| ring.owners(position, factor).contains(&me.as_str()))
        })
        .await
        .unwrap_or(0);
        *lock(&self.cleaned_version) = Some(table.version);
        if removed > 0 {
            tracing::info!(
                removed,
                "cópias de blocos que deixaram de ser do nó apagadas"
            );
        }
    }

    /// A tarefa da limpeza, até o nó parar.
    pub async fn run_cleanup(self: Arc<Self>) {
        let cancel = self.service.cancel_token().clone();
        let mut ticker = tokio::time::interval(self.service.rules().keepalive_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => self.clean_once().await,
            }
        }
    }
}
