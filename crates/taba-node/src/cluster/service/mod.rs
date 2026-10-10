//! O nó dentro do cluster: atende as chamadas de outros nós (`Handler`), coordena o ingresso de
//! um nó novo (doc 04, seção 6.2) e entra num cluster existente (o bootstrap).
//!
//! Registro e desregistro de nó usam o protocolo em duas fases e exigem a confirmação de
//! **todos** os nós, sem quórum reduzido. Antes do commit a operação pode ser abortada (rollback);
//! depois de decidida não volta atrás, e o coordenador insiste até o teto `write.commit.timeout`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::members::{Change, Member, MemberState, MemberTable, Membership, MembershipError};
use super::protocol::{
    Ack, JoinRequest, NodeRegisterRequest, NodeUnregisterRequest, OperationRef, Request, Response,
    UnregisterReason,
};
use super::replication::Replication;
use super::state::{NodeShared, NodeState};
use super::sync::SyncError;
use super::transport::{
    BoxFuture, CallError, ChannelLink, ChannelOpen, Handler, PeerAddr, Transport,
};
use crate::config::Config;
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::id;
use crate::telemetry::metrics::{CLUSTER_BOOTSTRAP_FAILURES, CLUSTER_MEMBERSHIP_CHANGES};
use crate::telemetry::{Telemetry, trace};

/// O prazo de uma chamada a outro nó. Não é configurável por ora (D93).
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// O prazo do `join`: o pedido espera a vez e coordena duas fases, então demora mais.
const JOIN_TIMEOUT: Duration = Duration::from_secs(60);
/// Segundos sugeridos em `Retry-After` quando o nó ainda não tem a tabela.
const SYNCING_RETRY_AFTER_SECS: u64 = 5;

/// Os parâmetros do protocolo de entrada (`write.*`, `cluster.*`).
#[derive(Debug, Clone)]
pub struct ClusterRules {
    /// `write.retry.attempts`
    pub retry_attempts: u32,
    /// `write.retry.backoff`
    pub retry_backoff: Duration,
    /// `write.commit.timeout`
    pub commit_timeout: Duration,
    /// `cluster.bootstrap.retry.interval`
    pub bootstrap_retry_interval: Duration,
    /// `cluster.bootstrap.retry.max-attempts`; `0` não limita.
    pub bootstrap_max_attempts: u32,
    /// `cluster.first-node`
    pub first_node: bool,
    /// `cluster.entry-address`
    pub entry: Option<PeerAddr>,
    /// `write.pending.ttl`
    pub pending_ttl: Duration,
    /// `ring.virtual-nodes`
    pub virtual_nodes: u32,
    /// `replication.factor`
    pub replication_factor: u32,
    /// `write.quorum`
    pub quorum: u32,
    /// `topics.volatile.idle-check.retry-interval` (provisório, D114: 30 s): a espera do gestor
    /// quando algum nó não respondeu à consulta de ociosidade.
    pub volatile_retry_interval: Duration,
    /// `topics.volatile.takeover.jitter` (provisório, D114: 2 s): a janela em que o gestor que
    /// assume uma fila espalha a primeira consulta.
    pub volatile_jitter: Duration,
    /// `delivery.get.max-attempts` (provisório, D114: 3): quantos registros de invisibilidade que
    /// não fecham um `GET` tolera, tentando o ID seguinte da lista, antes de parar.
    pub get_max_attempts: usize,
    /// `delivery.take.max-attempts` (a definir no doc 10: 3): quantos nós um `GET` com a lista
    /// local vazia consulta no máximo.
    pub take_max_attempts: usize,
    /// `cluster.notify.window`: quanto o responsável pelos avisos acumula as mensagens prontas.
    pub notify_window: Duration,
    /// `cluster.notify.wait` (a definir: 50 ms): quanto espera o nó avisado vir buscar (`take`).
    pub notify_wait: Duration,
    /// `cluster.channel.keepalive.interval` (a definir: 2 s): de quanto em quanto tempo o nó repete
    /// os sinais de estado, no lugar do sinal de vida do canal.
    pub keepalive_interval: Duration,
    pub call_timeout: Duration,
    pub join_timeout: Duration,
    /// `cluster.channel.reconnect.retries`: chamadas seguidas ao canal que falham antes de o nó
    /// abrir a votação de exclusão do peer.
    pub reconnect_retries: u32,
    /// `cluster.channel.reconnect.backoff`.
    pub reconnect_backoff: Duration,
    /// `cluster.channel.queue.max-size`.
    pub queue_max_size: usize,
    /// `cluster.channel.queue.max-age`.
    pub queue_max_age: Duration,
    /// `cluster.channel.keepalive.max-topics`.
    pub keepalive_max_topics: usize,
    /// `cluster.vote.quorum-percent`.
    pub vote_quorum_percent: u32,
    /// `cluster.vote.min-voters`.
    pub vote_min_voters: u32,
    /// `cluster.freezing.duration`.
    pub freezing_duration: Duration,
    /// `cluster.message-loss.ttl` (a definir: 14 dias, o `ttl` padrão da mensagem).
    pub message_loss_ttl: Duration,
    /// `cluster.recovery.timeout` (a definir: 5 minutos).
    pub recovery_timeout: Duration,
    /// `cluster.reunion.interval` (a definir: dez vezes o sinal de vida, no mínimo 1 s): de quanto
    /// em quanto tempo o nó confere se o outro lado de uma partição simétrica voltou (D101).
    pub reunion_interval: Duration,
    /// `fifo.forward.max-attempts` (a definir: 3).
    pub fifo_forward_max_attempts: u32,
}

impl ClusterRules {
    pub fn from_config(config: &Config) -> Self {
        let keepalive_interval = config
            .cluster
            .keepalive_interval
            .unwrap_or(Duration::from_secs(2));
        Self {
            retry_attempts: config.write.retry_attempts,
            retry_backoff: config.write.retry_backoff,
            commit_timeout: config.write.commit_timeout,
            bootstrap_retry_interval: config.cluster.bootstrap_retry_interval,
            bootstrap_max_attempts: config.cluster.bootstrap_retry_max_attempts,
            first_node: config.cluster.first_node,
            entry: config
                .cluster
                .entry_address
                .as_deref()
                .and_then(|a| a.parse().ok()),
            pending_ttl: config.write.pending_ttl,
            virtual_nodes: config.cluster.virtual_nodes,
            replication_factor: config.write.replication_factor,
            quorum: config.write.quorum,
            volatile_retry_interval: config
                .topics
                .volatile_idle_check_retry_interval
                .unwrap_or(Duration::from_secs(30)),
            volatile_jitter: config
                .topics
                .volatile_takeover_jitter
                .unwrap_or(Duration::from_secs(2)),
            get_max_attempts: config
                .delivery
                .get_max_attempts
                .map_or(3, |n| n.max(1) as usize),
            take_max_attempts: config.delivery.take_max_attempts.map_or(3, |n| n as usize),
            notify_window: config.cluster.notify_window,
            notify_wait: config
                .cluster
                .notify_wait
                .unwrap_or(Duration::from_millis(50)),
            keepalive_interval,
            call_timeout: CALL_TIMEOUT,
            join_timeout: JOIN_TIMEOUT,
            reconnect_retries: config.cluster.reconnect_retries.map_or(3, |n| n.max(1)),
            reconnect_backoff: config
                .cluster
                .reconnect_backoff
                .unwrap_or(Duration::from_secs(1)),
            queue_max_size: config.cluster.queue_max_size.map_or(10_000, |n| n as usize),
            queue_max_age: config
                .cluster
                .queue_max_age
                .unwrap_or(Duration::from_secs(60)),
            keepalive_max_topics: config
                .cluster
                .keepalive_max_topics
                .map_or(1000, |n| n as usize),
            vote_quorum_percent: config.cluster.vote_quorum_percent,
            vote_min_voters: config.cluster.vote_min_voters,
            freezing_duration: config
                .cluster
                .freezing_duration
                .unwrap_or(Duration::from_secs(300)),
            message_loss_ttl: config
                .cluster
                .message_loss_ttl
                .unwrap_or(Duration::from_secs(1_209_600)),
            recovery_timeout: config
                .cluster
                .recovery_timeout
                .unwrap_or(Duration::from_secs(300)),
            reunion_interval: config
                .cluster
                .reunion_interval
                .unwrap_or_else(|| (keepalive_interval * 10).max(Duration::from_secs(1))),
            fifo_forward_max_attempts: config.fifo.forward_max_attempts.map_or(3, |n| n.max(1)),
        }
    }
}

