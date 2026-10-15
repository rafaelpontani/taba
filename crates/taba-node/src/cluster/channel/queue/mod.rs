//! A channel's outgoing queue (doc 13, section 12.9): the frames with `seq` that have not yet been
//! confirmed by the other side. With the channel down it accumulates; the **state** signals of the same
//! topic replace each other (only the last matters), old notices are discarded, and a queue that exceeds
//! the maximum size or age makes the node on the other side a **failed** node (the exclusion vote is
//! opened without waiting for the end of the reconnections).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::cluster::protocol::{ChannelFrame, DefinitionKind, Signal};

/// How long a new-message notice applies: it is only a hint, and an old one only gets in the way.
pub const NOTIFY_TTL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
struct Queued {
    seq: u64,
    signal: Signal,
    at: Instant,
}

/// What an operation on the queue discarded, for the `taba.cluster.channel.queue.dropped` metric.
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

/// The key of a state signal: what replaces it in the queue. A notice has no key.
fn state_key(signal: &Signal) -> Option<(u8, &str)> {
    match signal {
        Signal::Listeners { topic, .. } => Some((0, topic)),
        Signal::Ready { topic, .. } => Some((1, topic)),
        Signal::NodeState { .. } => Some((2, "")),
        Signal::Frozen { node_id, .. } => Some((3, node_id)),
        Signal::Definition { kind, key, .. } => Some((
            match kind {
                DefinitionKind::Topic => 4,
                DefinitionKind::Token => 5,
            },
            key,
        )),
        Signal::Notify { .. } | Signal::Membership { .. } => None,
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

    /// Queues the signal and returns the `seq` and how many old frames it replaced.
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

    /// Discards the notices that passed [`NOTIFY_TTL`].
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

    /// The frames still unconfirmed, from oldest to newest, at most `limit`. A resent frame
    /// carries the same `seq`.
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

    /// The frames still unconfirmed with `seq` greater than `after`, at most `limit`: what a
    /// connection has not carried yet.
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

    /// Takes the confirmed frames out of the queue. What was confirmed is never repeated.
    pub fn confirm(&mut self, seqs: &[u64]) {
        self.items.retain(|queued| !seqs.contains(&queued.seq));
    }

    /// Does the frame still await confirmation?
    pub fn contains(&self, seq: u64) -> bool {
        self.items.iter().any(|queued| queued.seq == seq)
    }

    /// Did the queue exceed the maximum size or age?
    pub fn overflowed(&self, now: Instant) -> bool {
        self.items.len() > self.max_size
            || self
                .items
                .front()
                .is_some_and(|oldest| now.saturating_duration_since(oldest.at) > self.max_age)
    }

    /// Empties the queue (the node left the table: the queues to it are discarded).
    pub fn clear(&mut self) -> usize {
        let n = self.items.len();
        self.items.clear();
        n
    }
}

#[cfg(test)]
mod tests;
