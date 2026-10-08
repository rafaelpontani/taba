//! O canal permanente entre cada par de nós (doc 04, seções 6.1 e 6.3; doc 13, seções 12.8 e 12.9):
//! o **sinal de vida** (`keepalive`), os sinais de disponibilidade da entrega com `seq`,
//! confirmação e fila de saída, e a detecção de falha que abre a votação de exclusão.
//!
//! Há **uma conexão** por par (um WebSocket na rede real, `GET /cluster/channel`; um par de filas
//! na rede em memória), usada nos dois sentidos: cada nó manda **lotes** (o sinal de vida e os
//! quadros ainda sem confirmação) e responde a cada lote recebido com o `confirm`. O canal cai
//! quando a conexão fecha ou quando nada chega do par por duas vezes
//! `cluster.channel.keepalive.interval`; a primeira queda marca o canal como caído e as seguintes
//! tentativas de abrir de novo (a cada `cluster.channel.reconnect.backoff`) contam como tentativas
//! de reconexão. Esgotadas `cluster.channel.reconnect.retries`, o nó abre a votação de exclusão do
//! par. A fila de saída estourar (tamanho ou idade) abre a votação na hora.
//!
//! Quem abre a conexão: o nó que **entra ou volta** abre com todos os outros; os que já estavam no
//! cluster só aceitam (e, se o novo não aparece em um intervalo de sinal de vida, abrem eles). Se
//! os dois abrirem ao mesmo tempo, fica a aberta pelo nó de **maior hash do ID** (o de menor hash
//! fecha a que abriu): sempre um canal por par. O que ficou sem confirmação na conexão que caiu é
//! reenviado na que ficou.
//!
//! Uma tarefa por par abre a conexão quando não há; uma por conexão envia, e duas recebem: uma lê o
//! que chega (e marca que o par está vivo) e põe na **fila de entrada** do par, e a outra processa
//! a fila na ordem e devolve o `confirm`. Assim um processamento lento não esconde o sinal de vida
//! do par, só atrasa o `confirm`.

mod queue;

pub use queue::{Dropped, NOTIFY_TTL, OutQueue};

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

use super::protocol::{ChannelBatch, ChannelMessage, Confirm, Signal, SignalFrame, VoteReason};
use super::replication::Signals;
use super::service::ClusterService;
use super::state::NodeState;
use super::transport::{CallError, ChannelLink, ChannelOpen};
use crate::core::error::ErrorCode;
use crate::core::ring;
use crate::telemetry::metrics::{
    CHANNEL_CONNECTIONS, CHANNEL_QUEUE_DROPPED, CHANNEL_QUEUE_IN, CHANNEL_QUEUE_OUT,
    CHANNEL_RECONNECTS, CHANNEL_RTT, CHANNEL_STATE,
};

/// Quantos quadros um lote leva no máximo.
const MAX_FRAMES: usize = 256;
/// Quantas respostas `not-member` seguidas fazem o nó concluir que foi excluído.
const EXCLUDED_AFTER: u32 = 3;

/// Duas conexões com o mesmo par: fica a aberta pelo nó de **maior hash do ID** (doc 13, seção
/// 12.9); o de menor hash fecha a que abriu. `mine_wins`: o hash deste nó é o maior. Devolve se a
/// nova substitui a antiga: sempre que o mesmo lado a reabriu (a antiga está morta) e, entre
/// lados diferentes, quando a nova foi aberta pelo nó de maior hash.
fn keeps_new_link(old_opened_by_me: bool, new_opened_by_me: bool, mine_wins: bool) -> bool {
    old_opened_by_me == new_opened_by_me || new_opened_by_me == mine_wins
}

