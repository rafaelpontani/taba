//! O nó gestor das filas voláteis (doc 15, seções 14.5 e 14.6).
//!
//! O gestor de uma fila é o primário da posição do hash do `id` dela no anel, então a lista de
//! filas gerenciadas é **derivada** a cada volta da tarefa, não guardada: ela muda sozinha com a
//! tabela de membros e a de tópicos. O gestor só cuida da vida do tópico. No vencimento ele
//! pergunta a **todos** os nós há quanto tempo foi o último `GET` (`GET /cluster/topics/{id}/idle`)
//! e, se nenhum teve `GET` dentro do `idleTimeout`, exclui a fila pelo mesmo registro + commit do
//! `DELETE /topics/{topic}`, com o `id` esperado.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::protocol::{Request, Response};
use super::service::ClusterService;
use super::state::{DrainState, NodeState};
use crate::core::error::ErrorCode;
use crate::core::ring::topic_position;
use crate::engine::topics::{Topics, VolatileQueue};

/// Quando o gestor volta a olhar uma fila que gerencia.
#[derive(Debug, Clone, Copy)]
struct Entry {
    due_ms: u64,
}

pub struct VolatileManager {
    service: Arc<ClusterService>,
    topics: Arc<Topics>,
    /// Os prazos das filas que este nó gerencia, por `id` do tópico.
    entries: Mutex<HashMap<String, Entry>>,
    /// As filas com uma verificação em andamento: uma por vez cada.
    running: Mutex<HashSet<String>>,
}

/// O resultado de perguntar a todos os nós.
enum Check {
    /// A menor duração desde o último `GET`, em milissegundos.
    Idle(u64),
    /// Algum nó não respondeu: na dúvida, a fila continua viva.
    Blocked,
}

