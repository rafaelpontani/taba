//! O backup opcional em S3 (doc 23, "Backup em S3"): a coordenação entre o motor de mensagens, o
//! `_backup.db` local, o depósito de objetos e o cluster.
//!
//! O S3 é **só backup de falha total ou grave**: SQLite e memória seguem como armazenamento
//! primário, e nenhuma publicação, consumo ou sincronização espera o S3. Sem `backup.s3.bucket`
//! este módulo não existe para o nó. Com o S3 fora do ar o recurso fica parado e o cluster funciona
//! como sem ele; o que se acumula é só o `outbox` (IDs) no disco.
//!
//! | Arquivo | O que faz |
//! |---|---|
//! | `upload` | O `outbox` vira segmentos (`.seg`); o estado mutável vira diário (`.jrn`); a limpeza de segmentos mortos; o flush |
//! | `evict` | Com o disco cheio, apaga o corpo das mensagens já salvas e lê de volta por `Range` |
//! | `definitions` | Definições de tópico e tokens (`topics/`, `tokens/`), com retentativa e reconciliação |
//! | `heartbeat` | O batimento de cada nó (`nodes/<id>.json`) e a classificação dos outros (vivo, vivo em outra partição, morto) |
//! | `restore` | A trava de restauração, a restauração do bucket e a limpeza de `nodes/` |
//! | `stop` | A lista dos nós vivos na parada programada do cluster |

mod definitions;
mod evict;
mod heartbeat;
mod restore;
mod stop;
mod topology;
mod upload;

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

pub use heartbeat::{Heartbeat, Liveness};
pub use restore::{RestoreReport, StartupDecision};
pub use stop::StopList;
pub use topology::{ClusterTopology, Solo, Topology};
pub use upload::FlushReport;

use crate::cluster::state::{DrainState, NodeShared};
use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::{BackupSink, Messages};
use crate::engine::tokens::Tokens;
use crate::engine::topics::Topics;
use crate::storage::backup::{BackupDb, ObjectStore, SavedRef};
use crate::storage::messages::MessageStore;

/// Os valores de `backup.*` de que o serviço depende.
#[derive(Debug, Clone)]
pub struct BackupRules {
    pub delay: Duration,
    pub meta_interval: Duration,
    pub segment_max_bytes: u64,
    pub segment_max_messages: usize,
    pub segment_max_age: Duration,
    pub heartbeat_interval: Duration,
    pub dead_after: Duration,
    pub drain_timeout: Duration,
    pub evict_above: u32,
    pub evict_to: u32,
    pub pause_at: u32,
    pub reconcile_interval: Duration,
    pub restore_lock_ttl: Duration,
}

impl BackupRules {
    pub fn from_config(config: &Config) -> Self {
        let b = &config.backup;
        Self {
            delay: b.delay,
            meta_interval: b.meta_interval,
            segment_max_bytes: b.segment_max_bytes,
            segment_max_messages: b.segment_max_messages as usize,
            segment_max_age: b.segment_max_age,
            heartbeat_interval: b.heartbeat_interval,
            dead_after: b.dead_after,
            drain_timeout: b.drain_timeout,
            evict_above: b.evict_above,
            evict_to: b.evict_to,
            pause_at: b.pause_at,
            reconcile_interval: b.reconcile_interval,
            restore_lock_ttl: b.restore_lock_ttl,
        }
    }
}

/// Uma definição que precisa ir (ou voltar a ir) ao bucket.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DefRef {
    Topic(String),
    /// O tópico foi removido: grava a lápide e apaga o prefixo das mensagens (`nome`, `topicId`).
    TopicDeleted(String, String),
    Token(String),
    TokenRevoked(String),
}

/// O que o serviço guarda em memória e que muda sem pressa.
#[derive(Default)]
pub(crate) struct Runtime {
    /// O S3 respondeu na última chamada?
    pub s3_ok: bool,
    pub last_error: Option<String>,
    pub last_ok_ms: Option<u64>,
    /// Segmentos mortos inteiros a apagar do bucket.
    pub dead_segments: Vec<String>,
    /// Definições a gravar (a gravação falhou ou o nó acabou de virar o gestor).
    pub pending_defs: BTreeSet<DefRef>,
    /// Tópicos removidos cujo prefixo de mensagens ainda precisa ser apagado: `(nome, topicId)`.
    pub purges: Vec<(String, String)>,
}