/// Uma conexão aberta com o par.
struct Link {
    id: u64,
    /// Fui eu que abri a conexão (senão o par abriu).
    opened_by_me: bool,
    tx: mpsc::Sender<ChannelMessage>,
    cancel: CancellationToken,
    /// Há algo novo a enviar.
    wake: Notify,
    /// O maior `seq` de quadro já enviado por esta conexão: o que passou disso é novo.
    sent_max: AtomicU64,
    /// Quando cada lote sem `confirm` foi enviado (o `confirm` volta na mesma ordem).
    in_flight: Mutex<VecDeque<Instant>>,
    /// Quando chegou a última mensagem do par por esta conexão.
    heard: Mutex<Instant>,
}

#[derive(Debug)]
struct PeerState {
    queue: OutQueue,
    /// O canal está conectado (a conexão está aberta e o par está falando).
    up: bool,
    /// Já houve conexão com este par (a próxima é uma reconexão).
    connected_once: bool,
    /// A conexão de agora.
    link: Option<Arc<Link>>,
    /// Aberturas seguidas que falharam (ou conexões que caíram).
    failures: u32,
    /// A votação desta queda já foi aberta.
    vote_opened: bool,
    /// Respostas `not-member` seguidas.
    not_member: u32,
    /// O instantâneo dos sinais precisa ser enfileirado (canal novo ou que voltou).
    snapshot_pending: bool,
    last_keepalive_sent: Option<Instant>,
    last_attempt: Option<Instant>,
    /// Não abrir a conexão antes disto (o nó que já estava no cluster espera o novo aparecer).
    dial_not_before: Option<Instant>,
    /// O maior `seq` já processado deste par.
    in_max_seq: u64,
    /// O que o último sinal de vida dele disse.
    peer_version: Option<u64>,
    peer_state: Option<String>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("id", &self.id).finish()
    }
}

pub struct Peer {
    node_id: String,
    state: Mutex<PeerState>,
    /// Acorda a tarefa que abre a conexão (a conexão caiu, o par saiu).
    dialer: Notify,
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
    next_link: AtomicU64,
    /// Este nó já viu a tabela de membros com ele dentro: os pares que aparecem depois são os que
    /// entram, e é o novo quem abre a conexão com os antigos.
    established: AtomicBool,
}

impl Channels {
    pub fn new(service: Arc<ClusterService>) -> Arc<Self> {
        Arc::new(Self {
            service,
            signals: OnceLock::new(),
            peers: Mutex::new(HashMap::new()),
            on_down: OnceLock::new(),
            on_excluded: OnceLock::new(),
            next_link: AtomicU64::new(1),
            established: AtomicBool::new(false),
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

    /// Quanto silêncio do par derruba a conexão.
    fn dead_after(&self) -> Duration {
        (self.rules().keepalive_interval * 2).max(Duration::from_millis(500))
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
        // O nó que acaba de entrar abre a conexão com todos; os que já estavam esperam por ele.
        let joining = !self.established.swap(true, Ordering::SeqCst);
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
                            up: false,
                            connected_once: false,
                            link: None,
                            failures: 0,
                            vote_opened: false,
                            not_member: 0,
                            snapshot_pending: true,
                            last_keepalive_sent: None,
                            last_attempt: None,
                            dial_not_before: (!joining)
                                .then(|| Instant::now() + self.rules().keepalive_interval),
                            in_max_seq: 0,
                            peer_version: None,
                            peer_state: None,
                        }),
                        dialer: Notify::new(),
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
            registry.set(&CHANNEL_STATE, &[("taba.peer", &peer.node_id)], 0.0);
            let me = self.clone();
            tokio::spawn(async move { me.run_peer(peer).await });
        }
        for peer in removed {
            self.discard(&peer);
        }
    }

    fn drop_all(&self) {
        self.established.store(false, Ordering::SeqCst);
        let peers: Vec<Arc<Peer>> = self.lock().drain().map(|(_, p)| p).collect();
        for peer in peers {
            self.discard(&peer);
        }
    }

