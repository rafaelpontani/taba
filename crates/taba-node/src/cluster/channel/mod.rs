//! The permanent channel between each pair of nodes (doc 04, sections 6.1 and 6.3; doc 13, sections 12.8 and 12.9):
//! the **heartbeat** (`keepalive`), the delivery availability signals with `seq`,
//! confirmation and outgoing queue, and the failure detection that opens the exclusion vote.
//!
//! There is **one connection** per pair (a WebSocket on the real network, `GET /cluster/channel`; a pair of queues
//! on the in-memory network), used in both directions: each node sends **batches** (the heartbeat and the
//! frames still unconfirmed) and answers each batch received with the `confirm`. The channel drops
//! when the connection closes or when nothing arrives from the peer for twice
//! `cluster.channel.keepalive.interval`; the first drop marks the channel as down and the following
//! attempts to open again (every `cluster.channel.reconnect.backoff`) count as reconnection
//! attempts. Once `cluster.channel.reconnect.retries` run out, the node opens the exclusion vote on the
//! peer. The outgoing queue overflowing (size or age) opens the vote right away.
//!
//! Who opens the connection: the node that **joins or returns** opens it with all the others; those that were already in the
//! cluster only accept (and, if the new one does not show up within a heartbeat interval, they open it). If
//! both open at the same time, the one opened by the node with the **highest ID hash** stays (the one with the lowest hash
//! closes the one it opened): always one channel per pair. What was left unconfirmed on the connection that dropped is
//! resent on the one that stayed.
//!
//! One task per pair opens the connection when there is none; one per connection sends, and two receive: one reads what
//! arrives (and marks the peer as alive) and puts it in the peer's **incoming queue**, and the other processes
//! the queue in order and returns the `confirm`. This way slow processing does not hide the peer's heartbeat,
//! it only delays the `confirm`.

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

/// How many frames a batch carries at most.
const MAX_FRAMES: usize = 256;
/// How many consecutive `not-member` answers make the node conclude it was excluded.
const EXCLUDED_AFTER: u32 = 3;

/// Two connections with the same peer: the one opened by the node with the **highest ID hash** stays (doc 13, section
/// 12.9); the one with the lowest hash closes the one it opened. `mine_wins`: this node's hash is the highest. Returns whether the
/// new one replaces the old: whenever the same side reopened it (the old one is dead) and, between
/// different sides, when the new one was opened by the node with the highest hash.
fn keeps_new_link(old_opened_by_me: bool, new_opened_by_me: bool, mine_wins: bool) -> bool {
    old_opened_by_me == new_opened_by_me || new_opened_by_me == mine_wins
}

/// An open connection with the peer.
struct Link {
    id: u64,
    /// I opened the connection (otherwise the peer did).
    opened_by_me: bool,
    tx: mpsc::Sender<ChannelMessage>,
    cancel: CancellationToken,
    /// There is something new to send.
    wake: Notify,
    /// The highest frame `seq` already sent by this connection: anything past that is new.
    sent_max: AtomicU64,
    /// When each batch without a `confirm` was sent (the `confirm` comes back in the same order).
    in_flight: Mutex<VecDeque<Instant>>,
    /// When the peer's last message arrived on this connection.
    heard: Mutex<Instant>,
}

#[derive(Debug)]
struct PeerState {
    queue: OutQueue,
    /// The channel is connected (the connection is open and the peer is talking).
    up: bool,
    /// There has already been a connection with this peer (the next one is a reconnection).
    connected_once: bool,
    /// The current connection.
    link: Option<Arc<Link>>,
    /// Consecutive openings that failed (or connections that dropped).
    failures: u32,
    /// This drop's vote has already been opened.
    vote_opened: bool,
    /// Consecutive `not-member` answers.
    not_member: u32,
    /// The signal snapshot needs to be queued (new channel or one that came back).
    snapshot_pending: bool,
    last_keepalive_sent: Option<Instant>,
    last_attempt: Option<Instant>,
    /// Do not open the connection before this (the node that was already in the cluster waits for the new one to show up).
    dial_not_before: Option<Instant>,
    /// The highest `seq` already processed from this peer.
    in_max_seq: u64,
    /// What its last heartbeat said.
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
    /// Wakes the task that opens the connection (the connection dropped, the peer left).
    dialer: Notify,
    removed: AtomicBool,
}

impl Peer {
    fn lock(&self) -> MutexGuard<'_, PeerState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What a peer shows for the cluster conditions and for `GET /health/cluster`.
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
    /// This node has already seen the member table with it inside: the peers that appear later are those that
    /// join, and it is the new one that opens the connection with the old ones.
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

    /// Who is called when a peer falls or the queue to it overflows (opens the vote).
    pub fn set_on_down(&self, hook: DownHook) {
        let _ = self.on_down.set(hook);
    }

    /// Who is called when a peer insists on saying this node is not a member: checks whether the node was
    /// really excluded from the cluster.
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

    /// How much silence from the peer brings the connection down.
    fn dead_after(&self) -> Duration {
        (self.rules().keepalive_interval * 2).max(Duration::from_millis(500))
    }

    // ---- The peers ----

