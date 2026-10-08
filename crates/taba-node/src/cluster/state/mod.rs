//! O estado do nó dentro do cluster (`taba.node.state`, `drainState`) e o que ele
//! diz sobre a prontidão (`/health/ready`, doc 06b).

use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;

use crate::config::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Joining,
    Active,
    Syncing,
    Returning,
    /// Disco cheio ou banco corrompido (doc 05, seção 7.3): só uma nova partida resolve.
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

/// O que o nó sabe de si, compartilhado entre os listeners.
#[derive(Debug)]
pub struct NodeShared {
    /// Muda quando o nó volta do drain como nó novo (doc 04, seção 6.10).
    node_id: RwLock<String>,
    pub mode: Mode,
    pub started_at: SystemTime,
    state: RwLock<(NodeState, DrainState)>,
    /// Chamado quando o nó entra em falha (o cluster avisa os pares).
    failed_hook: OnceLock<FailedHook>,
}

/// O gancho de falha do nó (sem `Debug`: é uma função).
pub struct FailedHook(Box<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for FailedHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FailedHook")
    }
}

impl NodeShared {
    /// Em desenvolvimento o nó é um cluster de um só e já nasce `active`. Em produção
    /// nasce `joining` e só fica pronto quando o bootstrap e a sincronização acabam.
    pub fn new(node_id: String, mode: Mode, started_at: SystemTime) -> Self {
        let initial = match mode {
            Mode::Development => NodeState::Active,
            Mode::Production => NodeState::Joining,
        };
        Self {
            node_id: RwLock::new(node_id),
            mode,
            started_at,
            state: RwLock::new((initial, DrainState::Active)),
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

    /// Muda o `drainState` de `from` para `to` só se ele estiver em `from`; devolve se mudou.
    pub fn transition_drain(&self, from: DrainState, to: DrainState) -> bool {
        let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
        if guard.1 == from {
            guard.1 = to;
            true
        } else {
            false
        }
    }

    /// O nó falhou (disco cheio, banco corrompido): `/health/ready` passa a `503` `failed`.
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

    /// Quem é avisado quando o nó entra em falha. Vale só a primeira chamada.
    pub fn on_failed(&self, hook: Box<dyn Fn() + Send + Sync>) {
        let _ = self.failed_hook.set(FailedHook(hook));
    }

    /// `None` quando o nó está apto a receber tráfego; senão o `reason` do `HealthResponse`
    /// (`bootstrapping`, `syncing`, `returning`, `failed`, `draining` ou `drained`). O drain vem
    /// primeiro, pois tira o nó da rotação seja qual for o estado no cluster.
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