/// O atraso estável de uma fila dentro de uma janela de `window_ms`: de 0 a `window_ms`.
fn jitter_in(window_ms: u64, topic_id: &str) -> u64 {
    if window_ms == 0 {
        return 0;
    }
    crate::core::ring::position(topic_id.as_bytes()) % (window_ms + 1)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl VolatileManager {
    pub fn new(service: Arc<ClusterService>, topics: Arc<Topics>) -> Arc<Self> {
        Arc::new(Self {
            service,
            topics,
            entries: Mutex::new(HashMap::new()),
            running: Mutex::new(HashSet::new()),
        })
    }

    /// A tarefa do gestor, até o cancelamento.
    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        let mut ticker = tokio::time::interval(Duration::from_millis(200));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                _ = ticker.tick() => self.tick(),
            }
        }
    }

    /// Os `id` das filas que este nó gerencia agora.
    pub fn managed(&self) -> Vec<VolatileQueue> {
        // Só um nó pronto gerencia: até lá, nenhuma fila do trecho dele é excluída, o mesmo
        // efeito de um gestor fora do ar (doc 15, seção 14.6).
        if self.service.node_state() != NodeState::Active
            || self.service.drain_state() != DrainState::Active
        {
            return Vec::new();
        }
        let me = self.service.node_id();
        self.topics
            .volatile_queues()
            .into_iter()
            .filter(|queue| {
                self.service
                    .membership()
                    .ring()
                    .primary(topic_position(&queue.id))
                    .is_some_and(|primary| primary == me)
            })
            .collect()
    }

    /// Uma volta: esquece as filas que deixaram de ser dele, agenda as que passou a gerenciar
    /// (verificação imediata, espalhada pelo jitter) e dispara as vencidas.
    fn tick(self: &Arc<Self>) {
        let now = self.service.clock().now_millis();
        let managed = self.managed();
        let ids: HashSet<&str> = managed.iter().map(|q| q.id.as_str()).collect();
        let mut due = Vec::new();
        {
            let mut entries = lock(&self.entries);
            entries.retain(|id, _| ids.contains(id.as_str()));
            for queue in managed {
                let entry = entries.entry(queue.id.clone()).or_insert_with(|| Entry {
                    due_ms: now + self.jitter_ms(&queue.id),
                });
                if now >= entry.due_ms {
                    due.push(queue);
                }
            }
        }
        for queue in due {
            if !lock(&self.running).insert(queue.id.clone()) {
                continue;
            }
            let me = self.clone();
            tokio::spawn(async move {
                me.verify(&queue).await;
                lock(&me.running).remove(&queue.id);
            });
        }
    }

    /// Um atraso estável por fila, de 0 até `topics.volatile.takeover.jitter`: muitas filas que
    /// trocam de gestor de uma vez não consultam todas no mesmo instante.
    fn jitter_ms(&self, topic_id: &str) -> u64 {
        jitter_in(
            self.service.rules().volatile_jitter.as_millis() as u64,
            topic_id,
        )
    }

    fn schedule(&self, topic_id: &str, due_ms: u64) {
        lock(&self.entries).insert(topic_id.to_owned(), Entry { due_ms });
    }

    /// A verificação final (doc 15, seção 14.5, passos 2 a 4).
    async fn verify(self: &Arc<Self>, queue: &VolatileQueue) {
        let now = self.service.clock().now_millis();
        let timeout_ms = queue.idle_timeout_secs.saturating_mul(1000);
        let idle_ms = match self.ask_everyone(&queue.id).await {
            Check::Idle(ms) => ms.min(now.saturating_sub(queue.created_ms)),
            Check::Blocked => {
                tracing::warn!(
                    event.name = "taba.volatile.check_blocked",
                    messaging.destination.name = %queue.name,
                    topic.id = %queue.id,
                    "um nó não respondeu à consulta de ociosidade: a fila fica viva por ora"
                );
                let retry = self.service.rules().volatile_retry_interval;
                self.schedule(&queue.id, now + retry.as_millis() as u64);
                return;
            }
        };
        if idle_ms < timeout_ms {
            // Alguém teve `GET` mais recente: reagenda para o que falta.
            self.schedule(&queue.id, now + (timeout_ms - idle_ms));
            return;
        }
        match self.topics.delete_expected(&queue.name, &queue.id).await {
            Ok(()) => {
                lock(&self.entries).remove(&queue.id);
                tracing::info!(
                    event.name = "taba.volatile.deleted",
                    messaging.destination.name = %queue.name,
                    topic.id = %queue.id,
                    "fila volátil excluída por ociosidade"
                );
            }
            // Excluída (ou recriada com outro `id`) no meio do caminho: nada a fazer.
            Err(e) if e.code == ErrorCode::TopicNotFound => {
                lock(&self.entries).remove(&queue.id);
            }
            Err(e) => {
                tracing::warn!(
                    topic = %queue.name,
                    error = %e,
                    "não foi possível excluir a fila ociosa: nova tentativa depois"
                );
                let retry = self.service.rules().volatile_retry_interval;
                self.schedule(&queue.id, now + retry.as_millis() as u64);
            }
        }
    }

    /// Pergunta a todos os membros, este inclusive, há quanto tempo foi o último `GET`.
    async fn ask_everyone(self: &Arc<Self>, topic_id: &str) -> Check {
        let Some(table) = self.service.membership().table() else {
            return Check::Blocked;
        };
        let mut set = JoinSet::new();
        for member in table.members.clone() {
            let service = self.service.clone();
            let request = Request::Idle {
                topic_id: topic_id.to_owned(),
            };
            set.spawn(async move { service.call_member(&member, request).await });
        }
        let mut least = u64::MAX;
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(Response::Idle(idle))) => {
                    least = least.min(idle.idle_seconds.saturating_mul(1000));
                }
                // Um nó que não conhece o tópico ainda (o commit não chegou a ele) não permite
                // concluir nada.
                _ => return Check::Blocked,
            }
        }
        Check::Idle(least)
    }
}

#[cfg(test)]
mod tests;