    /// Adjusts the channels to the member table: opens the channel of whoever joined and discards that of whoever
    /// left, with the outgoing and incoming queue (doc 13, section 12.9).
    pub fn sync_members(self: &Arc<Self>) {
        let Some(table) = self.service.membership().table() else {
            self.drop_all();
            return;
        };
        let me = self.service.node_id();
        // A node that left its own table (drain, exclusion) has no channels left.
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
        // The node that just joined opens the connection with all; those that were already there wait for it.
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

    /// How each peer appears to whoever looks at the cluster.
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

    /// Is the channel with the node connected? A node this one does not know as a peer is not.
    pub fn is_up(&self, node_id: &str) -> bool {
        self.lock().get(node_id).is_some_and(|p| p.lock().up)
    }

    /// The peer is talking now: the connection is open and its last signal arrived less than the
    /// silence deadline ago. It is what the voter checks before voting on the exclusion.
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

    // ---- Sending signals ----

    /// Queues the signal for all the peers. A node that is still syncing only sends its state
    /// (doc 13, section 12.7: it neither notifies nor signals ready messages).
    pub fn broadcast(&self, signal: Signal) {
        let peers: Vec<Arc<Peer>> = self.lock().values().cloned().collect();
        for peer in peers {
            self.enqueue(&peer, signal.clone());
        }
    }

    /// Queues the signal for a peer.
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

    /// Queues the signal for all the peers and waits for the `confirm`s (up to `timeout`). It is what the node that
    /// just became ready uses: the others need to know before it starts serving.
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

    // ---- Opening the connection ----

    /// How long until the next attempt to open the connection.
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

    /// The peer's task that opens the connection when there is none.
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
            // Another process serves at that address: the node this channel is looking for is not there (D100).
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterWrongNode => {
                self.failed(peer)
            }
            Err(CallError::Remote(error)) if error.code == ErrorCode::ClusterNotMember => {
                // The peer does not have me in its table: either I was excluded, or the commit has not arrived yet.
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
            // The peer answered with an error (for example, still without a table): it is alive, but does not yet
            // accept the channel. Tries again at the next interval.
            Err(CallError::Remote(_)) => {
                self.mark_up(peer);
                peer.lock().dial_not_before = Some(Instant::now() + interval);
            }
            Err(_) => self.failed(peer),
        }
    }

    // ---- The connection ----

    /// The open connection (by me or by the peer) becomes the peer's channel. If there was already one, the
    /// one opened by the node with the highest ID hash stays (doc 13, section 12.9); the other is closed.
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
                    // The same side reopened: the old connection is dead.
                    Some(old) if old.opened_by_me == opened_by_me => {
                        old.cancel.cancel();
                        state.link = Some(link.clone());
                        true
                    }
                    // Two connections at the same time: the one of the node with the highest hash stays. What the one that closes
                    // carried and was not confirmed is resent on the one that stayed.
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

    /// The channel with the peer is up: resets the failures and counts the reconnection.
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
            tracing::info!(peer = %peer.node_id, "channel with the node reestablished");
        }
    }

    /// What a connection does while it lives: sending is this task, and reading and processing
    /// are two others.
    async fn run_link(
        self: Arc<Self>,
        peer: Arc<Peer>,
        link: Arc<Link>,
        mut rx: mpsc::Receiver<ChannelMessage>,
    ) {
        let (inbox_tx, mut inbox) = mpsc::unbounded_channel::<ChannelMessage>();
        let telemetry = self.service.telemetry().clone();
        // Reads what arrives and puts it in the incoming queue.
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
                    // The peer spoke: the connection is valid, and the previous failures are left behind.
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
        // Processes the incoming queue in order and returns the `confirm`.
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
                // What was left unprocessed leaves the count; the peer resends it on the next connection.
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

    /// The batch to send now: the snapshot (new channel), the heartbeat (when it is time) and the
    /// frames this connection has not carried yet.
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

    /// A message from the incoming queue: a batch is processed and confirmed; a `confirm` takes the
    /// frames out of the outgoing queue.
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
                // Something is left that only fit afterwards (more than one batch of frames).
                link.wake.notify_one();
            }
        }
    }

    /// The connection ended (the peer closed it, went mute or the node stopped): if it was the peer's, the channel fell and
    /// the peer's task opens another after the `backoff`.
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
            tracing::warn!(peer = %peer.node_id, "channel with the node down: trying to reconnect");
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

    // ---- Receiving ----

    /// The peer opened the connection with this node.
    pub fn accept(self: &Arc<Self>, from: &str, io: ChannelLink) {
        let peer = match self.lock().get(from).cloned() {
            Some(peer) => Some(peer),
            None => {
                // A member this table already has and the channel did not know yet.
                self.sync_members();
                self.lock().get(from).cloned()
            }
        };
        match peer {
            Some(peer) => self.install(&peer, io, false),
            None => drop(io),
        }
    }

    /// Stores what the heartbeat says, processes the new frames (an already-processed `seq` is
    /// ignored) and returns the `confirm` of all, after processing.
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
                        // The state change also applies until the next heartbeat.
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

    /// The state the peer said it has (`active`, `syncing`, `failed`...), through the heartbeat or through
    /// a `node-state`.
    pub fn peer_state(&self, node_id: &str) -> Option<String> {
        let peer = self.lock().get(node_id).cloned()?;
        let state = peer.lock();
        state.peer_state.clone()
    }

    /// The peer's exclusion vote ended without effect because of a transient error: the channel's next failure
    /// opens another, instead of waiting for the channel to come back.
    pub fn allow_new_vote(&self, node_id: &str) {
        if let Some(peer) = self.lock().get(node_id).cloned() {
            peer.lock().vote_opened = false;
        }
    }

    /// A peer notified that it entered failure (or the channel to it fell): opens the exclusion vote.
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
