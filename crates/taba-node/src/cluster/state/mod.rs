//! The node's state inside the cluster (`taba.node.state`, `drainState`) and what it
//! says about readiness (`/health/ready`, doc 06b).

use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Joining,
    Active,
    Syncing,
    Returning,
    /// Full disk or corrupt database (doc 05, section 7.3): only a new start fixes it.
    Failed,
}

impl NodeState {
    pub const fn as_str(self) -> &'static str {
        match self {
            NodeState::Joining => "joining",
            NodeState::Active => "active",
            NodeState::Syncing => "syncing",
            NodeState::Returning => "returning",
            NodeState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainState {
    Active,
    Draining,
    Drained,
}

impl DrainState {
    pub const fn as_str(self) -> &'static str {
        match self {
            DrainState::Active => "active",
            DrainState::Draining => "draining",
            DrainState::Drained => "drained",
        }
    }
}

/// What the node knows of itself, shared among the listeners.
#[derive(Debug)]
pub struct NodeShared {
    /// Changes when the node comes back from the drain as a new node (doc 04, section 6.10).
    node_id: RwLock<String>,
    pub started_at: SystemTime,
    state: RwLock<(NodeState, DrainState)>,
    /// Called when the node enters failure (the cluster notifies the peers).
    failed_hook: OnceLock<FailedHook>,
}

/// The node's failure hook (no `Debug`: it is a function).
pub struct FailedHook(Box<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for FailedHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FailedHook")
    }
}

impl NodeShared {
    /// The node is born `joining` and only becomes ready when the bootstrap and synchronization end (a node alone
    /// ends them at once).
    pub fn new(node_id: String, started_at: SystemTime) -> Self {
        Self {
            node_id: RwLock::new(node_id),
            started_at,
            state: RwLock::new((NodeState::Joining, DrainState::Active)),
            failed_hook: OnceLock::new(),
        }
    }

    pub fn node_id(&self) -> String {
        self.node_id
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_node_id(&self, node_id: String) {
        *self.node_id.write().unwrap_or_else(|e| e.into_inner()) = node_id;
    }

    pub fn state(&self) -> NodeState {
        self.state.read().unwrap_or_else(|e| e.into_inner()).0
    }

    pub fn drain_state(&self) -> DrainState {
        self.state.read().unwrap_or_else(|e| e.into_inner()).1
    }

    pub fn set_state(&self, state: NodeState) {
        self.state.write().unwrap_or_else(|e| e.into_inner()).0 = state;
    }

    pub fn set_drain_state(&self, drain: DrainState) {
        self.state.write().unwrap_or_else(|e| e.into_inner()).1 = drain;
    }

    /// Changes the `drainState` from `from` to `to` only if it is in `from`; returns whether it changed.
    pub fn transition_drain(&self, from: DrainState, to: DrainState) -> bool {
        let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
        if guard.1 == from {
            guard.1 = to;
            true
        } else {
            false
        }
    }

    /// The node failed (full disk, corrupt database): `/health/ready` becomes `503` `failed`.
    pub fn set_failed(&self) {
        let changed = {
            let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
            let changed = guard.0 != NodeState::Failed;
            guard.0 = NodeState::Failed;
            changed
        };
        if changed && let Some(hook) = self.failed_hook.get() {
            (hook.0)();
        }
    }

    /// Who is notified when the node enters failure. Only the first call counts.
    pub fn on_failed(&self, hook: Box<dyn Fn() + Send + Sync>) {
        let _ = self.failed_hook.set(FailedHook(hook));
    }

    /// `None` when the node is fit to receive traffic; otherwise the `reason` of the `HealthResponse`
    /// (`bootstrapping`, `syncing`, `returning`, `failed`, `draining` or `drained`). The drain comes
    /// first, since it takes the node out of rotation whatever the state in the cluster.
    pub fn not_ready_reason(&self) -> Option<&'static str> {
        let (state, drain) = *self.state.read().unwrap_or_else(|e| e.into_inner());
        match (drain, state) {
            (DrainState::Draining, _) => Some("draining"),
            (DrainState::Drained, _) => Some("drained"),
            (DrainState::Active, NodeState::Joining) => Some("bootstrapping"),
            (DrainState::Active, NodeState::Syncing) => Some("syncing"),
            (DrainState::Active, NodeState::Returning) => Some("returning"),
            (DrainState::Active, NodeState::Failed) => Some("failed"),
            (DrainState::Active, NodeState::Active) => None,
        }
    }
}

#[cfg(test)]
mod tests;
