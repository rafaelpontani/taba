//! The signals between nodes (doc 13, section 12.8): which nodes have ready messages of a topic, which
//! have long-polling listeners and which are in drain. Each node keeps this **in memory** and only
//! sends a signal on **changes** of state; with the table, the decision of whom to ask for a `take` or
//! notify is local.
//!
//! The signals travel through the channel between nodes (`cluster::channel`): outgoing queue with `seq`,
//! confirmation and resend, plus the heartbeat, which repeats the topics with ready messages and the
//! node's state (what fixes a signal that got lost).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::cluster::channel::Channels;
use crate::cluster::members::{Change, MemberState};
use crate::cluster::protocol::{
    DefinitionApply, Keepalive, ReadyCount, Signal, SignalFrame, VoteReason,
};
use crate::cluster::service::ClusterService;
use crate::cluster::state::{DrainState, NodeState};
use crate::core::time;
use crate::engine::messages::Messages;
use crate::telemetry::metrics::CLUSTER_NOTIFICATIONS;

#[derive(Default)]
struct Tables {
    /// Topic → nodes that have ready messages, with how many the heartbeat said (`None` while only a
    /// `Ready` signal said it: the quantity is not known).
    ready: HashMap<String, HashMap<String, Option<u64>>>,
    /// Topic → nodes that have listeners.
    listeners: HashMap<String, HashSet<String>>,
    /// Nodes in drain: nobody asks them for a `take`.
    draining: HashSet<String>,
    /// Topic → how many `GET`s wait here.
    local_listeners: HashMap<String, usize>,
    /// Topic → the node that notified last: the woken `GET` tries the `take` on it first.
    hints: HashMap<String, String>,
    /// The last node notified of each topic, to rotate among those that have a listener.
    last_notified: HashMap<String, String>,
    /// Topics with a notice cycle in progress (one responsible party per topic).
    cycles: HashSet<String>,
    /// How many `take`s each node made here per topic: the responsible party sees whether the notified one came.
    takes: HashMap<(String, String), u64>,
    /// Topic → nodes that notified and have not yet had the resulting `take` counted.
    notified: HashMap<String, HashSet<String>>,
    /// The quantity of the last notice received for each topic.
    notices: HashMap<String, u64>,
    /// Node → the load indicator of its last heartbeat. It stays after the channel closes: the last one known applies.
    loads: HashMap<String, u64>,
}

pub struct Signals {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    channels: OnceLock<Arc<Channels>>,
    tables: Mutex<Tables>,
    /// The engine notifies here (without blocking) when the topic starts or stops having ready ones.
    ready_tx: mpsc::UnboundedSender<(String, bool)>,
    ready_rx: Mutex<Option<mpsc::UnboundedReceiver<(String, bool)>>>,
}

/// While it exists, there is a `GET` waiting for a message of the topic on this node.
pub struct ListenerGuard {
    signals: Arc<Signals>,
    topic: String,
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.signals.listener_left(&self.topic);
    }
}

