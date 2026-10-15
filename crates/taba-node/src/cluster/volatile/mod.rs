//! The node that manages the volatile queues (doc 15, sections 14.5 and 14.6).
//!
//! A queue's manager is the primary of the position of the hash of its `id` on the ring, so the list of
//! managed queues is **derived** on each round of the task, not stored: it changes by itself with the
//! member table and the topic table. The manager only looks after the topic's life. At expiry it
//! asks **all** nodes how long ago the last `GET` was (`GET /cluster/topics/{id}/idle`)
//! and, if none had a `GET` within the `idleTimeout`, deletes the queue by the same register + commit as
//! `DELETE /topics/{topic}`, with the expected `id`.

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

/// When the manager looks again at a queue it manages.
#[derive(Debug, Clone, Copy)]
struct Entry {
    due_ms: u64,
}

pub struct VolatileManager {
    service: Arc<ClusterService>,
    topics: Arc<Topics>,
    /// The deadlines of the queues this node manages, by topic `id`.
    entries: Mutex<HashMap<String, Entry>>,
    /// The queues with a check under way: one at a time each.
    running: Mutex<HashSet<String>>,
}

/// The result of asking all the nodes.
enum Check {
    /// The smallest duration since the last `GET`, in milliseconds.
    Idle(u64),
    /// Some node did not answer: when in doubt, the queue stays alive.
    Blocked,
}

/// A queue's stable delay within a window of `window_ms`: from 0 to `window_ms`.
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

    /// The manager's task, until cancellation.
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

    /// The `id`s of the queues this node manages now.
    pub fn managed(&self) -> Vec<VolatileQueue> {
        // Only a ready node manages: until then, no queue in its stretch is deleted, the same
        // effect as a manager that is down (doc 15, section 14.6).
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

    /// One round: forgets the queues that stopped being its own, schedules the ones it started managing
    /// (immediate check, spread by the jitter) and fires the expired ones.
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

    /// A stable delay per queue, from 0 to `topics.volatile.takeover.jitter`: many queues that
    /// change manager at once do not all query at the same instant.
    fn jitter_ms(&self, topic_id: &str) -> u64 {
        jitter_in(
            self.service.rules().volatile_jitter.as_millis() as u64,
            topic_id,
        )
    }

    fn schedule(&self, topic_id: &str, due_ms: u64) {
        lock(&self.entries).insert(topic_id.to_owned(), Entry { due_ms });
    }

    /// The final check (doc 15, section 14.5, steps 2 to 4).
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
                    "a node did not answer the idleness query: the queue stays alive for now"
                );
                let retry = self.service.rules().volatile_retry_interval;
                self.schedule(&queue.id, now + retry.as_millis() as u64);
                return;
            }
        };
        if idle_ms < timeout_ms {
            // Someone had a more recent `GET`: reschedules for what remains.
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
                    "volatile queue deleted for idleness"
                );
            }
            // Deleted (or recreated with another `id`) midway: nothing to do.
            Err(e) if e.code == ErrorCode::TopicNotFound => {
                lock(&self.entries).remove(&queue.id);
            }
            Err(e) => {
                tracing::warn!(
                    topic = %queue.name,
                    error = %e,
                    "could not delete the idle queue: trying again later"
                );
                let retry = self.service.rules().volatile_retry_interval;
                self.schedule(&queue.id, now + retry.as_millis() as u64);
            }
        }
    }

    /// Asks all the members, this one included, how long ago the last `GET` was.
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
                // A node that does not know the topic yet (the commit has not reached it) does not allow
                // concluding anything.
                _ => return Check::Blocked,
            }
        }
        Check::Idle(least)
    }
}

#[cfg(test)]
mod tests;
