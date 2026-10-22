//! Drain mode (doc 04, section 6.10; doc 06b, "Drain"): leaving rotation and writing everything to disk;
//! in a cluster, also notifying the peers, deregistering and coming back as a new node (`attach`).
//!
//! States: `active` → `draining` → `drained`. It is entered by signal (`drain.on-stop`) or by the API.
//! `drain.timeout` covers the whole drain; once exceeded, the node records what is left in memory
//! and, if it was by signal, shuts down anyway.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use super::backup::Backup;
use super::service::{ClusterService, conflict_wait};
use super::state::{DrainState, NodeShared};
use crate::core::clock::Clock;
use crate::core::error::{ErrorCode, TabaError};
use crate::engine::messages::Messages;

/// Who requested the drain (`DrainStatus.trigger`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Signal,
    Api,
}

impl Trigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Trigger::Signal => "signal",
            Trigger::Api => "api",
        }
    }
}

/// O `DrainStatus` do doc 06b.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainStatus {
    pub state: DrainState,
    /// `None` in `active`.
    pub trigger: Option<Trigger>,
    pub started_at: Option<SystemTime>,
    /// Messages still in memory.
    pub pending_messages: u64,
}

/// Response of `POST /node/drain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// `202`: the drain started now.
    Started(DrainStatus),
    /// `200`: the node was already in drain.
    AlreadyDraining(DrainStatus),
}

/// Response of `DELETE /node/drain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cancelled {
    /// `200`: there was no drain.
    Nothing(DrainStatus),
    /// `200`: the drain was cancelled and the node went back to writing in memory.
    Cancelled(DrainStatus),
    /// `202`: the node that had already left comes back as a new node (in production).
    Returned(DrainStatus),
}

/// The drain in progress or completed.
struct Run {
    trigger: Trigger,
    started_at: SystemTime,
    /// Cancellation: `DELETE` sets it, and the drain task stops at the next message.
    stop: Arc<AtomicBool>,
}

pub struct DrainController {
    shared: Arc<NodeShared>,
    messages: Arc<Messages>,
    clock: Arc<dyn Clock>,
    timeout: Duration,
    run: Mutex<Option<Run>>,
    /// The process is stopping by signal: `DELETE /node/drain` answers `409`.
    stopping: AtomicBool,
    /// The node in the cluster, which notifies the peers, deregisters and comes back.
    cluster: OnceLock<Arc<ClusterService>>,
    /// The deregistration (step 5) is in progress: `DELETE /node/drain` answers `409`.
    leaving: AtomicBool,
    /// The optional S3 backup: the drain sends everything to the bucket before leaving (doc 23, section 1.10).
    backup: OnceLock<Arc<Backup>>,
}