#[derive(Default)]
pub(crate) struct Counters {
    pub segments_uploaded: AtomicU64,
    pub messages_uploaded: AtomicU64,
    pub bytes_uploaded: AtomicU64,
    pub journals_uploaded: AtomicU64,
    pub segments_deleted: AtomicU64,
    pub errors: AtomicU64,
    pub bodies_evicted: AtomicU64,
    pub bodies_fetched: AtomicU64,
    pub definitions_written: AtomicU64,
}

/// Como o backup se vê: o que `GET /node/backup` e as respostas do flush mostram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupStatus {
    pub s3_ok: bool,
    pub last_error: Option<String>,
    pub last_ok_ms: Option<u64>,
    /// Mensagens ainda sem segmento no bucket.
    pub outbox_pending: u64,
    /// Mudanças de estado ainda fora do diário.
    pub journal_pending: u64,
    pub segments: u64,
    pub segment_bytes: u64,
    /// Mensagens com referência no bucket (as que este nó, dono ou réplica, pode ler de lá).
    pub saved: u64,
    pub segments_uploaded: u64,
    pub messages_uploaded: u64,
    pub errors: u64,
    pub bodies_evicted: u64,
    pub definitions_pending: u64,
}

impl BackupStatus {
    /// O que ainda falta enviar: o que o operador confere antes de uma manutenção.
    pub fn pending(&self) -> u64 {
        self.outbox_pending + self.journal_pending + self.definitions_pending
    }
}

pub struct Backup {
    pub(crate) rules: BackupRules,
    pub(crate) store: Arc<dyn ObjectStore>,
    pub db: Arc<BackupDb>,
    pub(crate) messages: Arc<Messages>,
    pub(crate) disk: Arc<MessageStore>,
    pub(crate) topics: Arc<Topics>,
    pub(crate) tokens: Arc<Tokens>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) shared: Arc<NodeShared>,
    pub(crate) topology: Arc<dyn Topology>,
    pub(crate) runtime: Mutex<Runtime>,
    pub(crate) written: Mutex<definitions::Written>,
    pub(crate) counters: Counters,
    /// O envio ao bucket (segmentos abertos): uma passada por vez.
    pub(crate) uploader: tokio::sync::Mutex<upload::Uploader>,
    pub(crate) beats: heartbeat::Beats,
    /// O batimento do nó está ligado (desliga no drain concluído e na saída).
    pub(crate) beating: AtomicBool,
    pub(crate) rt: tokio::runtime::Handle,
    pub(crate) restoring: AtomicBool,
    /// Quem diz o uso do disco (%); os testes o trocam para simular o disco enchendo.
    pub(crate) disk_probe: Mutex<Option<Arc<dyn Fn() -> u32 + Send + Sync>>>,
    /// A última leitura real do uso do disco e quando foi feita.
    pub(crate) disk_usage: Mutex<Option<(std::time::Instant, u32)>>,
    /// Os membros da tabela na última volta: se algum sumiu, o nó procura no bucket o que só ele
    /// tinha (doc 23, seção 1.7, exemplo B).
    pub(crate) last_members: Mutex<std::collections::BTreeSet<String>>,
    /// O ETag da trava de restauração que outro nó segura e desde quando o vemos igual.
    pub(crate) lock_seen: Mutex<Option<(String, std::time::Instant)>>,
    /// Acorda o laço de fundo quando há trabalho novo (flush, evicção).
    pub(crate) wake: tokio::sync::Notify,
    pub(crate) cancel: CancellationToken,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `backup.not-configured`: o recurso não está ligado neste nó.
pub fn not_configured() -> TabaError {
    TabaError::new(
        ErrorCode::BackupNotConfigured,
        "O backup em S3 não está configurado (backup.s3.bucket).",
    )
}

pub struct BackupArgs {
    pub rules: BackupRules,
    pub store: Arc<dyn ObjectStore>,
    pub db: Arc<BackupDb>,
    pub messages: Arc<Messages>,
    pub disk: Arc<MessageStore>,
    pub topics: Arc<Topics>,
    pub tokens: Arc<Tokens>,
    pub clock: Arc<dyn Clock>,
    pub shared: Arc<NodeShared>,
    pub topology: Arc<dyn Topology>,
    pub cancel: CancellationToken,
}

