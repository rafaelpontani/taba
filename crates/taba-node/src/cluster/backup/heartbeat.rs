//! The nodes' heartbeat in the bucket and death detection (doc 23, "S3 backup", section 1.6).
//!
//! Each node writes `nodes/<id>.json` with the **id**, the **access ips**, the state and a **sequence
//! number** that grows with each heartbeat. The `seq` is mandatory: without it the content repeats, the
//! ETag does not change, and whoever watches does not notice the heartbeat.
//!
//! **No synchronized clock:** whoever watches does not compare times. It keeps each file's ETag and
//! marks, on its own monotonic clock, when the ETag last changed. If it did not change for
//! `backup.dead-after`, the node is dead **to whoever is watching**. A file seen for the first time
//! counts as fresh until `backup.dead-after` passes without changing (without a history there is no way to know).
//!
//! **Alive is not the same as reachable.** The heartbeat shows that the node is alive, whatever the
//! network between the nodes; the network only answers "can I talk to it right now?":
//!
//! | Network to Y | Y's heartbeat | Conclusion | X's action |
//! |---|---|---|---|
//! | reachable | fresh | alive | normal |
//! | unreachable | fresh | alive in another partition | **does not take over** Y's blocks |
//! | unreachable | expired | dead | may take over the blocks |

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{Backup, lock};
use crate::storage::backup::keys;

/// The content of `nodes/<id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub id: String,
    pub addresses: Vec<String>,
    /// The cluster port: with it and the `addresses` another node joins through the normal path.
    #[serde(default)]
    pub port: u16,
    /// Grows with each heartbeat.
    pub seq: u64,
    /// `member` (in the cluster), `joining` (still joining) or `draining`.
    pub state: String,
}

/// What whoever watches concludes about a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// The heartbeat changed less than `backup.dead-after` ago (or the file is too new to know).
    Fresh,
    /// The file did not change for `backup.dead-after`, or the node left cleanly and removed the file.
    Dead,
    /// S3 could not be read: nothing to say. Whoever decides falls back to the behavior without the backup.
    Unknown,
}

struct Seen {
    etag: String,
    changed_at: Instant,
    /// The ETag has changed since we saw it: the node really beats.
    proven: bool,
    body: Option<Heartbeat>,
}

#[derive(Default)]
struct BeatState {
    seen: HashMap<String, Seen>,
    seq: u64,
    /// The last listing of `nodes/` worked.
    listed_ok: bool,
}

#[derive(Default)]
pub(crate) struct Beats {
    state: Mutex<BeatState>,
}

/// What `wait_for_verdict` concludes from observing the bucket at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// At least one node is beating: the cluster is alive, the node joins through the normal path.
    Alive(Vec<Heartbeat>),
    /// There are node files and none beats any more.
    AllDead,
    /// No node file in the bucket.
    Empty,
    /// S3 did not answer.
    Unavailable,
}

impl Backup {
    fn me(&self, seq: u64) -> Heartbeat {
        use crate::cluster::state::DrainState;
        let state = match (self.topology.is_active(), self.shared.drain_state()) {
            (true, DrainState::Active) => "member",
            (_, DrainState::Draining | DrainState::Drained) => "draining",
            _ => "joining",
        };
        Heartbeat {
            id: self.node_id(),
            addresses: self.topology.addresses(),
            port: self.topology.port(),
            seq,
            state: state.to_owned(),
        }
    }

    /// One heartbeat: writes its own file and observes the others'.
    pub(crate) async fn heartbeat_pass(&self) {
        if self.beating.load(Ordering::SeqCst) && !self.drained() {
            let seq = {
                let mut state = lock_state(&self.beats.state);
                state.seq += 1;
                state.seq
            };
            let beat = self.me(seq);
            let body = serde_json::to_vec(&beat).expect("o batimento serializa");
            let put = self.store.put(&keys::node(&beat.id), body).await;
            self.note("PUT do batimento", &put);
        }
        self.observe().await;
    }

    /// Lists `nodes/` and updates what is known of each node.
    pub(crate) async fn observe(&self) {
        self.observe_bodies(false).await;
    }

