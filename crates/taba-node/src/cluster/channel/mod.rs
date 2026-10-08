//! O canal permanente entre cada par de nós (doc 04, seções 6.1 e 6.3; doc 13, seções 12.8 e 12.9):
//! o **sinal de vida** (`keepalive`), os sinais de disponibilidade da entrega com `seq`,
//! confirmação e fila de saída, e a detecção de falha que abre a votação de exclusão.
//!
//! Sem o WebSocket (etapa 16), cada lado **chama** o outro pelo `Transport`: uma chamada leva o
//! sinal de vida (a cada `cluster.channel.keepalive.interval`) e os quadros ainda sem confirmação,
//! e a resposta é o `confirm`. O que o doc chama de canal caído é a chamada que falha: a primeira
//! falha marca o canal como caído e as seguintes (a cada `cluster.channel.reconnect.backoff`)
//! contam como tentativas de reconexão; esgotadas `cluster.channel.reconnect.retries`, o nó abre a
//! votação de exclusão do par. A fila de saída estourar (tamanho ou idade) abre a votação na hora.
//!
//! Uma tarefa por par cuida do envio; a recepção ([`Channels::receive`]) confere o `seq` (basta
//! guardar o maior já processado) e devolve o `confirm` depois de processar.

mod queue;

pub use queue::{Dropped, NOTIFY_TTL, OutQueue};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use super::protocol::{ChannelBatch, Confirm, Request, Response, Signal, SignalFrame, VoteReason};
use super::replication::Signals;
use super::service::ClusterService;
use super::state::NodeState;
use super::transport::CallError;
use crate::core::error::ErrorCode;
use crate::telemetry::metrics::{
    CHANNEL_CONNECTIONS, CHANNEL_QUEUE_DROPPED, CHANNEL_QUEUE_OUT, CHANNEL_RECONNECTS, CHANNEL_RTT,
    CHANNEL_STATE,
};

/// Quantos quadros uma chamada leva no máximo.
const MAX_FRAMES: usize = 256;
/// Quantas respostas `not-member` seguidas fazem o nó concluir que foi excluído.
const EXCLUDED_AFTER: u32 = 3;

#[derive(Debug)]
struct PeerState {
    queue: OutQueue,
    /// O canal está conectado (a última chamada deu certo).
    up: bool,
    /// Chamadas seguidas que falharam.
    failures: u32,
    /// A votação desta queda já foi aberta.
    vote_opened: bool,
    /// Respostas `not-member` seguidas.
    not_member: u32,
    /// O instantâneo dos sinais precisa ser enfileirado (canal novo ou que voltou).
    snapshot_pending: bool,
    last_keepalive_sent: Option<Instant>,
    last_attempt: Option<Instant>,
    last_heard: Option<Instant>,
    /// O maior `seq` já processado deste par.
    in_max_seq: u64,
    /// O que o último sinal de vida dele disse.
    peer_version: Option<u64>,
    peer_state: Option<String>,
}

pub struct Peer {
    node_id: String,
    state: Mutex<PeerState>,
    wake: Notify,
    removed: AtomicBool,
}

impl Peer {
    fn lock(&self) -> MutexGuard<'_, PeerState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// O que um par mostra para as condições do cluster e para `GET /health/cluster`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub node_id: String,
    pub up: bool,
    pub queue: usize,
    pub version: Option<u64>,
    pub state: Option<String>,
}

type DownHook = Arc<dyn Fn(String, VoteReason) + Send + Sync>;
type ExcludedHook = Arc<dyn Fn(String) + Send + Sync>;

pub struct Channels {
    service: Arc<ClusterService>,
    signals: OnceLock<Weak<Signals>>,
    peers: Mutex<HashMap<String, Arc<Peer>>>,
    on_down: OnceLock<DownHook>,
    on_excluded: OnceLock<ExcludedHook>,
}

impl Channels {
    pub fn new(service: Arc<ClusterService>) -> Arc<Self> {
        Arc::new(Self {
            service,
            signals: OnceLock::new(),
            peers: Mutex::new(HashMap::new()),
            on_down: OnceLock::new(),
            on_excluded: OnceLock::new(),
        })
    }

    pub fn set_signals(&self, signals: &Arc<Signals>) {
        let _ = self.signals.set(Arc::downgrade(signals));
    }

    /// Quem é chamado quando um par cai ou a fila para ele estoura (abre a votação).
    pub fn set_on_down(&self, hook: DownHook) {
        let _ = self.on_down.set(hook);
    }

