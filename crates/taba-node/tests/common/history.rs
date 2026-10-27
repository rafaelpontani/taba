//! The history of a chaos run (doc 37, section "Checking a history"): every operation a client made, with the
//! instant it started and the instant it ended, and the checker that compares the history with the guarantees
//! I1 to I6.
//!
//! A client never knows more than what the API answered, so an operation has three outcomes: `Ok` (the `201`
//! or `204`), `Fail` (a definitive refusal) and `Indeterminate` (a timeout, a `5xx` or a closed connection: it
//! may or may not have happened). The checker only demands from what was confirmed, and only forbids what no
//! attempt can explain.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// What the client learned about an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Fail,
    Indeterminate,
}

/// A publication attempt: `seq` is unique in the run and travels in the message body.
#[derive(Debug, Clone)]
pub struct Publish {
    pub seq: u64,
    /// `Some` in the FIFO queue; the group's publications are made by one client, one at a time.
    pub group: Option<String>,
    pub start: u64,
    pub end: u64,
    pub outcome: Outcome,
    pub id: Option<String>,
    /// A message the consumers never ack: it must end in the dead-letter after `maxDeliveries` deliveries.
    pub poison: bool,
}

/// A message handed to a consumer.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub seq: u64,
    pub id: String,
    pub group: Option<String>,
    /// Taken from the dead-letter queue, not from the queue it was published to.
    pub dead: bool,
    /// The queue it came from has the limit of deliveries (the `maxDeliveries` told to `with_max_deliveries`).
    pub limited: bool,
    /// The `deliveryCount` the message carried.
    pub count: u64,
    /// The instant the request that got it started and the instant the answer arrived.
    pub start: u64,
    pub end: u64,
}

/// An acknowledgement attempt.
#[derive(Debug, Clone)]
pub struct Ack {
    pub seq: u64,
    /// The ack was of a delivery taken from the dead-letter queue.
    pub dead: bool,
    pub id: String,
    pub start: u64,
    pub end: u64,
    pub outcome: Outcome,
}

#[derive(Default)]
struct Log {
    publishes: Vec<Publish>,
    deliveries: Vec<Delivery>,
    acks: Vec<Ack>,
}

/// The shared recorder. The logical clock orders the instants of operations running at the same time.
#[derive(Clone, Default)]
pub struct History {
    log: Arc<Mutex<Log>>,
    clock: Arc<AtomicU64>,
    next_seq: Arc<AtomicU64>,
    /// The `maxDeliveries` of the queue with a dead-letter (0: the run has none).
    max_deliveries: Arc<AtomicU64>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tells the checker the `maxDeliveries` of the queue that has a dead-letter (I5).
    pub fn with_max_deliveries(self, max: u64) -> Self {
        self.max_deliveries.store(max, Ordering::SeqCst);
        self
    }

    /// A new logical instant.
    pub fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// A new publication number.
    pub fn next_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn record_publish(&self, publish: Publish) {
        self.lock().publishes.push(publish);
    }

    pub fn record_delivery(&self, delivery: Delivery) {
        self.lock().deliveries.push(delivery);
    }

    pub fn record_ack(&self, ack: Ack) {
        self.lock().acks.push(ack);
    }

    /// Whether every confirmed publication has been delivered at least once and every confirmed poison message has
    /// reached the dead-letter.
    pub fn all_confirmed_delivered(&self) -> bool {
        let log = self.lock();
        let delivered: HashSet<u64> = log.deliveries.iter().map(|d| d.seq).collect();
        let dead: HashSet<u64> = log
            .deliveries
            .iter()
            .filter(|d| d.dead)
            .map(|d| d.seq)
            .collect();
        log.publishes
            .iter()
            .filter(|p| p.outcome == Outcome::Ok)
            .all(|p| delivered.contains(&p.seq) && (!p.poison || dead.contains(&p.seq)))
    }

    /// The IDs of confirmed publications nobody received (for a diagnosis of where the messages are).
    pub fn undelivered_ids(&self) -> Vec<String> {
        let log = self.lock();
        let delivered: HashSet<u64> = log.deliveries.iter().map(|d| d.seq).collect();
        log.publishes
            .iter()
            .filter(|p| p.outcome == Outcome::Ok && !delivered.contains(&p.seq))
            .filter_map(|p| p.id.clone())
            .collect()
    }