impl Backup {
    /// Cria o serviço e o liga ao motor (as mensagens), às tabelas (tópicos e tokens) e ao nó.
    /// Chamar de dentro de um runtime do Tokio.
    pub fn new(args: BackupArgs) -> Arc<Self> {
        let backup = Arc::new(Self {
            rules: args.rules,
            store: args.store,
            db: args.db,
            messages: args.messages,
            disk: args.disk,
            topics: args.topics,
            tokens: args.tokens,
            clock: args.clock,
            shared: args.shared,
            topology: args.topology,
            runtime: Mutex::new(Runtime::default()),
            written: Mutex::new(definitions::Written::default()),
            counters: Counters::default(),
            uploader: tokio::sync::Mutex::new(upload::Uploader::default()),
            beats: heartbeat::Beats::default(),
            beating: AtomicBool::new(true),
            rt: tokio::runtime::Handle::current(),
            restoring: AtomicBool::new(false),
            lock_seen: Mutex::new(None),
            last_members: Mutex::new(std::collections::BTreeSet::new()),
            disk_probe: Mutex::new(None),
            disk_usage: Mutex::new(None),
            wake: tokio::sync::Notify::new(),
            cancel: args.cancel,
        });
        backup.messages.set_backup(backup.clone());
        definitions::attach(&backup);
        backup
    }

    /// Onde a mensagem está no bucket e quando foi salva, se este nó (dono ou réplica) sabe. É a
    /// consulta única sobre a referência, que mora no `_backup.db` e não no `_topic.db`.
    pub fn saved_info(&self, topic: &str, id: &str) -> Option<SavedRef> {
        self.db.saved(topic, id).ok().flatten()
    }

    pub fn rules(&self) -> &BackupRules {
        &self.rules
    }

    /// Troca a medida do uso do disco (em %): só para os testes simularem o disco enchendo.
    pub fn set_disk_probe(&self, probe: Arc<dyn Fn() -> u32 + Send + Sync>) {
        *lock(&self.disk_probe) = Some(probe);
    }

    pub fn node_id(&self) -> String {
        self.shared.node_id()
    }