    /// Quem é chamado quando um par insiste em dizer que este nó não é membro: confere se o nó foi
    /// mesmo excluído do cluster.
    pub fn set_on_excluded(&self, hook: ExcludedHook) {
        let _ = self.on_excluded.set(hook);
    }

    fn signals(&self) -> Option<Arc<Signals>> {
        self.signals.get().and_then(Weak::upgrade)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Peer>>> {
        self.peers.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn rules(&self) -> &super::service::ClusterRules {
        self.service.rules()
    }

    // ---- Os pares ----

    /// Acerta os canais com a tabela de membros: abre o canal de quem entrou e descarta o de quem
    /// saiu, com a fila de saída e a de entrada (doc 13, seção 12.9).
    pub fn sync_members(self: &Arc<Self>) {
        let Some(table) = self.service.membership().table() else {
            self.drop_all();
            return;
        };
        let me = self.service.node_id();
        // O nó que saiu da própria tabela (drain, exclusão) não tem mais canais.
        if table.member(&me).is_none() {
            self.drop_all();
            return;
        }
        let wanted: Vec<String> = table
            .members
            .iter()
            .filter(|m| m.node_id != me)
            .map(|m| m.node_id.clone())
            .collect();
        let mut added = Vec::new();
        let mut removed = Vec::new();
        {
            let mut peers = self.lock();
            for id in &wanted {
                if !peers.contains_key(id) {
                    let peer = Arc::new(Peer {
                        node_id: id.clone(),
                        state: Mutex::new(PeerState {
                            queue: OutQueue::new(
                                self.rules().queue_max_size,
                                self.rules().queue_max_age,
                            ),
                            up: true,
                            failures: 0,
                            vote_opened: false,
                            not_member: 0,
                            snapshot_pending: true,
                            last_keepalive_sent: None,
                            last_attempt: None,
                            last_heard: None,
                            in_max_seq: 0,
                            peer_version: None,
                            peer_state: None,
                        }),
                        wake: Notify::new(),
                        removed: AtomicBool::new(false),
                    });
                    peers.insert(id.clone(), peer.clone());
                    added.push(peer);
                }
            }
            let gone: Vec<String> = peers
                .keys()
                .filter(|id| !wanted.contains(id))
                .cloned()
                .collect();
            for id in gone {
                if let Some(peer) = peers.remove(&id) {
                    removed.push(peer);
                }
            }
        }
        let registry = &self.service.telemetry().registry;
        for peer in added {
            registry.add(&CHANNEL_CONNECTIONS, &[], 1.0);
            registry.set(&CHANNEL_STATE, &[("taba.peer", &peer.node_id)], 1.0);
            let me = self.clone();
            tokio::spawn(async move { me.run_peer(peer).await });
        }
        for peer in removed {
            self.discard(&peer);
        }
    }

    fn drop_all(&self) {
        let peers: Vec<Arc<Peer>> = self.lock().drain().map(|(_, p)| p).collect();
        for peer in peers {
            self.discard(&peer);
        }
    }

    fn discard(&self, peer: &Arc<Peer>) {
        peer.removed.store(true, Ordering::SeqCst);
        peer.wake.notify_one();
        let (queued, was_up) = {
            let mut state = peer.lock();
            (state.queue.clear(), state.up)
        };
        let registry = &self.service.telemetry().registry;
        let label = [("taba.peer", peer.node_id.as_str())];
        if was_up {
            registry.add(&CHANNEL_CONNECTIONS, &[], -1.0);
        }
        registry.add(&CHANNEL_QUEUE_OUT, &label, -(queued as f64));
        registry.set(&CHANNEL_STATE, &label, 0.0);
    }

    /// Como cada par aparece para quem olha o cluster.
    pub fn peers(&self) -> Vec<PeerInfo> {
        let mut out: Vec<PeerInfo> = self
            .lock()
            .values()
            .map(|peer| {
                let state = peer.lock();
                PeerInfo {
                    node_id: peer.node_id.clone(),
                    up: state.up,
                    queue: state.queue.len(),
                    version: state.peer_version,
                    state: state.peer_state.clone(),
                }
            })
            .collect();
        out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        out
    }

    /// O canal com o nó está conectado? Um nó que este não conhece como par não está.
    pub fn is_up(&self, node_id: &str) -> bool {
        self.lock().get(node_id).is_some_and(|p| p.lock().up)
    }

    // ---- Enviar sinais ----

    /// Enfileira o sinal para todos os pares. Um nó que ainda sincroniza só manda o seu estado
    /// (doc 13, seção 12.7: não avisa nem sinaliza mensagens prontas).
    pub fn broadcast(&self, signal: Signal) {
        let peers: Vec<Arc<Peer>> = self.lock().values().cloned().collect();
        for peer in peers {
            self.enqueue(&peer, signal.clone());
        }
    }

    /// Enfileira o sinal para um par.
    pub fn send(&self, node_id: &str, signal: Signal) {
        let peer = self.lock().get(node_id).cloned();
        if let Some(peer) = peer {
            self.enqueue(&peer, signal);
        }
    }

    fn enqueue(&self, peer: &Arc<Peer>, signal: Signal) -> Option<u64> {
        if self.service.node_state() != NodeState::Active
            && !matches!(signal, Signal::NodeState { .. })
        {
            return None;
        }
        let (seq, dropped) = peer.lock().queue.push(signal, Instant::now());
        self.count_push(peer, 1, dropped);
        peer.wake.notify_one();
        Some(seq)
    }

    fn count_push(&self, peer: &Peer, pushed: usize, dropped: Dropped) {
        let registry = &self.service.telemetry().registry;
        let label = [("taba.peer", peer.node_id.as_str())];
        registry.add(
            &CHANNEL_QUEUE_OUT,
            &label,
            pushed as f64 - dropped.coalesced as f64,
        );
        if dropped.coalesced > 0 {
            registry.add(
                &CHANNEL_QUEUE_DROPPED,
                &[
                    ("taba.peer", peer.node_id.as_str()),
                    ("taba.reason", "coalesced"),
                ],
                dropped.coalesced as f64,
            );
        }
    }

    /// Enfileira o sinal para todos os pares e espera os `confirm` (até `timeout`). É o que o nó que
    /// acabou de ficar pronto usa: os outros precisam saber antes de ele passar a atender.
    pub async fn broadcast_confirmed(&self, signal: Signal, timeout: Duration) {
        let peers: Vec<Arc<Peer>> = self.lock().values().cloned().collect();
        let mut waiting: Vec<(Arc<Peer>, u64)> = Vec::new();
        for peer in peers {
            if let Some(seq) = self.enqueue(&peer, signal.clone()) {
                waiting.push((peer, seq));
            }
        }
        let deadline = Instant::now() + timeout;
        while waiting.iter().any(|(peer, seq)| {
            !peer.removed.load(Ordering::SeqCst) && peer.lock().queue.contains(*seq)
        }) {
            if Instant::now() >= deadline || self.service.cancel_token().is_cancelled() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // ---- A tarefa de cada par ----

    fn keepalive_due(&self, state: &PeerState, now: Instant) -> bool {
        state
            .last_keepalive_sent
            .is_none_or(|at| now.saturating_duration_since(at) >= self.rules().keepalive_interval)
    }

    async fn run_peer(self: Arc<Self>, peer: Arc<Peer>) {
        let cancel = self.service.cancel_token().clone();
        loop {
            let wait = {
                let state = peer.lock();
                let interval = if state.failures > 0 {
                    self.rules().reconnect_backoff
                } else {
                    self.rules().keepalive_interval
                };
                let since = state
                    .last_attempt
                    .map_or(interval, |at| Instant::now().saturating_duration_since(at));
                interval.saturating_sub(since).max(Duration::from_millis(5))
            };
            tokio::select! {
                () = cancel.cancelled() => return,
                () = peer.wake.notified() => {}
                () = tokio::time::sleep(wait) => {}
            }
            if peer.removed.load(Ordering::SeqCst) || cancel.is_cancelled() {
                return;
            }
            // Durante uma queda, uma tentativa só a cada `backoff`, mesmo que cheguem quadros.
            let backing_off = {
                let state = peer.lock();
                state.failures > 0
                    && state.last_attempt.is_some_and(|at| {
                        Instant::now().saturating_duration_since(at)
                            < self.rules().reconnect_backoff
                    })
            };
            if !backing_off {
                self.tick(&peer).await;
            }
        }
    }

    async fn tick(self: &Arc<Self>, peer: &Arc<Peer>) {
        let Some(signals) = self.signals() else {
            return;
        };
        let now = Instant::now();
        let me = self.service.node_id();
        let batch = {
            let mut state = peer.lock();
            let dropped = state.queue.expire(now);
            if dropped.expired > 0 {
                let label = [
                    ("taba.peer", peer.node_id.as_str()),
                    ("taba.reason", "expired"),
                ];
                let registry = &self.service.telemetry().registry;
                registry.add(&CHANNEL_QUEUE_DROPPED, &label, dropped.expired as f64);
                registry.add(
                    &CHANNEL_QUEUE_OUT,
                    &[("taba.peer", peer.node_id.as_str())],
                    -(dropped.expired as f64),
                );
            }
            if state.snapshot_pending && self.service.node_state() == NodeState::Active {
                state.snapshot_pending = false;
                for signal in signals.snapshot() {
                    let (_, dropped) = state.queue.push(signal, now);
                    self.count_push(peer, 1, dropped);
                }
            }
            let keepalive = self
                .keepalive_due(&state, now)
                .then(|| signals.keepalive(self.rules().keepalive_max_topics));
            let frames = state.queue.pending(MAX_FRAMES);
            if frames.is_empty() && keepalive.is_none() {
                return;
            }
            state.last_attempt = Some(now);
            ChannelBatch {
                from: me,
                keepalive,
                frames,
            }
        };
        let sent_keepalive = batch.keepalive.is_some();
        let Some(member) = self
            .service
            .membership()
            .table()
            .and_then(|t| t.member(&peer.node_id).cloned())
        else {
            return;
        };
        let timeout = (self.rules().keepalive_interval * 2).max(Duration::from_millis(500));
        let started = Instant::now();
        let result = self
            .service
            .call_peer(&member, Request::Channel(batch), timeout)
            .await;
        if peer.removed.load(Ordering::SeqCst) {
            return;
        }
        match result {
            Ok(Response::Confirm(confirm)) => {
                self.succeeded(peer, &confirm.seqs, started.elapsed(), sent_keepalive, now)
            }
            Ok(_) => self.failed(peer),
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterNotMember => {
                // O par não me tem na tabela dele: ou fui excluído, ou o commit ainda não chegou.
                let excluded = {
                    let mut state = peer.lock();
                    state.not_member += 1;
                    state.not_member >= EXCLUDED_AFTER
                };
                if excluded
                    && self.service.node_state() == NodeState::Active
                    && let Some(hook) = self.on_excluded.get()
                {
                    hook(peer.node_id.clone());
                }
            }
            // O par respondeu com um erro (por exemplo, ainda sem tabela): está vivo.
            Err(CallError::Remote(_)) => {
                self.succeeded(peer, &[], started.elapsed(), sent_keepalive, now)
            }
            Err(_) => self.failed(peer),
        }
    }

    fn succeeded(
        &self,
        peer: &Arc<Peer>,
        confirmed: &[u64],
        rtt: Duration,
        sent_keepalive: bool,
        at: Instant,
    ) {
        let registry = &self.service.telemetry().registry;
        let label = [("taba.peer", peer.node_id.as_str())];
        let (before, after, recovered) = {
            let mut state = peer.lock();
            let before = state.queue.len();
            state.queue.confirm(confirmed);
            let after = state.queue.len();
            let recovered = !state.up;
            if recovered {
                state.up = true;
                // O que o outro lado perdeu enquanto o canal esteve fora se refaz pelo instantâneo.
                state.snapshot_pending = true;
            }
            state.failures = 0;
            state.vote_opened = false;
            state.not_member = 0;
            if sent_keepalive {
                state.last_keepalive_sent = Some(at);
            }
            (before, after, recovered)
        };
        registry.add(&CHANNEL_QUEUE_OUT, &label, after as f64 - before as f64);
        registry.observe(&CHANNEL_RTT, &label, rtt.as_secs_f64());
        if recovered {
            registry.add(&CHANNEL_CONNECTIONS, &[], 1.0);
            registry.set(&CHANNEL_STATE, &label, 1.0);
            registry.add(
                &CHANNEL_RECONNECTS,
                &[("taba.peer", peer.node_id.as_str()), ("taba.outcome", "ok")],
                1.0,
            );
            tracing::info!(peer = %peer.node_id, "canal com o nó restabelecido");
            peer.wake.notify_one();
        }
    }

    fn failed(&self, peer: &Arc<Peer>) {
        let registry = &self.service.telemetry().registry;
        let label = [("taba.peer", peer.node_id.as_str())];
        let (went_down, open_vote) = {
            let mut state = peer.lock();
            let went_down = state.up;
            state.up = false;
            state.failures += 1;
            let overflowed = state.queue.overflowed(Instant::now());
            let reason = if overflowed {
                Some(VoteReason::QueueOverflow)
            } else if state.failures >= self.rules().reconnect_retries {
                Some(VoteReason::ChannelDown)
            } else {
                None
            };
            let open = match reason {
                Some(reason) if !state.vote_opened => {
                    state.vote_opened = true;
                    Some(reason)
                }
                _ => None,
            };
            (went_down, open)
        };
        if went_down {
            registry.add(&CHANNEL_CONNECTIONS, &[], -1.0);
            registry.set(&CHANNEL_STATE, &label, 0.0);
            tracing::warn!(peer = %peer.node_id, "canal com o nó caído: tentando reconectar");
        } else {
            registry.add(
                &CHANNEL_RECONNECTS,
                &[
                    ("taba.peer", peer.node_id.as_str()),
                    ("taba.outcome", "failed"),
                ],
                1.0,
            );
        }
        if let Some(reason) = open_vote {
            if reason == VoteReason::QueueOverflow {
                registry.add(
                    &CHANNEL_QUEUE_DROPPED,
                    &[
                        ("taba.peer", peer.node_id.as_str()),
                        ("taba.reason", "overflow"),
                    ],
                    1.0,
                );
            }
            if let Some(hook) = self.on_down.get() {
                hook(peer.node_id.clone(), reason);
            }
        }
    }

    // ---- Receber ----

    /// Atende uma chamada do canal: guarda o que o sinal de vida diz, processa os quadros novos (um
    /// `seq` já processado é ignorado) e devolve o `confirm` de todos, depois de processar.
    pub fn receive(self: &Arc<Self>, batch: ChannelBatch) -> Confirm {
        let peer = self.lock().get(&batch.from).cloned();
        let peer = match peer {
            Some(peer) => Some(peer),
            None => {
                // Um membro que esta tabela já tem e o canal ainda não conhecia.
                self.sync_members();
                self.lock().get(&batch.from).cloned()
            }
        };
        let Some(signals) = self.signals() else {
            return Confirm::new(Vec::new());
        };
        if let Some(peer) = &peer {
            let mut state = peer.lock();
            state.last_heard = Some(Instant::now());
            if let Some(keepalive) = &batch.keepalive {
                state.peer_version = Some(keepalive.version);
                state.peer_state = Some(keepalive.state.clone());
            }
        }
        if let Some(keepalive) = &batch.keepalive {
            signals.apply_keepalive(&batch.from, keepalive);
        }
        let mut confirmed = Vec::with_capacity(batch.frames.len());
        for frame in batch.frames {
            let fresh = match &peer {
                Some(peer) => {
                    let mut state = peer.lock();
                    if frame.seq > state.in_max_seq {
                        state.in_max_seq = frame.seq;
                        // A mudança de estado também vale até o próximo sinal de vida.
                        if let Signal::NodeState { state: node_state } = &frame.signal {
                            state.peer_state = Some(node_state.clone());
                        }
                        true
                    } else {
                        false
                    }
                }
                None => true,
            };
            if fresh {
                signals.receive(SignalFrame {
                    from: batch.from.clone(),
                    signal: frame.signal,
                });
            }
            confirmed.push(frame.seq);
        }
        Confirm::new(confirmed)
    }

    /// O estado que o par disse ter (`active`, `syncing`, `failed`...), pelo sinal de vida ou por
    /// um `node-state`.
    pub fn peer_state(&self, node_id: &str) -> Option<String> {
        let peer = self.lock().get(node_id).cloned()?;
        let state = peer.lock();
        state.peer_state.clone()
    }

    /// A votação de exclusão do par acabou sem efeito por um erro passageiro: a próxima falha do
    /// canal abre outra, em vez de esperar o canal voltar.
    pub fn allow_new_vote(&self, node_id: &str) {
        if let Some(peer) = self.lock().get(node_id).cloned() {
            peer.lock().vote_opened = false;
        }
    }

    /// Um par avisou que entrou em falha (ou o canal com ele caiu): abre a votação de exclusão.
    pub fn report_failed(&self, node_id: &str, reason: VoteReason) {
        if node_id == self.service.node_id() {
            return;
        }
        if let Some(hook) = self.on_down.get() {
            hook(node_id.to_owned(), reason);
        }
    }
}