impl Signals {
    pub fn new(service: Arc<ClusterService>, messages: Arc<Messages>) -> Arc<Self> {
        let (ready_tx, ready_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            service,
            messages,
            channels: OnceLock::new(),
            tables: Mutex::new(Tables::default()),
            ready_tx,
            ready_rx: Mutex::new(Some(ready_rx)),
        })
    }

    /// Attaches the signals to the channel that carries them.
    pub fn set_channels(&self, channels: Arc<Channels>) {
        let _ = self.channels.set(channels);
    }

    pub fn channels(&self) -> Option<&Arc<Channels>> {
        self.channels.get()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tables> {
        self.tables.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What the engine calls (from inside the topic lock) when the ready ones change.
    pub fn ready_sender(&self) -> mpsc::UnboundedSender<(String, bool)> {
        self.ready_tx.clone()
    }

    /// The task that turns ready changes into signals, until the node stops.
    pub async fn run(self: Arc<Self>) {
        let Some(mut rx) = self
            .ready_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        let cancel = self.service.cancel_token().clone();
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                item = rx.recv() => {
                    let Some((topic, has_ready)) = item else { return };
                    self.local_ready_changed(&topic, has_ready);
                }
            }
        }
    }

    fn me(&self) -> String {
        self.service.node_id()
    }

    /// Sends the signal to all the other members through the channel queue.
    fn broadcast(&self, signal: Signal) {
        if let Some(channels) = self.channels() {
            channels.broadcast(signal);
        }
    }

    // ---- The heartbeat and the snapshot ----

    /// The node's state as the peers see it.
    fn state_name(&self) -> &'static str {
        match (self.service.node_state(), self.service.drain_state()) {
            (_, DrainState::Draining | DrainState::Drained) => "draining",
            (NodeState::Active, _) => "active",
            (NodeState::Failed, _) => "failed",
            (NodeState::Syncing, _) => "syncing",
            (NodeState::Returning, _) => "returning",
            (NodeState::Joining, _) => "joining",
        }
    }

    /// The heartbeat (doc 06c): date, load, table version, state and the topics with ready messages
    /// (up to `max_topics`).
    pub fn keepalive(&self, max_topics: usize) -> Keepalive {
        let load: usize = self.lock().local_listeners.values().sum();
        let mut ready: Vec<ReadyCount> = self
            .messages
            .ready_counts()
            .into_iter()
            .map(|(topic, count)| ReadyCount { topic, count })
            .collect();
        ready.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.topic.cmp(&b.topic)));
        ready.truncate(max_topics);
        Keepalive {
            at: time::format_ms(self.service.clock().now_millis()),
            load: load as u64,
            version: self.service.membership().version().unwrap_or(0),
            state: self.state_name().to_owned(),
            ready,
        }
    }

    /// The snapshot sent to a new or returning channel (doc 13, section 12.9): the node's state, a
    /// `listeners` and a `ready` for each topic where the state is true.
    pub fn snapshot(&self) -> Vec<Signal> {
        let mut out = vec![Signal::NodeState {
            state: self.state_name().to_owned(),
        }];
        for topic in self.messages.ready_topics() {
            out.push(Signal::Ready {
                topic,
                has_ready: true,
            });
        }
        let listeners: Vec<String> = self.lock().local_listeners.keys().cloned().collect();
        for topic in listeners {
            out.push(Signal::Listeners {
                topic,
                has_listeners: true,
            });
        }
        let frozen = self
            .service
            .membership()
            .frozen_nodes(self.service.now(), self.service.rules().freezing_duration);
        for node in frozen {
            out.push(Signal::Frozen {
                node_id: node.node_id,
                frozen_at: node.frozen_at,
            });
        }
        out
    }

    /// Applies a peer's heartbeat: its topics with ready ones (the list is the complete state,
    /// so what is not on it stops applying) and the node's state.
    pub fn apply_keepalive(&self, from: &str, keepalive: &Keepalive) {
        {
            let mut tables = self.lock();
            tables.loads.insert(from.to_owned(), keepalive.load);
            let listed: HashSet<&str> = keepalive.ready.iter().map(|r| r.topic.as_str()).collect();
            for (topic, nodes) in &mut tables.ready {
                if !listed.contains(topic.as_str()) {
                    nodes.remove(from);
                }
            }
            for ready in &keepalive.ready {
                tables
                    .ready
                    .entry(ready.topic.clone())
                    .or_default()
                    .insert(from.to_owned(), Some(ready.count));
            }
        }
        self.apply_state(from, &keepalive.state);
    }

    fn apply_state(&self, from: &str, state: &str) {
        match state {
            "active" => {
                // The node that was syncing became ready: the member change that was waiting can proceed.
                self.service
                    .membership()
                    .set_state(from, MemberState::Active);
                self.lock().draining.remove(from);
            }
            "draining" => {
                self.lock().draining.insert(from.to_owned());
            }
            "failed" => {
                if let Some(channels) = self.channels() {
                    channels.report_failed(from, VoteReason::NodeFailed);
                }
            }
            _ => {}
        }
    }

    // ---- What this node says ----

    /// The topic's ready ones changed here: notifies the others and, if a message arrived, opens the cycle of
    /// notices to the nodes with a listener (doc 13, section 12.8).
    fn local_ready_changed(self: &Arc<Self>, topic: &str, has_ready: bool) {
        self.broadcast(Signal::Ready {
            topic: topic.to_owned(),
            has_ready,
        });
        if has_ready && self.lock().cycles.insert(topic.to_owned()) {
            let signals = self.clone();
            let topic = topic.to_owned();
            tokio::spawn(async move {
                signals.notify_cycle(&topic).await;
                signals.lock().cycles.remove(&topic);
            });
        }
    }

    /// The party responsible for the topic's notices (one per topic, active only while there is a message to notify and
    /// a listener on some node): accumulates the ready ones for `cluster.notify.window`, sends **one** notice with the
    /// quantity to **one** node with a listener, waits `cluster.notify.wait` for it to come fetch and, if it
    /// did not come (or came and some were left), moves to the next, until the messages or the nodes run out.
    async fn notify_cycle(&self, topic: &str) {
        let rules = self.service.rules();
        let cancel = self.service.cancel_token().clone();
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(rules.notify_window) => {}
        }
        let mut tried: HashSet<String> = HashSet::new();
        loop {
            let count = self.messages.ready_count_of(topic);
            if count == 0 {
                return;
            }
            let Some(node) = self.next_listener(topic, &tried) else {
                return;
            };
            tried.insert(node.clone());
            let before = self.takes_of(topic, &node);
            self.service.telemetry().registry.add(
                &CLUSTER_NOTIFICATIONS,
                &[("taba.direction", "out"), ("taba.outcome", "sent")],
                1.0,
            );
            if let Some(channels) = self.channels() {
                channels.send(
                    &node,
                    Signal::Notify {
                        topic: topic.to_owned(),
                        count,
                    },
                );
            }
            // Waits for the node to come fetch; if it does not come in time, the next in turn.
            let deadline = tokio::time::Instant::now() + rules.notify_wait;
            while self.takes_of(topic, &node) == before && tokio::time::Instant::now() < deadline {
                tokio::select! {
                    () = cancel.cancelled() => return,
                    () = tokio::time::sleep(Duration::from_millis(2)) => {}
                }
            }
        }
    }

    /// The next node with a listener to notify: among those not yet tried in this cycle,
    /// rotating from the last notified. Excluding those in drain and those that left the table.
    fn next_listener(&self, topic: &str, tried: &HashSet<String>) -> Option<String> {
        let mut tables = self.lock();
        let me = self.me();
        let mut nodes: Vec<String> = tables
            .listeners
            .get(topic)
            .map(|set| set.iter().filter(|n| **n != me).cloned().collect())
            .unwrap_or_default();
        nodes.sort();
        nodes.retain(|n| {
            !tried.contains(n)
                && self.service.membership().is_active(n)
                && !tables.draining.contains(n)
        });
        if nodes.is_empty() {
            return None;
        }
        let after = tables.last_notified.get(topic).cloned().unwrap_or_default();
        let next = nodes
            .iter()
            .find(|n| **n > after)
            .unwrap_or(&nodes[0])
            .clone();
        tables.last_notified.insert(topic.to_owned(), next.clone());
        Some(next)
    }

    fn takes_of(&self, topic: &str, node: &str) -> u64 {
        self.lock()
            .takes
            .get(&(topic.to_owned(), node.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    /// A node asked for a `take` of the topic here: the notice sent to it was answered.
    pub fn took(&self, topic: &str, node: &str) {
        *self
            .lock()
            .takes
            .entry((topic.to_owned(), node.to_owned()))
            .or_default() += 1;
    }

    /// The `take` that a received notice provoked finished: `fetched` if it brought messages, `empty`
    /// if the notice was an old hint (doc 13, section 12.8). Counts once per notice.
    pub fn notified_outcome(&self, topic: &str, node: &str, fetched: bool) {
        let was_notified = self
            .lock()
            .notified
            .get_mut(topic)
            .is_some_and(|nodes| nodes.remove(node));
        if was_notified {
            self.service.telemetry().registry.add(
                &CLUSTER_NOTIFICATIONS,
                &[
                    ("taba.direction", "in"),
                    ("taba.outcome", if fetched { "fetched" } else { "empty" }),
                ],
                1.0,
            );
        }
    }

    /// A `GET` started waiting for a message of the topic on this node. On the first (0 → 1) it notifies the other
    /// nodes, once only, no matter how many wait.
    pub fn listener(self: &Arc<Self>, topic: &str) -> ListenerGuard {
        let first = {
            let mut tables = self.lock();
            let count = tables.local_listeners.entry(topic.to_owned()).or_default();
            *count += 1;
            *count == 1
        };
        if first {
            self.broadcast(Signal::Listeners {
                topic: topic.to_owned(),
                has_listeners: true,
            });
        }
        ListenerGuard {
            signals: self.clone(),
            topic: topic.to_owned(),
        }
    }

    fn listener_left(&self, topic: &str) {
        let last = {
            let mut tables = self.lock();
            let Some(count) = tables.local_listeners.get_mut(topic) else {
                return;
            };
            *count -= 1;
            let last = *count == 0;
            if last {
                tables.local_listeners.remove(topic);
            }
            last
        };
        if last {
            self.broadcast(Signal::Listeners {
                topic: topic.to_owned(),
                has_listeners: false,
            });
        }
    }

    /// The nodes that say they have a listener for the topic.
    pub fn listener_nodes(&self, topic: &str) -> Vec<String> {
        let mut nodes: Vec<String> = self
            .lock()
            .listeners
            .get(topic)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default();
        nodes.sort();
        nodes
    }

    /// The quantity of ready messages the topic's last notice brought.
    pub fn last_notice_count(&self, topic: &str) -> Option<u64> {
        self.lock().notices.get(topic).copied()
    }

    /// How many `GET`s wait for the topic on this node.
    pub fn local_listener_count(&self, topic: &str) -> usize {
        self.lock().local_listeners.get(topic).copied().unwrap_or(0)
    }

    /// The drain was cancelled: the peers go back to asking this node for `take`.
    pub fn announce_active(&self) {
        self.broadcast(Signal::NodeState {
            state: "active".to_owned(),
        });
    }

    /// A node was excluded by vote and is in freezing: all the peers get to know it, including the one
    /// that was down at the commit (the outgoing queue keeps the frame until the channel comes back).
    pub fn announce_frozen(&self, node_id: &str, frozen_at: u64) {
        self.broadcast(Signal::Frozen {
            node_id: node_id.to_owned(),
            frozen_at,
        });
    }

    /// A member change took effect on this node: all the peers get to know it, including the one that
    /// lost the register or the commit (the outgoing queue keeps the frame until the `confirm`).
    pub fn announce_membership(&self, operation_id: &str, version: u64, change: &Change) {
        self.broadcast(Signal::Membership {
            operation_id: operation_id.to_owned(),
            version,
            change: change.clone(),
        });
    }

    /// A definition applied on this node (as owner or heir): all the peers receive it through the channel, the
    /// one that was down when it comes back (the queue keeps the frame, and the next of the same definition
    /// replaces it).
    pub fn announce_definition(&self, change: DefinitionApply) {
        self.broadcast(Signal::Definition {
            kind: change.kind,
            key: change.key,
            version: change.version,
            deleted: change.deleted,
            body: change.body,
        });
    }

    /// This node entered drain: the peers stop asking it for `take`.
    pub fn announce_draining(&self) {
        self.broadcast(Signal::NodeState {
            state: "draining".to_owned(),
        });
    }

    /// This node entered failure: the others are free to vote on excluding it.
    pub fn announce_failed(&self) {
        self.broadcast(Signal::NodeState {
            state: "failed".to_owned(),
        });
    }

    /// Notifies **all** the peers of this node's state and waits for the `confirm`s (the node that became ready
    /// needs the others to know, so they go back to accepting member changes and choose it as a
    /// destination).
    pub async fn announce_state_reliably(&self, state: &str) {
        if let Some(channels) = self.channels() {
            channels
                .broadcast_confirmed(
                    Signal::NodeState {
                        state: state.to_owned(),
                    },
                    self.service.rules().commit_timeout,
                )
                .await;
        }
    }

    /// Forgets what is known about the other nodes (the node comes back as a new node).
    pub fn reset(&self) {
        *self.lock() = Tables::default();
    }

    // ---- What the others say ----

    /// A signal arrived from another node.
    pub fn receive(&self, frame: SignalFrame) {
        let SignalFrame { from, signal } = frame;
        match signal {
            Signal::Ready { topic, has_ready } => {
                let mut tables = self.lock();
                let set = tables.ready.entry(topic).or_default();
                if has_ready {
                    // A quantity the heartbeat already brought is kept: this signal does not carry one.
                    set.entry(from).or_insert(None);
                } else {
                    set.remove(&from);
                }
            }
            Signal::Listeners {
                topic,
                has_listeners,
            } => {
                let mut tables = self.lock();
                let set = tables.listeners.entry(topic).or_default();
                if has_listeners {
                    set.insert(from);
                } else {
                    set.remove(&from);
                }
            }
            Signal::Notify { topic, count } => {
                self.service.telemetry().registry.add(
                    &CLUSTER_NOTIFICATIONS,
                    &[("taba.direction", "in"), ("taba.outcome", "received")],
                    1.0,
                );
                // Marks who notified (the `take` goes to it first) and wakes the waiting `GET`s,
                // as many as there are.
                let waiting = {
                    let mut tables = self.lock();
                    tables
                        .notified
                        .entry(topic.clone())
                        .or_default()
                        .insert(from.clone());
                    tables.notices.insert(topic.clone(), count);
                    tables.hints.insert(topic.clone(), from);
                    tables.local_listeners.get(&topic).copied().unwrap_or(0)
                };
                let wake = waiting
                    .min(usize::try_from(count).unwrap_or(usize::MAX))
                    .max(1);
                if waiting > 0 {
                    self.messages.wake_listeners(&topic, wake);
                }
            }
            Signal::NodeState { state } => self.apply_state(&from, &state),
            Signal::Frozen { node_id, frozen_at } => self.apply_frozen(node_id, frozen_at),
            Signal::Membership {
                operation_id,
                version,
                change,
            } => self
                .service
                .learn_membership(&operation_id, version, change),
            Signal::Definition {
                kind,
                key,
                version,
                deleted,
                body,
            } => self.service.learn_definition(DefinitionApply {
                kind,
                key,
                version,
                deleted,
                body,
                relay: false,
            }),
        }
    }

    /// A node's freezing after exclusion, said by a peer: whoever did not know it stores it and passes it on
    /// to everyone; the echo (already known, expired or from the node itself) dies here.
    fn apply_frozen(&self, node_id: String, frozen_at: u64) {
        if node_id == self.me() {
            return;
        }
        let learned = self.service.membership().learn_frozen(
            &node_id,
            frozen_at,
            self.service.now(),
            self.service.rules().freezing_duration,
        );
        if learned {
            tracing::info!(node_id = %node_id, "node in freezing (said by a peer)");
            self.announce_frozen(&node_id, frozen_at);
        }
    }

    /// The nodes that have ready messages of the topic, in the order of a draw weighted by how many each said it has
    /// (uniform while any of them did not say), minus this one, those in drain, those that left the table and
    /// those with a down channel: asking a peer that does not answer for a `take` costs the call deadline
    /// (5 s) on each attempt, and the reopened channel brings the snapshot back.
    /// The node that notified last comes first.
    pub fn ready_peers(&self, topic: &str) -> Vec<String> {
        // The tables lock is released before asking the membership and the channels: `build_batch` holds a
        // channel's peer lock while it calls the signals, so the opposite order would deadlock.
        let me = self.me();
        let (candidates, hint): (Vec<(String, Option<u64>)>, Option<String>) = {
            let tables = self.lock();
            let candidates = tables
                .ready
                .get(topic)
                .map(|set| {
                    set.iter()
                        .filter(|(n, _)| **n != me && !tables.draining.contains(*n))
                        .map(|(n, count)| (n.clone(), *count))
                        .collect()
                })
                .unwrap_or_default();
            (candidates, tables.hints.get(topic).cloned())
        };
        let candidates: Vec<(String, Option<u64>)> = candidates
            .into_iter()
            .filter(|(n, _)| {
                self.service.membership().is_active(n)
                    && self.channels().is_none_or(|c| c.is_live(n))
            })
            .collect();
        let mut peers = draw_order(candidates, |bound| {
            let mut bytes = [0u8; 8];
            getrandom::fill(&mut bytes).expect("the system has no source of random numbers");
            u64::from_le_bytes(bytes) % bound
        });
        if let Some(hint) = hint
            && let Some(position) = peers.iter().position(|n| *n == hint)
        {
            let hinted = peers.remove(position);
            peers.insert(0, hinted);
        }
        peers
    }

    /// The load indicator of the node's last heartbeat (0 while none arrived).
    pub fn peer_load(&self, node: &str) -> u64 {
        self.lock().loads.get(node).copied().unwrap_or(0)
    }

    /// The `take` on a node came back empty: the hint that it had messages was old.
    pub fn mark_empty(&self, topic: &str, node: &str) {
        let mut tables = self.lock();
        if let Some(set) = tables.ready.get_mut(topic) {
            set.remove(node);
        }
        if tables.hints.get(topic).is_some_and(|n| n == node) {
            tables.hints.remove(topic);
        }
    }

    /// Does the node have the caller in drain? (to answer an empty `take`)
    pub fn is_draining(&self, node: &str) -> bool {
        self.lock().draining.contains(node)
    }

    // ---- Membros ----

    /// The member table changed: forgets what was known of those that left and adjusts the channels (the one of
    /// whoever joined opens with the snapshot; the one of whoever left is dropped with the queues).
    pub fn membership_changed(&self) {
        let Some(table) = self.service.membership().table() else {
            return;
        };
        let current: HashSet<String> = table.members.iter().map(|m| m.node_id.clone()).collect();
        {
            let mut tables = self.lock();
            for set in tables.ready.values_mut() {
                set.retain(|n, _| current.contains(n));
            }
            for set in tables.listeners.values_mut() {
                set.retain(|n| current.contains(n));
            }
            tables.draining.retain(|n| current.contains(n));
            tables.loads.retain(|n, _| current.contains(n));
        }
        if let Some(channels) = self.channels() {
            channels.sync_members();
        }
    }
}

/// Orders the candidates by successive draws without replacement, each with the chance of its quantity of ready
/// messages among those left. When some candidate has no quantity the draw is uniform. `below(n)` returns a
/// number in `0..n`.
fn draw_order(
    mut candidates: Vec<(String, Option<u64>)>,
    mut below: impl FnMut(u64) -> u64,
) -> Vec<String> {
    let weighted = candidates.iter().all(|(_, count)| count.is_some());
    let weight = |count: &Option<u64>| {
        if weighted {
            count.unwrap_or(1).max(1)
        } else {
            1
        }
    };
    let mut order = Vec::with_capacity(candidates.len());
    while !candidates.is_empty() {
        let total = candidates
            .iter()
            .map(|(_, c)| weight(c))
            .fold(0, u64::saturating_add);
        let mut point = below(total);
        let position = candidates
            .iter()
            .position(|(_, c)| {
                let w = weight(c);
                if point < w {
                    true
                } else {
                    point -= w;
                    false
                }
            })
            .unwrap_or(candidates.len() - 1);
        order.push(candidates.remove(position).0);
    }
    order
}

#[cfg(test)]
mod tests;
