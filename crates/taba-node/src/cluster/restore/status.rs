//! What this node knows of the restoration of the previous lives while it runs (doc 07, sections 10.8 and 10.7):
//! whether the cluster is still being restored (the FIFO is held), whether this node finished sending its lives,
//! whether the operator forced, and who the restoration is still waiting for.

use std::sync::Mutex;

/// Where the cluster is in the restoration, as this node sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    /// The node has not yet found out (the start of the process, before it asked the others).
    #[default]
    Unknown,
    /// The cluster is not yet **restored from disk**: the FIFO is held and the vote is open.
    Restoring,
    /// The vote closed (or there was nothing to restore): the FIFO is released.
    Restored,
}

impl Status {
    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Unknown => "unknown",
            Status::Restoring => "restoring",
            Status::Restored => "restored",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "restoring" => Status::Restoring,
            "restored" => Status::Restored,
            _ => Status::Unknown,
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    status: Status,
    /// This node sent everything its lives keep (or had nothing to send).
    finished: bool,
    /// The operator forced (doc 07, section 10.7).
    forced: bool,
    /// When the restoration began on this node (ms), the start of the grace period `G`.
    since_ms: u64,
    /// The old nodes the restoration still waits for.
    missing: Vec<String>,
}

/// The restoration status of the node. A node starts `Unknown`; it becomes `Restoring` when a cold start brings
/// lives to restore or when it joins a cluster that is restoring, and `Restored` when the vote closes or when there
/// is nothing to restore.
#[derive(Debug, Default)]
pub struct Restoration {
    inner: Mutex<Inner>,
}

/// A copy of the state, for the health report and the answer to `GET /cluster/restore`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub status: Status,
    pub finished: bool,
    pub forced: bool,
    pub missing: Vec<String>,
}

impl Restoration {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The cluster starts (or is found) in restoration. Does nothing if it already is.
    pub fn begin(&self, now_ms: u64) {
        let mut inner = self.lock();
        if inner.status != Status::Restoring {
            *inner = Inner {
                status: Status::Restoring,
                since_ms: now_ms,
                ..Inner::default()
            };
        }
    }

    /// The vote closed, or there was nothing to restore: the cluster is restored from disk.
    pub fn complete(&self) {
        let mut inner = self.lock();
        inner.status = Status::Restored;
        inner.missing.clear();
    }

    pub fn is_restoring(&self) -> bool {
        self.lock().status == Status::Restoring
    }

    pub fn status(&self) -> Status {
        self.lock().status
    }

    /// This node sent everything its lives keep.
    pub fn finish(&self) {
        self.lock().finished = true;
    }

    /// The operator forced.
    pub fn force(&self) {
        self.lock().forced = true;
    }

    pub fn is_forced(&self) -> bool {
        self.lock().forced
    }

    /// Milliseconds since the restoration began on this node.
    pub fn elapsed_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.lock().since_ms)
    }

    pub fn set_missing(&self, missing: Vec<String>) {
        self.lock().missing = missing;
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.lock();
        Snapshot {
            status: inner.status,
            finished: inner.finished,
            forced: inner.forced,
            missing: inner.missing.clone(),
        }
    }
}
