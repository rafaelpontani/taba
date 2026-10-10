//! A fila de saída de um canal (doc 13, seção 12.9): os quadros com `seq` que ainda não foram
//! confirmados pelo outro lado. Com o canal fora ela acumula; os sinais de **estado** do mesmo
//! tópico se substituem (só o último importa), os avisos velhos são descartados, e a fila que passa
//! do tamanho ou da idade máximos faz do nó do outro lado um nó **falho** (a votação de exclusão é
//! aberta sem esperar o fim das reconexões).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::cluster::protocol::{ChannelFrame, Signal};

/// Quanto tempo um aviso de mensagem nova vale: é só uma dica, e uma velha só atrapalha.
pub const NOTIFY_TTL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
struct Queued {
    seq: u64,
    signal: Signal,
    at: Instant,
}

/// O que uma operação da fila descartou, para a métrica `taba.cluster.channel.queue.dropped`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dropped {
    pub coalesced: usize,
    pub expired: usize,
}

#[derive(Debug)]
pub struct OutQueue {
    items: VecDeque<Queued>,
    next_seq: u64,
    max_size: usize,
    max_age: Duration,
}

/// A chave de um sinal de estado: o que o substitui na fila. Aviso não tem chave.
fn state_key(signal: &Signal) -> Option<(u8, &str)> {
    match signal {
        Signal::Listeners { topic, .. } => Some((0, topic)),
        Signal::Ready { topic, .. } => Some((1, topic)),
        Signal::NodeState { .. } => Some((2, "")),
        Signal::Frozen { node_id, .. } => Some((3, node_id)),
        Signal::Notify { .. } => None,
    }
}

impl OutQueue {
    pub fn new(max_size: usize, max_age: Duration) -> Self {
        Self {
            items: VecDeque::new(),
            next_seq: 1,
            max_size,
            max_age,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Enfileira o sinal e devolve o `seq` e quantos quadros velhos ele substituiu.
    pub fn push(&mut self, signal: Signal, now: Instant) -> (u64, Dropped) {
        let mut dropped = Dropped::default();
        if let Some(key) = state_key(&signal) {
            let before = self.items.len();
            self.items
                .retain(|queued| state_key(&queued.signal) != Some(key));
            dropped.coalesced = before - self.items.len();
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.items.push_back(Queued {
            seq,
            signal,
            at: now,
        });
        (seq, dropped)
    }

    /// Descarta os avisos que passaram de [`NOTIFY_TTL`].
    pub fn expire(&mut self, now: Instant) -> Dropped {
        let before = self.items.len();
        self.items.retain(|queued| {
            !matches!(queued.signal, Signal::Notify { .. })
                || now.saturating_duration_since(queued.at) < NOTIFY_TTL
        });
        Dropped {
            coalesced: 0,
            expired: before - self.items.len(),
        }
    }

    /// Os quadros ainda sem confirmação, do mais antigo ao mais novo, no máximo `limit`. Um quadro
    /// reenviado leva o mesmo `seq`.
    pub fn pending(&self, limit: usize) -> Vec<ChannelFrame> {
        self.items
            .iter()
            .take(limit)
            .map(|queued| ChannelFrame {
                seq: queued.seq,
                signal: queued.signal.clone(),
            })
            .collect()
    }

    /// Os quadros ainda sem confirmação com `seq` maior que `after`, no máximo `limit`: o que uma
    /// conexão ainda não levou.
    pub fn pending_after(&self, after: u64, limit: usize) -> Vec<ChannelFrame> {
        self.items
            .iter()
            .filter(|queued| queued.seq > after)
            .take(limit)
            .map(|queued| ChannelFrame {
                seq: queued.seq,
                signal: queued.signal.clone(),
            })
            .collect()
    }

    /// Tira da fila os quadros confirmados. O que foi confirmado nunca se repete.
    pub fn confirm(&mut self, seqs: &[u64]) {
        self.items.retain(|queued| !seqs.contains(&queued.seq));
    }

    /// O quadro ainda espera a confirmação?
    pub fn contains(&self, seq: u64) -> bool {
        self.items.iter().any(|queued| queued.seq == seq)
    }

    /// A fila passou do tamanho ou da idade máximos?
    pub fn overflowed(&self, now: Instant) -> bool {
        self.items.len() > self.max_size
            || self
                .items
                .front()
                .is_some_and(|oldest| now.saturating_duration_since(oldest.at) > self.max_age)
    }

    /// Esvazia a fila (o nó saiu da tabela: as filas para ele são descartadas).
    pub fn clear(&mut self) -> usize {
        let n = self.items.len();
        self.items.clear();
        n
    }
}

#[cfg(test)]
mod tests;