    /// For the first confirmed publication nobody received: the earlier messages of its group that were handed out
    /// and never acked with a `204` (what may be holding the group), as (seq, id).
    pub fn blockers(&self) -> Vec<(u64, String)> {
        let log = self.lock();
        let delivered: HashSet<u64> = log.deliveries.iter().map(|d| d.seq).collect();
        let Some(stuck) = log
            .publishes
            .iter()
            .find(|p| p.outcome == Outcome::Ok && p.group.is_some() && !delivered.contains(&p.seq))
        else {
            return Vec::new();
        };
        let acked: HashSet<u64> = log
            .acks
            .iter()
            .filter(|a| a.outcome == Outcome::Ok)
            .map(|a| a.seq)
            .collect();
        log.publishes
            .iter()
            .filter(|p| p.group == stuck.group && p.seq < stuck.seq && !acked.contains(&p.seq))
            .filter_map(|p| p.id.clone().map(|id| (p.seq, id)))
            .collect()
    }

    /// Everything the history knows about one message, in order, to read a violation.
    pub fn trail(&self, seq: u64) -> String {
        let log = self.lock();
        let mut lines: Vec<(u64, String)> = Vec::new();
        for p in log.publishes.iter().filter(|p| p.seq == seq) {
            lines.push((
                p.end,
                format!("publish {:?} id {:?} poison {}", p.outcome, p.id, p.poison),
            ));
        }
        for d in log.deliveries.iter().filter(|d| d.seq == seq) {
            lines.push((
                d.end,
                format!(
                    "delivery count {} dead {} (request at {})",
                    d.count, d.dead, d.start
                ),
            ));
        }
        for a in log.acks.iter().filter(|a| a.seq == seq) {
            lines.push((a.end, format!("ack {:?} dead {}", a.outcome, a.dead)));
        }
        lines.sort();
        lines
            .iter()
            .map(|(t, l)| format!("  [{t}] {l}"))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    }

    /// A summary line to print when a run ends (and to tell a run that did nothing from one that proved something).
    pub fn summary(&self) -> Summary {
        let log = self.lock();
        let count = |outcome| {
            log.publishes
                .iter()
                .filter(|p| p.outcome == outcome)
                .count()
        };
        let distinct: HashSet<u64> = log.deliveries.iter().map(|d| d.seq).collect();
        Summary {
            published_ok: count(Outcome::Ok),
            published_fail: count(Outcome::Fail),
            published_indeterminate: count(Outcome::Indeterminate),
            delivered: distinct.len(),
            redeliveries: log.deliveries.len() - distinct.len(),
            acked: log.acks.iter().filter(|a| a.outcome == Outcome::Ok).count(),
        }
    }