    /// Like `observe`; with `all_bodies`, it also re-reads the content of files that only changed (the
    /// `member`/`joining` state and the ips), which the node's startup needs. Day to day only a file
    /// seen for the first time is read: the ETag is enough to know the node beats.
    pub(crate) async fn observe_bodies(&self, all_bodies: bool) {
        let listed = self.store.list(keys::NODES_PREFIX).await;
        self.note("LIST of nodes/", &listed);
        let Ok(objects) = listed else {
            lock_state(&self.beats.state).listed_ok = false;
            return;
        };
        let me = self.node_id();
        let now = Instant::now();
        let mut present = Vec::new();
        let mut to_read = Vec::new();
        {
            let mut state = lock_state(&self.beats.state);
            state.listed_ok = true;
            for object in &objects {
                let Some(id) = keys::node_id_of(&object.key) else {
                    continue;
                };
                present.push(id.to_owned());
                match state.seen.get_mut(id) {
                    None => {
                        state.seen.insert(
                            id.to_owned(),
                            Seen {
                                etag: object.etag.clone(),
                                changed_at: now,
                                proven: false,
                                body: None,
                            },
                        );
                        to_read.push((id.to_owned(), object.key.clone()));
                    }
                    Some(seen) if seen.etag != object.etag => {
                        seen.etag.clone_from(&object.etag);
                        seen.changed_at = now;
                        seen.proven = true;
                        if all_bodies || !self.topology.is_active() {
                            to_read.push((id.to_owned(), object.key.clone()));
                        }
                    }
                    Some(_) => {}
                }
            }
            state.seen.retain(|id, _| present.contains(id));
        }
        // Reads the content only of what changed (ips and state); its own file does not need it.
        for (id, key) in to_read {
            if id == me {
                continue;
            }
            if let Ok(Some(object)) = self.store.get(&key).await
                && let Ok(beat) = serde_json::from_slice::<Heartbeat>(&object.body)
                && let Some(seen) = lock_state(&self.beats.state).seen.get_mut(&id)
            {
                seen.body = Some(beat);
            }
        }
    }

    /// What is concluded from node `id`'s heartbeat (never from itself).
    pub fn liveness(&self, id: &str) -> Liveness {
        let state = lock_state(&self.beats.state);
        if !state.listed_ok {
            return Liveness::Unknown;
        }
        match state.seen.get(id) {
            // No file: it never beat or left cleanly.
            None => Liveness::Dead,
            Some(seen) if seen.changed_at.elapsed() >= self.rules.dead_after => Liveness::Dead,
            Some(_) => Liveness::Fresh,
        }
    }

    /// The other nodes the bucket shows, with what is known about them.
    pub fn observed(&self) -> Vec<(String, Option<Heartbeat>, Liveness)> {
        let ids: Vec<String> = lock_state(&self.beats.state).seen.keys().cloned().collect();
        let me = self.node_id();
        ids.into_iter()
            .filter(|id| *id != me)
            .map(|id| {
                let beat = lock_state(&self.beats.state)
                    .seen
                    .get(&id)
                    .and_then(|s| s.body.clone());
                let liveness = self.liveness(&id);
                (id, beat, liveness)
            })
            .collect()
    }

    /// Observes the bucket for `backup.dead-after` and says whether there is a cluster beating (doc 23, 1.7,
    /// example C). Only counts those in the `member` state: a node that is also starting up does not prove
    /// that the cluster exists.
    pub async fn wait_for_verdict(&self) -> Verdict {
        loop {
            self.observe_bodies(true).await;
            let me = self.node_id();
            let (listed_ok, members): (bool, Vec<(Heartbeat, bool, Duration)>) = {
                let state = lock_state(&self.beats.state);
                (
                    state.listed_ok,
                    state
                        .seen
                        .iter()
                        .filter(|(id, _)| **id != me)
                        .filter_map(|(_, s)| {
                            s.body
                                .clone()
                                .map(|b| (b, s.proven, s.changed_at.elapsed()))
                        })
                        .filter(|(b, ..)| b.state == "member" || b.state == "draining")
                        .collect(),
                )
            };
            if !listed_ok {
                return Verdict::Unavailable;
            }
            let alive: Vec<Heartbeat> = members
                .iter()
                .filter(|(_, proven, since)| *proven && *since < self.rules.dead_after)
                .map(|(b, ..)| b.clone())
                .collect();
            if !alive.is_empty() {
                return Verdict::Alive(alive);
            }
            if members.is_empty() {
                // No file, or only from nodes that are also starting up (they prove nothing).
                return Verdict::Empty;
            }
            if members
                .iter()
                .all(|(_, _, since)| *since >= self.rules.dead_after)
            {
                return Verdict::AllDead;
            }
            if self.cancel.is_cancelled() {
                return Verdict::Unavailable;
            }
            // Gives the observer time to see the heartbeat change (or not) before concluding.
            let wait = (self.rules.heartbeat_interval / 2).max(Duration::from_millis(50));
            tokio::time::sleep(wait).await;
        }
    }

    /// The node left cleanly (drain finished, stop): removes its own file and stops beating.
    pub async fn leave(&self) {
        self.beating.store(false, Ordering::SeqCst);
        let deleted = self.store.delete(&keys::node(&self.node_id())).await;
        self.note("DELETE do batimento", &deleted);
    }

    /// The node came back from the drain as a new node: the heartbeat starts again.
    pub fn resume_beating(&self) {
        self.beating.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
}

fn lock_state(mutex: &Mutex<BeatState>) -> std::sync::MutexGuard<'_, BeatState> {
    lock(mutex)
}
