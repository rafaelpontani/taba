//! Os sinais entre nós (doc 13, seção 12.8): quais nós têm mensagens prontas de um tópico, quais
//! têm ouvintes de long polling e quais estão em drain. Cada nó guarda isso **em memória** e só
//! manda sinal nas **mudanças** de estado; com a tabela, a decisão de a quem pedir um `take` ou
//! avisar é local.
//!
//! Os sinais viajam pelo canal entre nós (`cluster::channel`): fila de saída com `seq`,
//! confirmação e reenvio, mais o sinal de vida, que repete os tópicos com mensagens prontas e o
//! estado do nó (o que corrige um sinal que se perdeu).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::mpsc;

use crate::cluster::channel::Channels;
use crate::cluster::members::MemberState;
use crate::cluster::protocol::{Keepalive, ReadyCount, Signal, SignalFrame, VoteReason};
use crate::cluster::service::ClusterService;
use crate::cluster::state::{DrainState, NodeState};
use crate::core::time;
use crate::engine::messages::Messages;
use crate::telemetry::metrics::CLUSTER_NOTIFICATIONS;

#[derive(Default)]
struct Tables {
    /// Tópico → nós que têm mensagens prontas.
    ready: HashMap<String, HashSet<String>>,
    /// Tópico → nós que têm ouvintes.
    listeners: HashMap<String, HashSet<String>>,
    /// Nós em drain: ninguém pede `take` a eles.
    draining: HashSet<String>,
    /// Tópico → quantos `GET` esperam aqui.
    local_listeners: HashMap<String, usize>,
    /// Tópico → o nó que avisou por último: o `GET` acordado tenta o `take` nele primeiro.
    hints: HashMap<String, String>,
    /// O último nó avisado de cada tópico, para revezar entre os que têm ouvinte.
    last_notified: HashMap<String, String>,
}

pub struct Signals {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    channels: OnceLock<Arc<Channels>>,
    tables: Mutex<Tables>,
    /// O motor avisa aqui (sem bloquear) quando o tópico passa a ter ou deixa de ter prontas.
    ready_tx: mpsc::UnboundedSender<(String, bool)>,
    ready_rx: Mutex<Option<mpsc::UnboundedReceiver<(String, bool)>>>,
}