impl DrainController {
    pub fn new(
        shared: Arc<NodeShared>,
        messages: Arc<Messages>,
        clock: Arc<dyn Clock>,
        timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            shared,
            messages,
            clock,
            timeout,
            run: Mutex::new(None),
            stopping: AtomicBool::new(false),
            cluster: OnceLock::new(),
            leaving: AtomicBool::new(false),
            backup: OnceLock::new(),
        })
    }

    /// Attaches the S3 backup to the drain.
    pub fn set_backup(&self, backup: Arc<Backup>) {
        let _ = self.backup.set(backup);
    }

    /// The drain requested by whoever stops the whole cluster (`POST /cluster/stop`): starts, waits to
    /// finish (memory to disk, upload to S3, leaving the cluster) and returns whether the node reached
    /// `drained`. Within the drain deadline plus the backup's.
    pub async fn drain_and_wait(self: &Arc<Self>) -> bool {
        let _ = self.start_api();
        let backup_timeout = self
            .backup
            .get()
            .map_or(Duration::ZERO, |b| b.rules().drain_timeout);
        let deadline = Instant::now() + self.timeout + backup_timeout + Duration::from_secs(5);
        while self.shared.drain_state() != DrainState::Drained && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.shared.drain_state() == DrainState::Drained
    }

    /// Attaches the drain to the cluster: the notice to the peers, the deregistration and the return as a new node.
    pub fn set_cluster(&self, cluster: Arc<ClusterService>) {
        let _ = self.cluster.set(cluster);
    }

    fn signals(&self) -> Option<Arc<crate::cluster::replication::Signals>> {
        self.cluster
            .get()
            .and_then(|c| c.replication())
            .map(|r| r.signals.clone())
    }

    fn run(&self) -> MutexGuard<'_, Option<Run>> {
        self.run.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> DrainStatus {
        let run = self.run();
        self.status_of(run.as_ref())
    }

    fn status_of(&self, run: Option<&Run>) -> DrainStatus {
        DrainStatus {
            state: self.shared.drain_state(),
            trigger: run.map(|r| r.trigger),
            started_at: run.map(|r| r.started_at),
            pending_messages: self.messages.pending(),
        }
    }

    /// `POST /node/drain`: puts the node in drain without ending the process. Progress is in
    /// `GET /node/drain`.
    pub fn start_api(self: &Arc<Self>) -> Started {
        let stop = match self.begin(Trigger::Api) {
            Some(stop) => stop,
            None => return Started::AlreadyDraining(self.status()),
        };
        let this = self.clone();
        tokio::spawn(async move { this.run_drain(stop).await });
        Started::Started(self.status())
    }

    /// Enters `draining` if the node was `active`. Returns the drain's cancellation signal.
    fn begin(&self, trigger: Trigger) -> Option<Arc<AtomicBool>> {
        let mut run = self.run();
        if !self
            .shared
            .transition_drain(DrainState::Active, DrainState::Draining)
        {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *run = Some(Run {
            trigger,
            started_at: self.clock.now(),
            stop: stop.clone(),
        });
        self.messages.begin_drain();
        // The peers stop asking this node for `take` (doc 04, section 6.10, step 2).
        if let Some(signals) = self.signals() {
            signals.announce_draining();
        }
        tracing::info!(trigger = trigger.as_str(), "drain started");
        Some(stop)
    }

    /// Empties memory and, if it worked, marks the save and moves to `drained`.
    async fn run_drain(self: Arc<Self>, stop: Arc<AtomicBool>) {
        let deadline = Instant::now() + self.timeout;
        let messages = self.messages.clone();
        let flag = stop.clone();
        let report = tokio::task::spawn_blocking(move || {
            messages.drain_until(|| flag.load(Ordering::Relaxed) || Instant::now() >= deadline)
        })
        .await;
        let report = match report {
            Ok(report) => report,
            Err(e) => {
                tracing::error!(error = %e, "the drain failed");
                return;
            }
        };
        // With all memory on disk, everything goes to S3 **before** the node leaves the cluster: after
        // leaving it stops being the owner of what it would send. It does not wait forever: once
        // `backup.drain.timeout` passes the node goes on with what it has (doc 23, section 1.10, scenario 18).
        if let Some(backup) = self.backup.get().cloned()
            && report.remaining == 0
            && !stop.load(Ordering::Relaxed)
        {
            let sent = backup.drain_upload().await;
            if sent.complete() {
                tracing::info!(
                    segments = sent.segments_uploaded,
                    messages = sent.messages_uploaded,
                    "drain: everything went to S3"
                );
            } else {
                tracing::error!(
                    outbox = sent.outbox_pending,
                    journal = sent.journal_pending,
                    "drain: the upload to S3 did not finish in time (backup.drain.timeout); the node leaves anyway and what is left depends on the replicas"
                );
            }
        }
        // With all memory on disk, the node asks to leave the cluster itself, without a vote.
        if let Some(cluster) = self.cluster.get().cloned()
            && report.remaining == 0
            && !stop.load(Ordering::Relaxed)
        {
            self.leaving.store(true, Ordering::Relaxed);
            // Another member change in progress: the request waits its turn, within `drain.timeout`
            // (doc 04, section 6.8).
            let left = tokio::time::timeout_at(deadline.into(), async {
                loop {
                    match cluster.leave().await {
                        Err(e) if e.code == ErrorCode::ClusterConflict => {
                            tokio::time::sleep(conflict_wait()).await;
                        }
                        other => return other,
                    }
                }
            })
            .await;
            self.leaving.store(false, Ordering::Relaxed);
            match left {
                Ok(Ok(())) => {}
                // Decided, but the commit is still running on some peer: the node has already left the table.
                Ok(Err(e)) if e.code == ErrorCode::ClusterCommitTimeout => {}
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "the drain could not leave the cluster: the node stays in draining");
                    return;
                }
                Err(_) => {
                    tracing::error!(
                        "drain.timeout ran out before the node left the cluster: the node stays in draining"
                    );
                    return;
                }
            }
        }
        self.finish(&stop, report.moved, report.failed, report.remaining);
        // Left cleanly: the heartbeat in the bucket disappears too (whoever watches does not wait for the death deadline).
        if self.shared.drain_state() == DrainState::Drained
            && let Some(backup) = self.backup.get().cloned()
        {
            backup.leave().await;
        }
    }

    fn finish(&self, stop: &Arc<AtomicBool>, moved: u64, failed: u64, remaining: u64) {
        let run = self.run();
        // Cancelled (or replaced by another drain) while emptying: there is nothing to conclude.
        let current = run.as_ref().is_some_and(|r| Arc::ptr_eq(&r.stop, stop));
        if !current || stop.load(Ordering::Relaxed) {
            return;
        }
        if remaining > 0 {
            tracing::error!(
                moved,
                failed,
                remaining,
                timeout = ?self.timeout,
                "incomplete drain: messages were left in memory (step: emptying memory)"
            );
            return;
        }
        self.shared
            .transition_drain(DrainState::Draining, DrainState::Drained);
        tracing::info!(moved, "drain completed: all memory is on disk");
    }

    /// `DELETE /node/drain`: cancels the drain (or, after leaving, comes back). Without a cluster there is no
    /// deregistration, so there is never a `409` for that reason; only with the process stopping by signal.
    pub fn cancel(&self) -> Result<Cancelled, TabaError> {
        if self.leaving.load(Ordering::Relaxed) {
            return Err(TabaError::new(
                ErrorCode::DrainConflict,
                "The node is deregistering from the cluster; try again when it is drained.",
            ));
        }
        if self.stopping.load(Ordering::Relaxed) {
            return Err(TabaError::new(
                ErrorCode::DrainConflict,
                "The process is stopping by signal; the drain cannot be cancelled.",
            ));
        }
        let mut run = self.run();
        let before = self.shared.drain_state();
        if before == DrainState::Active {
            return Ok(Cancelled::Nothing(self.status_of(run.as_ref())));
        }
        if let Some(r) = run.take() {
            r.stop.store(true, Ordering::Relaxed);
        }
        self.messages.cancel_drain();
        self.shared.set_drain_state(DrainState::Active);
        tracing::info!(
            from = before.as_str(),
            "drain cancelled: the node went back to writing in memory"
        );
        if let Some(backup) = self.backup.get() {
            backup.resume_beating();
        }
        let status = self.status_of(None);
        Ok(match before {
            DrainState::Drained => {
                // Already out of the cluster: comes back as a new node, with a new ID and a full bootstrap.
                if let Some(cluster) = self.cluster.get().cloned() {
                    tokio::spawn(async move { cluster.rejoin_as_new().await });
                }
                Cancelled::Returned(status)
            }
            _ => {
                if let Some(signals) = self.signals() {
                    signals.announce_active();
                }
                Cancelled::Cancelled(status)
            }
        })
    }

    /// The stop by signal (`drain.on-stop`): does the drain and waits for it to finish, within
    /// `drain.timeout`. Returns whether memory ended up entirely on disk. If the API had already started the
    /// drain, it waits for that one.
    pub async fn stop_by_signal(self: &Arc<Self>) -> bool {
        self.stopping.store(true, Ordering::Relaxed);
        match self.begin(Trigger::Signal) {
            Some(stop) => {
                self.clone().run_drain(stop).await;
            }
            None => self.wait_for_drained().await,
        }
        let done = self.shared.drain_state() == DrainState::Drained;
        if !done {
            tracing::error!(
                pending = self.messages.pending(),
                "the drain did not finish: shutting down anyway, what was only in memory is lost"
            );
        }
        done
    }

    async fn wait_for_drained(&self) {
        let deadline = Instant::now() + self.timeout;
        while self.shared.drain_state() == DrainState::Draining && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

#[cfg(test)]
mod tests;