    fn discard(&self, peer: &Arc<Peer>) {
        peer.removed.store(true, Ordering::SeqCst);
        peer.dialer.notify_one();
        let (queued, was_up, link) = {
            let mut state = peer.lock();
            let was_up = state.up;
            state.up = false;
            (state.queue.clear(), was_up, state.link.take())
        };
        if let Some(link) = link {
            link.cancel.cancel();
        }
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

    /// O par está falando agora: a conexão está aberta e o último sinal dele chegou há menos que o
    /// prazo de silêncio. É o que o votante confere antes de votar a exclusão.
    pub fn is_live(&self, node_id: &str) -> bool {
        let Some(peer) = self.lock().get(node_id).cloned() else {
            return false;
        };
        let state = peer.lock();
        state.up
            && state.link.as_ref().is_some_and(|link| {
                link.heard
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .elapsed()
                    < self.dead_after()
            })
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
        let (seq, dropped, link) = {
            let mut state = peer.lock();
            let (seq, dropped) = state.queue.push(signal, Instant::now());
            (seq, dropped, state.link.clone())
        };
        self.count_push(peer, 1, dropped);
        if let Some(link) = link {
            link.wake.notify_one();
        }
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

    // ---- Abrir a conexão ----

    /// Quanto falta para a próxima tentativa de abrir a conexão.
    fn dial_wait(&self, state: &PeerState) -> Duration {
        let now = Instant::now();
        let mut wait = state
            .dial_not_before
            .map_or(Duration::ZERO, |at| at.saturating_duration_since(now));
        if state.failures > 0
            && let Some(at) = state.last_attempt
        {
            wait = wait.max(
                self.rules()
                    .reconnect_backoff
                    .saturating_sub(now.saturating_duration_since(at)),
            );
        }
        wait
    }

    /// A tarefa do par que abre a conexão quando não há uma.
    async fn run_peer(self: Arc<Self>, peer: Arc<Peer>) {
        let cancel = self.service.cancel_token().clone();
        loop {
            if peer.removed.load(Ordering::SeqCst) || cancel.is_cancelled() {
                return;
            }
            let (connected, wait) = {
                let state = peer.lock();
                (state.link.is_some(), self.dial_wait(&state))
            };
            if connected {
                tokio::select! {
                    () = cancel.cancelled() => return,
                    () = peer.dialer.notified() => {}
                }
                continue;
            }
            if !wait.is_zero() {
                tokio::select! {
                    () = cancel.cancelled() => return,
                    () = peer.dialer.notified() => {}
                    () = tokio::time::sleep(wait) => {}
                }
                continue;
            }
            self.dial(&peer).await;
        }
    }

    async fn dial(self: &Arc<Self>, peer: &Arc<Peer>) {
        let interval = self.rules().keepalive_interval;
        let Some(member) = self
            .service
            .membership()
            .table()
            .and_then(|t| t.member(&peer.node_id).cloned())
        else {
            peer.lock().dial_not_before = Some(Instant::now() + interval);
            return;
        };
        peer.lock().last_attempt = Some(Instant::now());
        let open = ChannelOpen {
            from: self.service.node_id(),
            to: peer.node_id.clone(),
        };
        let result = self
            .service
            .open_channel_to(&member, open, self.dead_after())
            .await;
        if peer.removed.load(Ordering::SeqCst) {
            return;
        }
        match result {
            Ok(link) => self.install(peer, link, true),
            // Outro processo atende nesse endereço: o nó que este canal procura não está lá (D100).
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterWrongNode => {
                self.failed(peer)
            }
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterNotMember => {
                // O par não me tem na tabela dele: ou fui excluído, ou o commit ainda não chegou.
                let excluded = {
                    let mut state = peer.lock();
                    state.not_member += 1;
                    state.dial_not_before = Some(Instant::now() + interval);
                    state.not_member >= EXCLUDED_AFTER
                };
                if excluded
                    && self.service.node_state() == NodeState::Active
                    && let Some(hook) = self.on_excluded.get()
                {
                    hook(peer.node_id.clone());
                }
            }
            // O par respondeu com um erro (por exemplo, ainda sem tabela): está vivo, mas ainda não
            // aceita o canal. Tenta de novo no próximo intervalo.
            Err(CallError::Remote(_)) => {
                self.mark_up(peer);
                peer.lock().dial_not_before = Some(Instant::now() + interval);
            }
            Err(_) => self.failed(peer),
        }
    }

    // ---- A conexão ----

    /// A conexão aberta (por mim ou pelo par) passa a ser o canal do par. Se já havia uma, fica a
    /// aberta pelo nó de maior hash do ID (doc 13, seção 12.9); a outra é fechada.
    fn install(self: &Arc<Self>, peer: &Arc<Peer>, io: ChannelLink, opened_by_me: bool) {
        let ChannelLink { tx, rx } = io;
        let link = Arc::new(Link {
            id: self.next_link.fetch_add(1, Ordering::SeqCst),
            opened_by_me,
            tx,
            cancel: CancellationToken::new(),
            wake: Notify::new(),
            sent_max: AtomicU64::new(0),
            in_flight: Mutex::new(VecDeque::new()),
            heard: Mutex::new(Instant::now()),
        });
        let mine_wins = ring::position(self.service.node_id().as_bytes())
            > ring::position(peer.node_id.as_bytes());
        let kept = {
            let mut state = peer.lock();
            if peer.removed.load(Ordering::SeqCst) {
                false
            } else {
                match state.link.clone() {
                    // O mesmo lado reabriu: a conexão antiga está morta.
                    Some(old) if old.opened_by_me == opened_by_me => {
                        old.cancel.cancel();
                        state.link = Some(link.clone());
                        true
                    }
                    // Duas conexões ao mesmo tempo: fica a do nó de maior hash. O que a que fecha
                    // levou e não foi confirmado se reenvia na que ficou.
                    Some(old) => {
                        if keeps_new_link(old.opened_by_me, opened_by_me, mine_wins) {
                            old.cancel.cancel();
                            state.link = Some(link.clone());
                            true
                        } else {
                            old.sent_max.store(0, Ordering::SeqCst);
                            old.wake.notify_one();
                            false
                        }
                    }
                    None => {
                        state.link = Some(link.clone());
                        true
                    }
                }
            }
        };
        if !kept {
            link.cancel.cancel();
            return;
        }
        self.mark_up(peer);
        peer.lock().snapshot_pending = true;
        let channels = self.clone();
        let peer = peer.clone();
        tokio::spawn(async move { channels.run_link(peer, link, rx).await });
    }

    /// O canal com o par está de pé: zera as falhas e conta a reconexão.
    fn mark_up(&self, peer: &Arc<Peer>) {
        let registry = &self.service.telemetry().registry;
        let label = [("taba.peer", peer.node_id.as_str())];
        let (recovered, reconnect) = {
            let mut state = peer.lock();
            let recovered = !state.up;
            let reconnect = recovered && state.connected_once;
            state.up = true;
            state.last_keepalive_sent = None;
            state.dial_not_before = None;
            if state.link.is_some() {
                state.connected_once = true;
            }
            (recovered, reconnect)
        };
        if recovered {
            registry.add(&CHANNEL_CONNECTIONS, &[], 1.0);
            registry.set(&CHANNEL_STATE, &label, 1.0);
        }
        if reconnect {
            registry.add(
                &CHANNEL_RECONNECTS,
                &[("taba.peer", peer.node_id.as_str()), ("taba.outcome", "ok")],
                1.0,
            );
            tracing::info!(peer = %peer.node_id, "canal com o nó restabelecido");
        }
    }

    /// O que uma conexão faz enquanto vive: o envio é esta tarefa, e a leitura e o processamento
    /// são duas outras.
    async fn run_link(
        self: Arc<Self>,
        peer: Arc<Peer>,
        link: Arc<Link>,
        mut rx: mpsc::Receiver<ChannelMessage>,
    ) {
        let (inbox_tx, mut inbox) = mpsc::unbounded_channel::<ChannelMessage>();
        let telemetry = self.service.telemetry().clone();
        // Lê o que chega e põe na fila de entrada.
        let reader = {
            let (link, telemetry, peer) = (link.clone(), telemetry.clone(), peer.clone());
            tokio::spawn(async move {
                loop {
                    let message = tokio::select! {
                        () = link.cancel.cancelled() => break,
                        message = rx.recv() => message,
                    };
                    let Some(message) = message else { break };
                    *link.heard.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                    // O par falou: a conexão vale, e as falhas anteriores ficam para trás.
                    {
                        let mut state = peer.lock();
                        state.failures = 0;
                        state.vote_opened = false;
                        state.not_member = 0;
                    }
                    telemetry
                        .registry
                        .add(&CHANNEL_QUEUE_IN, &[("taba.peer", &peer.node_id)], 1.0);
                    if inbox_tx.send(message).is_err() {
                        break;
                    }
                }
                link.cancel.cancel();
            })
        };
        // Processa a fila de entrada na ordem e devolve o `confirm`.
        let processor = {
            let (channels, link, telemetry, peer) =
                (self.clone(), link.clone(), telemetry.clone(), peer.clone());
            tokio::spawn(async move {
                let label = [("taba.peer", peer.node_id.as_str())];
                loop {
                    let message = tokio::select! {
                        () = link.cancel.cancelled() => break,
                        message = inbox.recv() => message,
                    };
                    let Some(message) = message else { break };
                    channels.process(&peer, &link, message).await;
                    telemetry.registry.add(&CHANNEL_QUEUE_IN, &label, -1.0);
                }
                // O que ficou sem processar sai da conta; o par reenvia na conexão seguinte.
                let mut left = 0.0;
                while inbox.try_recv().is_ok() {
                    left += 1.0;
                }
                telemetry.registry.add(&CHANNEL_QUEUE_IN, &label, -left);
            })
        };

        let service_cancel = self.service.cancel_token().clone();
        loop {
            if peer.removed.load(Ordering::SeqCst) {
                break;
            }
            let wait = {
                let state = peer.lock();
                state.last_keepalive_sent.map_or(Duration::ZERO, |at| {
                    self.rules().keepalive_interval.saturating_sub(at.elapsed())
                })
            }
            .max(Duration::from_millis(5));
            tokio::select! {
                () = service_cancel.cancelled() => break,
                () = link.cancel.cancelled() => break,
                () = link.wake.notified() => {}
                () = tokio::time::sleep(wait) => {}
            }
            let silent = link
                .heard
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .elapsed()
                > self.dead_after();
            if silent {
                break;
            }
            if let Some(batch) = self.build_batch(&peer, &link) {
                link.in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push_back(Instant::now());
                if link.tx.send(ChannelMessage::Batch(batch)).await.is_err() {
                    break;
                }
            }
        }
        link.cancel.cancel();
        let _ = reader.await;
        let _ = processor.await;
        self.link_ended(&peer, &link);
    }

    /// O lote a enviar agora: o instantâneo (canal novo), o sinal de vida (quando é hora) e os
    /// quadros que esta conexão ainda não levou.
    fn build_batch(&self, peer: &Arc<Peer>, link: &Arc<Link>) -> Option<ChannelBatch> {
        let signals = self.signals()?;
        let now = Instant::now();
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
        let keepalive = state
            .last_keepalive_sent
            .is_none_or(|at| now.saturating_duration_since(at) >= self.rules().keepalive_interval)
            .then(|| signals.keepalive(self.rules().keepalive_max_topics));
        let frames = state
            .queue
            .pending_after(link.sent_max.load(Ordering::SeqCst), MAX_FRAMES);
        if frames.is_empty() && keepalive.is_none() {
            return None;
        }
        if keepalive.is_some() {
            state.last_keepalive_sent = Some(now);
        }
        if let Some(last) = frames.last() {
            link.sent_max.store(last.seq, Ordering::SeqCst);
        }
        Some(ChannelBatch {
            from: self.service.node_id(),
            to: peer.node_id.clone(),
            keepalive,
            frames,
        })
    }

    /// Uma mensagem da fila de entrada: um lote se processa e se confirma; um `confirm` tira os
    /// quadros da fila de saída.
    async fn process(
        self: &Arc<Self>,
        peer: &Arc<Peer>,
        link: &Arc<Link>,
        message: ChannelMessage,
    ) {
        match message {
            ChannelMessage::Batch(batch) => {
                let confirm = self.receive(batch);
                if link
                    .tx
                    .send(ChannelMessage::Confirm(confirm))
                    .await
                    .is_err()
                {
                    link.cancel.cancel();
                }
            }
            ChannelMessage::Confirm(confirm) => {
                let started = link
                    .in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .pop_front();
                let registry = &self.service.telemetry().registry;
                let label = [("taba.peer", peer.node_id.as_str())];
                let (before, after) = {
                    let mut state = peer.lock();
                    let before = state.queue.len();
                    state.queue.confirm(&confirm.seqs);
                    state.failures = 0;
                    state.vote_opened = false;
                    state.not_member = 0;
                    (before, state.queue.len())
                };
                registry.add(&CHANNEL_QUEUE_OUT, &label, after as f64 - before as f64);
                if let Some(started) = started {
                    registry.observe(&CHANNEL_RTT, &label, started.elapsed().as_secs_f64());
                }
                // Restou algo que coube só depois (mais de um lote de quadros).
                link.wake.notify_one();
            }
        }
    }

    /// A conexão acabou (o par fechou, ficou mudo ou o nó parou): se era a do par, o canal caiu e
    /// a tarefa do par abre outra depois do `backoff`.
    fn link_ended(&self, peer: &Arc<Peer>, link: &Arc<Link>) {
        let current = {
            let mut state = peer.lock();
            if state.link.as_ref().is_some_and(|l| l.id == link.id) {
                state.link = None;
                state.last_attempt = Some(Instant::now());
                true
            } else {
                false
            }
        };
        if current
            && !peer.removed.load(Ordering::SeqCst)
            && !self.service.cancel_token().is_cancelled()
        {
            self.failed(peer);
            peer.dialer.notify_one();
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

    /// O par abriu a conexão com este nó.
    pub fn accept(self: &Arc<Self>, from: &str, io: ChannelLink) {
        let peer = match self.lock().get(from).cloned() {
            Some(peer) => Some(peer),
            None => {
                // Um membro que esta tabela já tem e o canal ainda não conhecia.
                self.sync_members();
                self.lock().get(from).cloned()
            }
        };
        match peer {
            Some(peer) => self.install(&peer, io, false),
            None => drop(io),
        }
    }

    /// Guarda o que o sinal de vida diz, processa os quadros novos (um `seq` já processado é
    /// ignorado) e devolve o `confirm` de todos, depois de processar.
    pub fn receive(self: &Arc<Self>, batch: ChannelBatch) -> Confirm {
        let peer = self.lock().get(&batch.from).cloned();
        let peer = match peer {
            Some(peer) => Some(peer),
            None => {
                self.sync_members();
                self.lock().get(&batch.from).cloned()
            }
        };
        let Some(signals) = self.signals() else {
            return Confirm::new(Vec::new());
        };
        if let Some(peer) = &peer
            && let Some(keepalive) = &batch.keepalive
        {
            let mut state = peer.lock();
            state.peer_version = Some(keepalive.version);
            state.peer_state = Some(keepalive.state.clone());
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

#[cfg(test)]
mod tests;