/// O bootstrap esgotou as tentativas (`cluster.bootstrap.retry.max-attempts`) ou o nó parou.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("o nó não conseguiu entrar no cluster em {attempts} tentativas: {last}")]
    Exhausted { attempts: u32, last: String },
    #[error("o nó parou antes de entrar no cluster")]
    Cancelled,
}

/// Uma tentativa de bootstrap que não deu certo.
struct Failure {
    reason: &'static str,
    message: String,
}

pub struct ClusterService {
    local: RwLock<Member>,
    membership: Membership,
    node: Arc<NodeShared>,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    telemetry: Arc<Telemetry>,
    rules: ClusterRules,
    /// Uma mudança de membros por vez neste nó (doc 04, seção 6.8): o `join` espera a vez.
    change_turn: Mutex<()>,
    cancel: CancellationToken,
    /// A replicação de definições e de mensagens, ligada depois da subida do nó.
    replication: OnceLock<Arc<Replication>>,
    /// Chamado quando a tabela de membros muda (o motor reavalia quem é o dono).
    on_change: OnceLock<Arc<dyn Fn() + Send + Sync>>,
    /// As votações de exclusão em andamento (candidato → quando abriu), neste nó e nas que ele
    /// ajudou a votar: enquanto houver uma, nenhuma entrada de nó é efetivada (doc 04, 6.8).
    open_votes: StdMutex<HashMap<String, Instant>>,
    /// O nó está refazendo o ingresso por ter sido excluído.
    excluded: AtomicBool,
    /// O processo já consultou a lista de perdas dos outros nós (D111): só a primeira listagem do
    /// bootstrap faz isso, e o nó que volta de uma exclusão ou de um drain já o fez.
    boot_recovered: AtomicBool,
    /// Os membros que a listagem do reinício trouxe, a quem o nó devolve o que o disco da vida
    /// anterior tem das perdas, depois de ativo (PRB-048).
    old_disk_peers: StdMutex<Vec<Member>>,
    /// Quem monta o `NodeStatus` deste nó (a camada de operação; `GET /cluster/status`).
    status_source: OnceLock<StatusSource>,
}

/// Devolve o `NodeStatus` (doc 06b) deste nó, como JSON.
pub type StatusSource = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;

/// O `Handler` do nó: o que as rotas `/cluster/*` e a rede em memória chamam.
pub struct ClusterHandler(pub Arc<ClusterService>);

impl Handler for ClusterHandler {
    fn handle<'a>(
        &'a self,
        caller: &'a str,
        request: Request,
    ) -> BoxFuture<'a, Result<Response, TabaError>> {
        Box::pin(async move { self.0.handle(caller, request).await })
    }

    fn admit_channel(&self, caller: &str, open: &ChannelOpen) -> Result<(), TabaError> {
        self.0.admit_channel(caller, open)
    }

    fn accept_channel(&self, open: ChannelOpen, link: ChannelLink) {
        self.0.accept_channel(open, link);
    }
}

/// A espera entre duas tentativas de desregistro quando outra mudança de membros está em curso:
/// 100 a 200 ms. Com o prazo fixo, dois nós que saem juntos repetiam o registro no mesmo instante,
/// um recusava o outro de novo e só paravam no `drain.timeout`; o sorteio desfaz o compasso (a
/// ordem deterministicamente justa entre mudanças diferentes é a D94).
pub(crate) fn conflict_wait() -> Duration {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("o sistema não tem fonte de números aleatórios");
    Duration::from_millis(100 + u64::from_le_bytes(bytes) % 100)
}

/// Na partição simétrica, perde (refaz o ingresso) o lado com menos membros; no empate, o do
/// menor `nodeId`. Os dois lados chegam à mesma conclusão (D101).
fn loses_reunion(mine: &MemberTable, theirs: &MemberTable) -> bool {
    let smallest = |t: &MemberTable| t.members.iter().map(|m| m.node_id.clone()).min();
    (mine.members.len(), smallest(mine)) < (theirs.members.len(), smallest(theirs))
}

/// O `operationId` de uma fase 1, para quem a coordena conferir se a mudança ainda é dele.
fn request_operation(request: &Request) -> Option<&str> {
    match request {
        Request::Register(r) => Some(&r.operation_id),
        Request::Unregister(r) => Some(&r.operation_id),
        _ => None,
    }
}

fn node_syncing() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterNodeSyncing,
        "O nó ainda não tem a tabela de membros.",
    )
    .with_retry_after(SYNCING_RETRY_AFTER_SECS)
}

fn from_membership(error: MembershipError) -> TabaError {
    match error {
        MembershipError::NoTable => node_syncing(),
        MembershipError::Conflict(message) => TabaError::new(ErrorCode::ClusterConflict, message),
        MembershipError::UnknownOperation => TabaError::new(
            ErrorCode::ClusterOperationUnknown,
            "O nó não conhece essa operação (nunca chegou ou já expirou).",
        ),
    }
}

fn random_below(n: usize) -> usize {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("o sistema não tem fonte de números aleatórios");
    (u64::from_le_bytes(bytes) % n.max(1) as u64) as usize
}