/// Enquanto existe, há um `GET` em espera por mensagem do tópico neste nó.
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

    /// Liga os sinais ao canal que os leva.
    pub fn set_channels(&self, channels: Arc<Channels>) {
        let _ = self.channels.set(channels);
    }

    pub fn channels(&self) -> Option<&Arc<Channels>> {
        self.channels.get()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tables> {
        self.tables.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// O que o motor chama (de dentro da trava do tópico) quando as prontas mudam.
    pub fn ready_sender(&self) -> mpsc::UnboundedSender<(String, bool)> {
        self.ready_tx.clone()
    }

    /// A tarefa que transforma as mudanças de prontas em sinais, até o nó parar.
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

    /// Manda o sinal a todos os outros membros pela fila do canal.
    fn broadcast(&self, signal: Signal) {
        if let Some(channels) = self.channels() {
            channels.broadcast(signal);
        }
    }

    // ---- O sinal de vida e o instantâneo ----

    /// O estado do nó como os pares o veem.
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

    /// O sinal de vida (doc 06c): data, carga, versão da tabela, estado e os tópicos com mensagens
    /// prontas (até `max_topics`).
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

    /// O instantâneo que se manda a um canal novo ou que voltou (doc 13, seção 12.9): o estado do
    /// nó, um `listeners` e um `ready` para cada tópico em que o estado é verdadeiro.
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
        out
    }

    /// Aplica o sinal de vida de um par: os tópicos com prontas dele (a lista é o estado completo,
    /// então o que não está nela deixa de valer) e o estado do nó.
    pub fn apply_keepalive(&self, from: &str, keepalive: &Keepalive) {
        {
            let mut tables = self.lock();
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
                    .insert(from.to_owned());
            }
        }
        self.apply_state(from, &keepalive.state);
    }

    fn apply_state(&self, from: &str, state: &str) {
        match state {
            "active" => {
                // O nó que sincronizava ficou pronto: a mudança de membros que esperava pode seguir.
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

    // ---- O que este nó diz ----

    /// As prontas do tópico mudaram aqui: avisa os outros e, se chegou mensagem, avisa um nó com
    /// ouvinte (doc 13, seção 12.8).
    fn local_ready_changed(&self, topic: &str, has_ready: bool) {
        self.broadcast(Signal::Ready {
            topic: topic.to_owned(),
            has_ready,
        });
        if has_ready {
            self.notify_one_listener(topic);
        }
    }

    /// Há mensagem nova pronta neste nó (o dono) e algum outro nó tem ouvinte: avisa **um** deles,
    /// revezando entre os que têm. Ouvinte local é acordado pelo próprio motor.
    pub fn notify_one_listener(&self, topic: &str) {
        let target = {
            let mut tables = self.lock();
            let me = self.me();
            let mut nodes: Vec<String> = tables
                .listeners
                .get(topic)
                .map(|set| set.iter().filter(|n| **n != me).cloned().collect())
                .unwrap_or_default();
            nodes.sort();
            nodes
                .retain(|n| self.service.membership().is_active(n) && !tables.draining.contains(n));
            if nodes.is_empty() {
                None
            } else {
                let after = tables.last_notified.get(topic).cloned().unwrap_or_default();
                let next = nodes
                    .iter()
                    .find(|n| **n > after)
                    .unwrap_or(&nodes[0])
                    .clone();
                tables.last_notified.insert(topic.to_owned(), next.clone());
                Some(next)
            }
        };
        let Some(node) = target else { return };
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
                    count: 1,
                },
            );
        }
    }

    /// Um `GET` começou a esperar mensagem do tópico neste nó. Na primeira (0 → 1) avisa os outros
    /// nós, uma vez só, não importa quantos esperem.
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

    /// Quantos `GET` esperam o tópico neste nó.
    pub fn local_listener_count(&self, topic: &str) -> usize {
        self.lock().local_listeners.get(topic).copied().unwrap_or(0)
    }

    /// O drain foi cancelado: os pares voltam a pedir `take` a este nó.
    pub fn announce_active(&self) {
        self.broadcast(Signal::NodeState {
            state: "active".to_owned(),
        });
    }

    /// Este nó entrou em drain: os pares deixam de pedir `take` a ele.
    pub fn announce_draining(&self) {
        self.broadcast(Signal::NodeState {
            state: "draining".to_owned(),
        });
    }

    /// Este nó entrou em falha: os outros ficam liberados para votar a exclusão dele.
    pub fn announce_failed(&self) {
        self.broadcast(Signal::NodeState {
            state: "failed".to_owned(),
        });
    }

    /// Avisa **todos** os pares do estado deste nó e espera os `confirm` (o nó que ficou pronto
    /// precisa que os outros saibam, para voltarem a aceitar mudanças de membros e o escolherem como
    /// destino).
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

    /// Esquece o que se sabe dos outros nós (o nó volta como nó novo).
    pub fn reset(&self) {
        *self.lock() = Tables::default();
    }

    // ---- O que os outros dizem ----

    /// Um sinal chegou de outro nó.
    pub fn receive(&self, frame: SignalFrame) {
        let SignalFrame { from, signal } = frame;
        match signal {
            Signal::Ready { topic, has_ready } => {
                let mut tables = self.lock();
                let set = tables.ready.entry(topic).or_default();
                if has_ready {
                    set.insert(from);
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
                // Marca quem avisou (o `take` vai primeiro a ele) e acorda os `GET` que esperam,
                // tantos quantos são.
                let waiting = {
                    let mut tables = self.lock();
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
        }
    }

    /// Os nós que têm mensagens prontas do tópico, sem este, os em drain e os que saíram da tabela.
    /// O nó que avisou por último vem primeiro.
    pub fn ready_peers(&self, topic: &str) -> Vec<String> {
        let tables = self.lock();
        let me = self.me();
        let mut peers: Vec<String> = tables
            .ready
            .get(topic)
            .map(|set| {
                set.iter()
                    .filter(|n| {
                        **n != me
                            && !tables.draining.contains(*n)
                            && self.service.membership().is_active(n)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        // O sorteio entre os nós é uniforme: o sinal de vida só diz que há prontas.
        for i in (1..peers.len()).rev() {
            let mut bytes = [0u8; 8];
            getrandom::fill(&mut bytes).expect("o sistema não tem fonte de números aleatórios");
            peers.swap(i, (u64::from_le_bytes(bytes) % (i as u64 + 1)) as usize);
        }
        if let Some(hint) = tables.hints.get(topic)
            && let Some(position) = peers.iter().position(|n| n == hint)
        {
            let hinted = peers.remove(position);
            peers.insert(0, hinted);
        }
        peers
    }

    /// O `take` a um nó voltou vazio: a dica de que ele tinha mensagens estava velha.
    pub fn mark_empty(&self, topic: &str, node: &str) {
        let mut tables = self.lock();
        if let Some(set) = tables.ready.get_mut(topic) {
            set.remove(node);
        }
        if tables.hints.get(topic).is_some_and(|n| n == node) {
            tables.hints.remove(topic);
        }
    }

    /// O nó tem o chamador em drain? (para responder `take` vazio)
    pub fn is_draining(&self, node: &str) -> bool {
        self.lock().draining.contains(node)
    }

    // ---- Membros ----

    /// A tabela de membros mudou: esquece o que se sabia dos que saíram e acerta os canais (o de
    /// quem entrou abre com o instantâneo; o de quem saiu é descartado com as filas).
    pub fn membership_changed(&self) {
        let Some(table) = self.service.membership().table() else {
            return;
        };
        let current: HashSet<String> = table.members.iter().map(|m| m.node_id.clone()).collect();
        {
            let mut tables = self.lock();
            for set in tables.ready.values_mut() {
                set.retain(|n| current.contains(n));
            }
            for set in tables.listeners.values_mut() {
                set.retain(|n| current.contains(n));
            }
            tables.draining.retain(|n| current.contains(n));
        }
        if let Some(channels) = self.channels() {
            channels.sync_members();
        }
    }
}