    /// Roda de uma vez o que o serviço faz em segundo plano, até o nó parar.
    pub async fn run(self: Arc<Self>) {
        let mut upload = tokio::time::interval(Duration::from_millis(500));
        let mut meta = tokio::time::interval(self.rules.meta_interval);
        let mut beat = tokio::time::interval(self.rules.heartbeat_interval);
        let mut evict = tokio::time::interval(Duration::from_secs(2));
        let mut reconcile = tokio::time::interval(self.rules.reconcile_interval);
        for ticker in [
            &mut upload,
            &mut meta,
            &mut beat,
            &mut evict,
            &mut reconcile,
        ] {
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        }
        // O primeiro `tick` de um `interval` é imediato: o diário e a reconciliação esperam a vez.
        meta.reset();
        reconcile.reset();
        // Mensagens que já estavam no nó antes de o backup ligar (ou depois de uma queda) entram no
        // outbox: o nó adota o que é dele.
        self.adopt_all();
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => return,
                _ = upload.tick() => {
                    self.upload_pass(false).await;
                    self.purge_dead_segments().await;
                    self.flush_definitions().await;
                    self.purge_removed_topics().await;
                }
                () = self.wake.notified() => {
                    self.evict_pass();
                }
                _ = meta.tick() => { self.journal_pass().await; }
                _ = beat.tick() => {
                    self.heartbeat_pass().await;
                    self.check_membership_loss().await;
                }
                _ = evict.tick() => { self.evict_pass(); }
                _ = reconcile.tick() => {
                    self.reconcile_definitions().await;
                    self.sweep_expired_segments().await;
                    self.cleanup_nodes().await;
                    self.adopt_all();
                }
            }
        }
    }

    /// O nó entra no outbox como dono de tudo o que o motor lhe diz ser dele e que ainda não está
    /// salvo.
    pub fn adopt_all(&self) {
        if !self.topology.is_active() {
            return;
        }
        for topic in self.messages.topic_names() {
            for id in self.messages.owned_ids(&topic) {
                BackupSink::adopted(self, &topic, &id);
            }
        }
    }

    pub fn status(&self) -> BackupStatus {
        let (segments, segment_bytes) = self.db.segment_totals().unwrap_or((0, 0));
        let runtime = lock(&self.runtime);
        BackupStatus {
            s3_ok: runtime.s3_ok,
            last_error: runtime.last_error.clone(),
            last_ok_ms: runtime.last_ok_ms,
            outbox_pending: self.db.outbox_len().unwrap_or(0),
            journal_pending: self.db.dirty_len().unwrap_or(0),
            segments,
            segment_bytes,
            saved: self.db.saved_len().unwrap_or(0),
            segments_uploaded: self.counters.segments_uploaded.load(Ordering::Relaxed),
            messages_uploaded: self.counters.messages_uploaded.load(Ordering::Relaxed),
            errors: self.counters.errors.load(Ordering::Relaxed),
            bodies_evicted: self.counters.bodies_evicted.load(Ordering::Relaxed),
            definitions_pending: runtime.pending_defs.len() as u64,
        }
    }

    /// Anota o resultado de uma chamada ao S3: o recurso vive ou está fora do ar.
    pub(crate) fn note<T, E: std::fmt::Display>(&self, what: &str, result: &Result<T, E>) {
        let mut runtime = lock(&self.runtime);
        match result {
            Ok(_) => {
                runtime.s3_ok = true;
                runtime.last_error = None;
                runtime.last_ok_ms = Some(self.clock.now_millis());
            }
            Err(e) => {
                if runtime.s3_ok || runtime.last_error.is_none() {
                    tracing::warn!(error = %e, "backup: {what} falhou; o S3 está fora do ar? O cluster segue sem ele");
                }
                runtime.s3_ok = false;
                runtime.last_error = Some(format!("{what}: {e}"));
                self.counters.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Roda um futuro até o fim numa thread à parte (o motor é síncrono e chama o backup com a
    /// trava do tópico; ver `BackupSink::fetch_body`).
    pub(crate) fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future + Send,
        F::Output: Send,
    {
        let handle = self.rt.clone();
        std::thread::scope(|scope| {
            scope
                .spawn(move || handle.block_on(future))
                .join()
                .expect("a thread do backup não deve entrar em pânico")
        })
    }

    /// O nó está em drain ou drenado: o batimento e o envio seguem só até o drain terminar.
    pub(crate) fn drained(&self) -> bool {
        self.shared.drain_state() == DrainState::Drained
    }
}

impl BackupSink for Backup {
    fn stored(&self, topic: &str, id: &str, owned: bool) {
        if !owned {
            return;
        }
        if self.db.saved(topic, id).ok().flatten().is_some() {
            return;
        }
        if let Err(e) = self.db.enqueue(topic, id, self.clock.now_millis()) {
            self.disk_failure("não foi possível gravar no _backup.db", &e);
        }
    }

    fn removed(&self, topic: &str, id: &str, owned: bool) {
        let result = if owned {
            self.db.mark_dead(topic, id).map(|dead| {
                if let Some(segment) = dead {
                    lock(&self.runtime).dead_segments.push(segment);
                }
            })
        } else {
            self.db.forget(topic, id)
        };
        if let Err(e) = result {
            self.disk_failure("não foi possível gravar no _backup.db", &e);
        }
    }

    fn changed(&self, topic: &str, id: &str, owned: bool) {
        if owned && let Err(e) = self.db.touch(topic, id) {
            self.disk_failure("não foi possível gravar no _backup.db", &e);
        }
    }

    fn adopted(&self, topic: &str, id: &str) {
        BackupSink::stored(self, topic, id, true);
    }

    fn released(&self, topic: &str, id: &str) {
        if let Err(e) = self.db.unqueue(topic, id) {
            self.disk_failure("não foi possível gravar no _backup.db", &e);
        }
    }

    fn fetch_body(&self, topic: &str, id: &str) -> Option<Vec<u8>> {
        let saved: SavedRef = self.db.saved(topic, id).ok().flatten()?;
        let body = self.block_on(self.read_saved(&saved));
        if body.is_some() {
            self.counters.bodies_fetched.fetch_add(1, Ordering::Relaxed);
        }
        body
    }

    fn before_disk_write(&self) -> bool {
        self.wait_for_disk()
    }
}

impl Backup {
    /// Um erro do `_backup.db` não derruba a publicação: vira log. Disco cheio ou banco corrompido
    /// derrubam o nó, como em qualquer outro banco dele (doc 23: Q9).
    fn disk_failure(&self, what: &str, e: &std::io::Error) {
        tracing::error!(error = %e, "{what}");
        if crate::storage::is_fatal(e) {
            self.shared.set_failed();
        }
    }
}

/// Como o cluster fala com o backup sem conhecer o resto do nó: o serviço e a parada programada
/// (que precisa do drain, uma peça do nó). O `ClusterService` guarda só uma referência fraca.
pub trait BackupPort: Send + Sync + 'static {
    fn backup(&self) -> Arc<Backup>;

    /// O drain deste nó para a parada do cluster (`POST /cluster/stop`): sai da rotação, manda
    /// tudo ao S3 e responde quando o nó concluiu, com o relatório em JSON.
    fn stop_node(
        &self,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<serde_json::Value, TabaError>> + Send + '_>>;
}

impl Backup {
    /// A réplica recebe o aviso do primário: das mensagens que ela guarda, quais já estão no
    /// bucket. O que ela não conhece é ignorado.
    pub fn accept_saved(&self, saved: &crate::cluster::protocol::BackupSaved) -> usize {
        let mut kept = 0;
        for item in &saved.items {
            if !self.messages.holds(&saved.topic, &item.id) {
                continue;
            }
            let reference = SavedRef {
                segment: item.segment.clone(),
                offset: item.offset,
                len: item.len,
                saved_at_ms: item.saved_at_ms,
            };
            if self
                .db
                .set_saved(&saved.topic, &item.id, &reference)
                .is_ok()
            {
                kept += 1;
            }
        }
        kept
    }

    /// Antes de excluir `candidate` por voto: espera o batimento dele vencer. Devolve `true` se
    /// pode seguir com a votação (o batimento venceu, ou o S3 não diz nada: o comportamento é o de
    /// sempre) e `false` se o canal voltou no meio da espera.
    pub async fn confirm_dead(&self, candidate: &str, reached: impl Fn() -> bool) -> bool {
        let mut announced = false;
        // O canal de um nó que morre pisca (reconecta e cai de novo): só conta como "voltou" se
        // estiver aberto em várias conferências seguidas.
        let mut steady = 0;
        loop {
            steady = if reached() { steady + 1 } else { 0 };
            if steady >= 3 {
                return false;
            }
            match self.liveness(candidate) {
                Liveness::Dead | Liveness::Unknown => return true,
                Liveness::Fresh => {}
            }
            if !announced {
                tracing::warn!(
                    candidate,
                    "backup: o nó não responde, mas o batimento dele no S3 está fresco (vivo em outra partição?); a exclusão espera o batimento vencer"
                );
                announced = true;
            }
            if self.cancel.is_cancelled() {
                return false;
            }
            tokio::time::sleep((self.rules.heartbeat_interval / 2).max(Duration::from_millis(30)))
                .await;
        }
    }

    /// Espera, por até `2 × backup.dead-after`, todos os outros membros ficarem mortos para o S3.
    /// Quem exclui os mortos um a um vê o último só depois que o batimento dele vence.
    pub async fn wait_all_others_dead(&self) -> bool {
        let deadline = std::time::Instant::now() + self.rules.dead_after * 2;
        loop {
            if self.all_others_dead() {
                return true;
            }
            if std::time::Instant::now() >= deadline || self.cancel.is_cancelled() {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Os outros membros estão todos mortos (para o S3)? É o sobrevivente único, que pode excluir
    /// os mortos mesmo abaixo de `cluster.vote.min-voters`.
    pub fn all_others_dead(&self) -> bool {
        let me = self.node_id();
        let members = self.topology.members();
        members
            .iter()
            .filter(|id| **id != me)
            .all(|id| self.liveness(id) == Liveness::Dead)
    }
}

/// Os nós que o cluster conhece, com o estado do batimento de cada um.
pub type Observed = HashMap<String, Liveness>;

#[cfg(test)]
mod tests;