    /// Checks the history against the guarantees. `declared_losses` are the IDs the cluster declared lost
    /// (`/cluster/message-loss`). Returns one line per violation, each starting with the invariant (`I1`..`I6`).
    pub fn check(&self, declared_losses: &HashSet<String>) -> Vec<String> {
        let log = self.lock();
        let mut violations = Vec::new();

        let attempts: HashMap<u64, &Publish> = log.publishes.iter().map(|p| (p.seq, p)).collect();
        let mut delivered: HashMap<u64, Vec<&Delivery>> = HashMap::new();
        for delivery in &log.deliveries {
            delivered.entry(delivery.seq).or_default().push(delivery);
        }

        // I1: what got a `201` is delivered, unless the cluster declared it lost. I6: a loss is never silent.
        for publish in log.publishes.iter().filter(|p| p.outcome == Outcome::Ok) {
            if delivered.contains_key(&publish.seq) {
                continue;
            }
            let declared = publish
                .id
                .as_ref()
                .is_some_and(|id| declared_losses.contains(id));
            if !declared {
                violations.push(format!(
                    "I1/I6: seq {} (id {:?}) got a 201 and was never delivered nor declared lost",
                    publish.seq, publish.id
                ));
            }
        }

        // I2: nothing is delivered that no attempt explains (and a delivered ID is the one the 201 returned).
        for (seq, deliveries) in &delivered {
            let Some(publish) = attempts.get(seq) else {
                violations.push(format!("I2: seq {seq} was delivered but never published"));
                continue;
            };
            // The dead-letter queue may keep the ID or give another; only the queue it was published to is compared.
            let ids: HashSet<&str> = deliveries
                .iter()
                .filter(|d| !d.dead)
                .map(|d| d.id.as_str())
                .collect();
            if ids.len() > 1 && publish.group.is_some() {
                violations.push(format!(
                    "I4: seq {seq} of the FIFO queue was stored more than once: {ids:?}"
                ));
            }
            if let (Outcome::Ok, Some(id)) = (publish.outcome, &publish.id)
                && ids.iter().any(|d| *d != id)
            {
                violations.push(format!(
                    "I2: seq {seq} was delivered with an ID other than the one the 201 returned: {ids:?}"
                ));
            }
        }

        // I3: after a `204` the message is not handed out again (only requests that started after the ack).
        for ack in log.acks.iter().filter(|a| a.outcome == Outcome::Ok) {
            if let Some(again) = delivered
                .get(&ack.seq)
                .and_then(|ds| ds.iter().find(|d| d.start > ack.end))
            {
                violations.push(format!(
                    "I3: seq {} was acked (204 at {}) and delivered again by a request that started at {}",
                    ack.seq, ack.end, again.start
                ));
            }
        }

        // I5: a poison message ends in the dead-letter, after at most `maxDeliveries` deliveries, and a message is
        // never in both places.
        let max = self.max_deliveries.load(Ordering::SeqCst);
        if max > 0 {
            for delivery in log.deliveries.iter().filter(|d| d.limited && d.count > max) {
                violations.push(format!(
                    "I5: seq {} was delivered {} times, over maxDeliveries {max}",
                    delivery.seq, delivery.count
                ));
            }
        }
        for publish in log
            .publishes
            .iter()
            .filter(|p| p.poison && p.outcome == Outcome::Ok)
        {
            let deliveries = delivered
                .get(&publish.seq)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let dead_at = deliveries.iter().filter(|d| d.dead).map(|d| d.end).min();
            let declared = publish
                .id
                .as_ref()
                .is_some_and(|id| declared_losses.contains(id));
            match dead_at {
                None if !declared => violations.push(format!(
                    "I5: poison seq {} never reached the dead-letter",
                    publish.seq
                )),
                Some(at) => {
                    if let Some(late) = deliveries.iter().find(|d| !d.dead && d.start > at) {
                        violations.push(format!(
                            "I5: seq {} is in the dead-letter (since {at}) and was delivered again from the queue (request at {})",
                            publish.seq, late.start
                        ));
                    }
                }
                None => {}
            }
        }
        for ack in log
            .acks
            .iter()
            .filter(|a| !a.dead && a.outcome == Outcome::Ok)
        {
            if delivered
                .get(&ack.seq)
                .is_some_and(|ds| ds.iter().any(|d| d.dead))
            {
                violations.push(format!(
                    "I5: seq {} was acked on its queue and also reached the dead-letter",
                    ack.seq
                ));
            }
        }

        // I4: inside a group the first deliveries come in the order of publication.
        let mut groups: BTreeMap<&str, Vec<(u64, u64)>> = BTreeMap::new();
        for (seq, deliveries) in &delivered {
            let first = deliveries.iter().min_by_key(|d| d.end).expect("not empty");
            if let Some(group) = first.group.as_deref() {
                groups.entry(group).or_default().push((first.end, *seq));
            }
        }
        for (group, mut firsts) in groups {
            firsts.sort_unstable();
            for pair in firsts.windows(2) {
                if pair[0].1 > pair[1].1 {
                    violations.push(format!(
                        "I4: group {group}: seq {} was delivered before seq {}",
                        pair[0].1, pair[1].1
                    ));
                }
            }
        }

        violations
    }
}

/// The counts of a run.
#[derive(Debug, Clone, Copy)]
pub struct Summary {
    pub published_ok: usize,
    pub published_fail: usize,
    pub published_indeterminate: usize,
    pub delivered: usize,
    pub redeliveries: usize,
    pub acked: usize,
}

/// A small seeded generator (xorshift64*) for the nemesis: the same seed draws the same faults.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number from 0 to `bound - 1`.
    pub fn below(&mut self, bound: u64) -> u64 {
        (self.next() >> 11) % bound
    }
}
