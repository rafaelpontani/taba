//! Os sinais entre nós (doc 13, seção 12.8): quais nós têm mensagens prontas de um tópico, quais
//! têm ouvintes de long polling e quais estão em drain. Cada nó guarda isso **em memória** e só
//! manda sinal nas **mudanças** de estado; com a tabela, a decisão de a quem pedir um `take` ou
//! avisar é local.
//!
//! Enquanto o canal WebSocket não existe, os sinais vão como chamadas comuns do `Transport`, sem
//! `seq`, confirmação nem fila de saída (`protocol::Signal`): um sinal perdido custa só uma dica
//! velha, que o `take` confere.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::cluster::members::Member;
use crate::cluster::protocol::{Request, Signal, SignalFrame};
use crate::cluster::service::ClusterService;
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
    /// Os membros que este nó já conhecia, para mandar o instantâneo só aos novos.
    known: HashSet<String>,
}

pub struct Signals {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
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
            tables: Mutex::new(Tables::default()),
            ready_tx,
            ready_rx: Mutex::new(Some(ready_rx)),
        })
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
        // Sem o canal, o estado é repetido de tempos em tempos: um sinal perdido se corrige sozinho.
        let mut ticker = tokio::time::interval(self.service.rules().keepalive_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => self.repeat_state(),
                item = rx.recv() => {
                    let Some((topic, has_ready)) = item else { return };
                    self.local_ready_changed(&topic, has_ready);
                }
            }
        }
    }

    /// Repete a todos os outros o que este nó tem: os tópicos com mensagens prontas e os com
    /// ouvintes (o que o sinal de vida do canal carrega, doc 13, seção 12.8).
    fn repeat_state(&self) {
        let listeners: Vec<String> = self.lock().local_listeners.keys().cloned().collect();
        for topic in self.messages.ready_topics() {
            self.broadcast(Signal::Ready {
                topic,
                has_ready: true,
            });
        }
        for topic in listeners {
            self.broadcast(Signal::Listeners {
                topic,
                has_listeners: true,
            });
        }
    }

    fn me(&self) -> String {
        self.service.node_id().to_owned()
    }

    fn others(&self) -> Vec<Member> {
        self.service
            .membership()
            .table()
            .map(|t| {
                t.members
                    .into_iter()
                    .filter(|m| !self.service.is_self(m))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Manda o sinal a todos os outros membros, sem esperar (melhor esforço).
    fn broadcast(&self, signal: Signal) {
        let frame = SignalFrame {
            from: self.me(),
            signal,
        };
        for member in self.others() {
            self.send_to(member, frame.clone());
        }
    }

    fn send_to(&self, member: Member, frame: SignalFrame) {
        let service = self.service.clone();
        tokio::spawn(async move {
            let _ = service.call_member(&member, Request::Signal(frame)).await;
        });
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
                .retain(|n| self.service.membership().is_member(n) && !tables.draining.contains(n));
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
        let Some(member) = self
            .service
            .membership()
            .table()
            .and_then(|t| t.member(&node).cloned())
        else {
            return;
        };
        self.service.telemetry().registry.add(
            &CLUSTER_NOTIFICATIONS,
            &[("taba.direction", "out"), ("taba.outcome", "sent")],
            1.0,
        );
        self.send_to(
            member,
            SignalFrame {
                from: self.me(),
                signal: Signal::Notify {
                    topic: topic.to_owned(),
                    count: 1,
                },
            },
        );
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

    /// Esquece o que se sabe dos outros nós (o nó volta como nó novo).
    pub fn reset(&self) {
        *self.lock() = Tables::default();
    }

    /// Este nó entrou em drain: os pares deixam de pedir `take` a ele.
    pub fn announce_draining(&self) {
        self.broadcast(Signal::NodeState {
            state: "draining".to_owned(),
        });
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
            Signal::NodeState { state } => {
                let mut tables = self.lock();
                if state == "draining" {
                    tables.draining.insert(from);
                } else {
                    tables.draining.remove(&from);
                }
            }
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
                            && self.service.membership().is_member(n)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        // Sem a quantidade de prontas de cada nó (que o sinal de vida traria), o sorteio é uniforme.
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

    /// A tabela de membros mudou: manda o instantâneo dos sinais aos membros que este nó ainda não
    /// conhecia (o que o canal faria ao abrir, doc 13, seção 12.9) e esquece o que se sabia dos que
    /// saíram.
    pub fn membership_changed(&self) {
        let Some(table) = self.service.membership().table() else {
            return;
        };
        let current: HashSet<String> = table.members.iter().map(|m| m.node_id.clone()).collect();
        let (newcomers, listener_topics) = {
            let mut tables = self.lock();
            let newcomers: Vec<Member> = table
                .members
                .iter()
                .filter(|m| !tables.known.contains(&m.node_id) && !self.service.is_self(m))
                .cloned()
                .collect();
            tables.known = current.clone();
            for set in tables.ready.values_mut() {
                set.retain(|n| current.contains(n));
            }
            for set in tables.listeners.values_mut() {
                set.retain(|n| current.contains(n));
            }
            tables.draining.retain(|n| current.contains(n));
            let listener_topics: Vec<String> = tables.local_listeners.keys().cloned().collect();
            (newcomers, listener_topics)
        };
        if newcomers.is_empty() {
            return;
        }
        let ready_topics = self.messages.ready_topics();
        for member in newcomers {
            for topic in &ready_topics {
                self.send_to(
                    member.clone(),
                    SignalFrame {
                        from: self.me(),
                        signal: Signal::Ready {
                            topic: topic.clone(),
                            has_ready: true,
                        },
                    },
                );
            }
            for topic in &listener_topics {
                self.send_to(
                    member.clone(),
                    SignalFrame {
                        from: self.me(),
                        signal: Signal::Listeners {
                            topic: topic.clone(),
                            has_listeners: true,
                        },
                    },
                );
            }
        }
    }
}