impl ClusterService {
    pub fn new(
        local: Member,
        node: Arc<NodeShared>,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn Clock>,
        telemetry: Arc<Telemetry>,
        rules: ClusterRules,
        cancel: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            local: RwLock::new(local),
            membership: Membership::new(rules.pending_ttl, rules.virtual_nodes),
            node,
            transport,
            clock,
            telemetry,
            rules,
            change_turn: Mutex::new(()),
            cancel,
            replication: OnceLock::new(),
            on_change: OnceLock::new(),
            open_votes: StdMutex::new(HashMap::new()),
            excluded: AtomicBool::new(false),
            boot_recovered: AtomicBool::new(false),
            old_disk_peers: StdMutex::new(Vec::new()),
            status_source: OnceLock::new(),
        })
    }

    /// Liga quem monta o `NodeStatus`, para o `GET /cluster/status` responder.
    pub fn set_status_source(&self, source: StatusSource) {
        let _ = self.status_source.set(source);
    }

    /// Liga a replicação (definições, mensagens, sinais e repasse) ao serviço.
    pub fn set_replication(&self, replication: Arc<Replication>) {
        let _ = self.replication.set(replication);
    }

    pub fn replication(&self) -> Option<&Arc<Replication>> {
        self.replication.get()
    }

    /// Registra quem é avisado quando a tabela de membros muda.
    pub fn set_on_change(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.on_change.set(hook);
    }

    fn membership_changed(&self) {
        // A votação de um nó que saiu da tabela acabou: se ele voltar com o mesmo ID, a marca
        // antiga não vale de novo (e não bloqueia as entradas até `write.pending.ttl`).
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|candidate, _| self.membership.is_member(candidate));
        if let Some(hook) = self.on_change.get() {
            hook();
        }
    }

    /// Depois do commit de uma exclusão por votação, enfileira o quadro `frozen` para todos os pares:
    /// o membro que estava fora do ar no commit o recebe quando o canal volta (doc 04, seção 6.7).
    fn announce_frozen(&self, change: Option<&Change>) {
        let Some(Change::Exclude { node_id }) = change else {
            return;
        };
        let Some(frozen_at) = self.membership.frozen_at(node_id) else {
            return;
        };
        if let Some(replication) = self.replication.get() {
            replication.signals.announce_frozen(node_id, frozen_at);
        }
    }

    /// Depois do commit de uma mudança de membros, enfileira o quadro `membership` para todos os
    /// pares: o membro que perdeu o registro ou o commit a aplica ao receber (D126).
    fn announce_membership(&self, operation_id: &str, version: u64, change: &Change) {
        if let Some(replication) = self.replication.get() {
            replication
                .signals
                .announce_membership(operation_id, version, change);
        }
    }

    /// Uma mudança de membros efetivada que um par disse pelo canal (D126): este nó perdeu o
    /// registro ou o commit dela e a aplica agora, na ordem das versões.
    pub(crate) fn learn_membership(&self, operation_id: &str, version: u64, change: Change) {
        let me = self.node_id();
        if let Change::Remove { node_id } | Change::Exclude { node_id } = &change
            && *node_id == me
        {
            // A saída deste nó não se aplica à tabela dele: quem trata é a regra da exclusão.
            return;
        }
        let applied = self
            .membership
            .learn(operation_id, version, change, self.now());
        self.after_learning(applied);
    }

    /// Aplica as mudanças guardadas que passaram a ser as seguintes (o commit normal pode ter
    /// avançado a versão enquanto elas esperavam).
    fn settle_membership(&self) {
        let applied = self.membership.learn_held(self.now());
        self.after_learning(applied);
    }

    fn after_learning(&self, applied: Vec<(String, u64, Change)>) {
        if applied.is_empty() {
            return;
        }
        for (operation_id, version, change) in &applied {
            self.count_change(Some(change), "channel", "ok");
            tracing::info!(%operation_id, version, ?change, "mudança de membros aprendida pelo canal");
            self.announce_membership(operation_id, *version, change);
        }
        self.membership_changed();
    }

    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    pub fn telemetry(&self) -> &Arc<Telemetry> {
        &self.telemetry
    }

    /// Os nós responsáveis pela posição, em ordem (primário primeiro): `replication.factor` nós
    /// distintos, ou menos se o cluster tem menos. `503` se o nó ainda não tem a tabela.
    pub fn owners_of(&self, position: u64) -> Result<Vec<Member>, TabaError> {
        let table = self.membership.table().ok_or_else(node_syncing)?;
        let ring = self.membership.ring();
        let wanted = self.rules.replication_factor.max(1) as usize;
        Ok(ring
            .owners(position, wanted)
            .into_iter()
            .filter_map(|id| table.member(id).cloned())
            .collect())
    }

    /// O IP com que este nó se apresenta a si mesmo (uma chamada local passa pela mesma
    /// conferência de membro que uma que veio da rede).
    pub fn self_caller(&self) -> String {
        self.local().addresses.first().cloned().unwrap_or_default()
    }

    pub fn is_self(&self, member: &Member) -> bool {
        member.node_id == self.node_id()
    }

    /// Faz a chamada a um membro: a si mesmo sem rede, aos outros pelo `Transport`.
    pub(crate) async fn call_member(
        self: &Arc<Self>,
        member: &Member,
        request: Request,
    ) -> Result<Response, CallError> {
        if self.is_self(member) {
            return self
                .handle(&self.self_caller(), request)
                .await
                .map_err(CallError::Remote);
        }
        self.call_peer(member, request, self.rules.call_timeout)
            .await
    }

    /// `call_member` com as tentativas e o intervalo de `write.retry.*` para falha de rede e
    /// erro `5xx` do par.
    pub(crate) async fn call_member_retrying(
        self: &Arc<Self>,
        member: &Member,
        request: Request,
    ) -> Result<Response, CallError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            crate::telemetry::trace::attempt(attempt);
            let result = self.call_member(member, request.clone()).await;
            let retry = match &result {
                Err(e) if e.is_network() => true,
                Err(CallError::Remote(e)) => e.status() >= 500,
                _ => false,
            };
            if !retry || attempt >= self.rules.retry_attempts {
                return result;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return result,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    pub fn local(&self) -> Member {
        self.local.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn node_id(&self) -> String {
        self.local
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .node_id
            .clone()
    }

    /// Anota que há uma votação de exclusão do nó em andamento.
    pub(crate) fn mark_vote(&self, candidate: &str) {
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(candidate.to_owned(), Instant::now());
    }

    pub(crate) fn clear_vote(&self, candidate: &str) {
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(candidate);
    }

    /// Os candidatos de votações em andamento. A que passa de `write.pending.ttl` é esquecida e a
    /// de um nó que já saiu da tabela também.
    pub fn open_votes(&self) -> Vec<String> {
        let mut votes = self.open_votes.lock().unwrap_or_else(|e| e.into_inner());
        votes.retain(|candidate, at| {
            at.elapsed() < self.rules.pending_ttl && self.membership.is_member(candidate)
        });
        votes.keys().cloned().collect()
    }

    /// Chamado pelo canal quando um par cai ou avisa que falhou: abre a votação de exclusão.
    pub(crate) fn on_node_failed_hook(&self, hook: Box<dyn Fn() + Send + Sync>) {
        self.node.on_failed(hook);
    }

    /// O estado de drain do nó.
    pub fn drain_state(&self) -> super::state::DrainState {
        self.node.drain_state()
    }

    /// O estado do nó no cluster (`joining`, `syncing`, `active`...).
    pub fn node_state(&self) -> NodeState {
        self.node.state()
    }

    pub fn membership(&self) -> &Membership {
        &self.membership
    }

    pub fn rules(&self) -> &ClusterRules {
        &self.rules
    }

    pub(crate) fn now(&self) -> SystemTime {
        self.clock.now()
    }

    pub(crate) fn new_operation_id(&self) -> String {
        id::new_at(self.clock.now_millis())
    }

    /// Conta uma fase de uma mudança de membros: `register` para a entrada de nó, `unregister`
    /// (com `reason`: `vote` ou `drain`) para a saída.
    fn count_change(&self, change: Option<&Change>, phase: &str, outcome: &str) {
        let (operation, reason) = match change {
            Some(Change::Remove { .. }) => ("unregister", Some("drain")),
            Some(Change::Exclude { .. }) => ("unregister", Some("vote")),
            _ => ("register", None),
        };
        let mut labels = vec![
            ("taba.operation", operation),
            ("taba.phase", phase),
            ("taba.outcome", outcome),
        ];
        if let Some(reason) = reason {
            labels.push(("taba.reason", reason));
        }
        self.telemetry
            .registry
            .add(&CLUSTER_MEMBERSHIP_CHANGES, &labels, 1.0);
    }

    fn count_bootstrap_failure(&self, reason: &str) {
        self.telemetry
            .registry
            .add(&CLUSTER_BOOTSTRAP_FAILURES, &[("taba.reason", reason)], 1.0);
    }

    // ---- Quem atende ----

    /// Atende uma chamada de outro nó. `caller` é o IP de origem.
    pub async fn handle(
        self: &Arc<Self>,
        caller: &str,
        request: Request,
    ) -> Result<Response, TabaError> {
        match request {
            Request::ListNodes => self
                .membership
                .table_with_frozen(self.now(), self.rules.freezing_duration)
                .map(Response::Members)
                .ok_or_else(node_syncing),
            Request::Join(request) => self.join(request).await,
            Request::Register(request) => {
                request.validate()?;
                self.require_member(caller)?;
                self.refuse_while_busy(None)?;
                let id = request.operation_id.clone();
                let change = Change::Add(request.node);
                let result = self.membership.register(&id, change.clone(), self.now());
                self.count_change(
                    Some(&change),
                    "register",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                result.map_err(from_membership)?;
                Ok(ack(id))
            }
            Request::Unregister(request) => {
                request.validate()?;
                self.require_member(caller)?;
                // A saída do próprio nó que sincroniza (ele falhou) é a exceção (doc 04, 6.8).
                self.refuse_while_busy_unregistering(Some(&request.node_id))?;
                let id = request.operation_id.clone();
                let change = match request.reason {
                    UnregisterReason::Vote => Change::Exclude {
                        node_id: request.node_id,
                    },
                    UnregisterReason::Drain => Change::Remove {
                        node_id: request.node_id,
                    },
                };
                let result = self.membership.register(&id, change.clone(), self.now());
                self.count_change(
                    Some(&change),
                    "register",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                result.map_err(from_membership)?;
                Ok(ack(id))
            }
            Request::Commit(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let change = self.membership.pending_change(&request.operation_id);
                let result = self.membership.commit(&request.operation_id, self.now());
                self.count_change(
                    change.as_ref(),
                    "commit",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                let table = result.map_err(from_membership)?;
                self.membership_changed();
                self.announce_frozen(change.as_ref());
                if let Some(change) = change.as_ref() {
                    self.announce_membership(&request.operation_id, table.version, change);
                }
                self.settle_membership();
                Ok(ack(request.operation_id))
            }
            Request::Rollback(request) => {
                request.validate()?;
                self.require_member(caller)?;
                let change = self.membership.pending_change(&request.operation_id);
                let result = self.membership.rollback(&request.operation_id, self.now());
                self.count_change(
                    change.as_ref(),
                    "rollback",
                    if result.is_ok() { "ok" } else { "failed" },
                );
                result.map_err(from_membership)?;
                Ok(ack(request.operation_id))
            }
            Request::Status => {
                self.require_member(caller)?;
                let source = self
                    .status_source
                    .get()
                    .ok_or_else(TabaError::route_not_found)?;
                Ok(Response::Status(source()))
            }
            // O repasse de uma chamada de cliente também vale para quem saiu por drain (doc 04,
            // seção 6.9); quem o recebe nunca repassa de novo.
            Request::Forward(forward) => {
                let allowed = self.membership.is_member_address(caller)
                    || self.membership.is_departed_address(caller);
                if !allowed {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNotMember,
                        "Quem chama não consta na tabela de membros.",
                    ));
                }
                if self.node.drain_state() != super::state::DrainState::Active {
                    return Err(TabaError::new(
                        ErrorCode::ClusterNodeDraining,
                        "O nó está em drain e não atende o repasse.",
                    )
                    .with_retry_after(SYNCING_RETRY_AFTER_SECS));
                }
                if self.node.state() != NodeState::Active {
                    return Err(node_syncing());
                }
                let Some(replication) = self.replication.get().cloned() else {
                    return Err(TabaError::route_not_found());
                };
                replication.forwarder.execute(forward).await
            }
            // Definições, mensagens, sinais: a replicação atende.
            other => {
                let Some(replication) = self.replication.get().cloned() else {
                    return Err(TabaError::route_not_found());
                };
                // A lista de perdas e as mensagens recuperadas aceitam quem acabou de ser excluído
                // (doc 04, seção 6.6): o nó que volta ainda não consta na tabela.
                if !matches!(other, Request::MessageLoss(_) | Request::Recover { .. }) {
                    self.require_member(caller)?;
                }
                replication.handle(caller, other).await
            }
        }
    }

    /// Uma mudança de membros por vez, até o nó que entrou ficar pronto (doc 04, seção 6.8): com
    /// um nó em sincronização, o registro de outra mudança é um conflito, e quem coordena espera.
    /// `except` é o nó cuja saída é permitida mesmo assim.
    fn refuse_while_busy_unregistering(&self, except: Option<&str>) -> Result<(), TabaError> {
        self.refuse_while_syncing(except)
    }

    /// O registro de uma entrada também espera as votações de exclusão em andamento.
    fn refuse_while_busy(&self, except: Option<&str>) -> Result<(), TabaError> {
        self.refuse_while_syncing(except)?;
        if self.open_votes().is_empty() {
            Ok(())
        } else {
            Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "Há uma votação de exclusão em andamento: nenhuma entrada de nó é efetivada até ela acabar.",
            ))
        }
    }

    fn refuse_while_syncing(&self, except: Option<&str>) -> Result<(), TabaError> {
        let busy = self
            .membership
            .syncing()
            .into_iter()
            .any(|node| except != Some(node.as_str()));
        if busy {
            Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "Há um nó em sincronização: nenhuma outra mudança de membros é efetivada até ele ficar pronto.",
            ))
        } else {
            Ok(())
        }
    }

    /// O coordenador de um ingresso espera o nó que sincroniza ficar pronto (ou sair).
    async fn wait_for_sync_turn(&self, joining: &str) -> Result<(), TabaError> {
        let deadline = tokio::time::Instant::now() + self.rules.join_timeout;
        while self.membership.syncing().iter().any(|n| n != joining)
            || !self.open_votes().is_empty()
        {
            if tokio::time::Instant::now() >= deadline || self.cancel.is_cancelled() {
                return Err(TabaError::new(
                    ErrorCode::ClusterRegistrationFailed,
                    "Há um nó em sincronização e o pedido de ingresso esperou o prazo.",
                )
                .with_retry_after(SYNCING_RETRY_AFTER_SECS));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok(())
    }

    /// Só quem consta na tabela de membros chama as rotas de fase (doc 06c): o nó que ainda
    /// não tem a tabela não sabe quem é membro e responde `503`.
    pub(crate) fn require_member(&self, caller: &str) -> Result<(), TabaError> {
        if self.membership.table().is_none() {
            return Err(node_syncing());
        }
        if self.membership.is_member_address(caller) {
            Ok(())
        } else {
            Err(TabaError::new(
                ErrorCode::ClusterNotMember,
                "Quem chama não consta na tabela de membros.",
            ))
        }
    }

    // ---- Coordenar um ingresso ----

    /// `POST /cluster/nodes/join`: este nó coordena o registro do nó novo (doc 04, seção 6.2,
    /// passos 2 a 5) e só responde com a tabela depois do commit.
    async fn join(self: &Arc<Self>, request: JoinRequest) -> Result<Response, TabaError> {
        // Dois ingressos coordenados por membros diferentes se cruzam: o mais antigo vence e o
        // outro, que desfez o registro, espera um pouco e recomeça (D94).
        let give_up_at = tokio::time::Instant::now() + self.rules.join_timeout;
        loop {
            match self.join_once(request.clone()).await {
                Err(error) if error.code == ErrorCode::ClusterConflict => {
                    if tokio::time::Instant::now() >= give_up_at || self.cancel.is_cancelled() {
                        return Err(registration_failed());
                    }
                    tokio::time::sleep(conflict_wait()).await;
                }
                other => return other,
            }
        }
    }

    /// Uma tentativa do ingresso. `cluster.conflict` é a mudança de outro coordenador que ganhou.
    async fn join_once(self: &Arc<Self>, request: JoinRequest) -> Result<Response, TabaError> {
        let span = trace::operation_span("taba.cluster.join", &request.node_id);
        self.join_once_traced(request).instrument(span).await
    }

    async fn join_once_traced(
        self: &Arc<Self>,
        request: JoinRequest,
    ) -> Result<Response, TabaError> {
        request.validate()?;
        if self.membership.table().is_none() {
            return Err(node_syncing());
        }
        // Uma mudança de membros por vez: outro ingresso em curso neste nó faz este esperar.
        let _turn = self.change_turn.lock().await;
        let table = self.membership.table().ok_or_else(node_syncing)?;
        // Um nó excluído por votação não volta antes do fim do freezing (doc 04, seção 6.7).
        if let Some(left) = self.membership.frozen_remaining(
            &request.node_id,
            self.now(),
            self.rules.freezing_duration,
        ) {
            tracing::warn!(node_id = %request.node_id, left_secs = left.as_secs(), "ingresso rejeitado (freezing)");
            return Err(TabaError::new(
                ErrorCode::ClusterNodeFrozen,
                "O nó foi excluído do cluster e está em período de freezing.",
            )
            .with_retry_after(left.as_secs().max(1)));
        }
        if table.member(&request.node_id).is_some() {
            // O mesmo nó repetindo o pedido (o anterior foi aceito e a resposta se perdeu).
            return Ok(Response::Members(self.table_for_newcomer(table)));
        }
        self.wait_for_sync_turn(&request.node_id).await?;
        let table = self.membership.table().ok_or_else(node_syncing)?;

        self.probe(&request).await?;

        let node = Member {
            node_id: request.node_id.clone(),
            addresses: request.addresses.clone(),
            port: request.port,
            // O nó novo só fica ativo quando termina a sincronização (doc 05, seção 7.3).
            state: MemberState::Syncing,
        };
        let operation_id = self.new_operation_id();
        let peers: Vec<Member> = table
            .members
            .iter()
            .filter(|m| m.node_id != self.node_id())
            .cloned()
            .collect();

        // Fase 1: o registro em todos os nós, este inclusive.
        let change = Change::Add(node.clone());
        if let Err(e) =
            self.membership
                .register_coordinating(&operation_id, change.clone(), self.now())
        {
            self.count_change(Some(&change), "register", "failed");
            tracing::warn!(node_id = %node.node_id, error = ?e, "ingresso recusado neste nó");
            // Outra mudança já pendente aqui (a de um coordenador que chegou antes): o ingresso
            // espera a vez em vez de falhar (D94).
            let error = from_membership(e);
            return Err(if error.code == ErrorCode::ClusterConflict {
                error
            } else {
                registration_failed()
            });
        }
        let (confirmed, failures) = self
            .register_on_peers(
                &peers,
                Request::Register(NodeRegisterRequest {
                    operation_id: operation_id.clone(),
                    coordinator_id: self.node_id(),
                    node: node.clone(),
                }),
            )
            .await;
        if !failures.is_empty() {
            self.count_change(Some(&change), "register", "failed");
            for (peer, error) in &failures {
                tracing::warn!(node_id = %node.node_id, peer = %peer, %error, "registro sem confirmação");
            }
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            tracing::error!(
                node_id = %node.node_id,
                "ingresso abortado (rollback): a fase de registro não foi confirmada por todos"
            );
            let contended = failures.iter().any(
                |(_, e)| matches!(e, CallError::Remote(r) if r.code == ErrorCode::ClusterConflict),
            );
            return Err(if contended {
                TabaError::new(
                    ErrorCode::ClusterConflict,
                    "Há outra mudança de membros em andamento.",
                )
            } else {
                registration_failed()
            });
        }
        if !self.membership.is_pending(&operation_id) {
            // Um coordenador mais antigo deslocou este registro no meio da fase 1.
            self.count_change(Some(&change), "register", "failed");
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            return Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "Outra mudança de membros, mais antiga, tomou a vez.",
            ));
        }
        self.count_change(Some(&change), "register", "ok");

        // Fase 2: o quórum de todos foi atingido, a decisão está tomada e não volta atrás.
        let table = self
            .membership
            .commit(&operation_id, self.now())
            .map_err(from_membership)?;
        self.count_change(Some(&change), "commit", "ok");
        self.membership_changed();
        self.announce_membership(&operation_id, table.version, &change);
        self.settle_membership();
        if self.commit_on_peers(&peers, &operation_id).await {
            tracing::info!(
                node_id = %node.node_id,
                version = table.version,
                "nó novo registrado no cluster"
            );
            Ok(Response::Members(self.table_for_newcomer(table)))
        } else {
            self.count_change(Some(&change), "commit", "timeout");
            tracing::warn!(
                node_id = %node.node_id,
                "o commit continua em segundo plano: o nó foi aceito"
            );
            Err(TabaError::new(
                ErrorCode::ClusterCommitTimeout,
                "O registro foi confirmado e o commit segue em segundo plano: o nó foi aceito. \
                 Consulte GET /cluster/nodes antes de repetir.",
            ))
        }
    }

    /// A tabela que o nó novo recebe leva os nós em freezing (D106): quem entra depois do commit de
    /// uma exclusão herda o freezing de quem o recebeu.
    fn table_for_newcomer(&self, mut table: MemberTable) -> MemberTable {
        if let Some(with_frozen) = self
            .membership
            .table_with_frozen(self.now(), self.rules.freezing_duration)
        {
            table.frozen = with_frozen.frozen;
        }
        table
    }

    /// O teste de conexão (passo 2): conecta de volta ao nó novo. Uma resposta de erro do nó
    /// também prova que a conexão funciona (o nó novo ainda não tem a tabela e responde `503`).
    async fn probe(&self, request: &JoinRequest) -> Result<(), TabaError> {
        for address in &request.addresses {
            let to = PeerAddr::new(address.clone(), request.port);
            match self
                .call_addr(&to, Request::ListNodes, self.rules.call_timeout)
                .await
            {
                Ok(_) | Err(CallError::Remote(_)) => return Ok(()),
                Err(_) => {}
            }
        }
        tracing::warn!(node_id = %request.node_id, "o nó novo não é alcançável pelos IPs informados");
        Err(TabaError::new(
            ErrorCode::ClusterJoinUnreachable,
            "Não foi possível conectar de volta aos IPs informados.",
        ))
    }

    /// A fase 1 em todos os outros nós. Devolve quem confirmou e quem falhou.
    async fn register_on_peers(
        self: &Arc<Self>,
        peers: &[Member],
        request: Request,
    ) -> (Vec<Member>, Vec<(String, CallError)>) {
        let mut set = JoinSet::new();
        for peer in peers {
            let (me, peer, request) = (self.clone(), peer.clone(), request.clone());
            let span = trace::peer_span("taba.cluster.register", &peer.node_id, false);
            set.spawn(
                async move {
                    // Outra mudança pendente no par (409) pode ser passageira: tenta de novo.
                    let result = trace::traced(
                        tracing::Span::current(),
                        me.call_retrying(&peer, request, true),
                    )
                    .await;
                    (peer, result)
                }
                .instrument(span),
            );
        }
        let (mut confirmed, mut failures) = (Vec::new(), Vec::new());
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((peer, Ok(_))) => confirmed.push(peer),
                Ok((peer, Err(error))) => failures.push((peer.node_id, error)),
                Err(e) => tracing::error!(error = %e, "task do registro falhou"),
            }
        }
        (confirmed, failures)
    }

    /// Descarta o registro pendente aqui e nos que já o tinham confirmado.
    async fn rollback_everywhere(
        self: &Arc<Self>,
        confirmed: &[Member],
        operation_id: &str,
        change: &Change,
    ) {
        let _ = self.membership.rollback(operation_id, self.now());
        self.count_change(Some(change), "rollback", "ok");
        let request = Request::Rollback(OperationRef {
            operation_id: operation_id.to_owned(),
        });
        let mut set = JoinSet::new();
        for peer in confirmed {
            let (me, peer, request) = (self.clone(), peer.clone(), request.clone());
            let span = trace::peer_span("taba.cluster.rollback", &peer.node_id, true);
            set.spawn(
                async move {
                    if let Err(error) = me.call_retrying(&peer, request, false).await {
                        // O registro pendente expira sozinho em `write.pending.ttl`.
                        tracing::warn!(peer = %peer.node_id, %error, "rollback sem confirmação");
                    }
                }
                .instrument(span),
            );
        }
        while set.join_next().await.is_some() {}
    }

    /// A fase 2 em todos os outros nós. `true` se todos confirmaram dentro de
    /// `write.commit.timeout`; senão os que faltam seguem sendo tentados em segundo plano.
    async fn commit_on_peers(self: &Arc<Self>, peers: &[Member], operation_id: &str) -> bool {
        let mut set = JoinSet::new();
        for peer in peers {
            let (me, peer, operation_id) = (self.clone(), peer.clone(), operation_id.to_owned());
            let span = trace::peer_span("taba.cluster.commit", &peer.node_id, false);
            set.spawn(
                async move { me.commit_until_confirmed(peer, operation_id).await }.instrument(span),
            );
        }
        let all = tokio::time::timeout(self.rules.commit_timeout, async {
            while set.join_next().await.is_some() {}
        })
        .await
        .is_ok();
        if !all {
            set.detach_all();
        }
        all
    }

    /// Insiste no commit de um par até ele confirmar, sair da tabela ou o nó parar: depois da
    /// decisão não há rollback (doc 04, seção 6.2, passo 7).
    async fn commit_until_confirmed(self: Arc<Self>, peer: Member, operation_id: String) {
        let request = Request::Commit(OperationRef { operation_id });
        loop {
            match self
                .call_peer(&peer, request.clone(), self.rules.call_timeout)
                .await
            {
                Ok(_) => return,
                Err(CallError::Remote(error)) if error.status() < 500 => {
                    // O par não tem mais o registro (reiniciou ou expirou): não há o que insistir.
                    tracing::warn!(peer = %peer.node_id, code = %error.code, "commit recusado pelo par");
                    return;
                }
                Err(_) => {}
            }
            if !self.membership.is_member(&peer.node_id) {
                return;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    // ---- Chamadas a outros nós ----

    /// O canal que `caller` quer abrir é aceito? O canal diz com quem fala: outro processo no mesmo
    /// endereço não atende pelo antigo (D100); e só membro abre canal.
    pub fn admit_channel(&self, caller: &str, open: &ChannelOpen) -> Result<(), TabaError> {
        if !open.to.is_empty() && open.to != self.node_id() {
            return Err(TabaError::new(
                ErrorCode::ClusterWrongNode,
                "O nó neste endereço não é o que o canal procura.",
            ));
        }
        self.require_member(caller)
    }

    /// O canal foi aberto por outro nó: ele passa a ser o canal do par.
    pub fn accept_channel(&self, open: ChannelOpen, link: ChannelLink) {
        if let Some(replication) = self.replication.get() {
            replication.accept_channel(open, link);
        }
    }

    /// Abre o canal com um membro, pelos IPs dele na ordem, até um atender (mesmo que com erro).
    pub(crate) async fn open_channel_to(
        &self,
        peer: &Member,
        open: ChannelOpen,
        timeout: Duration,
    ) -> Result<ChannelLink, CallError> {
        let mut last = CallError::Unreachable("o nó não tem endereço".into());
        for address in &peer.addresses {
            let to = PeerAddr::new(address.clone(), peer.port);
            match tokio::time::timeout(timeout, self.transport.open_channel(&to, open.clone()))
                .await
            {
                Ok(Err(e)) if e.is_network() => last = e,
                Ok(other) => return other,
                Err(_) => last = CallError::Timeout,
            }
        }
        Err(last)
    }

    /// Chama um endereço, com prazo.
    async fn call_addr(
        &self,
        to: &PeerAddr,
        request: Request,
        timeout: Duration,
    ) -> Result<Response, CallError> {
        match tokio::time::timeout(timeout, self.transport.call(to, request)).await {
            Ok(result) => result,
            Err(_) => Err(CallError::Timeout),
        }
    }

    /// Chama um membro pelos IPs dele, na ordem, até um responder (mesmo que com erro).
    pub(crate) async fn call_peer(
        &self,
        peer: &Member,
        request: Request,
        timeout: Duration,
    ) -> Result<Response, CallError> {
        let mut last = CallError::Unreachable("o nó não tem endereço".into());
        for address in &peer.addresses {
            let to = PeerAddr::new(address.clone(), peer.port);
            match self.call_addr(&to, request.clone(), timeout).await {
                Err(e) if e.is_network() => last = e,
                other => return other,
            }
        }
        Err(last)
    }

    /// `call_peer` com `write.retry.attempts` tentativas e `write.retry.backoff` entre elas, para
    /// falha de rede e erro `5xx` do par. O conflito `409` (se pedido) é repetido por até
    /// `write.commit.timeout`, enquanto a mudança continua pendente aqui: a mais antiga das que se
    /// cruzam vence, e quem foi deslocada desiste na hora (D94).
    async fn call_retrying(
        &self,
        peer: &Member,
        request: Request,
        retry_conflict: bool,
    ) -> Result<Response, CallError> {
        let started = tokio::time::Instant::now();
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self
                .call_peer(peer, request.clone(), self.rules.call_timeout)
                .await;
            let (retry, conflict) = match &result {
                Err(e) if e.is_network() => (true, false),
                Err(CallError::Remote(e)) => {
                    let conflict = retry_conflict && e.code == ErrorCode::ClusterConflict;
                    (e.status() >= 500 || conflict, conflict)
                }
                _ => (false, false),
            };
            let exhausted = if conflict {
                started.elapsed() >= self.rules.commit_timeout
                    || request_operation(&request)
                        .is_some_and(|operation| !self.membership.is_pending(operation))
            } else {
                attempt >= self.rules.retry_attempts
            };
            if !retry || exhausted {
                return result;
            }
            tokio::select! {
                () = self.cancel.cancelled() => return result,
                () = tokio::time::sleep(self.rules.retry_backoff) => {}
            }
        }
    }

    // ---- Entrar num cluster ----

    /// O desregistro voluntário do drain (doc 04, seção 6.10, passo 5): este nó coordena a própria
    /// saída pelo protocolo de duas fases, **sem votação**, e espera a vez de outras mudanças de
    /// membros. Antes do commit, rollback; depois, o commit é insistido até `write.commit.timeout`.
    pub async fn leave(self: &Arc<Self>) -> Result<(), TabaError> {
        let me = self.node_id();
        self.unregister_node(&me, UnregisterReason::Drain).await
    }

    /// O desregistro de `target` pelo protocolo de duas fases (doc 04, seção 6.3): o próprio nó que
    /// sai (`drain`) ou o nó excluído por votação (`vote`, que ganha freezing). Exige a confirmação
    /// de todos os nós que sobram, menos os que este nó enxerga fora do ar: eles são excluídos por
    /// votação própria, e esperá-los travaria a exclusão de quem já caiu.
    pub async fn unregister_node(
        self: &Arc<Self>,
        target: &str,
        reason: UnregisterReason,
    ) -> Result<(), TabaError> {
        let span = trace::operation_span("taba.cluster.unregister", target);
        self.unregister_node_traced(target, reason)
            .instrument(span)
            .await
    }

    async fn unregister_node_traced(
        self: &Arc<Self>,
        target: &str,
        reason: UnregisterReason,
    ) -> Result<(), TabaError> {
        let _turn = self.change_turn.lock().await;
        let table = self.membership.table().ok_or_else(node_syncing)?;
        let me = self.node_id();
        if table.member(target).is_none() {
            return Ok(());
        }
        let down: HashSet<String> = match (reason, self.replication.get()) {
            (UnregisterReason::Vote, Some(replication)) => replication
                .channels
                .peers()
                .into_iter()
                .filter(|p| !p.up)
                .map(|p| p.node_id)
                .collect(),
            _ => HashSet::new(),
        };
        let peers: Vec<Member> = table
            .members
            .iter()
            .filter(|m| m.node_id != me && m.node_id != target && !down.contains(&m.node_id))
            .cloned()
            .collect();
        let operation_id = self.new_operation_id();
        let change = match reason {
            UnregisterReason::Drain => Change::Remove {
                node_id: target.to_owned(),
            },
            UnregisterReason::Vote => Change::Exclude {
                node_id: target.to_owned(),
            },
        };
        if let Err(e) =
            self.membership
                .register_coordinating(&operation_id, change.clone(), self.now())
        {
            self.count_change(Some(&change), "register", "failed");
            return Err(from_membership(e));
        }
        let (confirmed, failures) = self
            .register_on_peers(
                &peers,
                Request::Unregister(NodeUnregisterRequest {
                    operation_id: operation_id.clone(),
                    coordinator_id: me.clone(),
                    node_id: target.to_owned(),
                    reason,
                }),
            )
            .await;
        if !failures.is_empty() {
            self.count_change(Some(&change), "register", "failed");
            for (peer, error) in &failures {
                tracing::warn!(peer = %peer, %error, "desregistro sem confirmação");
            }
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            tracing::error!("desregistro abortado (rollback): a fase 1 não fechou em todos");
            // Outra mudança pendente num par: quem pede a saída espera a vez e tenta de novo.
            let contended = failures.iter().any(
                |(_, e)| matches!(e, CallError::Remote(r) if r.code == ErrorCode::ClusterConflict),
            );
            return Err(if contended {
                TabaError::new(
                    ErrorCode::ClusterConflict,
                    "Há outra mudança de membros em andamento.",
                )
            } else {
                registration_failed()
            });
        }
        if !self.membership.is_pending(&operation_id) {
            self.count_change(Some(&change), "register", "failed");
            self.rollback_everywhere(&confirmed, &operation_id, &change)
                .await;
            return Err(TabaError::new(
                ErrorCode::ClusterConflict,
                "Outra mudança de membros, mais antiga, tomou a vez.",
            ));
        }
        self.count_change(Some(&change), "register", "ok");
        let table = self
            .membership
            .commit(&operation_id, self.now())
            .map_err(from_membership)?;
        self.count_change(Some(&change), "commit", "ok");
        self.membership_changed();
        self.announce_frozen(Some(&change));
        self.announce_membership(&operation_id, table.version, &change);
        self.settle_membership();
        if self.commit_on_peers(&peers, &operation_id).await {
            tracing::info!(node_id = %target, ?reason, "nó fora do cluster");
            Ok(())
        } else {
            self.count_change(Some(&change), "commit", "timeout");
            tracing::warn!("o commit do desregistro segue em segundo plano em algum nó");
            Err(TabaError::new(
                ErrorCode::ClusterCommitTimeout,
                "O desregistro foi decidido e o commit segue em segundo plano.",
            ))
        }
    }

    /// O reencontro depois de uma partição em que os dois lados se excluíram (D101): de tempos em
    /// tempos o nó consulta a tabela de quem excluiu. Se ela é de outro cluster (nenhum membro em
    /// comum, e sem este nó) e este lado é o perdedor (menos membros; empate: o lado do menor
    /// `nodeId`), o nó refaz o ingresso do zero, como o excluído que volta.
    pub async fn run_reunion(self: Arc<Self>) {
        let period = self.rules.reunion_interval;
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => return,
                () = tokio::time::sleep(period) => {}
            }
            self.check_reunion().await;
        }
    }

    /// Uma volta do reencontro.
    pub async fn check_reunion(self: &Arc<Self>) {
        if self.node.state() != NodeState::Active
            || self.node.drain_state() != super::state::DrainState::Active
        {
            return;
        }
        for other in self.membership.excluded_members() {
            let Some(mine) = self.membership.table() else {
                return;
            };
            let Ok(Response::Members(theirs)) = self
                .call_peer(&other, Request::ListNodes, self.rules.call_timeout)
                .await
            else {
                continue;
            };
            let separate = !theirs.members.is_empty()
                && theirs.member(&self.node_id()).is_none()
                && theirs
                    .members
                    .iter()
                    .all(|m| mine.member(&m.node_id).is_none());
            if separate && loses_reunion(&mine, &theirs) {
                tracing::warn!(
                    node_id = %self.node_id(),
                    other = %other.node_id,
                    "as duas metades do cluster se excluíram e se reencontraram: este lado refaz o ingresso"
                );
                self.handle_exclusion().await;
                return;
            }
        }
    }

    /// Um par disse várias vezes que este nó não é membro. Pode ser que o commit do ingresso ainda
    /// não tenha chegado a ele (a tabela dele é mais velha) ou que este nó tenha sido excluído (a
    /// tabela dele é mais nova e não o tem): só neste caso o nó refaz o ingresso.
    pub async fn check_exclusion(self: &Arc<Self>, peer_id: &str) {
        let Some(member) = self
            .membership
            .table()
            .and_then(|t| t.member(peer_id).cloned())
        else {
            return;
        };
        let (Some(mine), Ok(Response::Members(theirs))) = (
            self.membership.version(),
            self.call_peer(&member, Request::ListNodes, self.rules.call_timeout)
                .await,
        ) else {
            return;
        };
        if theirs.member(&self.node_id()).is_none() && theirs.version > mine {
            self.handle_exclusion().await;
        }
    }

    /// O nó descobriu que foi excluído (os pares não o têm mais na tabela): apaga o estado local
    /// e refaz o ingresso do zero (doc 04, seção 6.6, passo 6), com o mesmo ID. O freezing atrasa o
    /// novo registro: o pedido é rejeitado até o período acabar e o laço do bootstrap repete.
    pub async fn handle_exclusion(self: &Arc<Self>) {
        if self.node.state() != NodeState::Active
            || self.node.drain_state() != super::state::DrainState::Active
            || self.excluded.swap(true, Ordering::SeqCst)
        {
            return;
        }
        tracing::warn!(node_id = %self.node_id(), "este nó foi excluído do cluster: recuperando as perdas e refazendo o ingresso do zero");
        self.recover_losses().await;
        self.boot_recovered.store(true, Ordering::SeqCst);
        if let Some(replication) = self.replication.get().cloned() {
            replication.reset_local().await;
        }
        self.membership.clear();
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.node.set_state(NodeState::Joining);
        if let Some(replication) = self.replication.get().cloned() {
            replication.channels.sync_members();
        }
        let me = self.clone();
        tokio::spawn(async move {
            let result = me.bootstrap().await;
            me.excluded.store(false, Ordering::SeqCst);
            if let Err(e) = result {
                tracing::error!(error = %e, "o nó não conseguiu voltar ao cluster");
            }
        });
    }

    /// Antes de apagar o estado local, o nó que volta (excluído ou saído por drain) recupera o que
    /// só ele tem das mensagens declaradas perdidas (doc 04, seção 6.6, passos 1 a 5). Fica em
    /// `returning` enquanto isso. Os pares são os da última tabela que o nó conheceu.
    async fn recover_losses(self: &Arc<Self>) {
        let Some(replication) = self.replication.get().cloned() else {
            return;
        };
        let me = self.node_id();
        let peers: Vec<Member> = self
            .membership
            .table()
            .map(|table| {
                table
                    .members
                    .into_iter()
                    .filter(|m| m.node_id != me)
                    .collect()
            })
            .unwrap_or_default();
        self.node.set_state(NodeState::Returning);
        replication.recovery.run(peers).await;
    }

    /// O reinício do processo: põe de lado o disco da vida anterior e apaga o resto, para o nó entrar
    /// vazio (D111; PRB-048).
    async fn recover_on_boot(self: &Arc<Self>, table: &MemberTable) {
        let Some(replication) = self.replication.get().cloned() else {
            return;
        };
        // O disco da vida anterior é posto de lado e o nó entra vazio: a recuperação não atrasa o
        // ingresso (PRB-048) e roda depois de ativo, em `recover_previous_life`.
        let messages = replication.messages.messages().clone();
        let aside = tokio::task::spawn_blocking(move || messages.set_aside_old_disk()).await;
        match aside {
            Ok(Ok(true)) => {
                *self
                    .old_disk_peers
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = table.members.clone();
            }
            Ok(Ok(false)) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "não foi possível pôr de lado o disco da vida anterior");
            }
            Err(_) => {}
        }
        self.release_old_disk().await;
    }

    /// Depois de ativo, o reinício devolve ao responsável o que o disco da vida anterior tem das
    /// perdas dos membros e apaga esse disco (doc 04, seção 6.6; PRB-048). Roda em segundo plano:
    /// um membro mudo só atrasa esta etapa, até `cluster.recovery.timeout`, nunca o ingresso.
    fn recover_previous_life(self: &Arc<Self>) {
        let peers = std::mem::take(
            &mut *self
                .old_disk_peers
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        );
        let Some(replication) = self.replication.get().cloned() else {
            return;
        };
        let messages = replication.messages.messages().clone();
        if peers.is_empty() {
            tokio::task::spawn_blocking(move || messages.finish_previous_life());
            return;
        }
        tokio::spawn(async move {
            replication.recovery.run_streaming(peers).await;
            let _ = tokio::task::spawn_blocking(move || messages.finish_previous_life()).await;
        });
    }

    /// Apaga o que ficou no disco da vida anterior e libera a varredura (D111).
    async fn release_old_disk(&self) {
        if let Some(replication) = self.replication.get().cloned() {
            replication.reset_local().await;
            replication.messages.messages().release_old_disk();
        }
    }

    /// Volta do drain **como nó novo** (doc 04, seção 6.10): ID novo, estado local limpo e o
    /// bootstrap completo, com nova posição no anel. Não há freezing: o ID é outro e a saída foi
    /// voluntária.
    pub async fn rejoin_as_new(self: &Arc<Self>) {
        self.recover_losses().await;
        self.boot_recovered.store(true, Ordering::SeqCst);
        let new_id = id::new_at(self.clock.now_millis());
        if let Some(replication) = self.replication.get().cloned() {
            replication.reset_local().await;
        }
        self.local
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .node_id = new_id.clone();
        self.node.set_node_id(new_id.clone());
        self.membership.clear();
        self.open_votes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.node.set_state(NodeState::Joining);
        tracing::info!(node_id = %new_id, "nó voltou do drain como nó novo; refazendo o bootstrap");
        let me = self.clone();
        tokio::spawn(async move {
            if let Err(e) = me.bootstrap().await {
                tracing::error!(error = %e, "o nó não conseguiu voltar ao cluster");
            }
        });
    }

    /// O primeiro nó de um cluster novo: a tabela é só ele.
    pub fn start_alone(&self) {
        self.membership.install(MemberTable {
            version: 1,
            members: vec![self.local()],
            ..MemberTable::default()
        });
        // O primeiro nó não tem de quem recuperar: o disco antigo segue o caminho de sempre.
        if let Some(replication) = self.replication.get() {
            replication.messages.messages().release_old_disk();
        }
        self.node.set_state(NodeState::Active);
        self.membership_changed();
        tracing::info!(node_id = %self.node_id(), "primeiro nó: o cluster começa só com ele");
    }

    /// A tabela chegou: o nó instala, passa a gravar os eventos no banco de eventos e sincroniza
    /// (definições, blocos, eventos, verificação final; doc 05, seção 7.3). Só ao fim fica ativo.
    /// Se a sincronização falha de um jeito que não se repara (disco, banco de eventos cheio, nenhuma
    /// origem), o nó vai a `failed`.
    async fn finish_join(
        self: &Arc<Self>,
        table: MemberTable,
        via: &Member,
    ) -> Result<(), Failure> {
        // O membro que coordenou o ingresso já tem o nó novo na tabela; os outros podem ainda não
        // ter (o commit chega a eles em seguida), então ele vem primeiro.
        let mut sources = vec![via.clone()];
        sources.extend(
            table
                .members
                .iter()
                .filter(|m| {
                    m.node_id != self.node_id()
                        && m.node_id != via.node_id
                        && m.state == MemberState::Active
                })
                .cloned(),
        );
        let Some(replication) = self.replication.get().cloned() else {
            self.membership.install(table.clone());
            self.activate(&table).await;
            return Ok(());
        };
        let syncer = replication.syncer.clone();
        syncer.begin().map_err(|e| Failure {
            reason: "sync",
            message: format!("não foi possível abrir o banco de eventos: {e}"),
        })?;
        self.membership.install(table.clone());
        self.node.set_state(NodeState::Syncing);
        replication.channels.sync_members();
        match syncer.run(&sources).await {
            Ok(()) => {
                self.activate(&table).await;
                self.recover_previous_life();
                Ok(())
            }
            Err(SyncError::Cancelled) => {
                syncer.abandon();
                Err(Failure {
                    reason: "sync",
                    message: "o nó parou durante a sincronização".into(),
                })
            }
            Err(e) => {
                tracing::error!(error = %e, "a sincronização falhou: o nó passa a `failed`");
                syncer.abandon();
                self.node.set_failed();
                Ok(())
            }
        }
    }

    /// O nó está íntegro: o estado dele na tabela vira `active`, os pares ficam sabendo (com
    /// tentativas) e só então ele passa a atender e a ser dono das mensagens dos blocos dele.
    async fn activate(&self, table: &MemberTable) {
        let me = self.node_id();
        self.membership.set_state(&me, MemberState::Active);
        self.local.write().unwrap_or_else(|e| e.into_inner()).state = MemberState::Active;
        if let Some(replication) = self.replication.get().cloned() {
            replication.signals.announce_state_reliably("active").await;
        }
        let version = table.version;
        let members = table.members.len();
        self.node.set_state(NodeState::Active);
        self.membership_changed();
        tracing::info!(node_id = %me, version, members, "nó no cluster");
    }

    /// O bootstrap (doc 04, seção 6.2): lista os membros pelo balanceador, pede o ingresso a
    /// um deles e instala a tabela que volta. Repete a cada `cluster.bootstrap.retry.interval`
    /// até `cluster.bootstrap.retry.max-attempts` (`0` não limita).
    pub async fn bootstrap(self: &Arc<Self>) -> Result<(), BootstrapError> {
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let failure = match self.attempt_join().await {
                Ok(()) => return Ok(()),
                Err(failure) => failure,
            };
            self.count_bootstrap_failure(failure.reason);
            tracing::error!(
                attempt = attempts,
                reason = failure.reason,
                "falha no bootstrap: {}",
                failure.message
            );
            let max = self.rules.bootstrap_max_attempts;
            if max != 0 && attempts >= max {
                return Err(BootstrapError::Exhausted {
                    attempts,
                    last: failure.message,
                });
            }
            tokio::select! {
                () = self.cancel.cancelled() => return Err(BootstrapError::Cancelled),
                () = tokio::time::sleep(self.rules.bootstrap_retry_interval) => {}
            }
        }
    }

    async fn attempt_join(self: &Arc<Self>) -> Result<(), Failure> {
        let entry = self.rules.entry.clone().ok_or_else(|| Failure {
            reason: "config",
            message: "cluster.entry-address não é um host:porta".into(),
        })?;

        // 1. A listagem de membros, pelo balanceador.
        let listed = self
            .call_addr(&entry, Request::ListNodes, self.rules.call_timeout)
            .await;
        let table = match listed {
            Ok(Response::Members(table)) => table,
            Ok(_) => {
                return Err(Failure {
                    reason: "list",
                    message: "resposta inesperada à listagem de nós".into(),
                });
            }
            Err(e) if e.is_network() && self.rules.first_node => {
                // Só o primeiro nó de um cluster novo começa sozinho, e só quando a listagem
                // não encontra ninguém.
                self.start_alone();
                return Ok(());
            }
            Err(e) => {
                return Err(Failure {
                    reason: "list",
                    message: format!("a listagem de nós em {entry} falhou: {e}"),
                });
            }
        };

        // O processo que reinicia nasce como nó novo, mas o que ficou no disco da vida anterior
        // ainda pode ser uma mensagem que os outros nós declararam perdida (doc 04, seção 6.6): antes
        // de entrar, ele a procura e a devolve ao responsável (D111).
        if !self.boot_recovered.swap(true, Ordering::SeqCst) {
            self.recover_on_boot(&table).await;
        }

        // 2. Um membro ao acaso; se não responde, outro.
        let mut candidates = table.members;
        let mut shuffled = Vec::with_capacity(candidates.len());
        while !candidates.is_empty() {
            shuffled.push(candidates.swap_remove(random_below(candidates.len())));
        }
        let request = Request::Join(JoinRequest {
            node_id: self.local().node_id,
            addresses: self.local().addresses,
            port: self.local().port,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        });
        let mut last = Failure {
            reason: "join",
            message: "nenhum membro respondeu ao pedido de ingresso".into(),
        };
        for member in shuffled {
            match self
                .call_peer(&member, request.clone(), self.rules.join_timeout)
                .await
            {
                Ok(Response::Members(table)) => {
                    self.finish_join(table, &member).await?;
                    return Ok(());
                }
                Ok(_) => {
                    last = Failure {
                        reason: "join",
                        message: "resposta inesperada ao pedido de ingresso".into(),
                    };
                }
                Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterCommitTimeout => {
                    // O nó foi aceito: antes de repetir, a tabela diz se ele já consta.
                    if let Ok(Response::Members(table)) = self
                        .call_peer(&member, Request::ListNodes, self.rules.call_timeout)
                        .await
                        && table.member(&self.node_id()).is_some()
                    {
                        self.finish_join(table, &member).await?;
                        return Ok(());
                    }
                    return Err(Failure {
                        reason: "phase2",
                        message: error.message,
                    });
                }
                Err(CallError::Remote(error)) => {
                    let reason = match error.code {
                        ErrorCode::ClusterJoinUnreachable => "connectivity",
                        ErrorCode::ClusterRegistrationFailed => "phase1",
                        ErrorCode::ClusterNodeFrozen => "frozen",
                        _ => "join",
                    };
                    return Err(Failure {
                        reason,
                        message: format!("{}: {}", error.code, error.message),
                    });
                }
                Err(e) => {
                    last = Failure {
                        reason: "join",
                        message: format!("o membro {} não respondeu: {e}", member.node_id),
                    };
                }
            }
        }
        Err(last)
    }
}

fn ack(operation_id: String) -> Response {
    Response::Ack(Ack {
        operation_id,
        accepted: true,
    })
}

fn registration_failed() -> TabaError {
    TabaError::new(
        ErrorCode::ClusterRegistrationFailed,
        "A fase de registro não foi confirmada por todos os nós; o registro foi cancelado.",
    )
    .with_retry_after(SYNCING_RETRY_AFTER_SECS)
}

#[cfg(test)]
mod tests;
